#!/usr/bin/env python3
"""CI-only maintainer refresh: one native binary invocation per workflow step.

The mutation step is PR-diff-limited: the base SHA comes from CI, changed
new-side lines are read from a merge-base diff of the two real targets, and a
single --lines value is shared by the one mutator invocation. An empty filter
uses an explicit beyond-EOF sentinel so the run still publishes a valid
zero-site record instead of silently broadening to the whole lane.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


MUTATE_TARGETS = (
    ('crates/ir/src/lib.rs', '.metrics/mutate/ir.edn'),
    ('crates/scene/src/dry.rs', '.metrics/mutate/scene/dry.edn'),
)
BASE_SHA_ENV = 'UML_REFRESH_BASE_SHA'
BASE_SHA_ALIASES = ('BASE_SHA', 'PR_BASE_SHA')
SHA = re.compile(r'^[0-9a-f]{40}$')
HUNK = re.compile(r'^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@')
OUTCOMES = re.compile(r':outcomes\s*\{([^{}]*)\}')
VERDICTS = re.compile(r':(killed|survived|uncovered)\b')


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def git_output(root, *arguments):
    result = subprocess.run(['git', *arguments], cwd=root, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, encoding='utf-8', check=False)
    require(result.returncode == 0,
            f'git {arguments[0]}: {(result.stderr or result.stdout).strip()}')
    return result.stdout


def base_commit():
    """Read one unambiguous base SHA from CI; never fall back to an open lane."""
    supplied = {}
    for name in (BASE_SHA_ENV,) + BASE_SHA_ALIASES:
        value = os.environ.get(name, '').strip()
        if value:
            supplied[name] = value
    require(bool(supplied), 'mutate: missing base SHA; refusing an unfiltered lane')
    require(len(set(supplied.values())) == 1,
            f'mutate: conflicting base SHAs: {supplied}')
    base = next(iter(supplied.values()))
    require(SHA.fullmatch(base) is not None, f'mutate: malformed base SHA: {base!r}')
    return base


def parse_diff(diff):
    """Map each configured target to its changed new-side lines.

    Only zero-context unified hunks are accepted. Any path, hunk, body line or
    trailing text that does not match the expected shape is a hard failure.
    """
    changed = {target: set() for target, _ in MUTATE_TARGETS}
    seen = set()
    current = None
    remaining = 0
    lines = diff.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        if line.startswith('\\'):
            index += 1
            continue
        if line.startswith('diff --git '):
            require(remaining == 0, 'mutate: unterminated diff hunk')
            fields = line.split()
            require(len(fields) >= 4 and fields[2].startswith('a/')
                    and fields[3].startswith('b/'),
                    f'mutate: malformed diff header: {line!r}')
            current = fields[3][2:]
            require(current in changed, f'mutate: unexpected diff target: {current!r}')
            require(current not in seen, f'mutate: duplicate diff target: {current}')
            seen.add(current)
            index += 1
            continue
        if current is None:
            require(not line, f'mutate: unexpected diff preamble: {line!r}')
            index += 1
            continue
        if line.startswith(('index ', 'old mode ', 'new mode ',
                            'deleted file mode ', 'new file mode ')):
            index += 1
            continue
        require(not line.startswith(('rename ', 'copy ', 'Binary files ')),
                f'mutate: ambiguous diff for {current}: {line!r}')
        if line.startswith('--- '):
            index += 1
            continue
        if line.startswith('+++ '):
            path = line[4:].split('\t', 1)[0]
            require(path == f'b/{current}',
                    f'mutate: wrong diff path for {current}: {path!r}')
            index += 1
            continue
        if line.startswith('@@'):
            require(remaining == 0, 'mutate: nested diff hunk')
            match = HUNK.match(line)
            require(match is not None, f'mutate: malformed hunk: {line!r}')
            old_count = 1 if match.group(2) is None else int(match.group(2))
            new_start = int(match.group(3))
            new_count = 1 if match.group(4) is None else int(match.group(4))
            require(new_start >= 1, f'mutate: invalid new-side line: {line!r}')
            if new_count:
                changed[current].update(range(new_start, new_start + new_count))
            remaining = old_count + new_count
            index += 1
            while remaining > 0:
                require(index < len(lines), 'mutate: truncated diff hunk')
                body = lines[index]
                if body.startswith('\\'):
                    index += 1
                    continue
                require(body[:1] in (' ', '-', '+'),
                        f'mutate: malformed hunk body: {body!r}')
                remaining -= 1
                index += 1
            continue
        require(not line, f'mutate: unexpected diff line: {line!r}')
        index += 1
    require(remaining == 0, 'mutate: unterminated diff hunk')
    return changed


def changed_lines(root, base):
    candidates = [line.strip() for line in
                  git_output(root, 'merge-base', '--all', base, 'HEAD').splitlines()
                  if line.strip()]
    require(len(candidates) == 1 and SHA.fullmatch(candidates[0]) is not None,
            f'mutate: ambiguous or invalid merge base: {candidates}')
    targets = [target for target, _ in MUTATE_TARGETS]
    diff = git_output(root, 'diff', '--unified=0', candidates[0], '--', *targets)
    return parse_diff(diff)


def mutation_line_filter(root, lines_by_target):
    """One positive --lines value for the single two-target mutator call."""
    selected = set()
    for lines in lines_by_target.values():
        selected.update(lines)
    zero_site = not selected
    if zero_site:
        # --lines rejects an empty value and 0. A line beyond every target's
        # EOF selects no site, making the zero outcome explicit to the mutator.
        for target, _ in MUTATE_TARGETS:
            source = root / target
            require(source.is_file(), f'mutate: missing target for zero-site run: {target}')
            selected.add(len(source.read_text(encoding='utf-8').splitlines()) + 1)
    require(bool(selected) and all(number > 0 for number in selected),
            'mutate: malformed line filter')
    return ','.join(str(number) for number in sorted(selected)), zero_site


def build_mutation_command(root, binary, base):
    """Keep both business targets and the full test command in one call."""
    reports = [path for folder in ('coverage', 'target/coverage')
               for path in (root / folder).rglob('lcov.info')]
    require(bool(reports) and all(path.stat().st_size > 0 for path in reports),
            'mutate: no non-empty coverage reports from the crap step')
    line_filter, zero_site = mutation_line_filter(root, changed_lines(root, base))
    command = [str(binary), 'quality', 'mutate', '--reuse-coverage',
               '--lines', line_filter]
    command.extend(target for target, _ in MUTATE_TARGETS)
    return command, zero_site


def validate(text, operation, code, root):
    require(len(text.splitlines()) == 1, f'{operation}: expected exactly one JSON line')
    record = json.loads(text)
    require(isinstance(record, dict), f'{operation}: expected a JSON object')
    require(type(record.get('version')) is int and record['version'] == 1,
            f'{operation}: expected version 1')
    require(record.get('operation') == operation, f'{operation}: wrong operation')
    require(record.get('validResult') is True, f'{operation}: invalid result: {record}')
    require(type(record.get('exitCode')) is int and record['exitCode'] == code,
            f'{operation}: record/process exit mismatch: {record}, process={code}')
    require(code in ({0, 2} if operation == 'crap' else {0, 3}),
            f'{operation}: unexpected process exit {code}')
    require(record.get('configured') is False, f'{operation}: expected standardTools')
    require(record.get('tool') == ('crapper' if operation == 'crap' else 'mutator'),
            f'{operation}: wrong tool')
    require(record.get('crapScope') == ('whole-tree' if operation == 'crap' else None),
            f'{operation}: wrong scope')
    require(record.get('reason') is None, f'{operation}: refused: {record}')
    require(Path(record['projectRoot']).resolve() == root, f'{operation}: wrong project root')
    return record


def validate_mutation_snapshots(root, started, zero_site=False):
    # The JSON record carries no per-file outcomes. Each published snapshot
    # does, and publish() rewrites it even when every mutation was reused.
    # A fresh mtime proves this run selected the file rather than merely
    # passing over the snapshots committed with the project.
    for target, relative in MUTATE_TARGETS:
        snapshot = root / relative
        require(snapshot.is_file(), f'mutate: missing snapshot for {target}: {relative}')
        require(snapshot.stat().st_mtime >= started - 2,
                f'mutate: stale snapshot for {target}: {relative}')
        text = snapshot.read_text(encoding='utf-8')
        require(f':source "{target}"' in text,
                f'mutate: snapshot for {target} has wrong source: {relative}')
        found = OUTCOMES.findall(text)
        require(len(found) == 1,
                f'mutate: snapshot for {target} has no single outcomes map: {relative}')
        verdicts = set(VERDICTS.findall(found[0]))
        if zero_site:
            # An explicit zero-site run has no executed site: its fresh map is
            # either empty or carries only uncovered verdicts from line 0.
            require(not verdicts or verdicts == {'uncovered'},
                    f'mutate: zero-site snapshot for {target} has active verdicts: {verdicts}')
        else:
            require(bool(verdicts),
                    f'mutate: snapshot for {target} has empty outcomes: {relative}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('operation', choices=('crap', 'mutate', 'ir'))
    args = parser.parse_args()
    root = Path.cwd().resolve()
    binary = args.binary.resolve(strict=True)
    state = Path(os.environ['RUNNER_TEMP']) / 'uml-maintainer-refresh'
    if args.operation == 'crap':
        # A fresh runner must not accept records from a previous refresh.
        state.mkdir()
    else:
        for operation in (('crap',) if args.operation == 'mutate' else ('crap', 'mutate')):
            record = json.loads((state / f'{operation}.json').read_text(encoding='utf-8'))
            validate(json.dumps(record), operation, record['exitCode'], root)

    if args.operation == 'ir':
        output = state / 'diagram.edn'
        require(not output.exists(), 'IR output already exists')
        subprocess.run([str(binary), 'ir', 'examples/uml-viewer.policy.edn', str(output)],
                       check=True)
        require(output.is_file() and output.stat().st_size > 0, 'IR output is empty or missing')
        print(f'Accepted maintainer refresh: {output.stat().st_size} IR bytes')
        return

    zero_site = False
    if args.operation == 'mutate':
        command, zero_site = build_mutation_command(root, binary, base_commit())
        if zero_site:
            # With no selected site the mutator would reuse an unchanged
            # snapshot; removing it forces publish() to write a fresh
            # zero-site record. The checkout is disposable CI state.
            for _, relative in MUTATE_TARGETS:
                snapshot = root / relative
                if snapshot.exists():
                    snapshot.unlink()
            print('mutate: no changed target lines; forcing an explicit zero-site refresh',
                  file=sys.stderr)
    else:
        command = [str(binary), 'quality', args.operation]

    started = time.time()
    # stdout is the record only; reports remain visible on inherited stderr.
    result = subprocess.run(command, stdout=subprocess.PIPE, encoding='utf-8', check=False)
    print(result.stdout, end='', flush=True)
    record = validate(result.stdout, args.operation, result.returncode, root)
    if args.operation == 'mutate':
        validate_mutation_snapshots(root, started, zero_site)
    (state / f'{args.operation}.json').write_text(json.dumps(record) + '\n', encoding='utf-8')
    if zero_site:
        print('Accepted maintainer refresh: valid zero-site record and fresh snapshots')


if __name__ == '__main__':
    main()
