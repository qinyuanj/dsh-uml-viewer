"""A scripted Codex TUI for the placeholder e2e. It reports its session the
way the Herdr integration hook does, then acts out the standing rules through
the public `./uml` entry: pop to-agent mail oldest first, gate each refresh on
its fresh quality record, run IR, and only then write :display. No model, no
login, no network; `fake-codex.jsonl` in the project is its journal.

This proves the mail files, the public quality and IR entries and the worker
carry that procedure under the Codex provider. Whether a real model follows
the rules is the real-Codex acceptance's question.
"""
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
import uuid

RECORD = Path('.uml-viewer/quality-result.json')
TO_AGENT = Path('.uml-viewer/to-agent.edn')
TO_VIEWER = Path('.uml-viewer/to-viewer.edn')


def split_queue(text):
    """`{:next-id n :queue [cmd …]}` into its id and the commands' literal text."""
    next_id = int(re.search(r':next-id (\d+)', text).group(1))
    items, depth, start, quoted = [], 0, 0, False
    body = text[text.index('[') + 1:text.rindex(']')]
    for index, char in enumerate(body):
        if char == '"':
            quoted = not quoted
        elif not quoted and char == '{':
            depth += 1
            start = index if depth == 1 else start
        elif not quoted and char == '}':
            depth -= 1
            if depth == 0:
                items.append(body[start:index + 1])
    return next_id, items


def join_queue(next_id, items):
    return f'{{:next-id {next_id} :queue [{" ".join(items)}]}}\n'


def write_atomic(path, text):
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(text)
    temporary.replace(path)


def journal(**entry):
    with open('fake-codex.jsonl', 'a') as stream:
        stream.write(json.dumps(entry) + '\n')


def herdr(method, **params):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.connect(os.environ['HERDR_SOCKET_PATH'])
        with connection.makefile('rw') as stream:
            stream.write(json.dumps({'id': 'fake-codex', 'method': method, 'params': {
                'pane_id': os.environ['HERDR_PANE_ID'], **params}}) + '\n')
            stream.flush()
            stream.readline()


def report_session(session):
    # The installed hook stays silent inside another thread's environment.
    if os.environ.get('CODEX_THREAD_ID', session) != session:
        return
    herdr('pane.report_agent_session', source='herdr:codex', agent='codex',
          agent_session_id=session, session_start_source='startup')


def fault_for(identifier):
    """The scripted outcome of one mail, by its id."""
    path = Path('quality-faults.json')
    faults = json.loads(path.read_text()) if path.exists() else {}
    return faults.get(str(identifier), {})


def apply_fault(fault):
    """Leave the record a broken publisher or an older run would have left."""
    match fault.get('fault'):
        case 'missing':
            RECORD.unlink(missing_ok=True)
        case 'stale':
            os.utime(RECORD, (1, 1))
        case 'malformed':
            RECORD.write_text('{not json')
        case None:
            pass
        case name:
            record = json.loads(RECORD.read_text())
            key, value = {'other-version': ('version', 2),
                          'other-operation': ('operation', 'dry'),
                          'other-root': ('projectRoot', '/another/project'),
                          'other-exit': ('exitCode', 99),
                          'invalid': ('validResult', 'true')}[name]
            record[key] = value
            RECORD.write_text(json.dumps(record))


def quality_refusal(fault, operation, *arguments):
    """Standing rule 4: why this invocation permits no IR, or None."""
    RECORD.unlink(missing_ok=True)
    started = time.time()
    env = dict(os.environ, QUALITY_EXIT=str(fault.get('exit', 0)))
    actual = subprocess.call(['./uml', operation, *arguments], env=env)
    apply_fault(fault)
    if not RECORD.exists():
        return 'missing record'
    if RECORD.stat().st_mtime < started:
        return 'stale record'
    try:
        record = json.loads(RECORD.read_text())
    except ValueError:
        return 'malformed record'
    expected = {'version': 1, 'operation': operation, 'projectRoot': os.path.realpath('.'),
                'exitCode': actual, 'validResult': True}
    for key, value in expected.items():
        if record.get(key) != value or type(record.get(key)) is not type(value):
            return f'mismatched {key}'
    return None


def regenerate_and_display(policy='policy.edn'):
    """Standing rule 5: only IR exit 0 permits :display. Returns the refusal."""
    if subprocess.call(['./uml', 'ir', policy, 'diagram.edn']) != 0:
        return 'IR failed'
    next_id, items = (split_queue(TO_VIEWER.read_text()) if TO_VIEWER.exists() else (1, []))
    items.append(f'{{:id {next_id} :op :display :path "diagram.edn"}}')
    write_atomic(TO_VIEWER, join_queue(next_id + 1, items))
    return None


def handle(identifier, operation):
    if operation == 'context':
        return {'displayed': False, 'cause': None}
    fault = fault_for(identifier)
    refusal = None
    if operation == 'refresh-crap':
        refusal = quality_refusal(fault, 'crap')
    elif operation == 'refresh-mutate':
        refusal = quality_refusal(fault, 'mutate', 'src/a.py')
    refusal = refusal or regenerate_and_display(fault.get('policy', 'policy.edn'))
    return {'displayed': refusal is None, 'cause': refusal}


def drain():
    """A wake means mail is waiting: pop and handle each command in order."""
    while TO_AGENT.exists():
        next_id, items = split_queue(TO_AGENT.read_text())
        if not items:
            return
        head = re.match(r'\{:id (\d+) :op :([\w-]+)', items[0])
        write_atomic(TO_AGENT, join_queue(next_id, items[1:]))
        identifier, operation = int(head.group(1)), head.group(2)
        journal(event='mail', id=identifier, op=operation, **handle(identifier, operation))


def main(argv):
    if argv == ['--version']:
        print('codex-cli 0.166.0')
        return 0
    if argv[:2] == ['login', 'status']:
        print('Logged in')
        return 0
    if argv == ['--enable', 'hooks', 'features', 'list']:
        print('hooks    stable    true')
        return 0
    session = str(uuid.uuid4())
    journal(event='start', pid=os.getpid(), session=session, argv=argv,
            thread_env=os.environ.get('CODEX_THREAD_ID'))
    report_session(session)
    journal(event='launch', cause=regenerate_and_display())
    prompts = Path(os.environ['UML_FAKE_HERDR']) / f'{os.environ["HERDR_PANE_ID"]}.prompts'
    seen = 0
    while True:
        lines = prompts.read_text().splitlines() if prompts.exists() else []
        if len(lines) == seen:
            time.sleep(0.02)
            continue
        for line in lines[seen:]:
            journal(event='prompt', text=line)
        seen = len(lines)
        drain()
        herdr('fake.agent_status', status='idle')


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
