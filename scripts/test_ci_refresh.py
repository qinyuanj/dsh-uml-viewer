import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time
import types
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name('ci-refresh.py')
SPEC = importlib.util.spec_from_file_location('ci_refresh', SCRIPT)
ci_refresh = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_refresh)

BASE = 'a' * 40
MERGE = 'b' * 40
IR = 'crates/ir/src/lib.rs'
DRY = 'crates/scene/src/dry.rs'

DIFF = f'''diff --git a/{IR} b/{IR}
index 111..222 100644
--- a/{IR}
+++ b/{IR}
@@ -10,2 +10,2 @@
-old
+new
-old2
+new2
diff --git a/{DRY} b/{DRY}
index 333..444 100644
--- a/{DRY}
+++ b/{DRY}
@@ -4,0 +5 @@ fn changed
+new line
@@ -20 +20,0 @@
-deleted line
'''

DIFF_ASYMMETRIC = f'''diff --git a/{IR} b/{IR}
index 111..222 100644
--- a/{IR}
+++ b/{IR}
@@ -2 +2 @@
-old
+new
'''


def write_targets(root, counts):
    for target, count in counts.items():
        path = root / target
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(''.join(f'fn f{n}() {{}}\n' for n in range(count)),
                        encoding='utf-8')


class RefreshPaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        write_targets(self.root, {IR: 3, DRY: 3})

    def test_changed_lines_use_new_side_of_each_target(self):
        def fake_git(root, *arguments):
            self.assertEqual(root, self.root)
            if arguments[0] == 'merge-base':
                return f'{MERGE}\n'
            self.assertEqual(arguments[0], 'diff')
            self.assertEqual(list(arguments[-2:]), [IR, DRY])
            return DIFF

        with (
            mock.patch.object(ci_refresh, 'git_output', side_effect=fake_git),
            mock.patch.dict(os.environ, {'UML_REFRESH_BASE_SHA': BASE}, clear=True),
        ):
            changed = ci_refresh.changed_lines(self.root, BASE)
        self.assertEqual(changed[IR], {10, 11})
        self.assertEqual(changed[DRY], {5})

    def test_deleting_file_header_with_zero_new_count_parses(self):
        header = '@@ -1,6 +0,0 @@'
        self.assertEqual(ci_refresh.hunk_lines(header), (6, 0, 0))
        diff = (
            f'diff --git a/{IR} b/{IR}\n'
            f'--- a/{IR}\n'
            f'+++ b/{IR}\n'
            f'{header}\n'
            '-//! IR generation (#136)\n'
            '-//!\n'
            '-//! Port of the CLI\n'
            '-//!\n'
            '-//! Public entry is run.\n'
            '-\n'
        )
        self.assertEqual(ci_refresh.parse_diff(diff), {IR: set(), DRY: set()})

    def test_zero_new_start_with_positive_new_count_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'invalid new-side line'):
            ci_refresh.hunk_lines('@@ -1,6 +0,2 @@')

    def test_asymmetric_changes_zero_only_the_unchanged_target(self):
        write_targets(self.root, {IR: 2, DRY: 10})
        line_filter, zero_targets = ci_refresh.per_target_line_filter(
            self.root, {IR: {2}, DRY: set()})
        self.assertEqual(line_filter, f'{IR}:2,{DRY}:11')
        self.assertEqual(zero_targets, {DRY})

    def test_unequal_length_zero_targets_get_their_own_sentinels(self):
        write_targets(self.root, {IR: 2, DRY: 10})
        line_filter, zero_targets = ci_refresh.per_target_line_filter(
            self.root, {IR: set(), DRY: set()})
        self.assertEqual(line_filter, f'{IR}:3,{DRY}:11')
        self.assertEqual(zero_targets, {IR, DRY})

    def test_snapshot_validation_is_per_target(self):
        started = time.time() - 1
        for target, relative in ci_refresh.MUTATE_TARGETS:
            snapshot = self.root / relative
            snapshot.parent.mkdir(parents=True, exist_ok=True)
            verdict = ':killed 1' if target == IR else ''
            snapshot.write_text(f'{{:source "{target}" :outcomes {{{verdict}}}}}',
                                encoding='utf-8')
        ci_refresh.validate_mutation_snapshots(self.root, started, {DRY})
        with self.assertRaises(RuntimeError):
            ci_refresh.validate_mutation_snapshots(self.root, started, {IR, DRY})
        with self.assertRaises(RuntimeError):
            ci_refresh.validate_mutation_snapshots(self.root, started, set())

    def test_mutation_command_keeps_both_targets_and_no_test_override(self):
        coverage = self.root / 'coverage'
        coverage.mkdir()
        (coverage / 'lcov.info').write_text(f'SF:{IR}\n', encoding='utf-8')
        write_targets(self.root, {IR: 8, DRY: 5})
        lines_by_target = {IR: {5312}, DRY: set()}
        with mock.patch.object(ci_refresh, 'changed_lines', return_value=lines_by_target):
            command, zero_targets = ci_refresh.build_mutation_command(
                self.root, self.root / 'uml-viewer.exe', BASE)
        self.assertEqual(zero_targets, {DRY})
        self.assertEqual(command[1:4], ['quality', 'mutate', '--reuse-coverage'])
        self.assertEqual(command[4], '--lines')
        self.assertEqual(command[5], f'{IR}:5312,{DRY}:6')
        self.assertEqual(command[6:], [IR, DRY])
        self.assertNotIn('--test-command', command)


