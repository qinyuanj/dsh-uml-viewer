"""公共 POSIX 入口验收；使用真实 Rust 二进制和隔离源码，不启动 companion。"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import unittest.mock

REPO = Path(__file__).resolve().parents[1]
# check.py runs this file with python -I: import the entry by directory.
sys.path.insert(0, str(REPO / 'scripts'))
from uml_posix import proc_identity  # noqa: E402


class PosixEntries(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='uml 空格项目 ')
        self.addCleanup(self.temporary.cleanup)
        # macOS: TMPDIR sits under the /var -> /private/var link.
        self.root = Path(self.temporary.name).resolve()
        shutil.copy2(REPO / 'scripts/uml', self.root / 'uml')
        (self.root / 'src').mkdir()
        (self.root / 'src/a.lua').write_text('local M={}\nfunction M.hello() return "hello" end\nreturn M\n')
        self.policy = '{:lang :lua :src "src" :prefix "src" :proposals [{:id :saved :name "保留" :layers []}]}'
        (self.root / 'policy.edn').write_text(self.policy)
        self.env = dict(os.environ, UML_VIEWER_TOOL_ROOT=str(REPO))

    def entry(self, *args):
        return subprocess.run([str(self.root / 'uml'), *args], cwd=self.root, env=self.env,
                              text=True, capture_output=True, timeout=30)

    def test_real_headless_ir_with_unicode_spaces_and_repeat(self):
        for _ in range(2):
            result = self.entry('ir', 'policy.edn', '结果 图.edn')
            self.assertEqual(0, result.returncode, result.stderr)
            text = (self.root / '结果 图.edn').read_text()
            self.assertIn('hello', text)
            self.assertIn('src.a', text)
            self.assertIn('保留', text)
            self.assertEqual(self.policy, (self.root / 'policy.edn').read_text())

    def test_ir_output_is_byte_identical_across_runs(self):
        outputs = set()
        for _ in range(8):
            result = self.entry('ir', 'policy.edn', 'stable.edn')
            self.assertEqual(0, result.returncode, result.stderr)
            outputs.add((self.root / 'stable.edn').read_bytes())
        self.assertEqual(1, len(outputs), 'IR bytes changed between identical runs')

    def test_four_real_scanners(self):
        samples = {
            'lua': ('sample.lua', 'local M={}\nfunction M.hello() return true end\nreturn M\n'),
            'python': ('sample.py', 'def hello():\n    return True\n'),
            'typescript': ('sample.ts', 'export function hello(): boolean { return true; }\n'),
            'rust': ('lib.rs', 'pub fn hello() -> bool { true }\n'),
        }
        for language, (filename, source) in samples.items():
            with self.subTest(language=language):
                folder = self.root / language
                folder.mkdir()
                (folder / filename).write_text(source)
                (self.root / 'scanner.edn').write_text('{:lang :' + language + ' :src "' + language + '" :prefix "demo"}')
                result = self.entry('ir', 'scanner.edn', language + '.edn')
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn('hello', (self.root / (language + '.edn')).read_text())


    def test_language_detection_from_markers(self):
        # No :lang and no :sources in the policy: the marker file in the tree
        # decides the scanner, and the result must carry the detected language.
        markers = {
            'lua': ('.', 'src/a.lua', 'local M={}\nfunction M.hello() return true end\nreturn M\n'),
            'python': ('pyproject.toml', 'src/a.py', 'def hello():\n    return True\n'),
            'typescript': ('package.json', 'src/a.ts', 'export function hello(): boolean { return true; }\n'),
            'rust': ('Cargo.toml', 'src/lib.rs', 'pub fn hello() -> bool { true }\n'),
        }
        for language, (marker, relative, source) in markers.items():
            with self.subTest(language=language):
                for stale in list(self.root.glob('src/*')):
                    stale.unlink()
                for other in ('pyproject.toml', 'package.json', 'Cargo.toml'):
                    (self.root / other).unlink(missing_ok=True)
                if marker != '.':
                    (self.root / marker).write_text('')
                path = self.root / relative
                path.parent.mkdir(exist_ok=True)
                path.write_text(source)
                (self.root / 'detect.edn').write_text('{:src "src" :prefix "app"}')
                name = language + '-detected.edn'
                result = self.entry('ir', 'detect.edn', name)
                self.assertEqual(0, result.returncode, result.stderr)
                document = (self.root / name).read_text()
                self.assertRegex(document, r':lang\s+:' + language + r'\b')
                self.assertRegex(document, r':classes\s+\[\s*\{')
                self.assertIn('hello', document)

    def test_ambiguous_markers_require_an_explicit_language(self):
        (self.root / 'src').mkdir(exist_ok=True)
        (self.root / 'src/a.lua').write_text('return {}\n')
        (self.root / 'Cargo.toml').write_text('')
        (self.root / 'ambiguous.edn').write_text('{:src "src" :prefix "app"}')
        result = self.entry('ir', 'ambiguous.edn', 'ambiguous-result.edn')
        self.assertNotEqual(0, result.returncode)
        self.assertIn('declare explicit :lang or :sources', result.stderr)
        self.assertFalse((self.root / 'ambiguous-result.edn').exists())

    def test_ir_matches_existing_language_goldens(self):
        fixtures = [('rust-fixture', 'fixtures/ir-bytes/rust-fixture.edn')]
        for fixture, golden in fixtures:
            with self.subTest(fixture=fixture):
                output = self.root / 'golden-result.edn'
                result = self.entry('ir', '--root', str(REPO / fixture), '--policy', 'policy.edn', '--output', str(output))
                self.assertEqual(0, result.returncode, result.stderr)
                comparison = subprocess.run(
                    [sys.executable, '-I', str(REPO / 'scripts/compare_ir.py'), str(output),
                     str(REPO / golden)],
                    cwd=REPO, capture_output=True, text=True)
                self.assertEqual(0, comparison.returncode, comparison.stdout + comparison.stderr)

    def test_default_policy_selection(self):
        result = self.entry('ir')
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((self.root / 'diagram.edn').exists())

    def test_ir_failure_preserves_previous_output(self):
        output = self.root / 'result.edn'
        output.write_text('previous')
        result = self.entry('ir', 'missing.edn', 'result.edn')
        self.assertNotEqual(0, result.returncode)
        self.assertIn('IR generation failed', result.stderr)
        self.assertEqual('previous', output.read_text())

    def test_symlink_sources_fail_without_following_cycle(self):
        (self.root / 'src/cycle').symlink_to(self.root / 'src', target_is_directory=True)
        result = self.entry('ir', 'policy.edn', 'result.edn')
        self.assertNotEqual(0, result.returncode)
        self.assertIn('symbolic link', result.stderr)
        self.assertFalse((self.root / 'result.edn').exists())

    def configured(self, codes=(0, 3)):
        worker = self.root / '质量 worker.py'
        worker.write_text('import json,sys\nfrom pathlib import Path\nPath("argv.json").write_text(json.dumps(sys.argv[1:]))\nraise SystemExit(3)\n')
        operation = {'argv': [sys.executable, str(worker)], 'forwardArgs': 'append',
                     'validResultExitCodes': list(codes)}
        crap = dict(operation, scope='targets')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'operations': {'crap': crap, 'mutate': operation}}))

    def test_configured_quality_real_exit_and_literal_argv(self):
        self.configured()
        result = self.entry('mutate', '带 空格.lua', '$(touch injected)')
        self.assertEqual(3, result.returncode, result.stderr)
        self.assertEqual(['带 空格.lua', '$(touch injected)'], json.loads((self.root / 'argv.json').read_text()))
        record = json.loads((self.root / '.uml-viewer/quality-result.json').read_text())
        self.assertTrue(record['validResult'])
        self.assertEqual(3, record['exitCode'])
        self.assertEqual(str(self.root.resolve()), record['projectRoot'])
        self.assertFalse((self.root / 'injected').exists())

    def test_invalid_config_removes_stale_result(self):
        state = self.root / '.uml-viewer'
        state.mkdir()
        (state / 'quality-result.json').write_text('{"validResult":true}')
        (self.root / 'uml-quality.json').write_text('{"version":false}')
        result = self.entry('crap')
        self.assertEqual(1, result.returncode)
        self.assertFalse((state / 'quality-result.json').exists())

    def test_undeclared_exit_three_is_invalid(self):
        self.configured((0,))
        result = self.entry('mutate', 'src/a.lua')
        self.assertEqual(3, result.returncode)
        self.assertFalse(json.loads((self.root / '.uml-viewer/quality-result.json').read_text())['validResult'])


    def test_reinstall_is_idempotent_and_provisions_all_three_tools(self):
        # A repo fixture the installer clones: the real installer path, not a
        # stub entry. Only the remote source is redirected to the checkout.
        source = self.root / 'viewer source'
        shutil.copytree(REPO, source, ignore=shutil.ignore_patterns(
            '.git', '.uml-viewer', 'target', 'coverage', '__pycache__'))
        subprocess.run(['git', 'init', '-q', '-b', 'main', str(source)], check=True)
        subprocess.run(['git', '-C', str(source), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(source), '-c', 'user.name=Test',
                        '-c', 'user.email=test@example.org', 'commit', '-qm', 'fixture'],
                       check=True, capture_output=True)
        self.write_project_policy()
        tools = {}
        for tool in ('crapper', 'mutator', 'dryer'):
            tools[tool] = self.tool_fixture(self.root / f'{tool} source', tool)
        cargo, cargo_calls = self.cargo_stub()
        face, font_env = self.font_fixture()
        env = dict(self.env, UML_VIEWER_TOOL_ROOT='', UML_VIEWER_REPO_URL=str(source),
                   UML_VIEWER_REF='main', UML_CARGO_EXECUTABLE=str(cargo), **font_env,
                   **{f'{tool.upper()}_REPO_URL': str(path) for tool, path in tools.items()},
                   **{f'{tool.upper()}_REF': 'main' for tool in tools})
        (self.root / '.gitignore').write_text('')
        policy = (self.root / 'policy.edn').read_bytes()
        # #167: the project was installed by the LOVE-era installer.
        leftover = self.root / '.uml-viewer/cimgui-love-1.90-1'
        (leftover / 'cimgui').mkdir(parents=True)
        (leftover / 'cimgui/init.lua').write_text('return {}\n')
        (self.root / '.uml-viewer/install.json').write_text('{"version": 1, "love": "/usr/bin/love"}\n')
        for attempt in range(2):
            result = subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                    cwd=self.root, env=env, capture_output=True, text=True, timeout=900)
            self.assertEqual(0, result.returncode, result.stdout + result.stderr)
            self.assertFalse(leftover.exists())
            # Reported on stderr once; the rerun has nothing left to remove.
            self.assertEqual(attempt == 0, 'cimgui-love-1.90-1' in result.stderr, result.stderr)
            self.assertNotIn('cimgui-love', result.stdout)
            self.assertTrue((self.root / 'uml').is_file())
            self.assertEqual(policy, (self.root / 'policy.edn').read_bytes())
        self.assertEqual(1, (self.root / '.gitignore').read_text().count('# BEGIN UML-VIEWER'))
        # #143: each install ran the pinned release build in the checkout, and
        # the binary the rust runtime dispatch expects exists afterwards.
        builds = [line.rsplit('|', 1)[-1] for line in cargo_calls.read_text().splitlines()]
        self.assertEqual(['build --release --locked'] * 2, builds)
        self.assertTrue(all('.uml-viewer/uml-viewer' in line
                            for line in cargo_calls.read_text().splitlines()))
        binary = self.root / '.uml-viewer/uml-viewer/target/release/uml-viewer'
        self.assertTrue(binary.is_file() and os.access(binary, os.X_OK))
        self.assertEqual(face.read_bytes(),
                         (self.root / '.uml-viewer/fonts/SarasaMonoSC-Regular.ttf').read_bytes())
        for tool in ('crapper', 'mutator', 'dryer'):
            directory = self.root / '.uml-viewer' / tool
            self.assertTrue((directory / '.git').exists(), tool)
            self.assertTrue((directory / '.venv/bin/python').exists(), tool)
        # Mutator needs a compatible crapper beside it.
        check = subprocess.run([str(self.root / '.uml-viewer/mutator/.venv/bin/python'),
                                '-I', '-m', 'pip', 'check'], capture_output=True, text=True)
        self.assertEqual(0, check.returncode, check.stdout + check.stderr)
        installed = json.loads((self.root / '.uml-viewer/install.json').read_text())
        # #144: the installer no longer provisions or records LOVE/cimgui.
        self.assertNotIn('love', installed)
        self.assertNotIn('cimgui', str(installed))
        self.assertEqual(str(binary), installed['rust'])
        self.assertTrue(all(installed['repositories'][name]['commit'] for name in
                            ('uml-viewer', 'crapper', 'mutator', 'dryer')))

    def font_fixture(self):
        """A local font pin: the face it yields and the UML_VIEWER_FONT_* overrides.

        A stand-in extractor keeps a test independent of 7z, and the file://
        archive keeps it off the network.
        """
        face = self.root / 'face.ttf'
        face.write_bytes(b'pinned sarasa face')
        archive = self.root / 'fixture.7z'
        archive.write_bytes(b'archive bytes')
        extractor = self.root / 'extractor'
        # Shell sees $1=-I $2=-c $3=script $4=archive $5=target $6=face.
        extractor.write_text('#!/bin/sh\n'
                             'mkdir -p "$5"\n'
                             f'cp "{face}" "$5/SarasaMonoSC-Regular.ttf"\n'
                             'echo SarasaMonoSC-Regular.ttf\n')
        extractor.chmod(0o755)
        return face, {'UML_VIEWER_FONT_URL': 'file://' + str(archive),
                      'UML_VIEWER_FONT_ARCHIVE_SHA256': hashlib.sha256(archive.read_bytes()).hexdigest(),
                      'UML_VIEWER_FONT_SHA256': hashlib.sha256(face.read_bytes()).hexdigest(),
                      'UML_VIEWER_FONT_EXTRACTOR': str(extractor)}

    def tool_fixture(self, directory, name):
        """A minimal installable Python package, enough for a real venv."""
        directory.mkdir(parents=True)
        (directory / 'pyproject.toml').write_text(
            f'[project]\nname = "{name}"\nversion = "0.1.0"\nrequires-python = ">=3.11"\n'
            '[build-system]\nrequires = ["setuptools>=68"]\nbuild-backend = "setuptools.build_meta"\n')
        package = directory / name
        package.mkdir()
        (package / '__init__.py').write_text('')
        (package / '__main__.py').write_text('raise SystemExit(0)\n')
        subprocess.run(['git', 'init', '-q', '-b', 'main', str(directory)], check=True)
        subprocess.run(['git', '-C', str(directory), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(directory), '-c', 'user.name=Test',
                        '-c', 'user.email=test@example.org', 'commit', '-qm', 'fixture'],
                       check=True, capture_output=True)
        return directory

    def write_project_policy(self):
        (self.root / 'src').mkdir(exist_ok=True)
        (self.root / 'src/a.lua').write_text('local M={}\nfunction M.hello() return "hello" end\nreturn M\n')
        (self.root / 'policy.edn').write_text(self.policy)

    def test_ignore_block_is_left_where_another_tool_put_it(self):
        """Another tool (magpie) also appends its block last; an intact
        UML-VIEWER block anywhere is left as is, so the two never trade
        places and the project's `.gitignore` stays clean across installs."""
        import importlib.util
        specification = importlib.util.spec_from_file_location(
            'uml_posix', REPO / 'scripts/uml_posix.py')
        installer = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(installer)
        ignore = self.root / '.gitignore'
        block = '# BEGIN UML-VIEWER\n.uml-viewer/\numl-viewer-log.txt\n# END UML-VIEWER\n'
        middle = f'/target/\n{block}\n# >>> magpie >>>\n/.agents/skills/a\n# <<< magpie <<<\n'
        ignore.write_text(middle)
        installer.ensure_ignore_block(self.root)
        self.assertEqual(middle, ignore.read_text())
        # Missing or altered: the stale block goes and the canonical one is appended.
        for before in ('/target/\n', f'/target/\n{block.replace("uml-viewer-log.txt", "old.txt")}x\n'):
            with self.subTest(before=before):
                ignore.write_text(before)
                installer.ensure_ignore_block(self.root)
                text = ignore.read_text()
                self.assertTrue(text.endswith('\n' + block), text)
                self.assertEqual(1, text.count('# BEGIN UML-VIEWER'))
                self.assertNotIn('old.txt', text)
        ignore.unlink()
        installer.ensure_ignore_block(self.root)
        self.assertEqual(block, ignore.read_text().lstrip())

    def test_installer_refuses_hosts_other_than_wsl_x86_64_and_macos(self):
        """WSL2 x86_64 and macOS are delivered (ADR 0007)."""
        import importlib.util
        from unittest import mock
        specification = importlib.util.spec_from_file_location(
            'uml_posix', REPO / 'scripts/uml_posix.py')
        installer = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(installer)
        hosts = {('Linux', 'aarch64', '6.6-microsoft-standard-WSL2'),
                 ('Linux', 'x86_64', '6.8.0-generic'), ('Windows', 'AMD64', '11')}
        for system, machine, release in sorted(hosts):
            with self.subTest(system=system, machine=machine), \
                    mock.patch.object(installer.platform, 'system', return_value=system), \
                    mock.patch.object(installer.platform, 'machine', return_value=machine), \
                    mock.patch.object(installer.platform, 'release', return_value=release):
                with self.assertRaises(ValueError) as refused:
                    installer.host()
                self.assertIn('WSL2', str(refused.exception))
        with mock.patch.object(installer.platform, 'system', return_value='Linux'), \
                mock.patch.object(installer.platform, 'machine', return_value='x86_64'), \
                mock.patch.object(installer.platform, 'release',
                                  return_value='6.18.33.2-microsoft-standard-WSL2'):
            self.assertIsNone(installer.host())
        for machine in ('arm64', 'x86_64'):
            with mock.patch.object(installer.platform, 'system', return_value='Darwin'), \
                    mock.patch.object(installer.platform, 'machine', return_value=machine):
                self.assertIsNone(installer.host())

    def test_installer_provisions_the_pinned_font_and_refuses_a_mismatch(self):
        """The font pin is enforced by hash, not by trusting the download.

        What is under test is the pin (archive hash, face hash, destination,
        idempotence) and the refusal, not py7zr itself.
        """
        import importlib.util
        specification = importlib.util.spec_from_file_location(
            'uml_posix', REPO / 'scripts/uml_posix.py')
        installer = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(installer)

        face, font_env = self.font_fixture()
        fonts = self.root / '.uml-viewer/fonts'
        with unittest.mock.patch.dict(os.environ, font_env):
            installer.font(fonts)
            self.assertEqual(face.read_bytes(), (fonts / 'SarasaMonoSC-Regular.ttf').read_bytes())
            written = (fonts / 'SarasaMonoSC-Regular.ttf').stat().st_mtime_ns
            # A second pass must keep the already verified face untouched.
            installer.font(fonts)
            self.assertEqual(written, (fonts / 'SarasaMonoSC-Regular.ttf').stat().st_mtime_ns)
            # A pin that does not match the face must leave nothing behind.
            (fonts / 'SarasaMonoSC-Regular.ttf').unlink()
            with unittest.mock.patch.dict(os.environ, {'UML_VIEWER_FONT_SHA256': '0' * 64}):
                with self.assertRaises(ValueError) as caught:
                    installer.font(fonts)
                self.assertIn('SHA256 mismatch', str(caught.exception))
            # And a substituted archive never reaches extraction.
            with unittest.mock.patch.dict(os.environ, {'UML_VIEWER_FONT_ARCHIVE_SHA256': '0' * 64}):
                with self.assertRaises(ValueError) as caught:
                    installer.font(fonts)
                self.assertIn('archive SHA256 mismatch', str(caught.exception))
            self.assertFalse((fonts / 'SarasaMonoSC-Regular.ttf').exists())

    def test_the_font_step_runs_on_its_own(self):
        """`uml_posix.py font <dir>` provisions only the pinned face.

        The viewer's real-font test provisions Sarasa through it when a clone
        has none, instead of skipping.
        """
        face, font_env = self.font_fixture()
        fonts = self.root / 'fonts'
        env = dict(os.environ, **font_env)
        result = subprocess.run([sys.executable, '-I', str(REPO / 'scripts/uml_posix.py'),
                                 'font', str(fonts)], env=env, capture_output=True, text=True,
                                timeout=60)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(face.read_bytes(), (fonts / 'SarasaMonoSC-Regular.ttf').read_bytes())

    def stalling_server(self, reply):
        """A local HTTP peer that sends `reply`, then holds the connection silently."""
        listener = socket.create_server(('127.0.0.1', 0))
        self.addCleanup(listener.close)
        release = threading.Event()
        self.addCleanup(release.set)

        def serve():
            connection, _ = listener.accept()
            with connection:
                connection.recv(65536)
                connection.sendall(reply)
                release.wait()

        threading.Thread(target=serve, daemon=True).start()
        return f'127.0.0.1:{listener.getsockname()[1]}/stalled.7z'

    def font_download_install(self):
        """Run the public install up to the font download; `run(url)` returns its result."""
        cargo, _ = self.cargo_stub()
        tools = {tool: self.tool_fixture(self.root / f'{tool} source', tool)
                 for tool in ('crapper', 'mutator', 'dryer')}
        _, font_env = self.font_fixture()
        temporary = self.root / 'install temp'
        temporary.mkdir()
        env = dict(self.env, UML_CARGO_EXECUTABLE=str(cargo), TMPDIR=str(temporary), **font_env,
                   UML_VIEWER_FONT_TIMEOUT='1',
                   **{f'{tool.upper()}_REPO_URL': str(path) for tool, path in tools.items()},
                   **{f'{tool.upper()}_REF': 'main' for tool in tools})

        def run(url):
            return subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                  cwd=self.root, env=dict(env, UML_VIEWER_FONT_URL=url),
                                  capture_output=True, text=True, timeout=60)
        return run, temporary

    def test_install_names_the_url_and_timeout_when_the_font_download_stalls(self):
        """A stalled server ends the install with a named cause, not a hang."""
        run, temporary = self.font_download_install()
        # The TLS case stalls in the handshake, which urllib reports wrapped in
        # URLError; the other two surface a bare TimeoutError.
        stalls = {'never answers': ('http', b''),
                  'TLS handshake never answers': ('https', b''),
                  'stops mid-body': ('http', b'HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\npartial')}
        for label, (scheme, reply) in stalls.items():
            with self.subTest(label):
                url = f'{scheme}://{self.stalling_server(reply)}'
                result = run(url)
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn(url, result.stderr)
                self.assertIn('1 秒', result.stderr)
                self.assertEqual([], list((self.root / '.uml-viewer/fonts').glob('*')))
                self.assertEqual([], list(temporary.iterdir()))
                self.assertFalse((self.root / '.uml-viewer/install.json').exists())

    def test_install_does_not_call_a_refused_font_download_a_timeout(self):
        run, _ = self.font_download_install()
        with socket.create_server(('127.0.0.1', 0)) as closed:
            url = f'http://127.0.0.1:{closed.getsockname()[1]}/refused.7z'
        result = run(url)
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn('refused', result.stderr.lower())
        self.assertNotIn('超时', result.stderr)

    def test_install_refuses_existing_ownership(self):
        state = self.root / '.uml-viewer'
        state.mkdir()
        ownership = state / 'companion.edn'
        ownership.write_text('{:backend :unknown :pid 123}')
        # #167: a refused install deletes nothing, LOVE leftovers included.
        (state / 'cimgui-love-1.90-1/cimgui').mkdir(parents=True)
        (state / 'cimgui-love-1.90-1/cimgui.so').write_bytes(b'ELF')
        (state / 'cimgui-love-1.90-1/cimgui/init.lua').write_text('return {}\n')
        before = self.tree(state)
        result = subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                cwd=self.root, env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(1, result.returncode)
        self.assertIn('保留已有运行态', result.stderr)
        self.assertEqual('{:backend :unknown :pid 123}', ownership.read_text())
        self.assertEqual(before, self.tree(state))

    def test_install_passes_over_a_closed_session_and_refuses_a_live_one(self):
        """A normal close leaves a terminal recovery record and the identity
        of an exited viewer; those are history, not a running session."""
        import json
        state = self.root / '.uml-viewer'
        directory = state / 'crapper'
        directory.mkdir(parents=True)
        subprocess.run(['git', 'init', '-q', str(directory)], check=True)
        (directory / 'unique-work.txt').write_text('用户未提交内容')
        (state / 'uml-viewer').symlink_to(REPO, target_is_directory=True)
        started = proc_identity(os.getpid())['start']
        alive = {'pid': os.getpid(), 'started': started, 'runtime': 'rust', 'cwd': str(self.root)}
        closed = dict(alive, started=str(int(started) + 1))
        cases = [
            ('closed', '{:phase :cancelled :reason "Viewer closed; recovery revoked"}', closed, False),
            ('recovering', '{:phase :backoff}', closed, True),
            ('viewer alive', '{:phase :normal}', alive, True),
        ]
        for label, recovery, identity, refused in cases:
            with self.subTest(label):
                (state / 'companion.edn').write_text('{}')
                (state / 'recovery.edn').write_text(recovery)
                (state / 'viewer-process-identity.json').write_text(json.dumps(identity))
                result = subprocess.run(
                    [str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                    cwd=self.root, env=self.env, capture_output=True, text=True, timeout=30)
                self.assertEqual(1, result.returncode)
                # A passed check runs on to the dirty tool checkout.
                self.assertIn('保留已有运行态' if refused else '本地修改', result.stderr)

    def test_installer_preserves_dirty_tool_checkout(self):
        directory = self.root / '.uml-viewer/crapper'
        directory.mkdir(parents=True)
        subprocess.run(['git', 'init', '-q', str(directory)], check=True)
        user_file = directory / 'unique-work.txt'
        user_file.write_text('用户未提交内容')
        (self.root / '.uml-viewer/uml-viewer').symlink_to(REPO, target_is_directory=True)
        result = subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                cwd=self.root, env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(1, result.returncode)
        self.assertIn('本地修改', result.stderr)
        self.assertEqual('用户未提交内容', user_file.read_text())

    def test_installer_leaves_a_linked_checkout_on_its_branch(self):
        # A link is a development checkout; fetching through it detached the
        # linked repository's HEAD.
        linked = self.root / 'linked'
        subprocess.run(['git', 'init', '-q', '-b', 'main', str(linked)], check=True)
        subprocess.run(['git', '-C', str(linked), '-c', 'user.name=Test',
                        '-c', 'user.email=test@example.org', 'commit', '-q',
                        '--allow-empty', '-m', 'work'], check=True)
        state = self.root / '.uml-viewer'
        state.mkdir()
        (state / 'uml-viewer').symlink_to(linked, target_is_directory=True)
        missing = str(self.root / 'no such remote')
        env = dict(self.env, UML_VIEWER_TOOL_ROOT='', UML_VIEWER_REPO_URL=str(linked),
                   **{f'{tool.upper()}_REPO_URL': missing for tool in ('crapper', 'mutator', 'dryer')})
        result = subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(1, result.returncode)
        self.assertIn('UML_VIEWER_TOOL_ROOT', result.stderr)
        branch = subprocess.run(['git', '-C', str(linked), 'symbolic-ref', '--short', 'HEAD'],
                                capture_output=True, text=True)
        self.assertEqual('main', branch.stdout.strip())

    def test_standard_mutation_passes_every_file_to_one_call(self):
        # One mutator call shares its coverage run and baseline across files;
        # mutator still publishes each source's snapshot on its own.
        self.record_calls('mutator', 'calls.jsonl')
        (self.root / 'src/b.lua').write_text('return {}')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {'sourceRoots': ['src']}}))
        result = self.entry('mutate', 'src/a.lua', 'src/b.lua')
        self.assertEqual(3, result.returncode, result.stderr)
        calls = [json.loads(line) for line in (self.root / 'calls.jsonl').read_text().splitlines()]
        self.assertEqual([['-m', 'mutator', '-s', 'src', 'src/a.lua', 'src/b.lua']], calls)
        self.assertTrue(json.loads((self.root / '.uml-viewer/quality-result.json').read_text())['validResult'])


    def stub_tool(self, tool, body='import sys\nraise SystemExit(3)\n', usage='  --exclude <glob>'):
        # `--help` answers like a tool that takes -s/--exclude unless `usage`
        # says otherwise; any other call runs `body`.
        directory = self.root / f'.uml-viewer/{tool}/.venv/bin'
        directory.mkdir(parents=True)
        python = directory / 'python'
        python.write_text('#!/usr/bin/python3\nimport sys\n'
                          f'if sys.argv[1:] == ["-m", "{tool}", "--help"]:\n'
                          f'    print({usage!r})\n    raise SystemExit(0)\n' + body)
        python.chmod(0o755)

    def record(self):
        return json.loads((self.root / '.uml-viewer/quality-result.json').read_text())

    def test_record_matches_operation_canonical_root_and_actual_exit(self):
        # The companion rejects a record whose operation, projectRoot or exitCode
        # differs from the call it made; the published record must carry exactly
        # those three values, even through a symlinked project path and in
        # either target order.
        self.stub_tool('mutator', 'import sys\nraise SystemExit(3 if any(a.endswith("a.lua") for a in sys.argv) else 0)\n')
        self.stub_tool('dryer', 'raise SystemExit(0)\n')
        (self.root / 'src/b.lua').write_text('return {}')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {'sourceRoots': ['src']}}))
        alias = Path(tempfile.mkdtemp(prefix='uml link ')) / 'project'
        self.addCleanup(shutil.rmtree, alias.parent)
        alias.symlink_to(self.root, target_is_directory=True)
        cases = [('mutate', ['src/a.lua', 'src/b.lua'], 3), ('mutate', ['src/b.lua', 'src/a.lua'], 3),
                 ('mutate', ['src/b.lua'], 0), ('dry', [], 0)]
        for operation, args, expected in cases:
            with self.subTest(operation=operation, args=args):
                result = subprocess.run([str(alias / 'uml'), operation, *args], cwd=alias, env=self.env,
                                        text=True, capture_output=True, timeout=30)
                self.assertEqual(expected, result.returncode, result.stderr)
                record = self.record()
                self.assertEqual((operation, str(self.root.resolve()), result.returncode),
                                 (record['operation'], record['projectRoot'], record['exitCode']))

    def test_standard_crap_requests_the_whole_business_tree(self):
        # Ported from the retired PowerShell public-entry suite: the standard
        # craspper invocation keeps declared scope, options and caller arguments
        # in that order, and crapper's declared finding (exit 2) is valid.
        self.stub_tool('crapper', 'import json,sys\nfrom pathlib import Path\n'
                       'Path("crap-call.json").write_text(json.dumps(sys.argv[1:]))\nraise SystemExit(2)\n')
        (self.root / 'business tree').mkdir()
        (self.root / 'business tree/generated').mkdir()
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['business tree'], 'exclude': ['business tree/generated'],
            'crapperOptions': ['--no-coverage']}}))
        result = self.entry('crap')
        self.assertEqual(2, result.returncode, result.stderr)
        self.assertEqual(['-m', 'crapper', '--full-tree', '--source-root', 'business tree',
                          '--exclude', 'business tree/generated', '--no-coverage'],
                         json.loads((self.root / 'crap-call.json').read_text()))
        self.assertEqual({'version': 1, 'operation': 'crap', 'exitCode': 2, 'validResult': True,
                          'crapScope': 'whole-tree', 'configured': False, 'tool': 'crapper',
                          'projectRoot': str(self.root.resolve())}, self.record())

    def record_calls(self, tool, name):
        # The stub appends each argv it receives, one JSON list per line.
        self.stub_tool(tool, 'import json,sys\nfrom pathlib import Path\n'
                       f'p=Path("{name}")\n'
                       'with p.open("a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n'
                       'raise SystemExit(3)\n')

    def calls(self, name):
        return [json.loads(line) for line in (self.root / name).read_text().splitlines()]

    def test_standard_mutation_keeps_declared_scope_and_literal_argv(self):
        # Ported from the retired suite: mutator receives the declared scope as
        # -s/--exclude, named files outside it are not forwarded, and spaces and
        # shell metacharacters reach mutator literally.
        self.record_calls('mutator', 'mutate-calls.jsonl')
        for name in ['business/keep.lua', 'business/generated/omit.lua', 'outside.lua',
                     'business/keep again.lua', 'business/$(touch injected);x.lua']:
            file = self.root / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('return 1')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['business'], 'exclude': ['**/generated'],
            'mutatorOptions': ['--max-workers', '2']}}))
        result = self.entry('mutate', 'business/keep.lua', 'business/generated/omit.lua',
                            'outside.lua', 'business/keep again.lua',
                            'business/$(touch injected);x.lua', 'business/keep.lua')
        self.assertEqual(3, result.returncode, result.stderr)
        self.assertEqual([['-m', 'mutator', '-s', 'business', '--exclude', '**/generated',
                           '--max-workers', '2', 'business/keep.lua', 'business/keep again.lua',
                           'business/$(touch injected);x.lua']],
                         self.calls('mutate-calls.jsonl'))
        self.assertFalse((self.root / 'injected').exists())
        self.assertTrue(self.record()['validResult'])
        # Each named file dropped by the declared scope (outside the roots or
        # excluded) gets one diagnostic line, in the order it was named.
        self.assertEqual('', result.stdout)
        self.assertEqual(['uml: omitted mutation target outside business scope: business/generated/omit.lua',
                          'uml: omitted mutation target outside business scope: outside.lua'],
                         result.stderr.splitlines())

    def test_standard_mutation_forwards_changed_with_the_declared_scope(self):
        # --changed reaches mutator as the flag itself: mutator asks git and
        # keeps the changed sources under -s minus --exclude, so uml-viewer
        # needs no repository of its own and names no dropped file.
        self.record_calls('mutator', 'mutate-calls.jsonl')
        (self.root / 'business').mkdir()
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['src', 'business'], 'exclude': ['**/generated']}}))
        result = self.entry('mutate', '--changed', '--max-workers', '2')
        self.assertEqual(3, result.returncode, result.stderr)
        self.assertEqual([['-m', 'mutator', '-s', 'src', '-s', 'business', '--exclude', '**/generated',
                           '--max-workers', '2', '--changed']], self.calls('mutate-calls.jsonl'))
        self.assertNotIn('omitted mutation target', result.stderr)

    def test_standard_mutation_forwards_a_named_directory_that_reaches_a_root(self):
        # A named directory is not expanded: one inside a root or holding one
        # is forwarded as named, and mutator picks its sources.
        self.record_calls('mutator', 'mutate-calls.jsonl')
        for name in ['business/core/x.lua', 'business/core/sub/y.lua', 'business/build/z.lua']:
            file = self.root / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('return 1')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['business/core'], 'exclude': ['**/generated']}}))
        result = self.entry('mutate', 'business', 'business/core/sub')
        self.assertEqual(3, result.returncode, result.stderr)
        self.assertEqual([['-m', 'mutator', '-s', 'business/core', '--exclude', '**/generated',
                           'business', 'business/core/sub']], self.calls('mutate-calls.jsonl'))
        self.assertEqual('', result.stderr)

    def test_standard_mutation_names_an_omitted_directory(self):
        # A named directory outside every root, or one the declaration
        # excludes (even when it holds a root), is not forwarded and gets the
        # same line as a named file.
        self.record_calls('mutator', 'mutate-calls.jsonl')
        for name in ['tools/cli.lua', 'src/generated/g.lua', 'generated/api/h.lua']:
            file = self.root / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('return 1')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['src', 'generated/api'], 'exclude': ['**/generated']}}))
        result = self.entry('mutate', 'tools', 'src/a.lua', 'src/generated', 'generated')
        self.assertEqual(3, result.returncode, result.stderr)
        self.assertEqual([['-m', 'mutator', '-s', 'src', '-s', 'generated/api', '--exclude', '**/generated',
                           'src/a.lua']], self.calls('mutate-calls.jsonl'))
        self.assertEqual(['uml: omitted mutation target outside business scope: tools',
                          'uml: omitted mutation target outside business scope: src/generated',
                          'uml: omitted mutation target outside business scope: generated'],
                         result.stderr.splitlines())

    def test_standard_mutation_refuses_a_missing_target_without_launching(self):
        # A name that is not a path is not a textual filter here: it fails
        # before mutator runs and leaves no record.
        self.stub_tool('mutator', 'from pathlib import Path\nPath("launched").write_text("")\n')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['src']}}))
        result = self.entry('mutate', 'src/a.lua', 'src/missing.lua')
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn('src/missing.lua', result.stderr)
        self.assertFalse((self.root / 'launched').exists())
        self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())

    def test_standard_mutation_refuses_changed_together_with_named_targets(self):
        # Mutator takes the changed set and ignores named paths when it gets
        # both, so the named file would silently go unmutated: refuse instead.
        self.stub_tool('mutator', 'from pathlib import Path\nPath("launched").write_text("")\n')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['src']}}))
        for args in (['--changed', 'src/a.lua'], ['src/a.lua', '--changed']):
            with self.subTest(args=args):
                result = self.entry('mutate', *args)
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('--changed', result.stderr)
                self.assertNotIn('omitted mutation target', result.stderr)
                self.assertFalse((self.root / 'launched').exists())
                self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())

    def test_declared_scope_needs_tools_that_take_it(self):
        # A mutator or dryer from before -s/--exclude narrowing would reject
        # --exclude, or mutate the whole -s tree instead of the named file.
        # With declared scope such a tool is refused before it runs; without
        # one nothing is passed that it lacks, so it still runs.
        launch = 'from pathlib import Path\nPath("launched").write_text("")\nraise SystemExit(3)\n'
        self.stub_tool('mutator', launch, usage='usage: mutator [options] [path ...]')
        self.stub_tool('dryer', launch, usage='usage: dryer [options] [path ...]')
        cases = [('mutate', ['src/a.lua'], {'sourceRoots': ['src']}),
                 ('dry', [], {'exclude': ['src/generated']})]
        for operation, args, settings in cases:
            with self.subTest(operation=operation):
                (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1,
                    'standardTools': settings}))
                result = self.entry(operation, *args)
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('get-uml-viewer --install-only', result.stderr)
                self.assertFalse((self.root / 'launched').exists())
                self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())
        (self.root / 'uml-quality.json').unlink()
        result = self.entry('mutate', 'src/a.lua')
        self.assertEqual(3, result.returncode, result.stdout + result.stderr)
        self.assertTrue((self.root / 'launched').exists())

    def test_standard_dry_forwards_declared_scope_without_listing_files(self):
        # Dryer applies -s and --exclude with crapper's selection rules, so
        # its rows join crap.edn; uml-viewer never enumerates files for it.
        self.record_calls('dryer', 'dry-calls.jsonl')
        for name in ['business tree/a.lua', 'business tree/generated/g.lua']:
            file = self.root / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('return 1')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['business tree'], 'exclude': ['business tree/generated'],
            'dryerOptions': ['--threshold', '0.9']}}))
        result = self.entry('dry', '--min-lines', '4')
        self.assertEqual(3, result.returncode, result.stderr)
        self.assertEqual([['-m', 'dryer', '-s', 'business tree', '--exclude', 'business tree/generated',
                           '--threshold', '0.9', '--min-lines', '4']], self.calls('dry-calls.jsonl'))
        self.assertFalse(self.record()['validResult'])

    def test_standard_mutation_names_every_dropped_target_before_refusing(self):
        # When no named file is in scope the caller still learns which ones
        # were dropped, then gets the existing refusal, and no tool runs.
        self.stub_tool('mutator', 'from pathlib import Path\nPath("launched").write_text("")\n')
        (self.root / 'tools').mkdir()
        (self.root / 'tools/cli.lua').write_text('return 1')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {
            'sourceRoots': ['src'], 'exclude': ['src/a.lua']}}))
        result = self.entry('mutate', 'tools/cli.lua', 'src/a.lua')
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        lines = result.stderr.splitlines()
        self.assertEqual(['uml: omitted mutation target outside business scope: tools/cli.lua',
                          'uml: omitted mutation target outside business scope: src/a.lua'], lines[:2])
        self.assertIn('没有业务 mutation 目标', '\n'.join(lines[2:]))
        self.assertFalse((self.root / 'launched').exists())
        self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())

    def test_standard_mutation_never_accepts_routing_overrides_from_the_caller(self):
        # Ported from the retired suite: scope belongs to the declaration, so a
        # caller cannot narrow or widen it, and no tool is launched.
        self.stub_tool('mutator')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1,
            'standardTools': {'sourceRoots': ['src']}}))
        for flag in ('--source-root', '--root', '--exclude'):
            with self.subTest(flag=flag):
                result = self.entry('mutate', flag, 'elsewhere', 'src/a.lua')
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('standardTools', result.stderr)
                self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())

    def test_invalid_standard_scope_never_launches_or_reuses_a_result(self):
        # Ported from the retired suite: unusable scope fails before the tool
        # runs and must not leave an older valid record behind.
        self.stub_tool('mutator')
        state = self.root / '.uml-viewer'
        for settings in ({'sourceRoots': ['missing']}, {'sourceRoots': ['..']},
                         {'mutatorOptions': ['--mutate-all']},
                         {'exclude': ['../outside']}):
            with self.subTest(settings=settings):
                (state / 'quality-result.json').write_text('{"validResult":true}')
                (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1,
                    'standardTools': settings}))
                result = self.entry('mutate')
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertFalse((state / 'quality-result.json').exists())

    def test_accessory_only_manifest_is_rejected_without_launching(self):
        # Ported from the retired suite: aliasResultExitCodes died with the JVM
        # implementation, so a manifest declaring only it cannot dispatch.
        self.stub_tool('mutator')
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1,
            'aliasResultExitCodes': {'mutate': [0, 3]}}))
        (self.root / '.uml-viewer/quality-result.json').write_text('{"validResult":true}')
        result = self.entry('mutate', 'src/a.lua')
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn('Clojure alias 已移除', result.stderr)
        self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())

    def test_love_and_unknown_runtime_values_are_refused(self):
        # #144: Rust is the only implementation. `love` is refused the same
        # way `jvm` was: nothing is dispatched and the message names rust.
        for args in (('--runtime',), ('--runtime', 'invalid'), ('--runtime', 'jvm'),
                     ('--runtime', 'love'), ('--runtime', 'love', 'ir'),
                     ('--runtime', 'rust', 'ir', '--runtime', 'x')):
            with self.subTest(args=args):
                result = self.entry(*args)
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('--runtime', result.stderr)
                self.assertIn('rust', result.stderr)
                self.assertIn('已移除', result.stderr)
                self.assertIsNone(self.launcher_record())

    def test_only_the_lifecycle_entry_takes_a_coding_agent(self):
        # #194: IR, scene and quality commands refuse the provider option
        # before anything is dispatched, wherever the option sits.
        record, env = self.rust_stub()
        for args in (('ir', '--coding-agent', 'codex'), ('--coding-agent', 'claude', 'ir'),
                     ('scene', '--coding-agent', 'claude'), ('crap', '--coding-agent', 'codex'),
                     ('mutate', 'src/a.lua', '--coding-agent', 'codex'), ('dry', '--coding-agent')):
            with self.subTest(args=args):
                (self.root / '.uml-viewer/quality-result.json').unlink(missing_ok=True)
                result = subprocess.run([str(self.root / 'uml'), *args], cwd=self.root, env=env,
                                        capture_output=True, text=True, timeout=30)
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('--coding-agent', result.stderr)
                self.assertIn(args[0] if args[0] != '--coding-agent' else 'ir', result.stderr)
                self.assertFalse(record.exists(), 'nothing may be dispatched')
                self.assertIsNone(self.launcher_record())

    def test_the_lifecycle_entry_forwards_a_coding_agent_literally(self):
        record, env = self.rust_stub()
        result = subprocess.run([str(self.root / 'uml'), '--coding-agent', 'codex', 'd.edn'],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(0, result.returncode, result.stderr)
        launcher = self.launcher_record()
        self.addCleanup(self.stop_stub, launcher)
        self.assertEqual(['viewer', '--backend', 'herdr', '--coding-agent', 'codex', 'd.edn'],
                         launcher['argv'])

    def provider_fixtures(self, herdr='0.9.5', protocol=22, integration='current', codex=None,
                          hooks='true'):
        """A PATH without the host's herdr/codex, plus fixture executables."""
        bin_dir = self.root / 'fixture bin'
        bin_dir.mkdir(exist_ok=True)
        herdr_path = bin_dir / 'herdr'
        herdr_path.write_text(
            '#!/bin/sh\ncase "$1" in\n'
            f"--version) echo 'herdr {herdr}' ;;\n"
            f"api) echo '{{\"$schema\":\"f\",\"protocol\":{protocol},\"schema_version\":1}}' ;;\n"
            f"integration) echo 'codex: {integration} (v10) (/h/codex.sh)' ;;\nesac\n")
        herdr_path.chmod(0o755)
        if codex:
            self.codex_fixture(bin_dir / 'codex', codex, hooks=hooks)
        host = [d for d in os.environ['PATH'].split(':')
                if d and not (Path(d) / 'herdr').exists() and not (Path(d) / 'codex').exists()]
        return dict(self.env, PATH=':'.join([str(bin_dir), *host]))

    def codex_fixture(self, path, version, login=0, record=None, hooks='true'):
        path.parent.mkdir(parents=True, exist_ok=True)
        log = f'printf "%s\\n" "$0" "$@" >> {str(record)!r}\n' if record else ''
        path.write_text(f'#!/bin/sh\n{log}case "$1" in\n--version) echo \'codex-cli {version}\' ;;\n'
                        f'login) echo "Not logged in"; exit {login} ;;\n'
                        f"--enable) echo 'hooks    stable    {hooks}' ;;\nesac\n")
        path.chmod(0o755)
        return path

    def assert_refused_before_resources(self, result, expected):
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn(expected, result.stderr)
        self.assertIsNone(self.launcher_record())
        self.assertFalse((self.root / '.uml-viewer/companion.edn').exists())

    def test_an_unknown_or_duplicate_coding_agent_is_refused_before_a_viewer(self):
        env = self.provider_fixtures(codex='0.166.0')
        for args in (('--coding-agent', 'gemini'), ('--coding-agent',),
                     ('--coding-agent', 'codex', '--coding-agent', 'codex', 'd.edn')):
            with self.subTest(args=args):
                result = subprocess.run([str(self.root / 'uml'), *args], cwd=self.root, env=env,
                                        capture_output=True, text=True, timeout=600)
                self.assert_refused_before_resources(result, '--coding-agent claude|codex')

    def test_codex_preflight_refuses_each_real_cause_before_a_viewer(self):
        cases = (
            ({}, 'Codex CLI not found'),
            ({'codex': '0.165.0'}, 'Codex CLI 0.165.0 at'),
            ({'codex': '0.166.0', 'herdr': '0.9.4'}, 'the selected Herdr 0.9.4'),
            ({'codex': '0.166.0', 'protocol': 21}, 'bundled schema is not protocol 22'),
            ({'codex': '0.166.0', 'integration': 'outdated'}, 'Codex integration is outdated'),
            ({'codex': '0.166.0', 'hooks': 'false'}, 'Codex hooks stay disabled for the companion'),
        )
        for fixtures, expected in cases:
            with self.subTest(**{k: str(v) for k, v in fixtures.items()}):
                env = self.provider_fixtures(**fixtures)
                result = subprocess.run([str(self.root / 'uml'), '--coding-agent', 'codex', 'd.edn'],
                                        cwd=self.root, env=env, capture_output=True, text=True,
                                        timeout=600)
                self.assert_refused_before_resources(result, expected)

    def test_codex_bin_reaches_the_preflight_as_a_literal_argv_and_overrides_path(self):
        # A current codex on PATH must not stand in for the CODEX_BIN one,
        # whose path keeps its spaces, Unicode and shell metacharacters.
        env = self.provider_fixtures(codex='0.166.0')
        record = self.root / 'codex-argv.txt'
        named = self.codex_fixture(self.root / '代码 $HOME;`x`' / "co dex'*", '0.150.0',
                                   record=record)
        env['CODEX_BIN'] = str(named)
        result = subprocess.run([str(self.root / 'uml'), '--coding-agent', 'codex'],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=600)
        self.assert_refused_before_resources(result, f'Codex CLI 0.150.0 at {named} is not a supported')
        self.assertEqual([str(named), '--version'], record.read_text().splitlines())
        env['CODEX_BIN'] = str(named) + ' missing'
        result = subprocess.run([str(self.root / 'uml'), '--coding-agent', 'codex'],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=600)
        self.assert_refused_before_resources(result, 'is not an executable file')

    def test_a_claude_project_keeps_the_herdr_093_floor(self):
        # The preflight role answers `herdr` for the default and the explicit
        # Claude choice on Herdr 0.9.3 even when no Codex is installed.
        env = self.provider_fixtures(herdr='0.9.3', integration='not installed')
        binary = subprocess.run([sys.executable, '-I', '-c',
                                 'import sys; sys.path.insert(0, sys.argv[1]); import uml_posix;'
                                 'print(uml_posix.rust_command(uml_posix.Path(sys.argv[2])))',
                                 str(REPO / 'scripts'), str(REPO)],
                                capture_output=True, text=True, timeout=600, env=env)
        self.assertEqual(0, binary.returncode, binary.stderr)
        for args in ([], ['--coding-agent', 'claude']):
            with self.subTest(args=args):
                result = subprocess.run([binary.stdout.strip(), 'companion-preflight',
                                         str(self.root), '--backend', 'herdr', *args],
                                        env=env, capture_output=True, text=True, timeout=60)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertEqual('herdr\n', result.stdout)
                self.assertIn('Herdr 0.9.3', result.stderr)
                self.assertIn('Claude Code', result.stderr)

    def test_default_runtime_dispatches_headless_subcommands_to_the_rust_binary(self):
        # #144: no --runtime flag selects the Rust binary.
        record, env = self.rust_stub()
        arguments = ['ir', 'policy.edn', '结果 图.edn', 'literal;$value']
        result = subprocess.run([str(self.root / 'uml'), *arguments], cwd=self.root,
                                env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(7, result.returncode, result.stderr)
        self.assertEqual(arguments, record.read_text().splitlines())

    def test_default_runtime_launches_the_rust_viewer_and_records_the_runtime(self):
        record, env = self.rust_stub()
        result = subprocess.run([str(self.root / 'uml'), 'diagram with spaces.edn'],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(0, result.returncode, result.stderr)
        launcher = self.launcher_record()
        self.addCleanup(self.stop_stub, launcher)
        self.assertEqual('rust', launcher['runtime'])
        self.assertEqual(['viewer', '--backend', 'herdr', 'diagram with spaces.edn'], launcher['argv'])
        observed = proc_identity(launcher['pid'])['start']
        self.assertEqual(observed, launcher['started'])

    def test_default_runtime_without_a_built_binary_names_the_build_step(self):
        env = dict(self.env, UML_VIEWER_RUST_EXECUTABLE=str(self.root / 'missing'))
        result = subprocess.run([str(self.root / 'uml'), 'ir'], cwd=self.root,
                                env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(1, result.returncode)
        self.assertIn('cargo build --release --locked', result.stderr)

    def test_a_binary_older_than_its_sources_is_rebuilt_before_use(self):
        """A development tool root must not run yesterday's viewer.

        `./uml --restart` once relaunched a binary built before the fix under
        test, and the fix was reported live. The entry now rebuilds when any
        Rust source or manifest is newer than the binary, and only then.
        """
        import importlib.util
        from unittest import mock
        specification = importlib.util.spec_from_file_location(
            'uml_posix', REPO / 'scripts/uml_posix.py')
        entry = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(entry)
        tool = self.root / 'tool'
        source = tool / 'crates/viewer/src/paint.rs'
        source.parent.mkdir(parents=True)
        source.write_text('fn main() {}\n')
        (tool / 'Cargo.toml').write_text('[workspace]\n')
        os.utime(tool / 'Cargo.toml', (0, 0))
        binary = tool / 'target/release/uml-viewer'
        binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\n')
        binary.chmod(0o755)
        env = {k: v for k, v in os.environ.items() if k != 'UML_VIEWER_RUST_EXECUTABLE'}
        for source_age, built in ((100, 200), (300, 200)):
            os.utime(binary, (built, built))
            os.utime(source, (source_age, source_age))
            with self.subTest(source=source_age, binary=built), \
                    mock.patch.dict(os.environ, env, clear=True), \
                    mock.patch.object(entry, 'cargo_command', return_value='cargo'), \
                    mock.patch.object(entry, 'rust_build') as build:
                self.assertEqual(str(binary), entry.rust_command(tool))
                self.assertEqual(source_age > built, build.called)

    def test_default_runtime_reaches_the_real_binary(self):
        binary = REPO / 'target/release/uml-viewer'
        if not binary.is_file():
            self.skipTest('run cargo build --release --locked first')
        env = dict(self.env)
        env.pop('UML_VIEWER_RUST_EXECUTABLE', None)
        result = subprocess.run([str(self.root / 'uml'), 'scene'], cwd=self.root,
                                env=env, capture_output=True, text=True, timeout=30)
        # #137: the ported `scene` role answers its own usage error.
        self.assertEqual(2, result.returncode, result.stderr)
        self.assertIn('uml-viewer scene', result.stderr)

    def test_rust_ir_matches_all_four_language_goldens(self):
        # #136: the ported ir role reproduces the JVM goldens structurally
        # for every fixture project through the public entry.
        binary = REPO / 'target/release/uml-viewer'
        if not binary.is_file():
            self.skipTest('run cargo build --release --locked first')
        env = dict(self.env)
        env.pop('UML_VIEWER_RUST_EXECUTABLE', None)
        for language in ('lua', 'typescript', 'rust', 'python'):
            with self.subTest(language=language):
                output = self.root / f'{language}.golden-result.edn'
                result = subprocess.run(
                    [str(self.root / 'uml'), 'ir',
                     '--root', str(REPO / f'{language}-fixture'),
                     '--policy', 'policy.edn', '--output', str(output)],
                    cwd=self.root, env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(0, result.returncode, result.stderr)
                comparison = subprocess.run(
                    [sys.executable, '-I', str(REPO / 'scripts/compare_ir.py'), str(output),
                     str(REPO / 'fixtures/jvm-goldens' / f'{language}.ir.edn')],
                    cwd=REPO, capture_output=True, text=True)
                self.assertEqual(0, comparison.returncode, comparison.stdout + comparison.stderr)

    def test_rust_ir_runs_on_this_repository(self):
        # #136/#144: the default runtime generates IR for this repository's
        # own Rust source tree.
        binary = REPO / 'target/release/uml-viewer'
        if not binary.is_file():
            self.skipTest('run cargo build --release --locked first')
        env = dict(self.env)
        env.pop('UML_VIEWER_RUST_EXECUTABLE', None)
        output = self.root / 'repo.edn'
        result = subprocess.run(
            [str(self.root / 'uml'), 'ir', '--root', str(REPO),
             '--policy', 'examples/uml-viewer.policy.edn', '--output', str(output)],
            cwd=self.root, env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(0, result.returncode, result.stderr)
        document = output.read_text()
        self.assertIn(':lang :rust', document)
        self.assertIn('"crates/edn/src/lib.rs"', document)

    def rust_stub(self, preflight='herdr'):
        """A stand-in Rust binary: records argv, answers the preflight."""
        stub = self.root / 'rust stub'
        record = self.root / 'rust-argv.txt'
        stub.write_text(
            '#!/bin/sh\n'
            'if [ "$1" = companion-preflight ]; then\n'
            f'  printf "%s\\n" {preflight!r}\n'
            '  exit 0\n'
            'fi\n'
            f'printf "%s\\n" "$@" > {str(record)!r}\n'
            'if [ "$1" = viewer ]; then sleep 30; fi\n'
            'exit 7\n')
        stub.chmod(0o755)
        return record, dict(self.env, UML_VIEWER_RUST_EXECUTABLE=str(stub))

    def launcher_record(self):
        path = self.root / '.uml-viewer/viewer-process.json'
        return json.loads(path.read_text()) if path.exists() else None

    def stop_stub(self, record):
        if not record:
            return
        pid = record['pid']
        try:
            proc_identity(pid) is not None and os.kill(pid, 15)
        except (ProcessLookupError, PermissionError):
            pass

    def test_preflight_failure_stops_launch_without_a_viewer(self):
        # Ported from the retired suite: an unusable preflight answer stops the
        # launch before any viewer, launcher record or ownership exists.
        for preflight in ('', 'tmux', 'herdr\nextra'):
            with self.subTest(preflight=preflight):
                _, env = self.rust_stub(preflight=preflight)
                result = subprocess.run([str(self.root / 'uml'), 'diagram.edn'], cwd=self.root,
                                        env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('preflight', result.stderr)
                self.assertIsNone(self.launcher_record())
                self.assertFalse((self.root / '.uml-viewer/companion.edn').exists())

    def test_launch_records_a_real_identity_and_forwards_literal_argv(self):
        # The launcher record must carry the viewer's own creation identity,
        # and argv reaches the Rust binary unchanged except for the backend the
        # preflight consumed.
        record, env = self.rust_stub()
        arguments = ['diagram with spaces.edn', 'literal;$value']
        result = subprocess.run([str(self.root / 'uml'), *arguments], cwd=self.root,
                                env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(0, result.returncode, result.stderr)
        launcher = self.launcher_record()
        self.addCleanup(self.stop_stub, launcher)
        self.assertIsNotNone(launcher, result.stdout)
        self.assertEqual('rust', launcher['runtime'])
        observed = proc_identity(launcher['pid'])['start']
        self.assertEqual(observed, launcher['started'])
        self.assertEqual(['viewer', '--backend', 'herdr', *arguments], launcher['argv'])
        # The stub writes its argv before it sleeps, so read what the runtime
        # itself received. Poll briefly: the launcher returns as soon as the
        # child starts, before the child's first write is necessarily visible.
        expected = ['viewer', '--backend', 'herdr', *arguments]
        deadline = time.monotonic() + 5
        received = []
        while time.monotonic() < deadline:
            if record.exists():
                received = record.read_text().splitlines()
                if received == expected:
                    break
            time.sleep(0.01)
        self.assertEqual(expected, received, 'the viewer stub did not receive the literal argv')

    def test_install_defaults_to_main_and_accepts_a_fixed_commit(self):
        source = self.root / 'viewer source'
        shutil.copytree(REPO, source, ignore=shutil.ignore_patterns(
            '.git', '.uml-viewer', 'target', 'coverage', '__pycache__'))
        subprocess.run(['git', 'init', '-q', '-b', 'main', str(source)], check=True)
        subprocess.run(['git', '-C', str(source), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(source), '-c', 'user.name=Test',
                        '-c', 'user.email=test@example.org', 'commit', '-qm', 'fixture'],
                       check=True, capture_output=True)
        first = subprocess.run(['git', '-C', str(source), 'rev-parse', 'HEAD'],
                               check=True, capture_output=True, text=True).stdout.strip()
        marker = source / 'scripts/uml_posix.py'
        marker.write_text(marker.read_text() + '\n# later revision\n')
        subprocess.run(['git', '-C', str(source), 'add', 'scripts/uml_posix.py'], check=True)
        subprocess.run(['git', '-C', str(source), '-c', 'user.name=Test',
                        '-c', 'user.email=test@example.org', 'commit', '-qm', 'later'],
                       check=True, capture_output=True)
        tools = {tool: self.tool_fixture(self.root / f'{tool} source', tool)
                 for tool in ('crapper', 'mutator', 'dryer')}
        cargo, _ = self.cargo_stub()
        base = dict(self.env, UML_VIEWER_TOOL_ROOT='', UML_VIEWER_REPO_URL=str(source),
                    UML_CARGO_EXECUTABLE=str(cargo),
                    **{f'{tool.upper()}_REPO_URL': str(path) for tool, path in tools.items()})
        for name in ('UML_VIEWER_REF', 'CRAPPER_REF', 'MUTATOR_REF', 'DRYER_REF'):
            base.pop(name, None)
        # Default ref is main for every repository, including the tools.
        result = subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                cwd=self.root, env=base, capture_output=True, text=True, timeout=900)
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        installed = json.loads((self.root / '.uml-viewer/install.json').read_text())
        self.assertEqual('main', installed['repositories']['crapper']['ref'])
        # An explicit commit pins the viewer even when main has moved on.
        pinned = dict(base, UML_VIEWER_REF=first)
        result = subprocess.run([str(REPO / 'scripts/get-uml-viewer'), '--install-only'],
                                cwd=self.root, env=pinned, capture_output=True, text=True, timeout=900)
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        installed = json.loads((self.root / '.uml-viewer/install.json').read_text())
        self.assertEqual(first, installed['repositories']['uml-viewer']['commit'])
        frozen = self.root / '.uml-viewer/uml-viewer/scripts/uml_posix.py'
        self.assertNotIn('later revision', frozen.read_text())
        commits = subprocess.run(['git', '-C', str(self.root / '.uml-viewer/uml-viewer'),
                                  'rev-list', '--all', '--count'],
                                 check=True, capture_output=True, text=True).stdout.strip()
        self.assertEqual('1', commits)

    def test_missing_tool_venv_reports_the_provisioning_step(self):
        state = self.root / '.uml-viewer/mutator'
        state.mkdir(parents=True)
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'standardTools': {'sourceRoots': ['src']}}))
        result = self.entry('mutate', 'src/a.lua')
        self.assertNotEqual(0, result.returncode)
        self.assertIn('get-uml-viewer', result.stderr)

    def test_installer_help_and_missing_dependency(self):
        entry = str(REPO / 'scripts/get-uml-viewer')
        result = subprocess.run([entry, '--help'], capture_output=True, text=True)
        self.assertEqual(0, result.returncode)
        env = dict(self.env, UML_CARGO_EXECUTABLE='/missing/cargo')
        result = subprocess.run([entry, '--install-only'], cwd=self.root, env=env, capture_output=True, text=True)
        self.assertEqual(1, result.returncode)
        self.assertIn('cargo', result.stderr)
        self.assertFalse((self.root / '.uml-viewer').exists())

    def test_installer_no_longer_provisions_love_lua_or_cimgui(self):
        # #144: the Lua runtime, its busted/luacov toolchain and the cimgui
        # binding are gone, so no precheck or provisioning function remains.
        installer = self.installer_module()
        for name in ('love_command', 'lua_command', 'cimgui'):
            self.assertFalse(hasattr(installer, name), name)
        self.assertFalse(hasattr(installer, 'CIMGUI_URL'))
        self.assertFalse(hasattr(installer, 'LUA_PROBE'))

    def legacy_install(self, record):
        """A project `.uml-viewer/` as a LOVE-era install left it (#167).

        Each `cimgui-love-*` directory carries one provisioning mark
        (`cimgui.so`, `cimgui.dll`, `cimgui/init.lua`); the rest of the tree is
        state the cleanup must leave byte for byte.
        """
        state = self.root / '.uml-viewer'
        state.mkdir()
        (state / 'cimgui-love-1.90-1/cimgui').mkdir(parents=True)
        (state / 'cimgui-love-1.90-1/cimgui.so').write_bytes(b'ELF')
        (state / 'cimgui-love-1.90-1/cimgui/init.lua').write_text('return {}\n')
        (state / 'cimgui-love-win').mkdir()
        (state / 'cimgui-love-win/cimgui.dll').write_bytes(b'MZ')
        (state / 'cimgui-love-lua/cimgui').mkdir(parents=True)
        (state / 'cimgui-love-lua/cimgui/init.lua').write_text('return {}\n')
        if record is not None:
            (state / 'install.json').write_text(json.dumps(record))
        for name, text in (('session.edn', '{:diagram "d.edn"}'),
                           ('to-agent.edn', '{:next-id 1 :queue []}'),
                           ('to-viewer.edn', '{:next-id 1 :queue []}'),
                           ('companion.edn', ''), ('recovery.edn', ''),
                           ('recovery-control.edn', '{}')):
            (state / name).write_text(text)
        (state / 'fonts').mkdir()
        (state / 'fonts/SarasaMonoSC-Regular.ttf').write_bytes(b'face')
        for tool in ('crapper', 'mutator', 'dryer', 'uml-viewer'):
            (state / tool / '.venv/bin').mkdir(parents=True)
            (state / tool / '.venv/bin/python').write_text('#!/bin/sh\n')
            (state / tool / 'pyproject.toml').write_text('[project]\n')
        (state / 'installer/bin').mkdir(parents=True)
        (state / 'installer/bin/python').write_text('#!/bin/sh\n')
        # A cimgui-shaped tree outside the project, reachable only by symlink.
        outside = self.root / 'outside'
        (outside / 'cimgui').mkdir(parents=True)
        (outside / 'cimgui.so').write_bytes(b'mine')
        (outside / 'cimgui/init.lua').write_text('mine')
        (state / 'cimgui-love-1.90-1/escape').symlink_to(outside, target_is_directory=True)
        return state, outside

    def tree(self, directory):
        """Every path under directory with its bytes or link target."""
        found = {}
        for path in sorted(directory.rglob('*')):
            relative = str(path.relative_to(directory))
            if path.is_symlink():
                found[relative] = ('link', os.readlink(path))
            elif path.is_file():
                found[relative] = path.read_bytes()
            else:
                found[relative] = 'dir'
        return found

    def cleanup(self, installer, state):
        """Runs the legacy cleanup; it reports on stderr only."""
        import contextlib
        import io
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            installer.remove_legacy_runtime(state)
        self.assertEqual('', out.getvalue())
        return err.getvalue()

    def test_legacy_cimgui_directories_go_whatever_install_json_records(self):
        installer = self.installer_module()
        legacy = ('cimgui-love-1.90-1', 'cimgui-love-lua', 'cimgui-love-win')
        for record in ({'version': 1, 'love': '/usr/bin/love'},
                       {'version': 1, 'rust': '/x/uml-viewer'}, None):
            with self.subTest(record=record):
                state, outside = self.legacy_install(record)
                kept = {path: value for path, value in self.tree(state).items()
                        if path.split('/', 1)[0] not in legacy}
                outside_before = self.tree(outside)
                report = self.cleanup(installer, state)
                for name in legacy:
                    self.assertFalse((state / name).exists(), name)
                    self.assertIn(str(state / name), report)
                self.assertIn('~/.local/share/love/uml-viewer/', report)
                self.assertIn('apt', report)
                self.assertIn('love', report.replace('~/.local/share/love', ''))
                self.assertIn('lua', report)
                # Runtime records, fonts/, the tool trees with their .venv and
                # installer/ are untouched; the inner link was not followed.
                self.assertEqual(kept, self.tree(state))
                self.assertEqual(outside_before, self.tree(outside))
                # Idempotent: nothing left, nothing reported.
                self.assertEqual('', self.cleanup(installer, state))
                shutil.rmtree(state)
                shutil.rmtree(outside)

    def test_cimgui_love_symlinks_and_unmarked_directories_are_kept(self):
        installer = self.installer_module()
        state = self.root / '.uml-viewer'
        state.mkdir()
        outside = self.root / 'outside'
        (outside / 'cimgui').mkdir(parents=True)
        (outside / 'cimgui.so').write_bytes(b'mine')
        (outside / 'cimgui/init.lua').write_text('mine')
        (state / 'cimgui-love-linked').symlink_to(outside, target_is_directory=True)
        (state / 'cimgui-love-notes').mkdir()
        (state / 'cimgui-love-notes/readme.txt').write_text('mine')
        (state / 'cimgui-love-near/cimgui').mkdir(parents=True)
        (state / 'cimgui-love-near/cimgui/love.lua').write_text('mine')
        (state / 'cimgui-love-file.so').write_text('not a directory')
        (state / 'fonts/cimgui-love-nested').mkdir(parents=True)
        (state / 'fonts/cimgui-love-nested/cimgui.so').write_bytes(b'nested')
        before, outside_before = self.tree(state), self.tree(outside)
        self.assertEqual('', self.cleanup(installer, state))
        self.assertEqual(before, self.tree(state))
        self.assertEqual(outside_before, self.tree(outside))

    def installer_module(self):
        import importlib.util
        specification = importlib.util.spec_from_file_location(
            'uml_posix', REPO / 'scripts/uml_posix.py')
        installer = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(installer)
        return installer

    def cargo_stub(self, version='1.99.1'):
        """A stand-in cargo: answers --version, records builds, emits a binary."""
        stub = self.root / 'cargo stub'
        record = self.root / 'cargo-calls.txt'
        stub.write_text(
            '#!/bin/sh\n'
            f'if [ "$1" = --version ]; then printf "cargo {version} (stub)\\n"; exit 0; fi\n'
            f'printf "%s|%s\\n" "$(pwd -P)" "$*" >> {str(record)!r}\n'
            'mkdir -p target/release\n'
            'printf "#!/bin/sh\\n" > target/release/uml-viewer\n'
            'chmod +x target/release/uml-viewer\n')
        stub.chmod(0o755)
        return stub, record

    def test_installer_preflight_names_a_missing_cargo(self):
        # #143: the installer preflights the Rust toolchain; a missing cargo
        # names the fix instead of surfacing an OS error mid-install.
        installer = self.installer_module()
        env = dict(os.environ)
        env.pop('UML_CARGO_EXECUTABLE', None)
        with unittest.mock.patch.dict(os.environ, env, clear=True), \
                unittest.mock.patch.object(installer.shutil, 'which', return_value=None):
            with self.assertRaises(ValueError) as caught:
                installer.cargo_command()
        message = str(caught.exception)
        self.assertIn('cargo', message)
        self.assertIn('rustup', message)

    def test_installer_preflight_requires_git(self):
        installer = self.installer_module()
        with unittest.mock.patch.object(installer.shutil, 'which', return_value=None):
            with self.assertRaises(ValueError) as caught:
                installer.git_command()
        self.assertIn('git', str(caught.exception))

    def test_rust_toolchain_channel_comes_from_the_toolchain_file(self):
        installer = self.installer_module()
        checkout = self.root / 'checkout'
        checkout.mkdir()
        (checkout / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.99.1"\n')
        self.assertEqual('1.99.1', installer.rust_channel(checkout))
        (checkout / 'rust-toolchain.toml').unlink()
        with self.assertRaises(ValueError) as caught:
            installer.rust_channel(checkout)
        self.assertIn('rust-toolchain.toml', str(caught.exception))

    def test_installer_rejects_a_cargo_mismatched_with_the_toolchain_pin(self):
        installer = self.installer_module()
        checkout = self.root / 'checkout'
        checkout.mkdir()
        (checkout / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.99.1"\n')
        cargo, _ = self.cargo_stub(version='1.98.0')
        with self.assertRaises(ValueError) as caught:
            installer.check_rust_toolchain(str(cargo), checkout)
        message = str(caught.exception)
        self.assertIn('1.99.1', message)
        self.assertIn('1.98.0', message)
        (checkout / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.98.0"\n')
        self.assertEqual('1.98.0', installer.check_rust_toolchain(str(cargo), checkout))

    def test_rust_build_runs_the_locked_release_build(self):
        installer = self.installer_module()
        checkout = self.root / 'checkout'
        checkout.mkdir()
        (checkout / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.99.1"\n')
        cargo, record = self.cargo_stub()
        binary = installer.rust_build(str(cargo), checkout)
        self.assertEqual(['build --release --locked'],
                         [line.rsplit('|', 1)[-1] for line in record.read_text().splitlines()])
        self.assertIn(str(checkout), record.read_text())
        self.assertEqual(checkout / 'target/release/uml-viewer', binary)
        self.assertTrue(os.access(binary, os.X_OK))
        # A build that produces no binary is an installer failure, not a pass.
        quiet = self.root / 'cargo quiet'
        quiet.write_text('#!/bin/sh\n'
                         'if [ "$1" = --version ]; then printf "cargo 1.99.1 (stub)\\n"; exit 0; fi\n'
                         'exit 0\n')
        quiet.chmod(0o755)
        (checkout / 'target/release/uml-viewer').unlink()
        with self.assertRaises(ValueError) as caught:
            installer.rust_build(str(quiet), checkout)
        self.assertIn('uml-viewer', str(caught.exception))

    def test_x11_library_absence_is_a_warning_not_a_requirement(self):
        # #139 chose the Wayland-undecorated window because libxkbcommon-x11
        # is absent on the delivery host; the installer only warns (#143).
        installer = self.installer_module()
        self.assertEqual([], installer.x11_warnings(present=True))
        warnings = installer.x11_warnings(present=False)
        self.assertTrue(warnings)
        self.assertIn('libxkbcommon-x11', warnings[0])
        self.assertIn('Wayland', warnings[0])

    def test_install_names_a_missing_cargo_before_touching_state(self):
        entry = str(REPO / 'scripts/get-uml-viewer')
        env = dict(self.env, UML_CARGO_EXECUTABLE='/missing/cargo')
        result = subprocess.run([entry, '--install-only'], cwd=self.root, env=env,
                                capture_output=True, text=True)
        self.assertEqual(1, result.returncode)
        self.assertIn('cargo', result.stderr)
        self.assertIn('rustup', result.stderr)
        self.assertFalse((self.root / '.uml-viewer').exists())

class ViewerLaunchEnv(unittest.TestCase):
    """#139/#144: the launcher sets the WSLg render defaults for the only
    runtime without overriding user values; macOS keeps Metal."""

    @classmethod
    def setUpClass(cls):
        # check.py runs this file with python -I: import by directory.
        sys.path.insert(0, str(Path(__file__).resolve().parent))

    def test_rust_runtime_gets_render_defaults(self):
        from uml_launch import viewer_env
        env = viewer_env({})
        if sys.platform == 'darwin':
            self.assertEqual({}, env)
            return
        self.assertEqual('d3d12', env['GALLIUM_DRIVER'])
        self.assertEqual('gl', env['WGPU_BACKEND'])

    def test_user_values_win(self):
        from uml_launch import viewer_env
        env = viewer_env({'GALLIUM_DRIVER': 'virgl', 'WGPU_BACKEND': 'vulkan'})
        self.assertEqual('virgl', env['GALLIUM_DRIVER'])
        self.assertEqual('vulkan', env['WGPU_BACKEND'])

    def test_a_viewer_restarted_by_its_companion_is_not_inside_it(self):
        from uml_launch import viewer_env
        env = viewer_env({'UML_COMPANION_ROOT': '/p', 'HOME': '/h'})
        self.assertNotIn('UML_COMPANION_ROOT', env)
        self.assertEqual('/h', env['HOME'])


class RemovedRuntimeRecord(unittest.TestCase):
    """A launcher record left by the removed love runtime blocks only a
    restart or a still-running process, never a fresh start."""

    @classmethod
    def setUpClass(cls):
        sys.path.insert(0, str(Path(__file__).resolve().parent))

    def state_with(self, pid, started):
        state = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, state)
        (state / 'viewer-process.json').write_text(json.dumps(
            {'pid': pid, 'started': started, 'runtime': 'love', 'cwd': str(state)}))
        return state

    def dead_identity(self):
        child = subprocess.Popen([sys.executable, '-c', 'pass'])
        started = proc_identity(child.pid)['start']
        child.wait()
        return child.pid, started

    def own_identity(self):
        started = proc_identity(os.getpid())['start']
        return os.getpid(), started

    def test_fresh_start_passes_over_a_dead_love_record(self):
        from uml_launch import verify_previous
        state = self.state_with(*self.dead_identity())
        verify_previous(state, state, restart=False)

    def test_restart_still_refuses_a_love_record(self):
        from uml_launch import verify_previous
        state = self.state_with(*self.dead_identity())
        with self.assertRaisesRegex(ValueError, '已移除'):
            verify_previous(state, state, restart=True)

    def test_fresh_start_refuses_a_live_love_record(self):
        from uml_launch import verify_previous
        state = self.state_with(*self.own_identity())
        with self.assertRaisesRegex(ValueError, '已移除'):
            verify_previous(state, state, restart=False)


# A stand-in viewer: records each launch (script, argv, the session.edn and
# diagram it found), publishes the markers named for its script, and on the
# save phase writes the session.edn the restore phase must find.
FAKE_VIEWER = r'''#!{python}
import json, os, sys
from pathlib import Path
project = Path(os.environ['UML_VIEWER_PROJECT_ROOT'])
evidence = Path(os.environ['UML_VIEWER_EVIDENCE'])
script = Path(os.environ['UML_VIEWER_TEST_DRIVER']).stem
session = project / '.uml-viewer/session.edn'
with open(os.environ['FAKE_VIEWER_LOG'], 'a') as log:
    log.write(json.dumps({{'script': script, 'argv': sys.argv[1:],
                          'session': session.read_text() if session.exists() else None,
                          'diagram': (project / 'diagram.edn').read_text()}}) + '\n')
for marker in json.loads(os.environ['FAKE_VIEWER_MARKERS']).get(script, []):
    (evidence / marker).write_text('')
if script == 'session':
    session.write_text('{{:saved-by :session}}')
'''

SESSION_MARKERS = {
    'session': ['saved-session.edn', 'session-save-pass.edn'],
    'session_restore': ['restored-session.edn', 'session-restart-pass.edn'],
    'session_legacy': ['session-legacy-pass.edn'],
    'session_love': ['session-love-pass.edn', 'love-session-restored.png'],
}


class ViewerDriverRunner(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='uml driver ')
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        viewer = root / 'fake-viewer'
        viewer.write_text(FAKE_VIEWER.format(python=sys.executable))
        viewer.chmod(0o755)
        self.log = root / 'launches.jsonl'
        self.root = root
        self.env = dict(os.environ, UML_VIEWER_RUST_EXECUTABLE=str(viewer),
                        FAKE_VIEWER_LOG=str(self.log),
                        FAKE_VIEWER_MARKERS=json.dumps(SESSION_MARKERS))

    def run_driver(self, name):
        evidence = self.root / f'evidence-{name}'
        result = subprocess.run([sys.executable, '-I', str(REPO / 'scripts/run_viewer_driver.py'),
                                 name, '--evidence', str(evidence)],
                                env=self.env, text=True, capture_output=True, timeout=60)
        launches = [json.loads(line) for line in self.log.read_text().splitlines()] \
            if self.log.exists() else []
        self.log.unlink(missing_ok=True)
        return result, launches

    def test_each_session_phase_runs_standalone_by_name(self):
        expected = {
            'session_restore': [('session', []), ('session_restore', ['--restart'])],
            'session_legacy': [('session_legacy', ['--restart'])],
            'session_love': [('session_love', ['--restart'])],
        }
        for name, launches_expected in expected.items():
            with self.subTest(driver=name):
                result, launches = self.run_driver(name)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn(f'PASS driver {name}', result.stdout)
                self.assertEqual(launches_expected,
                                 [(l['script'], l['argv'][1:-1]) for l in launches])
                for launch in launches:
                    self.assertEqual('viewer', launch['argv'][0])
                    self.assertEqual('diagram.edn', launch['argv'][-1])
                last = launches[-1]
                if name == 'session_restore':
                    # Restore reads the session the save phase wrote.
                    self.assertEqual('{:saved-by :session}', last['session'])
                    self.assertIn('"Session"', last['diagram'])
                elif name == 'session_legacy':
                    self.assertIn(':zoom 1.25', last['session'])
                    self.assertIn('"Session"', last['diagram'])
                else:
                    # The LOVE-written session, pointed at this project's diagram.
                    self.assertNotIn('/tmp/uml142', last['session'])
                    self.assertIn('diagram.edn', last['session'])

    def test_session_phase_requires_its_markers(self):
        self.env['FAKE_VIEWER_MARKERS'] = '{}'
        result, _ = self.run_driver('session_legacy')
        self.assertEqual(1, result.returncode)
        self.assertIn('legacy: driver did not publish session-legacy-pass.edn', result.stderr)

    def test_unknown_driver_names_the_known_ones_without_launching(self):
        result, launches = self.run_driver('session_nope')
        self.assertEqual(2, result.returncode)
        self.assertIn("unknown driver 'session_nope'", result.stderr)
        for known in ('session', 'session_restore', 'session_legacy', 'session_love', 'metrics'):
            self.assertIn(known, result.stderr)
        self.assertEqual([], launches)

if __name__ == '__main__':
    unittest.main()
