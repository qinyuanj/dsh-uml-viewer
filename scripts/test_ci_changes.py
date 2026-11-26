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


class DocumentationRouting(unittest.TestCase):
    def test_a_pure_documentation_change_runs_no_windows_lane(self):
        self.assertEqual({'docs_only'}, lanes([DOC, 'GLOSSARY.md',
                                               'acceptance-evidence/106-wsl/log.md']))

    def test_the_prompt_section_routes_the_plugin_and_panel_lanes(self):
        self.assertEqual({'fast', 'typescript', 'panel_e2e', 'check', 'release'},
                         lanes(['docs/companion-rules.md']))

    def test_a_bundled_document_routes_the_package_lanes(self):
        for document in ('docs/plugin-development.md', 'README.md'):
            with self.subTest(document=document):
                self.assertEqual({'fast', 'typescript', 'panel_e2e', 'release'},
                                 lanes([document]))

    def test_a_typescript_change_skips_the_rust_quality_lanes(self):
        self.assertEqual({'fast', 'typescript', 'panel_e2e', 'mutate', 'release'},
                         lanes(['packages/host/src/prompt.ts']))

    def test_a_viewer_runtime_change_routes_every_rust_lane(self):
        self.assertEqual({'fast', 'crap', 'mutate', 'quality', 'check',
                          'panel_e2e', 'release'},
                         lanes(['crates/uml-viewer/src/main.rs']))

    def test_a_standalone_quality_tool_change_skips_the_panel_lane(self):
        self.assertEqual({'fast', 'crap', 'mutate', 'quality', 'check', 'release'},
                         lanes(['crates/crapper/src/lib.rs']))

    def test_a_fixture_change_skips_the_panel_and_release_lanes(self):
        self.assertEqual({'fast', 'crap', 'mutate', 'quality', 'check'},
                         lanes(['fixtures/crap/python/sample.py']))

    def test_a_quality_guard_script_routes_the_lanes_that_run_it(self):
        self.assertEqual({'fast', 'mutate', 'quality', 'check'},
                         lanes(['scripts/check.py']))

    def test_a_packaging_script_routes_the_package_lanes(self):
        self.assertEqual({'fast', 'typescript', 'panel_e2e', 'release'},
                         lanes(['scripts/package.mjs']))

    def test_an_edn_snapshot_routes_the_guarded_lanes(self):
        self.assertEqual({'fast', 'quality', 'check'}, lanes(['examples/uml-viewer.edn']))


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
                self.assertEqual('true', flags['quality'])


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
        self.assertEqual('true', flags['crap'])
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

    def test_every_lane_owner_is_a_declared_domain(self):
        module = classifier()
        owners = {owner for names in module.LANES.values() for owner in names}
        self.assertEqual(set(), owners - set(module.DOMAIN_NAMES))

    def test_the_classifier_only_emits_declared_domains(self):
        module = classifier()
        for path in (DOC, 'README.md', 'docs/companion-rules.md',
                     'packages/host/src/prompt.ts', 'crates/crapper/src/lib.rs',
                     'crates/uml-viewer/src/main.rs',
                     'fixtures/crap/python/sample.py', 'scripts/check.py',
                     'scripts/package.mjs', 'examples/uml-viewer.edn',
                     '.github/workflows/ci.yml', 'scripts/ci-changes.py',
                     'uml-quality.json', '.gitattributes', 'tools/new-tool.ps1'):
            with self.subTest(path=path):
                self.assertTrue(module.domains_for(path) <= set(module.DOMAIN_NAMES), path)

    def test_the_release_workflow_gates_assembly_on_the_classifier(self):
        text = (WORKFLOWS / 'package-release.yml').read_text(encoding='utf-8')
        assembly = job_block(text, 'assemble')
        self.assertIn('needs: changes', assembly)
        self.assertIn('needs.changes.outputs.release == \'true\'', assembly)


if __name__ == '__main__':
    unittest.main()