class FailClosed(unittest.TestCase):
    def test_missing_or_malformed_base_sha_fails(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'missing UML_REFRESH_BASE_SHA'):
                ci_refresh.base_commit()
        with mock.patch.dict(os.environ, {'UML_REFRESH_BASE_SHA': 'short'}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'malformed base SHA'):
                ci_refresh.base_commit()

    def test_ambiguous_merge_base_fails(self):
        with mock.patch.object(ci_refresh, 'git_output',
                               return_value=f'{MERGE}\n{"c" * 40}\n'):
            with self.assertRaisesRegex(RuntimeError, 'ambiguous or invalid merge base'):
                ci_refresh.changed_lines(Path('.'), BASE)

    def test_unexpected_diff_path_and_malformed_hunk_fail(self):
        with self.assertRaisesRegex(RuntimeError, 'unexpected diff target'):
            ci_refresh.parse_diff(
                'diff --git a/other.rs b/other.rs\n'
                '--- a/other.rs\n+++ b/other.rs\n@@ -1 +1 @@\n-a\n+b\n')
        with self.assertRaisesRegex(RuntimeError, 'malformed hunk'):
            ci_refresh.parse_diff(
                f'diff --git a/{IR} b/{IR}\n'
                f'--- a/{IR}\n+++ b/{IR}\n@@ nonsense @@\n')


class MainFlow(unittest.TestCase):
    def test_asymmetric_main_publishes_changed_and_zero_site_targets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = root / 'runner-temp' / 'uml-maintainer-refresh'
            state.mkdir(parents=True)
            binary = root / 'uml-viewer.exe'
            binary.write_text('', encoding='utf-8')
            write_targets(root, {IR: 2, DRY: 10})
            coverage = root / 'coverage'
            coverage.mkdir()
            (coverage / 'lcov.info').write_text(f'SF:{IR}\n', encoding='utf-8')

            def record(operation, tool, scope):
                return json.dumps({
                    'version': 1, 'operation': operation, 'projectRoot': str(root),
                    'exitCode': 0, 'validResult': True, 'crapScope': scope,
                    'configured': False, 'tool': tool, 'reason': None,
                }) + '\n'

            (state / 'crap.json').write_text(
                record('crap', 'crapper', 'whole-tree'), encoding='utf-8')
            expected_lines = f'{IR}:2,{DRY}:11'

            def fake_run(command, **_):
                if command[0] == 'git':
                    stdout = f'{MERGE}\n' if command[1] == 'merge-base' else DIFF_ASYMMETRIC
                    return types.SimpleNamespace(returncode=0, stdout=stdout, stderr='')
                self.assertEqual(command[4], '--lines')
                self.assertEqual(command[5], expected_lines)
                self.assertEqual(command[6:], [IR, DRY])
                self.assertNotIn('--test-command', command)
                for target, relative in ci_refresh.MUTATE_TARGETS:
                    snapshot = root / relative
                    snapshot.parent.mkdir(parents=True, exist_ok=True)
                    verdict = ':killed 1' if target == IR else ''
                    snapshot.write_text(
                        f'{{:source "{target}" :outcomes {{{verdict}}}}}',
                        encoding='utf-8')
                return types.SimpleNamespace(
                    returncode=0, stdout=record('mutate', 'mutator', None), stderr='')

            previous = Path.cwd()
            os.chdir(root)
            try:
                with (
                    mock.patch.dict(os.environ, {
                        'RUNNER_TEMP': str(root / 'runner-temp'),
                        'UML_REFRESH_BASE_SHA': BASE,
                    }, clear=True),
                    mock.patch.object(ci_refresh.subprocess, 'run', side_effect=fake_run),
                    mock.patch.object(sys, 'argv',
                                      ['ci-refresh.py', str(binary), 'mutate']),
                ):
                    ci_refresh.main()
            finally:
                os.chdir(previous)
            self.assertIn(':killed 1', (root / '.metrics/mutate/ir.edn').read_text(
                encoding='utf-8'))
            self.assertIn(':outcomes {}',
                          (root / '.metrics/mutate/scene/dry.edn').read_text(
                              encoding='utf-8'))
            self.assertTrue((state / 'mutate.json').is_file())


class WorkflowContract(unittest.TestCase):
    def test_windows_mutate_fetches_the_canonical_base_sha(self):
        workflow = (Path(__file__).resolve().parents[1] /
                    '.github/workflows/ci.yml').read_text(encoding='utf-8')
        job = workflow.split('  windows-mutate:', 1)[1].split('\n  windows-quality:', 1)[0]
        checkout = job.index('- uses: actions/checkout@v4')
        fetch = job.index('name: Fetch refresh base SHA')
        self.assertLess(checkout, fetch)
        self.assertIn('^[0-9a-f]{40}$', job)
        self.assertIn('git fetch --no-tags origin "$UML_REFRESH_BASE_SHA"', job)

    def test_windows_mutate_checkout_has_full_history(self):
        workflow = (Path(__file__).resolve().parents[1] /
                    '.github/workflows/ci.yml').read_text(encoding='utf-8')
        job = workflow.split('  windows-mutate:', 1)[1].split('\n  windows-quality:', 1)[0]
        checkout = job.index('- uses: actions/checkout@v4')
        fetch = job.index('name: Fetch refresh base SHA')
        self.assertLess(checkout, fetch)
        self.assertIn('fetch-depth: 0', job[checkout:fetch])

    def test_windows_mutate_keeps_at_least_a_forty_five_minute_budget(self):
        workflow = (Path(__file__).resolve().parents[1] /
                    '.github/workflows/ci.yml').read_text(encoding='utf-8')
        job = workflow.split('  windows-mutate:', 1)[1].split('\n  windows-quality:', 1)[0]
        budget = re.search(r'^    timeout-minutes:\s*(\d+)$', job, re.MULTILINE)
        self.assertIsNotNone(budget)
        self.assertGreaterEqual(int(budget.group(1)), 45)


if __name__ == '__main__':
    unittest.main()
