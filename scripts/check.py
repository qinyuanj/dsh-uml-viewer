#!/usr/bin/env python3
"""One guardrail command: the checks that are safe to run at any time."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys

REPO = Path(__file__).resolve().parents[1]
FAST_SUITES = ['test_process_boundary.py', 'test_line_endings.py',
              'test_mutate_goldens_help.py', 'test_ci_changes.py']


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
    code, output = run([cargo, 'build', '--release', '--locked'])
    if code != 0:
        return 'failed', code, output
    code, output = run([cargo, 'test', '--workspace', '--locked'])
    return ('executed' if code == 0 else 'failed'), code, output


def python_suite(name):
    code, output = run([sys.executable, '-I', str(REPO / 'scripts' / name)])
    return ('executed' if code == 0 else 'failed'), code, output


def python_checks(names):
    return [(f'{name} (python -I scripts/{name})', lambda name=name: python_suite(name))
            for name in names]


def selected_checks(fast=False):
    checks = python_checks(FAST_SUITES)
    if not fast:
        checks += [('cargo build --release --locked + cargo test --workspace --locked',
                   cargo_suite)]
    return checks


def run_checks(checks):
    results = []
    for label, check in checks:
        status, code, output = check()
        results.append((label, status, code, output))
        if status == 'failed':
            break
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--fast', action='store_true',
                        help='run only the fast guard suites, not the release build '
                             'and the workspace suite')
    arguments = parser.parse_args()
    return summarize(run_checks(selected_checks(fast=arguments.fast)))


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
