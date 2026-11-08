#!/usr/bin/env python3
"""Record semantics compared against the Python dispatch on the same fixtures.

Usage: python scripts/t15-python-parity.py <uml-viewer binary>

Runs the Rust dispatcher (`uml-viewer quality`) and the Python dispatcher
(`scripts/uml_quality.py` `dispatch`, with `run` stubbed by a fixture script,
as test_posix_entries.py does) on copies of the same projects, and compares
the version-1 record fields. Exit 0 when they agree.

The differences that are intentional are reported, not hidden: the Rust record
is stdout and carries `reason`; the Python one is a file. A refusal publishes
no Python record, so only its exit is compared.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / 'scripts'
PY = sys.executable
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/uml-viewer'


def rust(root, operation, *args):
    result = subprocess.run([str(BINARY), 'quality', operation, '--root', str(root), *args],
                            cwd=root.parent, capture_output=True, text=True, check=False)
    record = json.loads(result.stdout)
    record['projectRoot'] = '<root>'
    return result.returncode, record, result.stderr


def python(root, operation, *args):
    """The real `uml_quality.dispatch`, with the tool argv replaced by a
    fixture executable, exactly as the Python acceptance stubs its tools."""
    probe = root / 'argv.json'
    if probe.exists():
        probe.unlink()
    driver = r'''
import json, os, sys
from pathlib import Path
sys.path.insert(0, %r)
import uml_quality
calls = []
import subprocess
def fake_run(argv, **kwargs):
    calls.append(argv)
    Path(%r).write_text(json.dumps([str(a) for a in argv]))
    if str(argv[0]) == sys.executable:
        # A declared operation: the same real command the Rust side runs.
        return subprocess.run([str(a) for a in argv], cwd=kwargs.get('cwd'))
    class R:
        returncode = %d
    return R()
uml_quality.run = fake_run
sys.exit(uml_quality.dispatch(Path(%r).resolve(), %r, %r))
''' % (str(SCRIPTS), str(root / 'argv.json'), 0, str(root), operation, list(args))
    result = subprocess.run([PY, '-I', '-B', '-c', driver], cwd=root,
                            capture_output=True, text=True, check=False)
    path = root / '.uml-viewer/quality-result.json'
    record = json.loads(path.read_text()) if path.exists() else None
    if record:
        record['projectRoot'] = '<root>'
    argv = json.loads((root / 'argv.json').read_text()) if (root / 'argv.json').exists() else None
    return result.returncode, record, result.stderr, argv


def scenario(name, config, files, operation, args):
    """Compare on the same fixture; returns a report line."""
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        outcomes = []
        for side in ('rust', 'python'):
            root = scratch / side / 'proj & space'
            root.mkdir(parents=True)
            for name_, text in files.items():
                (root / name_).parent.mkdir(parents=True, exist_ok=True)
                (root / name_).write_text(text)
            if config is not None:
                (root / 'uml-quality.json').write_text(json.dumps(config))
            if side == 'rust':
                code, record, err = rust(root, operation, *args)
                argv = None
            else:
                code, record, err, argv = python(root, operation, *args)
            outcomes.append((code, record, err, argv))
        (r_code, r_record, _, _), (p_code, p_record, p_err, p_argv) = outcomes
        fields = ['operation', 'projectRoot', 'exitCode', 'validResult', 'crapScope', 'configured', 'tool']
        agreement = []
        if r_code != p_code:
            agreement.append(f'EXIT rust={r_code} python={p_code}')
        if p_record is None:
            agreement.append('python published no record (refusal before the command ran)')
        else:
            for field in fields:
                if r_record[field] != p_record[field]:
                    agreement.append(f'{field}: rust={r_record[field]!r} python={p_record[field]!r}')
        return name, agreement, r_record, p_record, p_argv, p_err


def main():
    cases = [
        ('override valid 0',
         {'version': 1, 'operations': {
             'crap': {'argv': [PY, '-c', 'raise SystemExit(0)'], 'forwardArgs': 'none',
                      'scope': 'whole-tree', 'validResultExitCodes': [0]},
             'mutate': {'argv': [PY, '-c', 'raise SystemExit(3)'], 'forwardArgs': 'append', 'validResultExitCodes': [0, 3]}}},
         {}, 'mutate', ('src/a.lua',)),
        ('override undeclared exit 3',
         {'version': 1, 'operations': {
             'crap': {'argv': [PY, '-c', 'raise SystemExit(3)'], 'forwardArgs': 'append', 'scope': 'targets',
                      'validResultExitCodes': [0]},
             'mutate': {'argv': [PY], 'forwardArgs': 'append', 'validResultExitCodes': [0]}}},
         {}, 'crap', ('src/a.lua',)),
        ('override tool failure',
         {'version': 1, 'operations': {
             'crap': {'argv': [PY, '-c', 'raise SystemExit(1)'], 'forwardArgs': 'none', 'scope': 'whole-tree',
                      'validResultExitCodes': [0]},
             'mutate': {'argv': [PY], 'forwardArgs': 'append', 'validResultExitCodes': [0]}}},
         {}, 'crap', ('src/narrow.lua',)),
        ('override targets crap',
         {'version': 1, 'operations': {
             'crap': {'argv': [PY, '-c', 'raise SystemExit(0)'], 'forwardArgs': 'append', 'scope': 'targets',
                      'validResultExitCodes': [0]},
             'mutate': {'argv': [PY], 'forwardArgs': 'append', 'validResultExitCodes': [0, 3]}}},
         {}, 'crap', ('src/a.lua',)),
        ('missing declaration',
         {'version': 1}, {}, 'mutate', ('src/a.lua',)),
        ('scope override refused',
         {'version': 1, 'standardTools': {'sourceRoots': ['src']}},
         {'src/a.lua': 'return 1\n'}, 'mutate', ('--source-root', 'x')),
        ('changed plus named refused',
         {'version': 1, 'standardTools': {'sourceRoots': ['src']}},
         {'src/a.lua': 'return 1\n'}, 'mutate', ('--changed', 'src/a.lua')),
        ('missing target refused',
         {'version': 1, 'standardTools': {'sourceRoots': ['src']}},
         {'src/a.lua': 'return 1\n'}, 'mutate', ('src/missing.lua',)),
        ('no business targets refused',
         {'version': 1, 'standardTools': {'sourceRoots': ['src']}},
         {'tools/a.lua': 'return 1\n'}, 'mutate', ('tools/a.lua',)),
        ('missing source root',
         {'version': 1, 'standardTools': {'sourceRoots': ['missing']}}, {}, 'crap', ()),
        ('routing option in tool options',
         {'version': 1, 'standardTools': {'mutatorOptions': ['--mutate-all']}}, {}, 'mutate', ()),
        ('alias only rejected',
         {'version': 1, 'aliasResultExitCodes': {'mutate': [0, 3]}}, {}, 'mutate', ('src/a.lua',)),
        ('operations missing dry is standard (python side needs an installed dryer venv)',
         {'version': 1, 'operations': {
             'crap': {'argv': ['w'], 'forwardArgs': 'none', 'scope': 'whole-tree',
                      'validResultExitCodes': [0]},
             'mutate': {'argv': ['w'], 'forwardArgs': 'append', 'validResultExitCodes': [0, 3]}}},
         {'src/a.rs': 'pub fn a() {}\n'}, 'dry', ()),
    ]
    failures = 0
    for name, config, files, operation, args in cases:
        name, differences, r_record, p_record, p_argv, p_err = scenario(
            name, config, files, operation, args)
        status = 'AGREE' if not differences else 'DIFFER'
        if differences and not (
            differences == ['python published no record (refusal before the command ran)']
        ):
            failures += 1
        print(f'{status}: {name}: {differences}')
        print(f'   rust={r_record}')
        print(f'   python={p_record} argv={p_argv}')
        if differences and p_err:
            print(f'   python stderr={p_err.strip()[:200]}')
    print('FAIL' if failures else 'All compared scenarios agree (refusals differ only by design).')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
