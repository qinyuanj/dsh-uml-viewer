#!/usr/bin/env python3
"""One guardrail command: the checks that are safe to run at any time.

This is the only guardrail command; the earlier `check.ps1` was retired with
Windows native support (ADR 0007). The cargo check builds the release binary
used by the entry test files (which skip without it), then runs the workspace
tests. Stops at the first failure so the printed
label names the actual cause. The quality gate (`./uml crap` / `./uml mutate`) is
deliberately absent: it needs serialized, declared-scope runs against a real
project, so it cannot be a default check. Real-window drivers need the explicit
`--native` opt-in.

Usage:
    python3 -I scripts/check.py            # pure checks only
    python3 -I scripts/check.py --native   # plus every real-window driver
"""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys

REPO = Path(__file__).resolve().parents[1]


def interpreter(command):
    return shutil.which(command)


def run(argv, env=None, timeout=1800):
    result = subprocess.run([str(value) for value in argv], cwd=REPO, capture_output=True,
                            text=True, env=env, timeout=timeout)
    return result.returncode, (result.stdout or '') + (result.stderr or '')


def cargo_suite():
    cargo = interpreter('cargo')
    if not cargo:
        return 'failed', 1, 'cargo is required (the workspace tests)'
    # The entry suites exercise target/release/uml-viewer and skip when it is
    # absent. Build it first so this guardrail runs them for real instead of
    # silently passing with skips.
    code, output = run([cargo, 'build', '--release', '--locked'])
    if code != 0:
        return 'failed', code, output
    code, output = run([cargo, 'test', '--workspace', '--locked'])
    return ('executed' if code == 0 else 'failed'), code, output


def entries():
    python = interpreter('python3')
    if not python:
        return 'failed', 1, 'python3 is required by the installer and this suite'
    code, output = run([python, '-I', str(REPO / 'scripts/test_posix_entries.py')])
    return ('executed' if code == 0 else 'failed'), code, output


def restart_entries():
    python = interpreter('python3')
    if not python:
        return 'failed', 1, 'python3 is required'
    code, output = run([python, '-I', str(REPO / 'scripts/test_posix_restart_entries.py')])
    return ('executed' if code == 0 else 'failed'), code, output


def codex_companion():
    python = interpreter('python3')
    if not python:
        return 'failed', 1, 'python3 is required'
    code, output = run([python, '-I', str(REPO / 'scripts/test_posix_codex_companion.py')])
    return ('executed' if code == 0 else 'failed'), code, output


def native_driver(name, timeout):
    python = interpreter('python3')
    if not python:
        return 'skipped', 0, 'python3 not on PATH'
    code, output = run([python, '-I', str(REPO / 'scripts/run_viewer_driver.py'), name], timeout=timeout)
    if code == 0 and output.strip().startswith('SKIP'):
        # A driver whose precondition is absent (e.g. a font the installer has
        # not provisioned) verified nothing; it must not read as a pass.
        return 'skipped', code, output.strip()
    return ('executed' if code == 0 else 'failed'), code, output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--native', action='store_true', help='also run the real-window drivers')
    parser.add_argument('--native-timeout', type=int, default=300)
    arguments = parser.parse_args()

    checks = [
        ('cargo build --release --locked + cargo test --workspace --locked', cargo_suite),
        ('entries (python -I scripts/test_posix_entries.py)', entries),
        ('restart (python -I scripts/test_posix_restart_entries.py)', restart_entries),
        ('codex companion (python -I scripts/test_posix_codex_companion.py)', codex_companion),
    ]
    if arguments.native:
        sys.path.insert(0, str(REPO / 'scripts'))
        from run_viewer_driver import DRIVERS
        for name in sorted(DRIVERS):
            checks.append((f'native {name}',
                           lambda name=name: native_driver(name, arguments.native_timeout)))

    results = []
    for label, check in checks:
        status, code, output = check()
        results.append((label, status, code, output))
        if status == 'failed':
            break

    counts = {'executed': 0, 'skipped': 0, 'failed': 0}
    for label, status, code, output in results:
        counts[status] = counts.get(status, 0) + 1
        prefix = {'executed': 'PASS', 'skipped': 'SKIP', 'failed': 'FAIL'}[status]
        print(f'{prefix} {label} (exit {code})')
        if status == 'skipped':
            print(f'     {output.strip()}')
        elif status == 'failed' and output:
            print(output.rstrip())
    print('note: ./uml crap and ./uml mutate are not run here; the quality gate needs '
          'serialized, declared-scope runs.')
    print(f"check: {counts['executed']} passed, {counts['skipped']} skipped, {counts['failed']} failed")
    return 1 if counts['failed'] else 0


if __name__ == '__main__':
    sys.exit(main())
