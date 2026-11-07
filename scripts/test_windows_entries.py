"""Native Windows entries (ADR 0010): `uml.cmd`, `get-uml-viewer.cmd`, junction
development installs, `.exe` naming, `GetProcessTimes` identity and NTSTATUS exits.

The shared entry behaviour lives in test_posix_entries.py and runs on Windows
too; this file holds what only a Windows host has. Off Windows it skips.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
import uml_posix  # noqa: E402
from test_posix_entries import herdr_fixture, native_program, tree  # noqa: E402

WINDOWS = os.name == 'nt'
# FILETIME ticks (100 ns since 1601) at the Unix epoch.
UNIX_EPOCH_TICKS = 116444736000000000
IO_REPARSE_TAG_MOUNT_POINT = 0xA0000003


@unittest.skipUnless(WINDOWS, 'native Windows entries')
class WindowsEntries(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='uml 空格项目 ')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        shutil.copy2(REPO / 'scripts/uml.cmd', self.root / 'uml.cmd')
        self.env = dict(os.environ, UML_VIEWER_TOOL_ROOT=str(REPO))

    def recording_python(self, code=0):
        """A fake interpreter that records its argv as JSON and exits `code`."""
        record = self.root / 'python-argv.json'
        python = native_program(self.root / 'fake python/python', (
            'import json, sys\n'
            f'open({str(record)!r}, "w", encoding="utf-8").write(json.dumps(sys.argv[1:]))\n'
            f'raise SystemExit({code})\n'))
        return python, record

    # --- the .cmd entries -------------------------------------------------

    def test_the_cmd_entries_are_ascii_crlf_and_the_root_copy_is_identical(self):
        # cmd reads a batch file in the OEM code page and seeks labels by
        # byte offset: ASCII and CRLF on every checkout (.gitattributes).
        for name in ('scripts/uml.cmd', 'scripts/get-uml-viewer.cmd', 'uml.cmd'):
            with self.subTest(name=name):
                data = (REPO / name).read_bytes()
                self.assertTrue(data.isascii())
                self.assertEqual(data.count(b'\n'), data.count(b'\r\n'))
                attribute = subprocess.run(['git', '-C', str(REPO), 'check-attr', 'eol', name],
                                           capture_output=True, text=True)
                self.assertTrue(attribute.stdout.strip().endswith('eol: crlf'), attribute.stdout)
        self.assertEqual((REPO / 'scripts/uml.cmd').read_bytes(), (REPO / 'uml.cmd').read_bytes())
        self.assertIn(b'Thin Windows entry', (REPO / 'scripts/uml.cmd').read_bytes())

    def test_uml_cmd_help_is_the_shared_help(self):
        # The same Python command() answers `./uml --help` on POSIX.
        expected = subprocess.run([sys.executable, '-I', '-B', str(REPO / 'scripts/uml_posix.py'),
                                   'command', str(self.root), '--help'],
                                  capture_output=True, text=True, encoding='utf-8')
        result = subprocess.run([str(self.root / 'uml.cmd'), '--help'], cwd=self.root, env=self.env,
                                capture_output=True, text=True, encoding='utf-8', timeout=30)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(expected.stdout, result.stdout)
        for command in ('--restart', 'ir', 'scene', 'crap', 'mutate', 'dry'):
            self.assertIn(command, result.stdout)

    def test_uml_cmd_runs_the_configured_python_with_the_literal_argv(self):
        python, record = self.recording_python()
        env = dict(self.env, UML_PYTHON_EXECUTABLE=str(python))
        arguments = ['ir', '结果 图.edn', "it's", 'literal;$value', '$(touch injected)']
        result = subprocess.run([str(self.root / 'uml.cmd'), *arguments], cwd=self.root, env=env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(['-I', '-B', str(REPO / 'scripts/uml_posix.py'), 'command',
                          str(self.root) + '\\.', *arguments], json.loads(record.read_text('utf-8')))
        self.assertFalse((self.root / 'injected').exists())

    def test_uml_cmd_defaults_the_tool_root_to_the_project_install(self):
        python, record = self.recording_python()
        env = dict(self.env, UML_PYTHON_EXECUTABLE=str(python), UML_VIEWER_TOOL_ROOT='')
        subprocess.run([str(self.root / 'uml.cmd'), 'ir'], cwd=self.root, env=env, timeout=30)
        self.assertEqual(str(self.root / '.uml-viewer/uml-viewer/scripts/uml_posix.py'),
                         json.loads(record.read_text('utf-8'))[2])

    def test_uml_cmd_returns_the_real_exit_from_cmd_and_powershell(self):
        python, _ = self.recording_python(code=7)
        env = dict(self.env, UML_PYTHON_EXECUTABLE=str(python))
        entry = str(self.root / 'uml.cmd')
        callers = {
            'direct': [entry, 'ir'],
            # A raw command line: list2cmdline would escape cmd's own quotes.
            'cmd': f'cmd.exe /d /s /c ""{entry}" ir"',
            # PowerShell only calls the entry here; there is no PowerShell entry.
            'powershell': ['powershell.exe', '-NoProfile', '-NonInteractive', '-Command',
                           f"& '{entry}' ir; exit $LASTEXITCODE"],
        }
        for caller, argv in callers.items():
            with self.subTest(caller=caller):
                result = subprocess.run(argv, cwd=self.root, env=env, capture_output=True,
                                        text=True, timeout=60)
                self.assertEqual(7, result.returncode, result.stdout + result.stderr)

    def test_uml_cmd_names_a_missing_configured_python(self):
        env = dict(self.env, UML_PYTHON_EXECUTABLE=str(self.root / 'no such python.exe'))
        result = subprocess.run([str(self.root / 'uml.cmd'), 'ir'], cwd=self.root, env=env,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(1, result.returncode)
        self.assertIn('Python 3.11+', result.stderr)

    def test_the_standalone_installer_bootstraps_with_curl(self):
        # Without uml_posix.py beside it, get-uml-viewer.cmd fetches the
        # implementation with curl.exe and removes its download afterwards.
        alone = self.root / 'download'
        alone.mkdir()
        shutil.copy2(REPO / 'scripts/get-uml-viewer.cmd', alone / 'get-uml-viewer.cmd')
        temporary = self.root / 'bootstrap temp'
        temporary.mkdir()
        env = dict(self.env, TEMP=str(temporary), TMP=str(temporary),
                   UML_VIEWER_INSTALLER_URL=(REPO / 'scripts/uml_posix.py').as_uri())
        result = subprocess.run([str(alone / 'get-uml-viewer.cmd'), '--help'], cwd=self.root,
                                env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        self.assertIn('usage: get-uml-viewer [--install-only]', result.stdout)
        self.assertEqual([], list(temporary.iterdir()))
        env['UML_VIEWER_INSTALLER_URL'] = (self.root / 'missing.py').as_uri()
        result = subprocess.run([str(alone / 'get-uml-viewer.cmd'), '--help'], cwd=self.root,
                                env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(1, result.returncode)
        self.assertEqual([], list(temporary.iterdir()))

    def test_the_font_downloads_over_http_with_curl_and_reads_files_directly(self):
        # urllib stalls on GitHub release downloads on Windows hosts where
        # curl.exe completes, so http(s) goes through curl.
        record = self.root / 'curl-argv.json'
        curl = native_program(self.root / 'fake curl/curl', (
            'import json, sys\n'
            f'open({str(record)!r}, "w", encoding="utf-8").write(json.dumps(sys.argv[1:]))\n'
            'open(sys.argv[sys.argv.index("-o") + 1], "wb").write(b"archive")\n'))
        archive = self.root / 'archive.7z'
        url = 'https://example.invalid/Sarasa.7z'
        with mock.patch.object(uml_posix.shutil, 'which', return_value=str(curl)):
            uml_posix.download_font_archive(url, archive, 60)
        self.assertEqual(['-fsSL', '--connect-timeout', '60', '--speed-limit', '1', '--speed-time', '60',
                          '-o', str(archive), url], json.loads(record.read_text('utf-8')))
        self.assertEqual(b'archive', archive.read_bytes())
        # A file:// pin (offline provisioning) never needs curl.
        source = self.root / 'local.7z'
        source.write_bytes(b'local')
        with mock.patch.object(uml_posix.shutil, 'which', side_effect=AssertionError('curl')):
            uml_posix.download_font_archive(source.as_uri(), archive, 60)
        self.assertEqual(b'local', archive.read_bytes())

    # --- declared platform ------------------------------------------------

    def test_an_old_windows_build_is_refused_before_anything_is_created(self):
        version = mock.Mock(build=19044)
        cwd = os.getcwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, cwd)
        with mock.patch.object(uml_posix.sys, 'getwindowsversion', return_value=version):
            with self.assertRaises(ValueError) as refused:
                uml_posix.install(['--install-only'])
        self.assertEqual('Windows build 19044 is not supported: the minimum is '
                         'Windows 10 22H2 (build 19045)', str(refused.exception))
        self.assertEqual(['uml.cmd'], sorted(p.name for p in self.root.iterdir()))

    # --- junction development installs ------------------------------------

    def test_a_development_link_is_a_junction_and_fetch_refuses_it(self):
        state = self.root / '.uml-viewer'
        state.mkdir()
        link = state / 'uml-viewer'
        uml_posix.link_directory(link, REPO)
        # A junction (no symlink privilege needed), recognised as a link.
        self.assertEqual(IO_REPARSE_TAG_MOUNT_POINT, os.lstat(link).st_reparse_tag)
        self.assertFalse(link.is_symlink())
        self.assertTrue(uml_posix.is_link(link))
        self.assertEqual(REPO.resolve(), link.resolve())
        with self.assertRaises(ValueError) as refused:
            uml_posix.fetch(link, 'uml-viewer')
        self.assertIn('UML_VIEWER_TOOL_ROOT', str(refused.exception))
        # The legacy cleanup never deletes through it.
        before = tree(state)
        uml_posix.remove_legacy_runtime(state)
        self.assertEqual(before, tree(state))
        self.assertTrue((REPO / 'Cargo.toml').is_file())

    # --- .exe naming ------------------------------------------------------

    def test_tool_paths_use_exe_and_scripts(self):
        self.assertEqual('target/release/uml-viewer.exe', uml_posix.BINARY)
        self.assertEqual('Scripts/python.exe', uml_posix.VENV_PYTHON)
        self.assertEqual(['herdr.com', 'herdr.exe'],
                         sorted(uml_posix.native_names('herdr')))

    def test_herdr_selection_takes_the_exe_and_never_a_script_wrapper(self):
        chosen = self.root / 'chosen'
        herdr_fixture(chosen)
        wrapper = self.root / 'wrapper'
        wrapper.mkdir()
        (wrapper / 'herdr.cmd').write_text('@echo herdr 9.9.9\r\n')
        path = os.pathsep.join([str(wrapper), str(chosen)])
        with mock.patch.dict(os.environ, {'PATH': path}):
            self.assertEqual((chosen / 'herdr.exe').resolve(), uml_posix.select_herdr())

    def test_the_viewer_gets_its_own_group_hidden_console_and_leaves_the_job(self):
        from uml_launch import RENDER_DEFAULTS, detached
        flags = detached()['creationflags']
        for flag in (subprocess.CREATE_NEW_PROCESS_GROUP, subprocess.CREATE_NO_WINDOW,
                     subprocess.CREATE_BREAKAWAY_FROM_JOB):
            self.assertTrue(flags & flag, hex(flag))
        self.assertFalse(flags & subprocess.DETACHED_PROCESS)
        self.assertEqual({}, RENDER_DEFAULTS)

    def test_preflight_decodes_utf8_under_a_declared_legacy_locale(self):
        from uml_launch import preflight
        diagnostic = 'selected Herdr: C:\\Users\\\u4e01\\Herdr 空格\\herdr.exe'
        script = self.root / 'preflight.py'
        script.write_text('import sys\n'
                          f'sys.stderr.buffer.write({diagnostic.encode("utf-8")!r})\n'
                          'sys.stdout.buffer.write(b"herdr\\n")\n'
                          'raise SystemExit(int(sys.argv[-1]))\n', encoding='utf-8')
        # Declare the decoding locale independently of the machine's code page.
        with mock.patch('locale.getencoding', return_value='cp1252'):
            preflight([sys.executable, str(script)], self.root, ['0'])
            with self.assertRaisesRegex(ValueError, '丁') as refused:
                preflight([sys.executable, str(script)], self.root, ['1'])
        self.assertEqual(diagnostic, str(refused.exception))

    def test_herdr_schema_decodes_utf8_under_a_declared_legacy_locale(self):
        directory = self.root / 'herdr'
        native_program(directory / 'herdr',
                       'import sys\n'
                       'sys.stdout.buffer.write(b"herdr 0.9.5" if "--version" in sys.argv '
                       'else \'{"protocol":22,"description":"丁"}\'.encode("utf-8"))\n')
        with mock.patch.dict(os.environ, {'PATH': str(directory)}), \
                mock.patch('locale.getencoding', return_value='cp1252'):
            self.assertEqual((directory / 'herdr.exe').resolve(), uml_posix.select_herdr())

    def test_native_close_targets_only_the_titled_viewer_root(self):
        import ctypes
        from ctypes import wintypes
        import run_viewer_driver
        import test_windows_e2e
        user = ctypes.WinDLL('user32', use_last_error=True)
        user.CreateWindowExW.restype = wintypes.HWND
        user.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR,
                                        wintypes.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                        ctypes.c_int, wintypes.HWND, wintypes.HMENU,
                                        wintypes.HINSTANCE, wintypes.LPVOID]
        user.DestroyWindow.argtypes = [wintypes.HWND]
        user.PeekMessageW.argtypes = [ctypes.POINTER(wintypes.MSG), wintypes.HWND,
                                     wintypes.UINT, wintypes.UINT, wintypes.UINT]
        windows = []
        for title in ('UML viewer — close regression', '', 'Winit Thread Event Target', 'Other app'):
            hwnd = user.CreateWindowExW(0, 'STATIC', title, 0x10000000,
                                        0, 0, 100, 100, None, None, None, None)
            self.assertTrue(hwnd, ctypes.get_last_error())
            windows.append(hwnd)
            self.addCleanup(user.DestroyWindow, hwnd)
        for close in (lambda: run_viewer_driver.terminate_politely(mock.Mock(pid=os.getpid())),
                      lambda: test_windows_e2e.close_window(os.getpid())):
            close()
            for index, hwnd in enumerate(windows):
                message = wintypes.MSG()
                posted = bool(user.PeekMessageW(ctypes.byref(message), hwnd, 0x0010, 0x0010, 1))
                self.assertEqual(index == 0, posted, f'WM_CLOSE window index {index}')

    # --- process identity -------------------------------------------------

    def test_identity_is_the_decimal_creation_filetime(self):
        began = time.time()
        child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])
        self.addCleanup(child.wait)
        self.addCleanup(child.kill)
        observed = uml_posix.proc_identity(child.pid)
        self.assertEqual(child.pid, observed['pid'])
        self.assertTrue(observed['start'].isdigit(), observed)
        created = (int(observed['start']) - UNIX_EPOCH_TICKS) / 1e7
        self.assertLess(abs(created - began), 5)
        self.assertEqual(observed, uml_posix.proc_identity(child.pid))

    def test_identity_is_none_once_exited_even_while_its_handle_is_open(self):
        child = subprocess.Popen([sys.executable, '-c', 'pass'])
        child.wait()
        # Popen still holds the process handle, so the pid is not reused.
        self.assertIsNone(uml_posix.proc_identity(child.pid))
        self.assertIsNone(uml_posix.proc_identity(0))

    # --- quality dispatch -------------------------------------------------

    def configure(self, argv, codes=(0,)):
        operation = {'argv': argv, 'forwardArgs': 'none', 'validResultExitCodes': list(codes)}
        (self.root / 'uml-quality.json').write_text(json.dumps(
            {'version': 1, 'operations': {'crap': dict(operation, scope='whole-tree'),
                                          'mutate': operation}}))

    def entry(self, *args):
        return subprocess.run([str(self.root / 'uml.cmd'), *args], cwd=self.root, env=self.env,
                              capture_output=True, text=True, encoding='utf-8', timeout=60)

    def test_a_configured_exe_runs_and_script_wrappers_are_refused(self):
        self.configure([sys.executable, '-c', 'raise SystemExit(0)'])
        self.assertEqual(0, self.entry('crap').returncode)
        self.assertTrue(json.loads((self.root / '.uml-viewer/quality-result.json')
                                   .read_text('utf-8'))['validResult'])
        for wrapper in ('tool.cmd', 'tool.BAT', 'tool.ps1'):
            with self.subTest(wrapper=wrapper):
                self.configure([str(self.root / wrapper)])
                result = self.entry('crap')
                self.assertEqual(1, result.returncode)
                self.assertIn('.exe', result.stderr)
                self.assertFalse((self.root / '.uml-viewer/quality-result.json').exists())

    def test_an_ntstatus_exit_becomes_128_plus_signal_in_the_record_and_the_entry(self):
        crash = ('import ctypes; ctypes.windll.kernel32.ExitProcess.argtypes = (ctypes.c_uint,);'
                 ' ctypes.windll.kernel32.ExitProcess({code})')
        for raw, expected in ((0xC000013A, 130), (0xC0000005, 139), (0xC0000409, 134),
                              (0xE0434352, 255)):
            with self.subTest(raw=hex(raw)):
                self.configure([sys.executable, '-c', crash.format(code=raw)], codes=(0, 130))
                result = self.entry('mutate')
                self.assertEqual(expected, result.returncode, result.stderr)
                self.assertIn(f'0x{raw:08X}', result.stderr)
                record = json.loads((self.root / '.uml-viewer/quality-result.json').read_text('utf-8'))
                self.assertEqual((expected, expected == 130), (record['exitCode'], record['validResult']))

    def test_native_exit_maps_only_out_of_range_codes(self):
        for code in (0, 1, 3, 255):
            self.assertEqual(code, uml_posix.native_exit(code))
        self.assertEqual(136, uml_posix.native_exit(0xC0000094))
        self.assertEqual(255, uml_posix.native_exit(256))


if __name__ == '__main__':
    unittest.main()
