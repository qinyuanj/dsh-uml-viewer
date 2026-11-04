"""A Herdr stand-in for the placeholder Codex e2e: the CLI the adapter calls
and the socket the Codex integration hook reports to, with no terminal behind
them. Sessions live as JSON under $UML_FAKE_HERDR; every CLI call is appended
to its ledger.jsonl, so a test can prove what was never asked of another
session. A pane's commands really run, with the server's environment plus the
pane variables, the way a real pane inherits them.
"""
import contextlib
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import uuid

STATE = Path(os.environ['UML_FAKE_HERDR'])
VERSION = '0.9.5'
PROTOCOL = 22


def socket_path(name):
    return str(Path(os.environ['XDG_CONFIG_HOME']) / 'herdr/sessions' / name / 'herdr.sock')


def read_session(name):
    path = STATE / f'{name}.json'
    return json.loads(path.read_text()) if path.exists() else None


@contextlib.contextmanager
def session(name):
    """The session's state under its lock, written back on the way out."""
    with open(STATE / f'{name}.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        state = read_session(name) or {'workspaces': [], 'panes': []}
        yield state
        temporary = STATE / f'{name}.json.tmp'
        temporary.write_text(json.dumps(state))
        temporary.replace(STATE / f'{name}.json')


def pane_of(state, pane_id):
    return next(pane for pane in state['panes'] if pane['pane_id'] == pane_id)


def spawn(argv, pane, server_env, name):
    env = dict(server_env, HERDR_ENV='1', HERDR_PANE_ID=pane['pane_id'],
               HERDR_SOCKET_PATH=socket_path(name))
    return subprocess.Popen(argv, cwd=pane['cwd'], env=env, start_new_session=True,
                            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL).pid


def status(name):
    state = read_session(name) or {}
    return {'session': name, 'socket': socket_path(name), 'running': state.get('running', False),
            'version': VERSION, 'protocol': PROTOCOL, 'compatible': True,
            'endpoint_compatible': True}


def serve(name):
    path = Path(socket_path(name))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.unlink(missing_ok=True)
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(str(path))
    listener.listen()
    with session(name) as state:
        state.update(running=True, server_pid=os.getpid(), server_env=dict(os.environ))
    while True:
        connection, _ = listener.accept()
        with connection, connection.makefile('rw') as stream:
            report(name, json.loads(stream.readline()))
            stream.write('{"result":{}}\n')


def report(name, request):
    params = request['params']
    with session(name) as state:
        pane = pane_of(state, params['pane_id'])
        if request['method'] == 'pane.report_agent_session':
            # Occupant detection and the integration report, as one fact.
            pane['agent'] = params['agent']
            pane['agent_session'] = {'source': params['source'], 'agent': params['agent'],
                                     'kind': 'id', 'value': params['agent_session_id']}
        elif request['method'] == 'fake.agent_status':
            # Real Herdr reads this from the pane's screen.
            pane['agent_status'] = params['status']
            pane['state_change_seq'] += 1


def snapshot(name):
    state = read_session(name) or {'workspaces': [], 'panes': []}
    agents = [{key: pane[key] for key in ('pane_id', 'terminal_id', 'agent', 'agent_session',
                                          'agent_status', 'state_change_seq')}
              for pane in state['panes'] if pane['agent']]
    panes = [{key: pane[key] for key in ('pane_id', 'terminal_id', 'workspace_id', 'cwd')}
             for pane in state['panes']]
    return {'result': {'snapshot': {'version': VERSION, 'protocol': PROTOCOL, 'panes': panes,
                                    'workspaces': state['workspaces'], 'agents': agents}}}


def create_workspace(name, cwd):
    identifier = uuid.uuid4().hex
    pane = {'pane_id': f'pane-{identifier}', 'terminal_id': f'terminal-{identifier}',
            'workspace_id': f'workspace-{identifier}', 'cwd': cwd, 'agent': None,
            'agent_session': None, 'agent_status': 'idle', 'state_change_seq': 0, 'processes': []}
    with session(name) as state:
        pane['shell_pid'] = spawn(['/bin/sh', '-c', 'while :; do sleep 1; done'], pane,
                                  state['server_env'], name)
        pane['processes'].append(pane['shell_pid'])
        state['panes'].append(pane)
        state['workspaces'].append({'workspace_id': pane['workspace_id']})
    return {'result': {'root_pane': {'pane_id': pane['pane_id'], 'terminal_id': pane['terminal_id']},
                       'workspace': {'workspace_id': pane['workspace_id']}}}


def prompt(name, pane_id, text):
    with session(name) as state:
        pane = pane_of(state, pane_id)
        if not pane['agent']:
            print('agent_not_found', file=sys.stderr)
            raise SystemExit(1)
        with open(STATE / f'{pane_id}.prompts', 'a') as prompts:
            prompts.write(text + '\n')
        pane['agent_status'] = 'working'
        pane['state_change_seq'] += 1
        return {'result': {'agent': {'state_change_seq': pane['state_change_seq']}}}


def close_workspace(name, workspace):
    with session(name) as state:
        for pane in [pane for pane in state['panes'] if pane['workspace_id'] == workspace]:
            for pid in pane['processes']:
                with contextlib.suppress(ProcessLookupError, PermissionError):
                    os.killpg(pid, signal.SIGHUP)
            state['panes'].remove(pane)
        state['workspaces'] = [w for w in state['workspaces'] if w['workspace_id'] != workspace]
    return {'result': {}}


def stop(name):
    with session(name) as state:
        os.kill(state['server_pid'], signal.SIGTERM)
        state['running'] = False
    Path(socket_path(name)).unlink(missing_ok=True)
    return {'stopped': True, 'session': {'name': name, 'socket_path': socket_path(name)}}


def delete(name):
    (STATE / f'{name}.json').unlink()
    return {'deleted': True, 'session': {'name': name, 'socket_path': socket_path(name)}}


def sessions():
    return {'sessions': [{'name': path.stem} for path in STATE.glob('*.json')]}


def run_in_pane(name, pane_id, text):
    with session(name) as state:
        pane = pane_of(state, pane_id)
        pane['processes'].append(spawn(['/bin/sh', '-c', text], pane, state['server_env'], name))
    return {'result': {}}


def process_info(name, pane_id):
    pane = pane_of(read_session(name), pane_id)
    return {'result': {'process_info': {'pane_id': pane_id, 'shell_pid': pane['shell_pid'],
                                        'foreground_processes': []}}}


def main(argv):
    if argv == ['--version']:
        print(f'herdr {VERSION}')
        return 0
    if argv[:2] == ['integration', 'status']:
        print('codex: current (v10) (/fake/herdr-agent-state.sh)')
        return 0
    if argv[:2] == ['api', 'schema']:
        print(json.dumps({'$schema': 'fake', 'protocol': PROTOCOL, 'schema_version': 1}))
        return 0
    if argv[:1] != ['--session']:
        print(f'fake herdr: unexpected {argv}', file=sys.stderr)
        return 2
    name, command = argv[1], argv[2:]
    with open(STATE / 'ledger.jsonl', 'a') as ledger:
        ledger.write(json.dumps({'session': name, 'command': command}) + '\n')
    match command:
        case ['server']:
            serve(name)
        case ['status', 'server', '--json']:
            answer = status(name)
        case ['session', 'list', '--json']:
            answer = sessions()
        case ['session', 'stop', _, '--json']:
            answer = stop(name)
        case ['session', 'delete', _, '--json']:
            answer = delete(name)
        case ['api', 'snapshot']:
            answer = snapshot(name)
        case ['workspace', 'create', '--cwd', cwd, '--no-focus']:
            answer = create_workspace(name, cwd)
        case ['workspace', 'close', workspace]:
            answer = close_workspace(name, workspace)
        case ['pane', 'process-info', '--pane', pane]:
            answer = process_info(name, pane)
        case ['pane', 'read', _]:
            print('$ ')
            return 0
        case ['pane', 'run', pane, text]:
            answer = run_in_pane(name, pane, text)
        case ['agent', 'prompt', pane, text, *_]:
            answer = prompt(name, pane, text)
        case _:
            print(f'fake herdr: unexpected {command}', file=sys.stderr)
            return 2
    print(json.dumps(answer))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
