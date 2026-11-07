"""POSIX 公共入口的安全 restart：身份核对、真实退出等待与拒绝规则。

只通过公共 ./uml --restart（Windows: uml.cmd --restart）观察行为；进程用真实内核身份
（Linux /proc，macOS proc_pidinfo，Windows GetProcessTimes）。
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

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
from uml_posix import proc_identity  # noqa: E402
from test_posix_entries import (ENTRY, codex_fixture, fixture_path_without,  # noqa: E402
                                herdr_fixture, sleeper_argv)


def start_identity(pid):
    observed = proc_identity(pid)
    return None if observed is None else observed['start']


class RestartEntries(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='uml restart ')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.state = self.root / '.uml-viewer'
        self.state.mkdir()
        shutil.copy2(REPO / 'scripts' / ENTRY, self.root / ENTRY)
        (self.root / ENTRY).chmod(0o755)
        self.env = dict(os.environ, UML_VIEWER_TOOL_ROOT=str(REPO))
        self.env.pop('UML_VIEWER_PROJECT_ROOT', None)

    def invoke(self, *args):
        return subprocess.run([str(self.root / ENTRY), *args], cwd=self.root, env=self.env,
                              capture_output=True, text=True, timeout=60)

    def write(self, name, record):
        (self.state / name).write_text(json.dumps(record) + '\n')

    def running_sleeper(self):
        # A live process whose recorded identity can never match: the wrapper
        # must refuse it rather than take over an unrelated process.
        sleeper = subprocess.Popen(sleeper_argv(60))
        self.addCleanup(sleeper.wait, 10)
        self.addCleanup(sleeper.terminate)
        return sleeper

    def test_restart_without_a_launcher_record_is_refused(self):
        result = self.invoke('--restart', 'diagram.edn')
        self.assertEqual(1, result.returncode)
        self.assertIn('launcher 进程记录', result.stderr)

    def test_restart_refuses_a_reused_pid(self):
        sleeper = self.running_sleeper()
        self.write('viewer-process.json',
                   {'pid': sleeper.pid, 'started': '1', 'runtime': 'rust'})
        result = self.invoke('--restart', 'diagram.edn')
        self.assertEqual(1, result.returncode)
        self.assertIn('身份已变化', result.stderr)
        self.assertIsNone(sleeper.poll(), 'a refused restart must not touch the live process')

    def test_restart_refuses_an_unknown_runtime(self):
        sleeper = self.running_sleeper()
        self.write('viewer-process.json',
                   {'pid': sleeper.pid, 'started': start_identity(sleeper.pid), 'runtime': 'jvm'})
        result = self.invoke('--restart', 'diagram.edn')
        self.assertEqual(1, result.returncode)
        self.assertIn('运行时', result.stderr)

    def test_restart_refuses_a_removed_love_record(self):
        # #144: a session recorded by the retired LOVE viewer must be refused
        # instead of being taken over by the Rust viewer.
        sleeper = self.running_sleeper()
        self.write('viewer-process.json',
                   {'pid': sleeper.pid, 'started': start_identity(sleeper.pid), 'runtime': 'love'})
        result = self.invoke('--restart', 'diagram.edn')
        self.assertEqual(1, result.returncode)
        self.assertIn('运行时', result.stderr)
        self.assertIn('love', result.stderr)
        self.assertIsNone(sleeper.poll(), 'a refused restart must not touch the live process')

    def test_restart_refuses_a_malformed_record(self):
        self.write('viewer-process.json', {'pid': 'not-a-pid', 'started': 'abc'})
        result = self.invoke('--restart', 'diagram.edn')
        self.assertEqual(1, result.returncode)
        self.assertIn('身份记录无效', result.stderr)


    def test_restart_refuses_unverifiable_viewer_identity_without_waiting(self):
        # Ported from the retired Windows suite: a changed viewer identity and
        # a missing identity both refuse before any wait, without touching the
        # live launcher or writing a launch record.
        sleeper = self.running_sleeper()
        # A launcher that has already exited: the wait for it returns at once,
        # so only the viewer identity can refuse the restart.
        exited = subprocess.Popen([sys.executable, '-c', 'pass'])
        exited.wait()
        launcher = {'pid': exited.pid, 'started': '1', 'runtime': 'rust'}
        self.write('viewer-process.json', launcher)
        before = (self.state / 'viewer-process.json').read_bytes()
        identity = self.state / 'viewer-process-identity.json'
        for name, record in (
            ('changed identity', {'pid': sleeper.pid, 'started': '1', 'runtime': 'rust'}),
            ('invalid identity', None),
        ):
            with self.subTest(name=name):
                if record is None:
                    identity.write_text('{"pid": 1}\n')
                else:
                    self.write('viewer-process-identity.json', record)
                began = time.monotonic()
                result = self.invoke('--restart', 'diagram.edn')
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn('身份', result.stderr)
                self.assertNotIn('仍在运行', result.stderr)
                self.assertLess(time.monotonic() - began, 10,
                                'an identity refusal must not wait on the live launcher')
                self.assertEqual(before, (self.state / 'viewer-process.json').read_bytes())
                self.assertEqual([], list(self.state.glob('launch-*.json')))


    SESSION = '01a1a458-f60d-7561-aa06-d04df3bd14f7'

    def ownership(self, provider=''):
        root = str(self.root.resolve())
        text = ('{:backend :herdr :owner-role :companion :cwd ' + json.dumps(root) +
                ' :session "uml-viewer-A" :pane "w1:p1" :socket "/s/herdr.sock" :workspace "w1"'
                ' :terminal-id "term_A" :shell-pid 42 :owner-start "ss" :server-pid 41'
                ' :server-start "rs" ' + provider + '}')
        (self.state / 'companion.edn').write_text(text)
        return text

    def live_launcher(self):
        # A live launcher whose identity matches: a refusal must come from the
        # preflight, before the wrapper waits for it to exit.
        sleeper = self.running_sleeper()
        self.write('viewer-process.json',
                   {'pid': sleeper.pid, 'started': start_identity(sleeper.pid), 'runtime': 'rust'})
        return sleeper

    def fixture_path(self, herdr='0.9.5', codex=None):
        bin_dir = self.root / 'fixture bin'
        herdr_fixture(bin_dir, herdr)
        if codex:
            codex_fixture(bin_dir / 'codex', codex)
        self.env['PATH'] = fixture_path_without(('herdr', 'codex'), bin_dir)

    def test_restart_refuses_a_provider_that_conflicts_with_ownership_before_waiting(self):
        self.fixture_path(codex='0.166.0')
        codex = f':coding-agent :codex :codex-session "{self.SESSION}"'
        for recorded, requested, label in (('', 'codex', 'Claude Code'), (codex, 'claude', 'Codex CLI')):
            with self.subTest(recorded=label):
                text = self.ownership(recorded)
                sleeper = self.live_launcher()
                before = (self.state / 'viewer-process.json').read_bytes()
                began = time.monotonic()
                result = self.invoke('--restart', '--coding-agent', requested, 'diagram.edn')
                self.assertEqual(1, result.returncode, result.stdout + result.stderr)
                self.assertIn(f'records {label}', result.stderr)
                self.assertIn(f'--coding-agent {requested} conflicts', result.stderr)
                self.assertLess(time.monotonic() - began, 10, 'refused without waiting')
                self.assertIsNone(sleeper.poll(), 'the live viewer is not touched')
                self.assertEqual(text, (self.state / 'companion.edn').read_text())
                self.assertEqual(before, (self.state / 'viewer-process.json').read_bytes())

    def test_restart_reuses_the_recorded_codex_provider_and_its_preflight(self):
        # No option: the recorded Codex provider decides, so its Herdr floor
        # refuses a 0.9.4 that a Claude project would accept.
        self.fixture_path(herdr='0.9.4', codex='0.166.0')
        text = self.ownership(f':coding-agent :codex :codex-session "{self.SESSION}"')
        sleeper = self.live_launcher()
        result = self.invoke('--restart', 'diagram.edn')
        self.assertEqual(1, result.returncode, result.stdout + result.stderr)
        self.assertIn('the selected Herdr 0.9.4', result.stderr)
        self.assertIsNone(sleeper.poll())
        self.assertEqual(text, (self.state / 'companion.edn').read_text())


if __name__ == '__main__':
    unittest.main()
