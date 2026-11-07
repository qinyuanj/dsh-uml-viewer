"""Native Windows seam 2, through uml.cmd and real isolated Herdr servers.

No real coding-agent conversation: a native placeholder records its starts and
exits on request, as in test_posix_recovery_e2e. The runner, controller, pane,
viewer, restart entry, ownership checks and normal WM_CLOSE cleanup are real.
Every started project is closed in finally, including on assertion failure.
Run after the offline installer acceptance recipe. --quality-project may
name an empty scratch project installed with get-uml-viewer.cmd --install-only;
it adds deterministic companion mail through real crap/dry/mutate and IR.
That option writes fixture sources in the named scratch project.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
from test_posix_entries import native_program  # noqa: E402
from uml_posix import proc_identity  # noqa: E402
from uml_windows import close_window  # noqa: E402
from compare_ir import parse, canonical  # noqa: E402


def session_fields(text):
    kind, pairs = parse(text)
    assert kind == 'map'
    return {key[1]: canonical(value) for key, value in pairs}

PLACEHOLDER = '''import os,sys,time,json
from pathlib import Path
here = Path(sys.argv[0]).parent
with (here/'starts.log').open('a',encoding='utf-8') as log:
    log.write(json.dumps({'at':time.time(),'pid':os.getpid(),'root':os.environ.get('UML_COMPANION_ROOT'),'cwd':os.getcwd()})+'\\n')
while True:
    request = here/'exit-code'
    if request.exists():
        code=int(request.read_text()); request.unlink(); raise SystemExit(code)
    time.sleep(.025)
'''


def wait_for(test, seconds=30, message='condition', interval=.05):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = test()
        if value:
            return value
        time.sleep(interval)
    raise AssertionError('timed out waiting for ' + message)


def read(path):
    try:
        return Path(path).read_text(encoding='utf-8')
    except (FileNotFoundError, PermissionError):
        return ''


def field(text, key):
    # JSON and EDN string escaping coincide for the paths and tokens here.
    found = re.search(r':' + re.escape(key) + r' ("(?:[^"\\]|\\.)*")', text)
    return json.loads(found[1]) if found else None


def number(text, key):
    found = re.search(r':' + re.escape(key) + r' (\d+)', text)
    return int(found[1]) if found else None


def phase(text):
    found = re.search(r':phase :([a-z-]+)', text)
    return found[1] if found else None


def alive(pid, start=None):
    identity = proc_identity(pid) if pid else None
    return identity is not None and (start is None or identity['start'] == start)


def run(argv, env, cwd=None, timeout=30):
    return subprocess.run(argv, env=env, cwd=cwd, capture_output=True, text=True,
                          encoding='utf-8', errors='replace', timeout=timeout)


def inventory(env):
    result = run(['herdr', 'session', 'list', '--json'], env)
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)['sessions']


class Project:
    def __init__(self, root, runtime, evidence, tool):
        self.root = root
        self.state = root / '.uml-viewer'
        self.evidence = evidence / root.name
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.state.mkdir(parents=True)
        self.pid = None
        self.session = None
        self.next_id = 1
        self.env = dict(os.environ, XDG_CONFIG_HOME=str(runtime), UML_VIEWER_TOOL_ROOT=str(tool),
                        UML_VIEWER_EVIDENCE=str(self.evidence), UML_VIEWER_TEST_SECONDS='600',
                        UML_COMPANION_NO_ATTACH='1', PYTHONIOENCODING='utf-8',
                        UML_VIEWER_RUST_EXECUTABLE=str(tool / 'target/release/uml-viewer.exe'))
        for key in ('UML_VIEWER_PROJECT_ROOT', 'UML_VIEWER_TEST_DRIVER', 'UML_COMPANION_ROOT'):
            self.env.pop(key, None)
        child = native_program(root / 'companion-placeholder', PLACEHOLDER)
        self.env['CLAUDE_BIN'] = str(child)
        shutil.copy2(REPO / 'scripts/uml.cmd', root / 'uml.cmd')
        shutil.copy2(REPO / 'examples/uml-viewer.edn', root / 'diagram.edn')
        shutil.copy2(REPO / 'examples/uml-viewer.edn', root / 'second.edn')

    def ownership(self):
        return read(self.state / 'companion.edn').strip()

    def starts(self):
        return [json.loads(line) for line in read(self.root / 'starts.log').splitlines() if line]

    def journal(self):
        return read(self.state / 'recovery.edn')

    def launch(self, restart=False):
        args = [str(self.root / 'uml.cmd')]
        if restart:
            args.append('--restart')
        result = run(args, self.env, self.root, 90)
        assert result.returncode == 0, f'{self.root.name}: {result.stdout}\n{result.stderr}'
        self.pid = int(result.stdout.split('launcher pid ')[1].split(')')[0])
        def ready():
            events = [read(path) for path in self.state.glob('companion-worker-*.event-*.edn')]
            failures = [event for event in events if ':phase :failed' in event]
            assert not failures, failures
            return any(':phase :ready' in event for event in events)
        wait_for(ready, 120, message=f'{self.root.name} ready')
        record = self.ownership()
        self.session = field(record, 'session')
        assert ':backend :herdr' in record and ':provisional' not in record, record
        for key in ('owner-start', 'server-start'):
            assert field(record, key).isdigit() and len(field(record, key)) >= 17, record
        wait_for(lambda: self.starts(), message='child start')
        child = self.starts()[-1]
        assert Path(child['root']) == self.root and Path(child['cwd']) == self.root, child
        assert alive(self.pid), 'viewer alive'
        print(f'PASS {self.root.name}: native Herdr ready, FILETIME identities', flush=True)

    def mail(self, operations):
        commands = []
        for operation in operations:
            commands.append('{:id ' + str(self.next_id) + ' ' + operation + '}')
            self.next_id += 1
        text = '{:next-id ' + str(self.next_id) + ' :queue [' + ' '.join(commands) + ']}'
        target = self.state / 'to-viewer.edn'
        temporary = target.with_suffix('.tmp')
        temporary.write_text(text, encoding='utf-8', newline='\n')
        os.replace(temporary, target)
        wait_for(lambda: ':queue []' in read(target), message='FIFO drained')

    def request_exit(self, code):
        request = self.root / 'exit-code'
        temporary = request.with_suffix('.tmp')
        temporary.write_text(str(code))
        os.replace(temporary, request)

    def crash(self, attempt, recover=True):
        before = len(self.starts())
        self.request_exit(7)
        backoff = wait_for(lambda: self.journal() if phase(self.journal()) == 'backoff' else None,
                           5, message=f'backoff {attempt}', interval=.01)
        runner = read(field(backoff, 'runner-state'))
        scheduled = number(backoff, 'deadline') - number(runner, 'at')
        assert 1000 * 2 ** (attempt - 1) <= scheduled <= 1000 * 2 ** (attempt - 1) + 500, backoff
        if recover:
            wait_for(lambda: len(self.starts()) == before + 1, 25, message=f'retry {attempt}')
            wait_for(lambda: phase(self.journal()) == 'running' and
                     number(self.journal(), 'attempts') == attempt, 15, message='recovery running')
        print(f'PASS {self.root.name}: retry {attempt} scheduled {scheduled} ms', flush=True)

    def close(self):
        if alive(self.pid):
            wait_for(lambda: close_window(self.pid), 10, message='viewer window')
            wait_for(lambda: not alive(self.pid), 30, message='WM_CLOSE viewer exit')
        wait_for(lambda: self.ownership() in ('', '{}'), 30, message='ownership released')
        if self.session:
            assert self.session not in [entry['name'] for entry in inventory(self.env)], 'session removed'
        for child in self.starts():
            assert not alive(child['pid']), 'child released'
        print(f'PASS {self.root.name}: WM_CLOSE cancels recovery and releases only owned resources', flush=True)

    def cleanup(self):
        # Always try the public normal-close path first. On a failed startup,
        # clean only the isolated session this harness read from its own file.
        try:
            self.close()
        except (AssertionError, OSError, subprocess.TimeoutExpired) as error:
            print(f'cleanup fallback for {self.root.name}: {error}', flush=True)
            record = self.ownership()
            name = self.session or field(record, 'session')
            if name and name.startswith('uml-viewer-'):
                run(['herdr', '--session', name, 'session', 'stop', name, '--json'], self.env)
                run(['herdr', '--session', name, 'session', 'delete', name, '--json'], self.env)
            if alive(self.pid):
                subprocess.run(['taskkill', '/PID', str(self.pid), '/T', '/F'], capture_output=True, timeout=20)
        finally:
            for path in self.state.glob('*'):
                if path.is_file() and path.suffix in ('.edn', '.log', '.err', '.json', '.toml'):
                    shutil.copy2(path, self.evidence / path.name)
            if (self.root / 'starts.log').exists():
                shutil.copy2(self.root / 'starts.log', self.evidence / 'starts.log')


QUALITY_PLACEHOLDER = r'''
import json, os, re, subprocess, sys, time
from pathlib import Path
state = Path('.uml-viewer')
mail = state / 'to-agent.edn'
while True:
    if mail.exists():
        text = mail.read_text(encoding='utf-8')
        match = re.search(r':queue \[\s*(\{:id (\d+) :op :refresh-(crap|dry|mutate)[^}]*\})', text)
        if match:
            head, identifier, operation = match[1], int(match[2]), match[3]
            temporary = mail.with_suffix('.fixture.tmp')
            temporary.write_text(text.replace(head, '', 1), encoding='utf-8', newline='\n')
            os.replace(temporary, mail)
            record = state / 'quality-result.json'
            record.unlink(missing_ok=True)
            started = time.time()
            with open('quality-' + operation + '.log', 'w', encoding='utf-8') as output:
                actual = subprocess.run(['uml.cmd', operation, *(['src/a.py'] if operation == 'mutate' else [])],
                                        stdout=output, stderr=subprocess.STDOUT, timeout=180).returncode
            value = json.loads(record.read_text(encoding='utf-8')) if record.exists() else {}
            expected = {'version':1, 'operation':operation, 'projectRoot':str(Path.cwd().resolve()),
                        'exitCode':actual, 'validResult':True}
            valid = record.exists() and record.stat().st_mtime >= started and all(
                value.get(key) == wanted and type(value.get(key)) is type(wanted) for key,wanted in expected.items())
            displayed = False
            if valid:
                ir = subprocess.run(['uml.cmd','ir','policy.edn','diagram.edn'],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
                if ir.returncode == 0:
                    inbox = state / 'to-viewer.edn'
                    current = inbox.read_text(encoding='utf-8') if inbox.exists() else '{:next-id 1 :queue []}'
                    next_id = int(re.search(r':next-id (\d+)',current)[1])
                    command = '{:id ' + str(next_id) + ' :op :display :path "diagram.edn"}'
                    current = re.sub(r':next-id \d+', ':next-id '+str(next_id+1), current)
                    end = current.rindex(']')
                    temporary = inbox.with_suffix('.fixture.tmp')
                    temporary.write_text(current[:end]+' '+command+current[end:],encoding='utf-8',newline='\n')
                    os.replace(temporary,inbox)
                    displayed = True
            with open('quality-mail.jsonl','a',encoding='utf-8') as log:
                log.write(json.dumps({'id':identifier,'op':operation,'actual':actual,'valid':valid,'displayed':displayed})+'\n')
    time.sleep(.05)
'''


def quality_acceptance(root, evidence, tool):
    """An installed scratch project, with a deterministic fixture companion.

    The child polls the real to-agent queue and follows the standing quality
    procedure. This checks native runner + actual public dispatch + result/IR
    gate + viewer display; model compliance and TUI wake are separate claims.
    """
    assert (root / 'uml.cmd').is_file() and (root / '.uml-viewer/install.json').is_file(), root
    for name in ('crapper', 'dryer', 'mutator'):
        assert (root / '.uml-viewer' / name / '.venv/Scripts/python.exe').is_file(), name
    marker = root / '.uv5-quality-fixture'
    assert marker.exists() or not any((root / name).exists() for name in ('src/a.py', 'test_a.py', 'policy.edn')), \
        'quality-project must be an installed empty scratch project'
    marker.write_text('UV-5 deterministic quality fixture\n', encoding='utf-8')
    (root / 'src').mkdir(exist_ok=True)
    (root / 'src/a.py').write_text('def hello():\n    return 1\n', encoding='utf-8', newline='\n')
    (root / 'test_a.py').write_text(
        'import unittest\nfrom src.a import hello\nclass TestHello(unittest.TestCase):\n'
        '    def test_hello(self):\n        self.assertEqual(1, hello())\n', encoding='utf-8', newline='\n')
    (root / 'policy.edn').write_text('{:lang :python :src "src" :prefix "src"}', encoding='utf-8')
    for name in ('quality-mail.jsonl', '.uml-viewer/to-agent.edn'):
        (root / name).unlink(missing_ok=True)
    placeholder = native_program(root / 'quality-placeholder', QUALITY_PLACEHOLDER)
    env = dict(os.environ, UML_VIEWER_TOOL_ROOT=str(tool), UML_COMPANION_NO_ATTACH='1',
               UML_VIEWER_RUST_EXECUTABLE=str(tool / 'target/release/uml-viewer.exe'),
               CLAUDE_BIN=str(placeholder), XDG_CONFIG_HOME=str(root / '.uv5-quality-config'),
               UML_VIEWER_EVIDENCE=str(evidence), UML_VIEWER_TEST_SECONDS='600')
    for key in ('UML_COMPANION_ROOT', 'UML_VIEWER_PROJECT_ROOT', 'UML_VIEWER_TEST_DRIVER'):
        env.pop(key, None)
    result = run([str(root / 'uml.cmd')], env, root, 90)
    assert result.returncode == 0, (result.stdout, result.stderr)
    pid = int(result.stdout.split('launcher pid ')[1].split(')')[0])
    try:
        wait_for(lambda: any(':phase :ready' in read(path) for path in
                             (root / '.uml-viewer').glob('companion-worker-*.event-*.edn')),
                 120, message='installed quality companion ready')
        mail = root / '.uml-viewer/to-agent.edn'
        mail.write_text('{:next-id 4 :queue [{:id 1 :op :refresh-crap} '
                        '{:id 2 :op :refresh-dry} {:id 3 :op :refresh-mutate}]}',
                        encoding='utf-8', newline='\n')
        wait_for(lambda: len(read(root / 'quality-mail.jsonl').splitlines()) == 3,
                 360, message='all three quality dispatches')
        entries = [json.loads(line) for line in read(root / 'quality-mail.jsonl').splitlines()]
        assert [(e['id'], e['op']) for e in entries] == [(1, 'crap'), (2, 'dry'), (3, 'mutate')], entries
        assert all(e['valid'] and e['displayed'] for e in entries), entries
        wait_for(lambda: ':queue []' in read(root / '.uml-viewer/to-viewer.edn'), message='quality displays consumed')
        (evidence / 'quality-mail.json').write_text(json.dumps(entries, indent=2), encoding='utf-8')
        print('PASS installed companion mail: crap, dry, mutate -> fresh record -> IR -> display, FIFO', flush=True)
    finally:
        if alive(pid):
            close_window(pid)
            wait_for(lambda: not alive(pid), 30, message='quality viewer close')
        wait_for(lambda: read(root / '.uml-viewer/companion.edn').strip() in ('', '{}'),
                 30, message='quality companion released')


def main():
    if os.name != 'nt':
        print('SKIP native Windows seam; use the POSIX e2e scripts on WSL/macOS')
        return 0
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--tool-root', type=Path, default=REPO)
    parser.add_argument('--quality-project', type=Path, help='installed empty scratch project; writes fixture sources')
    args = parser.parse_args()
    evidence = args.evidence or Path(tempfile.mkdtemp(prefix='uml-uv5-evidence-'))
    evidence.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='uml uv5 中文 ')).resolve()
    runtime = root / 'config'
    baseline = inventory(dict(os.environ))
    projects = []
    result = {'ok': False, 'root': str(root), 'evidence': str(evidence)}
    try:
        spectator = Project(root / 'spectator', runtime, evidence, args.tool_root)
        projects.append(spectator)
        spectator.launch()
        spectator_record = spectator.ownership()
        active = Project(root / "active it's", runtime, evidence, args.tool_root)
        projects.append(active)
        active.launch()
        # FIFO + saved path, context and cursor. Both displays and context
        # transitions occur through the viewer mailbox; the session saves them.
        active.mail([':op :context :context :real', ':op :display :path "second.edn"',
                     ':op :context :context :proposal :proposal-id :uv5 :name "UV5"'])
        before = active.ownership()
        old = active.pid
        active.mail([':op :quit-for-restart'])
        wait_for(lambda: not alive(old), 30, message='quit-for-restart')
        saved = read(active.state / 'session.edn')
        assert 'second.edn' in saved and ':uv5' in saved, saved
        assert active.ownership() == before, 'restart keeps exact ownership'
        # Snapshot the replacement viewer's restored view for comparison.
        driver = root / 'restore.edn'
        driver.write_text('{:version 1 :steps [{:op :wait-frames :n 10} '
                          '{:op :dump-session :evidence "restored.edn"} '
                          '{:op :start-timer :as :hold} {:op :wait-timer :as :hold :ms 600000}]}', encoding='utf-8')
        active.env['UML_VIEWER_TEST_DRIVER'] = str(driver)
        active.launch(restart=True)
        restored = wait_for(lambda: read(active.evidence / 'restored.edn'), message='restored snapshot')
        saved_fields, restored_fields = session_fields(saved), session_fields(restored)
        assert Path(saved_fields[':path'][1]).resolve() == Path(restored_fields[':path'][1]).resolve()
        for key in (':discussion-context', ':mail-seen', ':focus', ':cam-x', ':cam-y', ':zoom'):
            assert saved_fields[key] == restored_fields[key], f'{key}: {saved}\n{restored}'
        assert saved_fields[':mail-seen'] == ('number', 4), saved
        assert 'second.edn' in restored and ':uv5' in restored, restored
        assert active.ownership() == before, 'restart reuses companion'
        active.mail([':op :context :context :real'])
        print('PASS restart: saved path/context/view/cursor and same ownership', flush=True)
        active.crash(1)
        active.crash(2)
        active.crash(3)
        active.request_exit(7)
        stopped = wait_for(lambda: active.journal() if phase(active.journal()) == 'failed' else None,
                           15, message='retry budget exhausted')
        assert 'Recovery exhausted after three retries' in stopped, stopped
        assert number(stopped, 'attempts') == 3 and len(active.starts()) == 4, stopped
        print('PASS recovery: max three retries, no fifth child', flush=True)
        active.close()
        # Normal child exit is not recovered.
        normal = Project(root / 'normal', runtime, evidence, args.tool_root)
        projects.append(normal)
        normal.launch()
        normal.request_exit(0)
        wait_for(lambda: phase(normal.journal()) == 'normal', 15, message='normal exit stops recovery')
        assert len(normal.starts()) == 1
        normal.close()
        # Closing during a backoff durably cancels the due retry.
        cancel = Project(root / 'cancel', runtime, evidence, args.tool_root)
        projects.append(cancel)
        cancel.launch()
        cancel.crash(1, recover=False)
        cancel.close()
        assert len(cancel.starts()) == 1, 'close cancels pending retry'
        assert alive(spectator.pid) and spectator.ownership() == spectator_record
        assert alive(spectator.starts()[0]['pid']), 'other project child stays up'
        current = inventory(dict(os.environ))
        assert baseline == current, 'user sessions unchanged (including default)'
        print('PASS isolation: spectator and user session inventory unchanged', flush=True)
        spectator.close()
        if args.quality_project:
            quality_acceptance(args.quality_project.resolve(), evidence, args.tool_root)
        result['ok'] = True
    except Exception as error:
        result['error'] = f'{type(error).__name__}: {error}'
        raise
    finally:
        for project in reversed(projects):
            project.cleanup()
        (evidence / 'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
        print('evidence: ' + str(evidence), flush=True)
        if result['ok']:
            shutil.rmtree(root)
    return 0


if __name__ == '__main__':
    sys.exit(main())
