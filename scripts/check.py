#!/usr/bin/env python3
"""One guardrail command: the checks that are safe to run at any time."""
import argparse
import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys

REPO = Path(__file__).resolve().parents[1]
FAST_SUITES = ['test_process_boundary.py', 'test_line_endings.py',
              'test_mutate_goldens_help.py', 'test_ci_changes.py',
              'test_ci_refresh.py']


def load_graph_solver():
    spec = importlib.util.spec_from_file_location('ci_changes', REPO / 'scripts' / 'ci-changes.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def interpreter(command):
    return shutil.which(command)


def run(argv, env=None, timeout=1800):
    result = subprocess.run([str(value) for value in argv], cwd=REPO, capture_output=True,
                            text=True, env=env, timeout=timeout)
    return result.returncode, (result.stdout or '') + (result.stderr or '')


def run_rust_tests(cargo, crates=None, features=None, test_target=None, include_docs=True):
    """Run Rust tests via cargo-nextest (honoring .config/nextest.toml) + doctests."""
    has_nextest = bool(interpreter('cargo-nextest'))
    pkg_args = []
    if crates is None:
        pkg_args = ['--workspace']
    else:
        for crate in crates:
            pkg_args.extend(['-p', crate])

    feature_args = ['--features', features] if features else []
    target_args = ['--test', test_target] if test_target else []

    if has_nextest:
        argv = [cargo, 'nextest', 'run', '--locked', *pkg_args, *feature_args, *target_args]
    else:
        argv = [cargo, 'test', '--locked', *pkg_args, *feature_args, *target_args]
    code, output = run(argv)
    if code != 0:
        return code, output
    if include_docs and not test_target:
        doc_code, doc_out = run([cargo, 'test', '--doc', '--locked', *pkg_args])
        return doc_code, output + doc_out
    return 0, output


def cargo_suite():
    cargo = interpreter('cargo')
    if not cargo:
        return 'failed', 1, 'cargo is required (the workspace tests)'
    code, output = run([cargo, 'build', '--release', '--locked'])
    if code != 0:
        return 'failed', code, output
    code, output = run_rust_tests(cargo, crates=None, include_docs=True)
    return ('executed' if code == 0 else 'failed'), code, output


def affected_cargo_suite(crates, build_release=False):
    cargo = interpreter('cargo')
    if not cargo:
        return 'failed', 1, 'cargo is required (the affected workspace tests)'
    if build_release:
        code, output = run([cargo, 'build', '--release', '--locked'])
        if code != 0:
            return 'failed', code, output
    code, output = run_rust_tests(cargo, crates=crates, include_docs=True)
    return ('executed' if code == 0 else 'failed'), code, output


def toolchain_contract_suite():
    cargo = interpreter('cargo')
    if not cargo:
        return 'failed', 1, 'cargo is required (the toolchain contract suite)'
    code, output = run_rust_tests(
        cargo,
        crates=['mutator', 'uml-viewer'],
        features='toolchain-contract',
        test_target='toolchain_contract',
        include_docs=False,
    )
    return ('executed' if code == 0 else 'failed'), code, output


def python_suite(name):
    code, output = run([sys.executable, '-I', str(REPO / 'scripts' / name)])
    return ('executed' if code == 0 else 'failed'), code, output


def python_checks(names):
    return [(f'{name} (python -I scripts/{name})', lambda name=name: python_suite(name))
            for name in names]


def detect_local_changes(base=None):
    """Collect changed paths against base (or working tree + untracked files)."""
    paths = set()
    diff_cmd = ['git', '-C', str(REPO), 'diff', '--name-only', '-z']
    if base:
        diff_cmd.append(f'{base}...HEAD')
    else:
        diff_cmd.append('HEAD')
    completed = subprocess.run(diff_cmd, capture_output=True, text=True)
    if completed.returncode == 0:
        paths.update(p for p in completed.stdout.split('\0') if p)
    if not base:
        untracked = subprocess.run(
            ['git', '-C', str(REPO), 'ls-files', '--others', '--exclude-standard', '-z'],
            capture_output=True, text=True,
        )
        if untracked.returncode == 0:
            paths.update(p for p in untracked.stdout.split('\0') if p)
    return sorted(paths)


def selected_checks(fast=False, affected=False, base=None, toolchain_contract=False,
                    files_override=None):
    checks = python_checks(FAST_SUITES)
    if toolchain_contract:
        checks.append(('toolchain contract suite (--features toolchain-contract)',
                       toolchain_contract_suite))
        return checks
    if fast:
        return checks
    if affected:
        changed_files = (list(files_override) if files_override is not None
                         else detect_local_changes(base=base))
        if not changed_files:
            checks.append(('affected check (no local changes detected)',
                           lambda: ('skipped', 0, 'working tree clean; nothing affected')))
            return checks
        solver = load_graph_solver()
        plan = solver.analyze_changes(changed_files, repository=REPO)
        if plan['docs_only']:
            checks.append(('affected check (docs-only change)',
                           lambda: ('skipped', 0, 'pure documentation change; Rust suite skipped')))
            return checks
        if plan['full']:
            checks.append(('affected check -> full workspace hermetic suite', cargo_suite))
            return checks
        if plan['rust_crates']:
            crates = plan['rust_crates']
            label = f"affected Rust hermetic suite ({', '.join(crates)})"
            checks.append((label, lambda crates=crates: affected_cargo_suite(crates)))
        else:
            checks.append(('affected Rust hermetic suite',
                           lambda: ('skipped', 0, 'no Rust crates affected')))
        return checks
    checks += [('cargo build --release --locked + hermetic workspace suite', cargo_suite)]
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
    parser.add_argument('--affected', action='store_true',
                        help='prune Rust tests to the affected rdeps closure from '
                             'scripts/ci-changes.py')
    parser.add_argument('--base', default=None,
                        help='git ref to diff against HEAD when using --affected')
    parser.add_argument('--toolchain-contract', action='store_true',
                        help='run the opt-in external toolchain contract suite')
    arguments = parser.parse_args()
    return summarize(run_checks(selected_checks(
        fast=arguments.fast,
        affected=arguments.affected,
        base=arguments.base,
        toolchain_contract=arguments.toolchain_contract,
    )))


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
