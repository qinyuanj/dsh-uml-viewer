#!/usr/bin/env python3
"""Emit real `uml-viewer quality` records for the committed contracts samples.

Usage: python scripts/t15-samples.py <uml-viewer binary> <destination.jsonl>

The records are actual binary output on a scratch project whose name holds a
space and `&`, with the scratch root replaced by `/project`.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

binary = Path(sys.argv[1]).resolve()
destination = Path(sys.argv[2]).resolve()

with tempfile.TemporaryDirectory() as scratch:
    root = Path(scratch) / 'my proj & one'
    (root / 'crates/a/src').mkdir(parents=True)
    (root / 'crates/a/src/lib.rs').write_text('pub fn a(x: i64) -> i64 { x + 1 }\n')

    def record(operation, *args):
        result = subprocess.run([str(binary), 'quality', operation, '--root', str(root), *args],
                                cwd=scratch, capture_output=True, text=True, check=False)
        return json.loads(result.stdout)

    (root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
        'sourceRoots': ['crates'], 'crapperOptions': ['--no-coverage']}}))
    # No target: refused identically on every platform.
    records = [record('crap'), record('dry'), record('mutate')]
    (root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'operations': {
        'crap': {'argv': [sys.executable, '-c', 'raise SystemExit(1)'], 'forwardArgs': 'none',
                 'scope': 'whole-tree', 'validResultExitCodes': [0]},
        'mutate': {'argv': [sys.executable, '-c', 'raise SystemExit(3)'], 'forwardArgs': 'append',
                   'validResultExitCodes': [0, 3]}}}))
    records += [record('crap'), record('mutate', 'x.rs')]
    (root / 'uml-quality.json').write_text('{"version":1}')
    records.append(record('crap'))

for item in records:
    item['projectRoot'] = item['projectRoot'].replace(str(root.resolve()), '/project')
    assert item['projectRoot'] == '/project', item
lines = [json.dumps(item, sort_keys=True, separators=(',', ':')) for item in records]
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_bytes(('\n'.join(lines) + '\n').encode())
print('\n'.join(lines))
