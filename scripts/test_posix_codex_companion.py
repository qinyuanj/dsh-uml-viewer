#!/usr/bin/env python3
"""占位进程 e2e：Codex 提供方的邮件、质量门控与正常关闭（#197）。

真实的部分是 companion worker/runner/controller 二进制和公共 ./uml；测试自己扮演
viewer，只用 worker 文件协议和邮件队列。Herdr 与 Codex 是 fixtures/codex-companion
下的替身：不启动窗口，不接触真实 Herdr 会话，不需要 Codex 登录或网络。
"""
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
import uuid

REPO = Path(__file__).resolve().parents[1]
FIXTURES = REPO / 'fixtures/codex-companion'
sys.path.insert(0, str(REPO / 'scripts'))
sys.path.insert(0, str(FIXTURES))
from fake_codex import join_queue, split_queue  # noqa: E402
from uml_posix import proc_identity, rust_command  # noqa: E402

UUID = re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}')
LEAKED_THREAD = '01a1a458-f60d-7561-aa06-d04df3bd14f7'


def wait_for(test, timeout=30, message='condition'):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = test()
        if value:
            return value
        time.sleep(0.02)
    raise AssertionError(f'timed out waiting for {message}')


def alive(pid):
    # A zombie is gone by contract; its parent may simply not have reaped it.
    if sys.platform == 'darwin':
        return proc_identity(pid) is not None
    try:
        return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[-1].split()[0] != 'Z'
    except (FileNotFoundError, ProcessLookupError):
        return False


class Companion:
    """One project's helper, driven the way the viewer drives it."""

    def __init__(self, test, name):
        self.test = test
        self.root = Path(tempfile.mkdtemp(prefix=f'uml codex {name} 项目 ', dir=test.base)).resolve()
        self.state = self.root / '.uml-viewer'
        self.state.mkdir()
        self.token = uuid.uuid4().hex
        self.base = self.state / f'companion-worker-{self.token}'
        self.sequence = 0
        self.seen = 0
        self.phases = []
        self.messages = []
        self.worker = None
        (self.root / 'src').mkdir()
        (self.root / 'src/a.py').write_text('def hello():\n    return 1\n')
        (self.root / 'policy.edn').write_text('{:lang :python :src "src" :prefix "src"}')
        shutil.copy2(REPO / 'scripts/uml', self.root / 'uml')
        (self.root / 'uml').chmod(0o755)
        tool = self.root / 'quality_tool.py'
        tool.write_text('import os, sys\nsys.exit(int(os.environ["QUALITY_EXIT"]))\n')
        operation = {'argv': [sys.executable, '-I', str(tool)], 'forwardArgs': 'none',
                     'validResultExitCodes': [0]}
        (self.root / 'uml-quality.json').write_text(json.dumps({'version': 1, 'operations': {
            'crap': dict(operation, scope='whole-tree'),
            'mutate': dict(operation, validResultExitCodes=[0, 3])}}))

    def start(self):
        viewer = proc_identity(os.getpid())
        self.request = self.base.with_name(self.base.name + '.request.edn')
        self.request.write_text(
            f'{{:base "{self.base}" :coding-agent :codex :restart false :root "{self.root}" '
            f':token "{self.token}" :viewer {{:pid {viewer["pid"]} :start "{viewer["start"]}"}}}}')
        self.worker = subprocess.Popen([self.test.binary, 'companion-worker', str(self.request)],
                                       cwd=self.root, env=self.test.env)
        self.test.addCleanup(self.abandon)
        return self

    def abandon(self):
        """A failed test may leave its helper running; never leave it behind."""
        if self.worker.poll() is None:
            self.worker.kill()
            self.worker.wait()

    def poll(self):
        while True:
            path = self.base.with_name(f'{self.base.name}.event-{self.seen + 1}.edn')
            if not path.exists():
                return
            text = path.read_text()
            self.test.assertIn(f':token "{self.token}"', text)
            self.phases.append(re.search(r':phase :([\w-]+)', text).group(1))
            message = re.search(r':message "((?:[^"\\]|\\.)*)"', text)
            self.messages.append(message.group(1) if message else None)
            self.seen += 1

    def wait_phase(self, phase, timeout=60):
        def reached():
            self.poll()
            return phase in self.phases or self.worker.poll() is not None
        wait_for(reached, timeout, f'{phase} in {self.root}')
        self.poll()
        self.test.assertIn(phase, self.phases, self.messages)

    def command(self, operation, extra):
        self.sequence += 1
        self.base.with_name(f'{self.base.name}.command-{self.sequence}.edn').write_text(
            f'{{:extra {extra} :operation :{operation} :token "{self.token}"}}')

    def mail(self, *operations):
        """Append mail exactly as the viewer does, waking once per delivery."""
        path = self.state / 'to-agent.edn'
        next_id, items = split_queue(path.read_text()) if path.exists() else (1, [])
        identifiers = []
        for operation in operations:
            items.append(f'{{:id {next_id} :op :{operation}}}')
            identifiers.append(next_id)
            next_id += 1
        path.write_text(join_queue(next_id, items))
        for identifier in identifiers:
            self.command('wake', f'{{:id {identifier}}}')
        return identifiers

    def journal(self, event=None):
        path = self.root / 'fake-codex.jsonl'
        entries = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return [entry for entry in entries if event is None or entry['event'] == event]

    def handled(self):
        return self.journal('mail')

    def agent_queue(self):
        return split_queue((self.state / 'to-agent.edn').read_text())[1]

    def take_displays(self):
        """Pop every queued viewer command, as the viewer session does."""
        path = self.state / 'to-viewer.edn'
        next_id, items = split_queue(path.read_text()) if path.exists() else (1, [])
        path.write_text(join_queue(next_id, []))
        return items

    def ownership(self):
        return (self.state / 'companion.edn').read_text()

    def close(self):
        """The viewer's normal close: stop without keeping the companion."""
        self.command('stop', '{:keep false}')
        self.wait_phase('stopped')
        self.test.assertEqual(0, self.worker.wait(30))


