#!/usr/bin/env python3
"""CI-only maintainer refresh: one native binary invocation per workflow step.

The mutation step is PR-diff-limited: the base SHA comes from CI, changed
new-side lines are read from a merge-base diff, and the two real targets stay
in one mutator call. Each target gets its own file-qualified --lines set; an
unchanged target gets a beyond-EOF line so it publishes an explicit zero-site
snapshot instead of borrowing a line number from the other target.
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
SHA = re.compile(r'^[0-9a-f]{40}$')
HUNK = re.compile(r'^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@')
OUTCOMES = re.compile(r':outcomes\s*\{([^{}]*)\}')
VERDICTS = re.compile(r':(killed|survived|uncovered)\b')
MODE_LINES = ('index ', 'old mode ', 'new mode ',
              'deleted file mode ', 'new file mode ')


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
    base = os.environ.get(BASE_SHA_ENV, '').strip()
    require(bool(base), f'mutate: missing {BASE_SHA_ENV}; refusing an unfiltered lane')
    require(SHA.fullmatch(base) is not None, f'mutate: malformed base SHA: {base!r}')
    return base


def diff_target(header, allowed, seen):
    fields = header.split()
    require(len(fields) >= 4 and fields[2].startswith('a/')
            and fields[3].startswith('b/'),
            f'mutate: malformed diff header: {header!r}')
    target = fields[3][2:]
    require(target in allowed, f'mutate: unexpected diff target: {target!r}')
    require(target not in seen, f'mutate: duplicate diff target: {target}')
    seen.add(target)
    return target


def hunk_lines(header):
    match = HUNK.match(header)
    require(match is not None, f'mutate: malformed hunk: {header!r}')
    old_count = 1 if match.group(2) is None else int(match.group(2))
    new_start = int(match.group(3))
    new_count = 1 if match.group(4) is None else int(match.group(4))
    require(new_start >= 1 or (new_start == 0 and new_count == 0),
            f'mutate: invalid new-side line: {header!r}')
    return old_count, new_start, new_count


def parse_diff(diff):
    changed = {target: set() for target, _ in MUTATE_TARGETS}
    seen = set()
    current = None
    remaining = 0
    for line in diff.splitlines():
        if line.startswith('\\'):
            continue
        if line.startswith('diff --git '):
            require(remaining == 0, 'mutate: unterminated diff hunk')
            current = diff_target(line, changed, seen)
            continue
        if current is None:
            require(not line, f'mutate: unexpected diff preamble: {line!r}')
            continue
        if line.startswith(MODE_LINES) or line.startswith('--- '):
            continue
        require(not line.startswith(('rename ', 'copy ', 'Binary files ')),
                f'mutate: ambiguous diff for {current}: {line!r}')
        if line.startswith('+++ '):
            path = line[4:].split('\t', 1)[0]
            require(path == f'b/{current}',
                    f'mutate: wrong diff path for {current}: {path!r}')
            continue
        if line.startswith('@@'):
            require(remaining == 0, 'mutate: nested diff hunk')
            old_count, new_start, new_count = hunk_lines(line)
            if new_count:
                changed[current].update(range(new_start, new_start + new_count))
            remaining = old_count + new_count
            continue
        if remaining:
            require(line[:1] in (' ', '-', '+'),
                    f'mutate: malformed hunk body: {line!r}')
            remaining -= 1
            continue
        require(not line, f'mutate: unexpected diff line: {line!r}')
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


def per_target_line_filter(root, lines_by_target):
    entries = []
    zero_targets = set()
    for target, lines in lines_by_target.items():
        selected = set(lines)
        if not selected:
            source = root / target
            require(source.is_file(), f'mutate: missing target for zero-site run: {target}')
            selected.add(len(source.read_text(encoding='utf-8').splitlines()) + 1)
            zero_targets.add(target)
        entries.extend(f'{target}:{number}' for number in sorted(selected))
    require(bool(entries), 'mutate: malformed line filter')
    return ','.join(entries), zero_targets


def build_mutation_command(root, binary, base):
    reports = [path for folder in ('coverage', 'target/coverage')
               for path in (root / folder).rglob('lcov.info')]
    require(bool(reports) and all(path.stat().st_size > 0 for path in reports),
            'mutate: no non-empty coverage reports from the crap step')
    line_filter, zero_targets = per_target_line_filter(root, changed_lines(root, base))
    command = [str(binary), 'quality', 'mutate', '--reuse-coverage',
               '--lines', line_filter]
    command.extend(target for target, _ in MUTATE_TARGETS)
    return command, zero_targets


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


def validate_mutation_snapshots(root, started, zero_targets):
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
        if target in zero_targets:
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

    zero_targets = set()
    if args.operation == 'mutate':
        command, zero_targets = build_mutation_command(root, binary, base_commit())
        for target, relative in MUTATE_TARGETS:
            if target in zero_targets:
                snapshot = root / relative
                if snapshot.exists():
                    snapshot.unlink()
        if zero_targets:
            print(f'mutate: no changed lines for {", ".join(sorted(zero_targets))}; '
                  'publishing explicit zero-site snapshots', file=sys.stderr)
    else:
        command = [str(binary), 'quality', args.operation]

    started = time.time()
    result = subprocess.run(command, stdout=subprocess.PIPE, encoding='utf-8', check=False)
    print(result.stdout, end='', flush=True)
    record = validate(result.stdout, args.operation, result.returncode, root)
    if args.operation == 'mutate':
        validate_mutation_snapshots(root, started, zero_targets)
    (state / f'{args.operation}.json').write_text(json.dumps(record) + '\n', encoding='utf-8')
    if zero_targets:
        print(f'Accepted maintainer refresh: zero-site snapshots for '
              f'{", ".join(sorted(zero_targets))}')


if __name__ == '__main__':
    main()
