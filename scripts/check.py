#!/usr/bin/env python3
"""One guardrail command: the checks that are safe to run at any time.

This is the only guardrail command on every host, native Windows included
(ADR 0010; the earlier `check.ps1` stays retired). Python suites run with the
interpreter that runs this file. The cargo check builds the release binary
the real-window drivers run, then runs the workspace tests. Stops at the
first failure so the printed label names the actual cause. The quality
refresh (`uml-viewer quality crap|mutate|dry`, then `uml-viewer ir`) is
deliberately absent: it needs serialized, declared-scope runs against a real
project, so it cannot be a default check. Real-window drivers need the
explicit `--native` opt-in. The old public entry and its Python suites were
deleted with the companion (#18).

Usage:
    python3 -I scripts/check.py            # pure checks only
    python3 -I scripts/check.py --native   # plus every real-window driver
    py -3 -I scripts\\check.py              # native Windows (no python3 there)
"""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys

REPO = Path(__file__).resolve().parents[1]
# Static source rules: seconds, no build, so they run first.
FAST_SUITES = ['test_process_boundary.py', 'test_line_endings.py']


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
    # The real-window drivers run target/release/uml-viewer; build it here so
    # `--native` never runs a stale binary.
    code, output = run([cargo, 'build', '--release', '--locked'])
    if code != 0:
        return 'failed', code, output
    code, output = run([cargo, 'test', '--workspace', '--locked'])
    return ('executed' if code == 0 else 'failed'), code, output


def python_suite(name):
    """A Python suite under this interpreter (Windows has no `python3`)."""
    code, output = run([sys.executable, '-I', str(REPO / 'scripts' / name)])
    return ('executed' if code == 0 else 'failed'), code, output


def native_driver(name, timeout):
    code, output = run([sys.executable, '-I', str(REPO / 'scripts/run_viewer_driver.py'), name],
                       timeout=timeout)
    if code == 0 and output.strip().startswith('SKIP'):
        # A driver whose precondition is absent (e.g. a font the installer has
        # not provisioned) verified nothing; it must not read as a pass.
        return 'skipped', code, output.strip()
    return ('executed' if code == 0 else 'failed'), code, output


def python_checks(names):
    return [(f'{name} (python -I scripts/{name})', lambda name=name: python_suite(name))
            for name in names]


def native_checks(timeout):
    sys.path.insert(0, str(REPO / 'scripts'))
    from run_viewer_driver import DRIVERS
    return [(f'native {name}', lambda name=name: native_driver(name, timeout))
            for name in sorted(DRIVERS)]


def selected_checks(native, native_timeout):
    """Fast source rules, the cargo build and tests, then (opt-in) the drivers."""
    checks = python_checks(FAST_SUITES)
    checks += [('cargo build --release --locked + cargo test --workspace --locked', cargo_suite)]
    if native:
        checks += native_checks(native_timeout)
    return checks


def run_checks(checks):
    """Each check in order, stopping after the first failure."""
    results = []
    for label, check in checks:
        status, code, output = check()
        results.append((label, status, code, output))
        if status == 'failed':
            break
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--native', action='store_true', help='also run the real-window drivers')
    parser.add_argument('--native-timeout', type=int, default=300)
    arguments = parser.parse_args()
    return summarize(run_checks(selected_checks(arguments.native, arguments.native_timeout)))


def summarize(results):
    counts = {'executed': 0, 'skipped': 0, 'failed': 0}
    for label, status, code, output in results:
        counts[status] = counts.get(status, 0) + 1
        prefix = {'executed': 'PASS', 'skipped': 'SKIP', 'failed': 'FAIL'}[status]
        print(f'{prefix} {label} (exit {code})')
        if status == 'skipped':
            print(f'     {output.strip()}')
        elif status == 'failed' and output:
            print(output.rstrip())
    print('note: uml-viewer quality crap|mutate|dry are not run here; the quality refresh '
          'needs serialized, declared-scope runs.')
    print(f"check: {counts['executed']} passed, {counts['skipped']} skipped, {counts['failed']} failed")
    return 1 if counts['failed'] else 0


if __name__ == '__main__':
    sys.exit(main())