class CodexCompanion(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            cls.binary = rust_command(REPO)
        except ValueError:
            raise unittest.SkipTest('run cargo build --release --locked first')

    def setUp(self):
        # The socket path limit (101 bytes) rules out a long temporary root.
        self.base = tempfile.mkdtemp(prefix='umlcx')
        self.addCleanup(shutil.rmtree, self.base, True)
        self.herdr_state = Path(self.base, 'h')
        self.herdr_state.mkdir()
        bin_dir = Path(self.base, 'bin')
        bin_dir.mkdir()
        for name, script in (('herdr', 'fake_herdr.py'), ('codex', 'fake_codex.py')):
            (bin_dir / name).write_text(f'#!/bin/sh\nexec "{sys.executable}" -I -B '
                                        f'"{FIXTURES / script}" "$@"\n')
            (bin_dir / name).chmod(0o755)
        host = [d for d in os.environ['PATH'].split(':')
                if d and not (Path(d) / 'herdr').exists() and not (Path(d) / 'codex').exists()]
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith('HERDR_') and key != 'UML_VIEWER_PROJECT_ROOT'}
        self.env.update(PATH=':'.join([str(bin_dir), *host]), UML_COMPANION_NO_ATTACH='1',
                        UML_FAKE_HERDR=str(self.herdr_state), UML_VIEWER_TOOL_ROOT=str(REPO),
                        UML_VIEWER_RUST_EXECUTABLE=self.binary, CODEX_BIN=str(bin_dir / 'codex'),
                        XDG_CONFIG_HOME=str(Path(self.base, 'c')),
                        # A launch from inside another Codex session.
                        CODEX_THREAD_ID=LEAKED_THREAD)
        self.addCleanup(self.stop_everything)

    def stop_everything(self):
        """Whatever a failed test left: only the fake sessions' own processes."""
        for path in self.herdr_state.glob('*.json'):
            state = json.loads(path.read_text())
            pids = [pid for pane in state['panes'] for pid in pane['processes']]
            for pid in pids:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError):
                    pass
            if state.get('running'):
                try:
                    os.kill(state['server_pid'], signal.SIGKILL)
                except ProcessLookupError:
                    pass
        for journal in Path(self.base).glob('*/.uml-viewer/recovery.edn'):
            found = re.search(r':controller \{:pid (\d+)', journal.read_text())
            if found and not re.search(r':phase :(cancelled|failed|normal)', journal.read_text()):
                try:
                    os.kill(int(found.group(1)), signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def report(self, socket_path, pane, session):
        """What the integration hook sends for a Codex session in `pane`."""
        with socket.socket(socket.AF_UNIX) as connection:
            connection.connect(socket_path)
            with connection.makefile('rw') as stream:
                stream.write(json.dumps({'id': 'test', 'method': 'pane.report_agent_session', 'params': {
                    'pane_id': pane, 'source': 'herdr:codex', 'agent': 'codex',
                    'agent_session_id': session, 'session_start_source': 'startup'}}) + '\n')
                stream.flush()
                stream.readline()

    def herdr(self, *args):
        result = subprocess.run(['herdr', *args], env=self.env, capture_output=True, text=True,
                                check=True, timeout=30)
        return json.loads(result.stdout)

    def independent_codex(self, cwd):
        """A Codex TUI the user opened themselves, in their own Herdr session."""
        name = 'users-own-session'
        server = subprocess.Popen(['herdr', '--session', name, 'server'], env=self.env,
                                  stdout=subprocess.DEVNULL)
        self.addCleanup(server.wait)
        self.addCleanup(server.terminate)
        wait_for(lambda: self.herdr('--session', name, 'status', 'server', '--json')['running'],
                 message='the independent Herdr')
        pane = self.herdr('--session', name, 'workspace', 'create', '--cwd', str(cwd),
                          '--no-focus')['result']['root_pane']['pane_id']
        self.herdr('--session', name, 'pane', 'run', pane, 'exec sleep 300')
        session = str(uuid.uuid4())
        status = self.herdr('--session', name, 'status', 'server', '--json')
        self.report(status['socket'], pane, session)
        return name, pane, session

    def ledger(self):
        path = self.herdr_state / 'ledger.jsonl'
        return [json.loads(line) for line in path.read_text().splitlines()]

    def sessions(self):
        return {path.stem for path in self.herdr_state.glob('*.json')}

    def test_fresh_ready_mail_pop_display_and_normal_close(self):
        companion = Companion(self, 'fresh').start()
        companion.wait_phase('ready')
        self.assertEqual(['starting', 'server', 'workspace', 'runner', 'recovery', 'committed', 'ready'],
                         companion.phases)

        # Ready names the session the integration reported, and how to resume it.
        started = wait_for(lambda: companion.journal('start'), message='codex start')[0]
        owned = companion.ownership()
        self.assertRegex(started['session'], UUID)
        self.assertIn(':coding-agent :codex', owned)
        self.assertIn(f':codex-session "{started["session"]}"', owned)
        self.assertIn('"resume"', owned)
        self.assertNotIn(':claude-session', owned)
        self.assertIsNone(started['thread_env'], 'another session id leaked into the companion')
        self.assertIn('--dangerously-bypass-approvals-and-sandbox', started['argv'])

        # The launch prompt drives the first display, which really loads.
        wait_for(lambda: companion.journal('launch'), message='first display')
        first = companion.take_displays()
        self.assertEqual(['{:id 1 :op :display :path "diagram.edn"}'], first)
        subprocess.run([self.binary, 'scene', '--ir', str(companion.root / 'diagram.edn')], check=True,
                       capture_output=True, cwd=companion.root, env=self.env)

        # Three deliveries, woken as fast as the viewer writes them.
        identifiers = companion.mail('context', 'regen', 'regen')
        wait_for(lambda: len(companion.handled()) == 3, message='mail drained')
        self.assertEqual(identifiers, [entry['id'] for entry in companion.handled()],
                         'lost, reordered or repeated mail')
        self.assertEqual(['context', 'regen', 'regen'], [entry['op'] for entry in companion.handled()])
        self.assertEqual([], companion.agent_queue())
        self.assertEqual(['{:id 2 :op :display :path "diagram.edn"}',
                          '{:id 3 :op :display :path "diagram.edn"}'], companion.take_displays())

        companion.poll()
        lost = [m for m in companion.messages if m and 'wake lost' in m]
        self.assertEqual([], lost)
        self.assert_normal_close(companion, started)

    def test_quality_gate_refuses_display_and_later_fifo_mail_still_runs(self):
        companion = Companion(self, 'quality').start()
        companion.wait_phase('ready')
        wait_for(lambda: companion.journal('launch'), message='first display')
        companion.take_displays()
        cases = [
            ('context', {}, False, None),
            ('refresh-crap', {}, True, None),
            ('refresh-crap', {'exit': 7}, False, 'mismatched validResult'),
            ('refresh-crap', {'fault': 'stale'}, False, 'stale record'),
            ('refresh-crap', {'fault': 'missing'}, False, 'missing record'),
            ('refresh-crap', {'fault': 'malformed'}, False, 'malformed record'),
            ('refresh-crap', {'fault': 'other-version'}, False, 'mismatched version'),
            ('refresh-crap', {'fault': 'other-operation'}, False, 'mismatched operation'),
            ('refresh-crap', {'fault': 'other-root'}, False, 'mismatched projectRoot'),
            ('refresh-crap', {'fault': 'other-exit'}, False, 'mismatched exitCode'),
            ('refresh-crap', {'fault': 'invalid'}, False, 'mismatched validResult'),
            # Exit 3 is valid only where the project's operation declares it.
            ('refresh-mutate', {'exit': 3}, True, None),
            ('refresh-crap', {'exit': 3}, False, 'mismatched validResult'),
            # A valid result still needs IR exit 0.
            ('refresh-crap', {'policy': 'no-such-policy.edn'}, False, 'IR failed'),
            ('regen', {}, True, None),
        ]
        (companion.root / 'quality-faults.json').write_text(json.dumps(
            {str(index): case[1] for index, case in enumerate(cases, 1) if case[1]}))
        identifiers = companion.mail(*[case[0] for case in cases])
        wait_for(lambda: len(companion.handled()) == len(cases), 60, 'every mail handled')
        self.assertEqual(
            [(identifier, op, displayed, cause)
             for identifier, (op, _, displayed, cause) in zip(identifiers, cases)],
            [(entry['id'], entry['op'], entry['displayed'], entry['cause'])
             for entry in companion.handled()])
        self.assertEqual([], companion.agent_queue())
        self.assertEqual(3, len(companion.take_displays()), 'one :display per permitted refresh')
        # The public dispatch published this record, not the placeholder.
        record = json.loads((companion.state / 'quality-result.json').read_text())
        self.assertEqual((1, 'crap', str(companion.root), 0, True),
                         (record['version'], record['operation'], record['projectRoot'],
                          record['exitCode'], record['validResult']))
        companion.close()

    def test_quality_rules_reach_codex_in_its_launch_argv(self):
        companion = Companion(self, 'rules').start()
        companion.wait_phase('ready')
        argv = wait_for(lambda: companion.journal('start'), message='codex start')[0]['argv']
        instructions = tomllib.loads(next(a for a in argv if a.startswith('developer_instructions=')))
        rules = instructions['developer_instructions']
        for sentence in ('validResult exactly true', 'only IR exit 0 permits :display',
                         'Continue later FIFO', 'Pop the head of\n:queue as you handle it'):
            self.assertIn(sentence, rules)
        companion.close()

    def test_other_projects_and_an_independent_codex_session_are_left_alone(self):
        owned = Companion(self, 'owned')
        name, pane, session = self.independent_codex(owned.root)
        before_user = self.herdr('--session', name, 'api', 'snapshot')
        ledger_start = len(self.ledger())
        other = Companion(self, 'other').start()
        other.wait_phase('ready')
        owned.start()
        owned.wait_phase('ready')
        other_record, owned_record = other.ownership(), owned.ownership()
        other_session = re.search(r':session "([^"]+)"', other_record).group(1)
        owned_session = re.search(r':session "([^"]+)"', owned_record).group(1)
        self.assertNotEqual(other_session, owned_session)
        self.assertNotIn(session, owned_record, 'took over the independent session')
        self.assertNotIn(session, other_record, 'took over the independent session')

        identifiers = owned.mail('regen')
        wait_for(lambda: owned.handled(), message='owned project mail')
        self.assertEqual(identifiers, [entry['id'] for entry in owned.handled()])
        self.assertEqual([], other.handled(), 'mail crossed projects')
        started = owned.journal('start')[0]
        other_started = other.journal('start')[0]
        self.assert_normal_close(owned, started)

        # Another project's companion keeps running and keeps answering mail.
        self.assertEqual(other_record, other.ownership())
        self.assertTrue(alive(other_started['pid']))
        self.assertIn(other_session, self.sessions())
        other.mail('regen')
        wait_for(lambda: other.handled(), message='other project mail')
        self.assertEqual([1], [entry['id'] for entry in other.handled()])

        # The independent session was never asked anything and did not change.
        self.assertIn(name, self.sessions())
        self.assertEqual(before_user, self.herdr('--session', name, 'api', 'snapshot'))
        self.assertFalse((self.herdr_state / f'{pane}.prompts').exists())
        by_session = {}
        for entry in self.ledger()[ledger_start:]:
            by_session.setdefault(entry['session'], []).append(entry['command'])
        self.assertEqual([['api', 'snapshot']], by_session[name], 'only this test read it')
        for commands in (by_session[owned_session], by_session[other_session]):
            flat = json.dumps(commands)
            self.assertNotIn(name, flat)
        self.assertNotIn(other_session, json.dumps(by_session[owned_session]))
        self.assertNotIn(owned_session, json.dumps(by_session[other_session]))
        other.close()
        self.assertIn(name, self.sessions())

    def test_a_later_session_in_the_same_pane_gets_no_wake_and_keeps_the_mail(self):
        companion = Companion(self, 'later').start()
        companion.wait_phase('ready')
        wait_for(lambda: companion.journal('launch'), message='first display')
        started = companion.journal('start')[0]
        record = companion.ownership()
        socket_path = re.search(r':socket "([^"]+)"', record).group(1)
        pane = re.search(r':pane "([^"]+)"', record).group(1)
        self.report(socket_path, pane, str(uuid.uuid4()))

        identifiers = companion.mail('regen')

        def refused():
            companion.poll()
            return [m for m in companion.messages if m and 'Companion wake failed' in m]
        message = wait_for(refused, message='the wake refusal')[0]
        self.assertIn('Codex CLI agent occupies the owned Herdr pane with its saved session', message)
        self.assertEqual([f'{{:id {identifiers[0]} :op :regen}}'], companion.agent_queue(),
                         'the durable queue keeps refused mail')
        self.assertEqual([], [e for e in self.ledger() if e['command'][:2] == ['agent', 'prompt']])
        self.assert_normal_close(companion, started)

    def assert_normal_close(self, companion, started):
        before = self.sessions()
        companion.close()
        self.assertEqual('{}', companion.ownership().strip())
        self.assertEqual(['stopping', 'stopped'], companion.phases[-2:])
        self.assertEqual(1, len(before - self.sessions()), 'exactly its own session goes')
        wait_for(lambda: not alive(started['pid']), 10, 'the companion process to exit')
        self.assertFalse((companion.state / 'session.edn').exists(), 'normal close keeps no snapshot')
        journal = (companion.state / 'recovery.edn').read_text()
        self.assertRegex(journal, r':phase :(cancelled|normal)')
        controller = int(re.search(r':controller \{:pid (\d+)', journal).group(1))
        wait_for(lambda: not alive(controller), 10, 'the recovery controller to exit')
        time.sleep(0.3)
        self.assertEqual(1, len(companion.journal('start')), 'no recovery relaunch after a normal close')


if __name__ == '__main__':
    unittest.main()
