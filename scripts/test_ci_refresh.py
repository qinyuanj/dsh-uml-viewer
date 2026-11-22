"""Focused tests for the CI-only diff-limited maintainer refresh."""
import importlib.util
import json
import os
from pathlib import Path
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

DIFF = '''diff --git a/crates/ir/src/lib.rs b/crates/ir/src/lib.rs
index 111..222 100644
--- a/crates/ir/src/lib.rs
+++ b/crates/ir/src/lib.rs
@@ -10,2 +10,2 @@
-old
+new
-old2
+new2
diff --git a/crates/scene/src/dry.rs b/crates/scene/src/dry.rs
index 333..444 100644
--- a/crates/scene/src/dry.rs
+++ b/crates/scene/src/dry.rs
@@ -4,0 +5 @@ fn changed
+new line
@@ -20 +20,0 @@
-deleted line
'''


class RefreshPaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        for target, _ in ci_refresh.MUTATE_TARGETS:
            path = self.root / target
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('fn a() {}\nfn b() {}\nfn c() {}\n', encoding='utf-8')

    def test_changed_lines_use_new_side_of_each_target(self):
        def fake_git(root, *arguments):
            self.assertEqual(root, self.root)
            if arguments[0] == 'merge-base':
                return f'{MERGE}\n'
            self.assertEqual(arguments[0], 'diff')
            self.assertEqual(list(arguments[-2:]),
                             [target for target, _ in ci_refresh.MUTATE_TARGETS])
            return DIFF

        with mock.patch.object(ci_refresh, 'git_output', side_effect=fake_git),                 mock.patch.dict(os.environ, {'UML_REFRESH_BASE_SHA': BASE}, clear=True):
            changed = ci_refresh.changed_lines(self.root, BASE)
        self.assertEqual(changed['crates/ir/src/lib.rs'], {10, 11})
        self.assertEqual(changed['crates/scene/src/dry.rs'], {5})

    def test_zero_site_filter_uses_beyond_eof_sentinels(self):
        empty = {target: set() for target, _ in ci_refresh.MUTATE_TARGETS}
        line_filter, zero_site = ci_refresh.mutation_line_filter(self.root, empty)
        self.assertTrue(zero_site)
        self.assertEqual(line_filter, '4')

    def test_zero_site_snapshots_require_empty_or_uncovered_outcomes(self):
        started = time.time() - 1
        for target, relative in ci_refresh.MUTATE_TARGETS:
            snapshot = self.root / relative
            snapshot.parent.mkdir(parents=True, exist_ok=True)
            snapshot.write_text(
                f'{{:source "{target}" :outcomes {{}}}}', encoding='utf-8')
        ci_refresh.validate_mutation_snapshots(self.root, started, zero_site=True)
        with self.assertRaises(RuntimeError):
            ci_refresh.validate_mutation_snapshots(self.root, started, zero_site=False)

    def test_mutation_command_keeps_both_targets_and_no_test_override(self):
        coverage = self.root / 'coverage'
        coverage.mkdir()
        (coverage / 'lcov.info').write_text('SF:crates/ir/src/lib.rs\n', encoding='utf-8')
        lines_by_target = {
            'crates/ir/src/lib.rs': {5312},
            'crates/scene/src/dry.rs': set(),
        }
        with mock.patch.object(ci_refresh, 'changed_lines', return_value=lines_by_target):
            command, zero_site = ci_refresh.build_mutation_command(
                self.root, self.root / 'uml-viewer.exe', BASE)
        self.assertFalse(zero_site)
        self.assertEqual(command[1:4], ['quality', 'mutate', '--reuse-coverage'])
        self.assertEqual(command[4:6], ['--lines', '5312'])
        self.assertEqual(command[6:], [target for target, _ in ci_refresh.MUTATE_TARGETS])
        self.assertNotIn('--test-command', command)


class FailClosed(unittest.TestCase):
    def test_missing_or_conflicting_base_sha_fails(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'missing base SHA'):
                ci_refresh.base_commit()
        with mock.patch.dict(os.environ, {'BASE_SHA': BASE, 'PR_BASE_SHA': 'c' * 40},
                             clear=True):
            with self.assertRaisesRegex(RuntimeError, 'conflicting base SHAs'):
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
                'diff --git a/crates/ir/src/lib.rs b/crates/ir/src/lib.rs\n'
                '--- a/crates/ir/src/lib.rs\n+++ b/crates/ir/src/lib.rs\n'
                '@@ nonsense @@\n')


class MainFlow(unittest.TestCase):
    def test_zero_site_main_republishes_fresh_snapshots(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = root / 'runner-temp' / 'uml-maintainer-refresh'
            state.mkdir(parents=True)
            binary = root / 'uml-viewer.exe'
            binary.write_text('', encoding='utf-8')
            for target, _ in ci_refresh.MUTATE_TARGETS:
                path = root / target
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('fn a() {}\n', encoding='utf-8')
            coverage = root / 'coverage'
            coverage.mkdir()
            (coverage / 'lcov.info').write_text('SF:crates/ir/src/lib.rs\n', encoding='utf-8')

            def record(operation, tool, scope):
                return json.dumps({
                    'version': 1, 'operation': operation, 'projectRoot': str(root),
                    'exitCode': 0, 'validResult': True, 'crapScope': scope,
                    'configured': False, 'tool': tool, 'reason': None,
                }) + '\n'

            (state / 'crap.json').write_text(
                record('crap', 'crapper', 'whole-tree'), encoding='utf-8')

            def fake_run(command, **_):
                if command[0] == 'git':
                    stdout = f'{MERGE}\n' if command[1] == 'merge-base' else ''
                    return types.SimpleNamespace(returncode=0, stdout=stdout, stderr='')
                self.assertEqual(command[4:6], ['--lines', '2'])
                self.assertNotIn('--test-command', command)
                self.assertEqual(command[6:],
                                 [target for target, _ in ci_refresh.MUTATE_TARGETS])
                for target, relative in ci_refresh.MUTATE_TARGETS:
                    snapshot = root / relative
                    snapshot.parent.mkdir(parents=True, exist_ok=True)
                    snapshot.write_text(
                        f'{{:source "{target}" :outcomes {{}}}}', encoding='utf-8')
                return types.SimpleNamespace(
                    returncode=0, stdout=record('mutate', 'mutator', None), stderr='')

            previous = Path.cwd()
            os.chdir(root)
            try:
                with mock.patch.dict(os.environ, {'RUNNER_TEMP': str(root / 'runner-temp'), 'UML_REFRESH_BASE_SHA': BASE}, clear=True), mock.patch.object(ci_refresh.subprocess, 'run', side_effect=fake_run), mock.patch.object(sys, 'argv', ['ci-refresh.py', str(binary), 'mutate']):
                    ci_refresh.main()
            finally:
                os.chdir(previous)
            for _, relative in ci_refresh.MUTATE_TARGETS:
                self.assertIn(':outcomes {}', (root / relative).read_text(encoding='utf-8'))
            self.assertTrue((state / 'mutate.json').is_file())


if __name__ == '__main__':
    unittest.main()
