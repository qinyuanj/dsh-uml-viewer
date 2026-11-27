from pathlib import Path
import importlib.util
import re
import subprocess
import sys
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / 'scripts' / 'ci-changes.py'
WORKFLOWS = REPO / '.github' / 'workflows'
REFERENCE = re.compile(r'needs[.]changes[.]outputs[.]([a-z0-9_]+)')
DOC = 'docs/ci-routing.md'
BASE = 'a' * 40
HEAD = 'b' * 40


def classifier():
    spec = importlib.util.spec_from_file_location('ci_changes', SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def flags_for(arguments, cwd=None):
    """Run the CLI over a fresh output file and return its flags."""
    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / 'output.txt'
        completed = subprocess.run([sys.executable, '-I', str(SCRIPT), *arguments,
                                    '--output', str(output)],
                                   capture_output=True, text=True, cwd=cwd or REPO)
        if completed.returncode != 0:
            raise AssertionError('ci-changes.py failed: ' + completed.stderr + completed.stdout)
        return dict(line.split('=', 1) for line in
                    output.read_text(encoding='utf-8').splitlines() if line)


def routed(files, event='pull_request', ref='refs/pull/7/merge'):
    """The flags for an explicit changed-file list."""
    with tempfile.TemporaryDirectory() as directory:
        listing = Path(directory) / 'changed.txt'
        listing.write_text(''.join(path + '\n' for path in files), encoding='utf-8')
        return flags_for(['--files-from', str(listing), '--event', event, '--ref', ref])


def lanes(files, **options):
    return {key for key, value in routed(files, **options).items() if value == 'true'}


def job_block(text, name):
    lines = text.split('\n')
    start = lines.index('  ' + name + ':')
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if lines[index].startswith('  ') and not lines[index].startswith('   '):
            end = index
            break
    return '\n'.join(lines[start:end])


class PresubmitAndPostsubmitRouting(unittest.TestCase):
    def test_a_pure_documentation_change_runs_no_windows_lane(self):
        self.assertEqual({'docs_only'}, lanes([DOC, 'GLOSSARY.md',
                                               'acceptance-evidence/106-wsl/log.md']))

    def test_the_prompt_section_routes_the_plugin_and_panel_lanes(self):
        self.assertEqual({'fast', 'typescript', 'panel_e2e', 'mutate_refresh', 'release'},
                         lanes(['docs/companion-rules.md']))

    def test_a_bundled_document_routes_the_package_and_upgrade_lanes(self):
        for document in ('docs/plugin-development.md', 'README.md'):
            with self.subTest(document=document):
                self.assertEqual({'fast', 'typescript', 'panel_e2e', 'upgrade_e2e', 'release'},
                                 lanes([document]))

    def test_a_typescript_change_skips_rust_and_external_toolchain_lanes(self):
        self.assertEqual({'fast', 'typescript', 'panel_e2e', 'mutate_refresh', 'release'},
                         lanes(['packages/host/src/prompt.ts']))

    def test_a_viewer_runtime_change_skips_heavy_toolchain_contracts_in_presubmit(self):
        self.assertEqual({'fast', 'rust_hermetic', 'panel_e2e', 'mutate_refresh', 'release'},
                         lanes(['crates/uml-viewer/src/main.rs']))

    def test_a_standalone_quality_tool_change_skips_panel_e2e(self):
        self.assertEqual({'fast', 'rust_hermetic', 'mutate_refresh',
                          'toolchain_crapper', 'release'},
                         lanes(['crates/crapper/src/lib.rs']))

    def test_a_mutator_adapter_change_triggers_toolchain_mutator_directly(self):
        self.assertEqual({'fast', 'rust_hermetic', 'mutate_refresh',
                          'toolchain_mutator', 'release'},
                         lanes(['crates/mutator/src/golang.rs']))

    def test_a_mutator_non_adapter_change_skips_panel_e2e_and_crapper_contract(self):
        self.assertEqual({'fast', 'rust_hermetic', 'mutate_refresh', 'release'},
                         lanes(['crates/mutator/src/metrics.rs']))

    def test_a_packaging_script_routes_panel_and_upgrade_e2e(self):
        self.assertEqual({'fast', 'typescript', 'panel_e2e', 'upgrade_e2e', 'release'},
                         lanes(['scripts/package.mjs']))

    def test_real_snapshot_metrics_and_policy_route_panel_e2e(self):
        for path in ('.metrics/crap.edn', 'examples/uml-viewer.policy.edn'):
            with self.subTest(path=path):
                self.assertIn('panel_e2e', lanes([path]))
                self.assertIn('rust_hermetic', lanes([path]))

    def test_post_submit_main_push_promotes_toolchain_contracts_and_upgrade_e2e(self):
        main_lanes = lanes(['crates/uml-viewer/src/main.rs'],
                           event='push', ref='refs/heads/main')
        self.assertIn('toolchain_crapper', main_lanes)
        self.assertIn('toolchain_mutator', main_lanes)
        self.assertIn('upgrade_e2e', main_lanes)

    def test_post_submit_main_docs_only_still_skips_windows_lanes(self):
        self.assertEqual({'docs_only'}, lanes([DOC], event='push', ref='refs/heads/main'))


class DependencyGraphProperties(unittest.TestCase):
    def test_every_tracked_file_in_git_ls_files_is_classified(self):
        module = classifier()
        completed = subprocess.run(['git', '-C', str(REPO), 'ls-files', '-z'],
                                   capture_output=True, text=True, check=True)
        tracked = [p for p in completed.stdout.split('\0') if p]
        self.assertTrue(tracked)
        unknown = [p for p in tracked if module.classify_node(p) == 'unknown']
        self.assertEqual([], unknown, f'unmapped tracked paths: {unknown[:10]}')

    def test_rdeps_closure_is_monotonic(self):
        module = classifier()
        a = ['crates/edn/src/lib.rs']
        b = ['packages/contracts/src/index.ts']
        plan_a = module.analyze_changes(a, repository=REPO)
        plan_b = module.analyze_changes(b, repository=REPO)
        plan_ab = module.analyze_changes(a + b, repository=REPO)
        self.assertTrue(set(plan_a['rust_crates']) <= set(plan_ab['rust_crates']))
        self.assertTrue(set(plan_b['pnpm_packages']) <= set(plan_ab['pnpm_packages']))
        self.assertIn('uml-viewer', plan_a['rust_crates'])
        self.assertIn('host', plan_b['pnpm_packages'])
        self.assertIn('client', plan_b['pnpm_packages'])
        self.assertTrue(plan_b['lanes']['panel_e2e'])

    def test_leaf_crate_edit_prunes_unrelated_crates(self):
        module = classifier()
        plan = module.analyze_changes(['crates/dryer/src/lib.rs'], repository=REPO)
        self.assertIn('dryer', plan['rust_crates'])
        self.assertIn('uml-viewer', plan['rust_crates'])
        self.assertNotIn('edn', plan['rust_crates'])
        self.assertNotIn('scene', plan['rust_crates'])
        self.assertFalse(plan['lanes']['panel_e2e'])


class FullMatrixFallback(unittest.TestCase):
    def test_a_workflow_or_classifier_edit_forces_the_whole_matrix(self):
        for path in ('.github/workflows/ci.yml', '.github/workflows/package-release.yml',
                     'scripts/ci-changes.py', 'scripts/test_ci_changes.py'):
            with self.subTest(path=path):
                self.assertEqual('true', routed([path])['full'])

    def test_a_manual_or_scheduled_run_forces_the_whole_matrix(self):
        for event in ('workflow_dispatch', 'schedule'):
            with self.subTest(event=event):
                flags = routed([DOC], event=event, ref='refs/heads/main')
                self.assertEqual('true', flags['full'])
                self.assertEqual('true', flags['release'])
                self.assertEqual('false', flags['docs_only'])

    def test_a_release_tag_forces_the_whole_matrix(self):
        flags = routed([DOC], event='push', ref='refs/tags/v1.2.3')
        self.assertEqual('true', flags['full'])
        self.assertEqual('true', flags['panel_e2e'])

    def test_an_unmapped_path_falls_back_to_the_whole_matrix(self):
        self.assertEqual('true', routed(['tools/new-tool.ps1'])['full'])

    def test_an_empty_change_set_falls_back_to_the_whole_matrix(self):
        self.assertEqual('true', routed([])['full'])

    def test_a_missing_or_unresolvable_base_commit_forces_the_whole_matrix(self):
        for base in ('', 'not-a-sha', '0' * 40):
            with self.subTest(base=base):
                flags = flags_for(['--event', 'pull_request', '--ref', 'refs/pull/7/merge',
                                   '--base', base, '--head', HEAD])
                self.assertEqual('true', flags['full'])
                self.assertEqual('true', flags['toolchain_mutator'])


class GitRangeRouting(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.git('init', '--quiet')
        self.git('config', 'user.email', 'ci@example.invalid')
        self.git('config', 'user.name', 'ci')

    def git(self, *arguments):
        return subprocess.run(['git', '-C', str(self.root), *arguments],
                              capture_output=True, text=True, check=True).stdout.strip()

    def commit(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding='utf-8')
        self.git('add', path)
        self.git('commit', '--quiet', '-m', 'add ' + path)
        return self.git('rev-parse', 'HEAD')

    def range_flags(self, base, head):
        return flags_for(['--event', 'pull_request', '--ref', 'refs/pull/7/merge',
                          '--base', base, '--head', head,
                          '--repository', str(self.root)])

    def test_a_real_range_routes_the_committed_files(self):
        base = self.commit(DOC, '# routing\n')
        head = self.commit('crates/crapper/src/lib.rs', 'fn f() {}\n')
        flags = self.range_flags(base, head)
        self.assertEqual('false', flags['docs_only'])
        self.assertEqual('true', flags['rust_hermetic'])
        self.assertEqual('true', flags['toolchain_crapper'])
        self.assertEqual('false', flags['typescript'])

    def test_a_documentation_only_range_skips_the_windows_lanes(self):
        base = self.commit('crates/crapper/src/lib.rs', 'fn f() {}\n')
        head = self.commit('GLOSSARY.md', '# glossary\n')
        flags = self.range_flags(base, head)
        self.assertEqual('true', flags['docs_only'])
        self.assertEqual('false', flags['fast'])


class WorkflowContract(unittest.TestCase):
    def test_every_output_a_workflow_reads_exists_in_the_classifier(self):
        declared = set(classifier().OUTPUT_KEYS)
        referenced = set()
        for workflow in sorted(WORKFLOWS.glob('*.yml')):
            referenced |= set(REFERENCE.findall(workflow.read_text(encoding='utf-8')))
        self.assertTrue(referenced, 'no workflow reads a changes output')
        self.assertEqual(set(), referenced - declared,
                         'a workflow reads an output the classifier never writes')

    def test_the_classifier_writes_every_declared_output(self):
        self.assertEqual(set(classifier().OUTPUT_KEYS), set(routed([DOC])))

    def test_ci_workflow_exposes_a_single_summary_gate_job(self):
        text = (WORKFLOWS / 'ci.yml').read_text(encoding='utf-8')
        gate = job_block(text, 'ci-gate')
        self.assertIn('if: always()', gate)
        for lane in ('changes', 'windows-fast', 'typescript', 'windows-hermetic',
                     'panel-e2e', 'windows-mutate', 'toolchain-crapper',
                     'toolchain-mutator'):
            self.assertIn(f'- {lane}', gate)

    def test_ci_clippy_checks_all_features_including_toolchain_contracts(self):
        text = (WORKFLOWS / 'ci.yml').read_text(encoding='utf-8')
        fast = job_block(text, 'windows-fast')
        self.assertIn('--all-targets --all-features', fast)

    def test_the_release_workflow_gates_assembly_on_the_classifier(self):
        text = (WORKFLOWS / 'package-release.yml').read_text(encoding='utf-8')
        assembly = job_block(text, 'assemble')
        self.assertIn('needs: changes', assembly)
        self.assertIn("needs.changes.outputs.release == 'true'", assembly)


if __name__ == '__main__':
    unittest.main()
