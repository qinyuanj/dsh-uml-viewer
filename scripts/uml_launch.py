"""POSIX viewer 生命周期入口：preflight、fresh 启动与安全 restart。

进程身份来自 /proc（PID + starttime），比较按精确文本；不猜测、不接管未知
ownership。restart 只等待记录中那个旧进程真实退出，绝不 SIGKILL。
"""
import json
import os
import platform
from pathlib import Path
import subprocess
import sys
import time

from uml_posix import RUNTIME, atomic_write, proc_identity, runtime_argv

LAUNCHER = 'viewer-process.json'
IDENTITY = 'viewer-process-identity.json'


def read_record(path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def write_record(path, record):
    atomic_write(path, json.dumps(record, sort_keys=True) + '\n')


def await_exit(record, label, timeout=30.0, interval=0.05):
    """等待记录中的进程真实退出；PID 复用或身份变化一律拒绝。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        observed = proc_identity(record['pid'])
        if observed is None:
            return
        if observed['start'] != record['started']:
            raise ValueError(f'{label} 进程身份已变化（PID 被复用）；拒绝继续。')
        time.sleep(interval)
    raise ValueError(f'旧 {label} 仍在运行；先发送 :quit-for-restart 并等待退出，再运行 ./uml --restart。')


def verify_previous(state, root, restart):
    """restart 前核对旧 launcher 与 viewer 身份；缺失或未知一律拒绝。"""
    launcher = read_record(state / LAUNCHER)
    viewer = read_record(state / IDENTITY)
    if restart and launcher is None:
        raise ValueError('没有 launcher 进程记录；请通过 ./uml 启动后再使用 --restart。')
    # An empty or unreadable state directory is not an identity at all.
    for path in (state / LAUNCHER, state / IDENTITY):
        if path.exists() and read_record(path) is None:
            raise ValueError(f'{path.name} 不是有效记录；拒绝 restart。')
    for label, record in (('launcher', launcher), ('viewer', viewer)):
        if record is None:
            continue
        pid = record.get('pid')
        started = record.get('started')
        if type(pid) is not int or pid < 1 or not isinstance(started, str) or not started.isdigit():
            raise ValueError(f'{label} 进程身份记录无效；拒绝 restart。')
        observed = proc_identity(pid)
        if record.get('runtime') != RUNTIME:
            # A fresh start may pass over a removed runtime's record once its
            # process is gone; launch() overwrites the launcher record.
            if not restart and (observed is None or observed['start'] != started):
                continue
            raise ValueError(f"{label} 记录的运行时 {record.get('runtime')!r} 已移除；请 fresh 启动 viewer。")
        if observed is not None and observed['start'] != started:
            raise ValueError(f'{label} 进程身份已变化；拒绝 restart。')
    return launcher, viewer


def preflight(prefix, root, viewer_args):
    """只读 Herdr 选择；失败时不启动 viewer。prefix 来自 runtime_argv。"""
    result = subprocess.run([*prefix, 'companion-preflight', str(root), *viewer_args],
                            capture_output=True, text=True)
    if result.returncode != 0:
        raise ValueError((result.stderr or 'companion preflight failed without diagnostic.').strip())
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if lines != ['herdr']:
        raise ValueError('后端 preflight 输出无效；未启动 viewer。' + (result.stderr or '').strip())


# WSLg GPU path (ADR 0008): without these the Rust viewer falls back to
# llvmpipe. The launcher sets defaults only; user values win. macOS keeps
# wgpu's native Metal backend.
RENDER_DEFAULTS = {} if platform.system() == 'Darwin' else {'GALLIUM_DRIVER': 'd3d12', 'WGPU_BACKEND': 'gl'}


def viewer_env(environ):
    """The spawn environment: render defaults, user values never overridden.
    A viewer restarted by its companion is not inside that companion, so the
    runner's UML_COMPANION_ROOT marker stops here."""
    env = dict(environ)
    env.pop('UML_COMPANION_ROOT', None)
    for key, value in RENDER_DEFAULTS.items():
        env.setdefault(key, value)
    return env


def forward_args(viewer_args):
    """只剥掉 preflight 消费的 --backend 选项对，其余 argv 原样保留。"""
    forwarded, index = [], 0
    while index < len(viewer_args):
        if viewer_args[index] == '--backend':
            index += 2
        else:
            forwarded.append(viewer_args[index])
            index += 1
    return forwarded


def launch(root, tool_root, viewer_args, restart=False):
    state = root / '.uml-viewer'
    state.mkdir(exist_ok=True)
    # Refuse an unusable identity before spending seconds on a preflight that
    # would otherwise run first.
    launcher, viewer = verify_previous(state, root, restart)
    prefix = runtime_argv(tool_root)
    preflight(prefix, root, viewer_args)
    forwarded = forward_args(viewer_args)
    if restart:
        # 旧进程必须在 preflight 之后、替换之前才等待；核对已经发生。
        if launcher:
            await_exit(launcher, 'launcher')
        if viewer:
            await_exit(viewer, 'viewer')
    # The subcommand leads the viewer's own arguments so either runtime
    # dispatches to its viewer role.
    rest = forwarded[1:] if forwarded and forwarded[0] == 'viewer' else forwarded
    command = ['--backend', 'herdr', *rest]
    env = viewer_env(os.environ)
    log_path = state / 'viewer.log'
    log = open(log_path, 'ab')
    try:
        child = subprocess.Popen([*prefix, 'viewer', *command],
                                 cwd=root, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                 start_new_session=True, env=env)
    finally:
        log.close()
    observed = proc_identity(child.pid)
    if observed is None:
        raise ValueError('viewer 在写入身份前已退出；请查看 .uml-viewer/viewer.log。')
    write_record(state / LAUNCHER,
                 {'pid': observed['pid'], 'started': observed['start'], 'runtime': RUNTIME,
                  'cwd': str(root), 'argv': ['viewer', *command]})
    print(f'UML Rust viewer starting (launcher pid {child.pid}). Log: {log_path}')
    return 0


def main(root, tool_root, args):
    restart = '--restart' in args
    return launch(root, tool_root, args, restart=restart)


if __name__ == '__main__':
    sys.exit(main(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3:]))
