#!/usr/bin/env python3
"""One guardrail command: the checks that are safe to run at any time.

This is the only guardrail command on every host, native Windows included
(ADR 0010; the earlier `check.ps1` stays retired). Python suites run with the
interpreter that runs this file. The cargo check builds the release binary
used by the entry test files (which skip without it), then runs the workspace
tests. Stops at the first failure so the printed
label names the actual cause. The quality gate (`./uml crap` / `./uml mutate`) is
deliberately absent: it needs serialized, declared-scope runs against a real
project, so it cannot be a default check. Real-window drivers need the explicit
`--native` opt-in.

Usage:
    python3 -I scripts/check.py            # pure checks only
    python3 -I scripts/check.py --native   # plus every real-window driver
    py -3 -I scripts\\check.py              # native Windows (no python3 there)
"""
import argparse
import os
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
    # The entry suites exercise target/release/uml-viewer and skip when it is
    # absent. Build it first so this guardrail runs them for real instead of
    # silently passing with skips.
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


def host_refusal():
    """Why this host is not a delivered platform (uml_posix.host), else None."""
    sys.path.insert(0, str(REPO / 'scripts'))
    import uml_posix
    try:
        uml_posix.host()
    except ValueError as refusal:
        return str(refusal)
    return None


def skipped(reason):
    return lambda: ('skipped', 0, reason)


def selected_checks(native, native_timeout, refusal=None):
    """Fast source rules, the cargo build and tests, then the entry suites.

    The entry suites drive the public entry, which refuses an undelivered host
    (e.g. native Linux on a CI runner, ADR 0007); there they are listed as
    skipped with that reason instead of failing on the refusal.
    """
    suites = ['test_posix_entries.py', 'test_posix_restart_entries.py']
    if os.name == 'nt':
        # `uml.cmd`, junction dev installs and `.exe` naming.
        suites.insert(1, 'test_windows_entries.py')
    else:
        # POSIX by construction (/bin/sh fakes, ':' PATH, killpg, AF_UNIX);
        # the Windows Codex preflight is covered by the entry suites and
        # test_windows_e2e.py.
        suites.append('test_posix_codex_companion.py')
    checks = python_checks(FAST_SUITES)
    checks += [('cargo build --release --locked + cargo test --workspace --locked', cargo_suite)]
    if refusal:
        checks += [(f'{name} (undelivered host)', skipped(refusal)) for name in suites]
    else:
        checks += python_checks(suites)
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
    return summarize(run_checks(selected_checks(arguments.native, arguments.native_timeout,
                                                host_refusal())))


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
    print('note: ./uml crap and ./uml mutate are not run here; the quality gate needs '
          'serialized, declared-scope runs.')
    print(f"check: {counts['executed']} passed, {counts['skipped']} skipped, {counts['failed']} failed")
    return 1 if counts['failed'] else 0


if __name__ == '__main__':
    sys.exit(main())
