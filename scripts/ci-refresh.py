#!/usr/bin/env python3
"""CI-only maintainer refresh: one native binary invocation per workflow step."""
import argparse
import json
import os
from pathlib import Path
import subprocess


TARGET = 'crates/platform/src/lib.rs'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


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
    else:
        command = [str(binary), 'quality', args.operation]
        if args.operation == 'mutate':
            command.append(TARGET)
        # stdout is the record only; reports remain visible on inherited stderr.
        result = subprocess.run(command, stdout=subprocess.PIPE, encoding='utf-8', check=False)
        print(result.stdout, end='', flush=True)
        record = validate(result.stdout, args.operation, result.returncode, root)
        (state / f'{args.operation}.json').write_text(json.dumps(record) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
