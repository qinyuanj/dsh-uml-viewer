"""公共入口（POSIX 与原生 Windows）；安装与子进程始终使用字面 argv。

宿主差异只在本模块按平台选定一次（ADR 0010）：Windows 的二进制带 `.exe`，
venv 解释器在 `Scripts\\python.exe`，开发安装链接是目录 junction，进程身份是
`GetProcessTimes` 创建时刻。
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib
import urllib.error
import urllib.request

WINDOWS = os.name == 'nt'
# The oldest delivered Windows build: Windows 10 22H2 (crates/platform).
MINIMUM_WINDOWS_BUILD = 19045
# What `cargo build --release` produces and the entry dispatch runs.
BINARY = 'target/release/' + ('uml-viewer.exe' if WINDOWS else 'uml-viewer')
# A venv's interpreter, relative to the venv directory.
VENV_PYTHON = 'Scripts/python.exe' if WINDOWS else 'bin/python'
# The thin project entry this host installs, and the marker that says a
# project file of that name is ours to replace.
ENTRY = ('uml.cmd', b'Thin Windows entry') if WINDOWS else ('uml', b'Thin Unix entry')

# The viewer's only UI font (#82). These are the same archive and face hashes
# the retired PowerShell installer pinned; only the extraction path changed.
FONT_URL = ('https://github.com/be5invis/Sarasa-Gothic/releases/download/v1.0.42/'
            'SarasaMonoSC-TTF-1.0.42.7z')
FONT_FACE = 'SarasaMonoSC-Regular.ttf'
FONT_ARCHIVE_SHA256 = 'aa2150e99eb38c5f9d3a00fe58e3f90a9d89495c795a8ac80934d1c3e6c377ee'
FONT_SHA256 = '82e09460f6f55010296f670b85e346ddeaf91b967724840d2bf5514137f68a83'
# Seconds one socket wait (connect, response, each read) may stall. It is not a
# total deadline, so a slow link that keeps delivering can still finish.
FONT_TIMEOUT = 60
# Upstream ships the TTF release only as 7z, and no 7z binary is assumed on the
# host. py7zr lives in the installer's own environment so no tool venv is
# polluted with an unrelated dependency.
PY7ZR = 'py7zr==1.1.4'


def run(argv, **kwargs):
    """Every synchronous launch: a literal argv, never a shell.

    This and `spawn` are the only launches in the production scripts
    (scripts/test_process_boundary.py)."""
    kwargs.setdefault('check', True)
    return subprocess.run([str(x) for x in argv], **kwargs)


def detached():
    """Popen options that keep the viewer alive after its caller is gone.

    POSIX: its own session. Windows ignores `start_new_session`: the viewer
    gets its own process group and a hidden console of its own (closing the
    caller's window does not end it, and a console-control close can still
    reach it), and breaks away from a job the caller runs in, such as the
    companion's bounded run. A job that forbids breakaway refuses the flag;
    see `spawn`.
    """
    if not WINDOWS:
        return {'start_new_session': True}
    return {'creationflags': subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.CREATE_NO_WINDOW
            | subprocess.CREATE_BREAKAWAY_FROM_JOB}


def spawn(argv, **options):
    """The detached launch: `run`'s literal argv, without waiting."""
    options.update(detached())
    try:
        return subprocess.Popen(argv, **options)
    except PermissionError:
        if not WINDOWS:
            raise
        # ERROR_ACCESS_DENIED: the caller's job forbids breakaway; the viewer
        # then stays in it, as the companion's detached spawn does.
        options['creationflags'] &= ~subprocess.CREATE_BREAKAWAY_FROM_JOB
        return subprocess.Popen(argv, **options)


def platform_refusal(system, machine, release, build):
    """The delivered-platform decision over a declared host (ADR 0007, 0010).

    None when delivered, else the one-line reason. A pure function, so every
    platform's answer is testable on any host; `host()` reads the machine.
    The Windows text is the Rust `platform::decide` refusal.
    """
    if system == 'Darwin':
        return None
    if system == 'Windows':
        if machine != 'AMD64':
            return f'Windows on {machine} is not supported: uml-viewer is delivered on x64 (AMD64) Windows.'
        if build < MINIMUM_WINDOWS_BUILD:
            return (f'Windows build {build} is not supported: the minimum is '
                    f'Windows 10 22H2 (build {MINIMUM_WINDOWS_BUILD})')
        return None
    if system != 'Linux' or machine != 'x86_64':
        return ('仅支持 WSL2 x86_64、macOS 与 Windows 10 22H2（build 19045）及以上（ADR 0007、0010），'
                '请在 Ubuntu 24.04 WSL2、macOS 或原生 Windows 中运行。')
    if 'microsoft' not in release.lower():
        return 'Linux 执行路径仅交付 WSL2，请在 Ubuntu 24.04 WSL2 中运行。'
    return None


def host():
    build = sys.getwindowsversion().build if WINDOWS else 0
    refusal = platform_refusal(platform.system(), platform.machine(), platform.release(), build)
    if refusal:
        raise ValueError(refusal)


def is_link(path):
    """A symlink, or on Windows any reparse point (a directory junction).

    `Path.is_symlink()` is false for a junction, and `os.path.isjunction`
    needs Python 3.12; the 3.11 floor reads the attribute instead.
    """
    path = Path(path)
    if path.is_symlink():
        return True
    if not WINDOWS:
        return False
    try:
        attributes = os.lstat(path).st_file_attributes
    except OSError:
        return False
    return bool(attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def link_directory(link, target):
    """A development install's link: a symlink, or on Windows a junction.

    Windows symlinks need Developer Mode or elevation; a junction needs
    neither and `Path.resolve()` follows it like a symlink.
    """
    if WINDOWS:
        import _winapi
        _winapi.CreateJunction(str(Path(target).resolve()), str(link))
    else:
        Path(link).symlink_to(target, target_is_directory=True)


def runnable(path):
    """An existing file this host can execute (on Windows: it exists)."""
    path = Path(path)
    return path.is_file() and (WINDOWS or os.access(path, os.X_OK))


# NTSTATUS exits a Windows process reports where POSIX would report a signal,
# mapped to the 128+signal value a POSIX shell gives (SIGINT, SIGILL, SIGFPE,
# SIGSEGV, SIGABRT).
NTSTATUS_SIGNALS = {
    0xC000013A: 2,   # STATUS_CONTROL_C_EXIT (Ctrl+C / Ctrl+Break)
    0xC000001D: 4,   # STATUS_ILLEGAL_INSTRUCTION
    0xC0000094: 8,   # STATUS_INTEGER_DIVIDE_BY_ZERO
    0xC0000005: 11,  # STATUS_ACCESS_VIOLATION
    0xC00000FD: 11,  # STATUS_STACK_OVERFLOW
    0xC0000409: 6,   # STATUS_STACK_BUFFER_OVERRUN (fail-fast, Rust abort)
}


def native_exit(code):
    """A child's exit as an entry exit status (0–255 on Windows).

    Windows exit codes are 32-bit, so an NTSTATUS such as 0xC0000005 arrives
    as a large positive number: a known one becomes 128+signal, any other
    out-of-range code 255, and the raw value is named on stderr. POSIX codes
    pass through unchanged.
    """
    if code > 255:
        mapped = 128 + NTSTATUS_SIGNALS[code] if code in NTSTATUS_SIGNALS else 255
        print(f'uml: 子进程以 NTSTATUS 0x{code:08X} 结束，按退出码 {mapped} 报告。', file=sys.stderr)
        return mapped
    return code


def git_command():
    """The installer fetches every tool repository with a literal `git`."""
    executable = shutil.which('git')
    if not executable:
        hint = 'winget install Git.Git' if WINDOWS else 'sudo apt install git'
        raise ValueError(f'需要 git（安装器用它获取工具仓库）：{hint}。')
    return executable


def cargo_command():
    """Locate a runnable cargo; the pinned version is checked per checkout."""
    executable = shutil.which(os.environ.get('UML_CARGO_EXECUTABLE', 'cargo'))
    if not executable:
        raise ValueError('需要 Rust 工具链（cargo）：用 rustup 安装（https://rustup.rs），'
                         '或设置 UML_CARGO_EXECUTABLE。所需版本见工具仓库的 rust-toolchain.toml。')
    try:
        result = run([executable, '--version'], check=False, capture_output=True, text=True)
    except OSError as error:
        raise ValueError(f'cargo 不可执行：{executable}（{error}）') from error
    if result.returncode != 0 or not re.match(r'cargo \d+\.\d+\.\d+', result.stdout):
        raise ValueError(f'cargo 不可用：{executable} --version 输出 '
                         f'{(result.stdout or result.stderr).strip()!r}')
    return executable


def rust_channel(tool_root):
    """The toolchain pin every build must honour (#134): rust-toolchain.toml."""
    pin = Path(tool_root) / 'rust-toolchain.toml'
    if not pin.is_file():
        raise ValueError(f'工具仓库缺少 rust-toolchain.toml：{pin}；安装副本不完整。')
    channel = tomllib.loads(pin.read_text()).get('toolchain', {}).get('channel')
    if not channel:
        raise ValueError(f'{pin} 没有 toolchain.channel；安装副本不完整。')
    return channel


def check_rust_toolchain(cargo, tool_root):
    """The installed toolchain must match the checkout's pin.

    A rustup shim resolves rust-toolchain.toml per directory, so the probe
    runs inside the checkout; a plain distro cargo must match on its own.
    """
    channel = rust_channel(tool_root)
    result = run([cargo, '--version'], check=False, cwd=tool_root,
                 capture_output=True, text=True)
    match = re.match(r'cargo (\d+\.\d+\.\d+)', result.stdout or '')
    if result.returncode != 0 or not match:
        raise ValueError(f'无法确认 Rust 工具链（rust-toolchain.toml 要求 {channel}）：'
                         f'{(result.stderr or result.stdout).strip()}')
    if re.fullmatch(r'\d+\.\d+\.\d+', channel) and match.group(1) != channel:
        raise ValueError(f'Rust 工具链版本不符：rust-toolchain.toml 要求 {channel}，'
                         f'实际 {match.group(1)}；rustup toolchain install {channel}。')
    return channel


def rust_build(cargo, tool_root):
    """Build the single uml-viewer binary the entry dispatch expects (#143)."""
    check_rust_toolchain(cargo, tool_root)
    run([cargo, 'build', '--release', '--locked'], cwd=tool_root)
    binary = Path(tool_root) / BINARY
    if not runnable(binary):
        raise ValueError(f'cargo build 未产出可执行文件 {binary}')
    return binary


def x11_library_present():
    """Probe libxkbcommon-x11 without elevating (mirrors the viewer's)."""
    name = 'libxkbcommon-x11.so.0'
    if any((Path(directory) / name).exists() for directory in
           ('/usr/lib/x86_64-linux-gnu', '/usr/lib/aarch64-linux-gnu', '/usr/lib')):
        return True
    result = run(['ldconfig', '-p'], check=False, capture_output=True, text=True)
    return name in result.stdout


def x11_warnings(present=None):
    """X11 libs are optional: #139 chose the Wayland-undecorated window on the
    delivery host, so an absent libxkbcommon-x11 is a hint, never a refusal.
    macOS and Windows window natively and need neither."""
    if present is None and platform.system() in ('Darwin', 'Windows'):
        return []
    if present is None:
        present = x11_library_present()
    if present:
        return []
    return ['未找到 libxkbcommon-x11.so.0：viewer 将使用 Wayland 无装饰窗口（#139 已实测）；'
            '需要 X11 标题栏可安装 libxkbcommon-x11-0（sudo apt install libxkbcommon-x11-0）。']


# Rust is the only implementation (#144). `love` and the older `jvm` value are
# refused before any dispatch so an old invocation fails with a clear message.
RUNTIME = 'rust'


def take_runtime(args):
    """Strip one `--runtime rust` pair; refuse every other value.

    Rust is the default and the only runtime. `love` and the older `jvm`
    value are refused before any dispatch, the way the JVM was.
    """
    if '--runtime' not in args:
        return args
    index = args.index('--runtime')
    value = args[index + 1] if index + 1 < len(args) else None
    if value != RUNTIME or args.count('--runtime') != 1:
        raise ValueError('--runtime 只接受 rust（默认）；love 与 JVM 已移除。')
    return args[:index] + args[index + 2:]


def rust_sources(tool_root):
    """What the binary is built from: every crate's src, the manifests and the pin."""
    root = Path(tool_root)
    yield from root.glob('crates/*/src/**/*.rs')
    yield from root.glob('crates/*/Cargo.toml')
    for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'):
        yield root / name


def stale(binary, tool_root):
    built = binary.stat().st_mtime
    return any(path.is_file() and path.stat().st_mtime > built for path in rust_sources(tool_root))


def rust_command(tool_root):
    configured = os.environ.get('UML_VIEWER_RUST_EXECUTABLE')
    executable = Path(configured) if configured else Path(tool_root) / BINARY
    if not runnable(executable):
        raise ValueError(f'缺少 Rust 二进制 {executable}；请运行 get-uml-viewer 构建（或在 {tool_root} '
                         '运行 cargo build --release --locked），或设置 UML_VIEWER_RUST_EXECUTABLE。')
    # A development tool root changes under the session: rebuild a binary its
    # sources have outgrown, so a relaunch never runs the code before an edit.
    if not configured and stale(executable, tool_root):
        rust_build(cargo_command(), tool_root)
    return str(executable)


def runtime_argv(tool_root):
    """argv prefix that precedes a role (`viewer`, `ir`, …)."""
    return [rust_command(tool_root)]


def native_names(name):
    """The file names a PATH lookup of `name` may match on this host.

    Windows tries PATHEXT in order but keeps only native executables
    (`.exe`, `.com`): a `.cmd`/`.bat`/`.ps1` wrapper re-parses its arguments
    and is refused as a tool entry, as the companion refuses it.
    """
    if not WINDOWS:
        return [name]
    extensions = os.environ.get('PATHEXT', '.COM;.EXE').split(';')
    return [name + ext.lower() for ext in extensions if ext.lower() in ('.exe', '.com')]


def select_herdr():
    candidates = []
    reasons = []
    for directory in os.get_exec_path():
        for name in native_names('herdr'):
            path = Path(directory or '.') / name
            if runnable(path):
                break
        else:
            continue
        try:
            version = run([path, '--version'], capture_output=True, text=True, encoding='utf-8', timeout=10).stdout.strip()
            match = re.fullmatch(r'herdr (\d+)\.(\d+)\.(\d+)', version)
            if not match or tuple(map(int, match.groups())) < (0, 9, 3):
                raise ValueError('需要纯数字版本 >= 0.9.3')
            schema = json.loads(run([path, 'api', 'schema', '--json'], capture_output=True,
                                    text=True, encoding='utf-8', timeout=10).stdout)
            if schema.get('protocol') != 22:
                raise ValueError('需要 protocol 22')
            candidates.append((tuple(map(int, match.groups())), path.resolve()))
        except (ValueError, subprocess.SubprocessError) as error:
            reasons.append(f'{path}: {error}')
    if not candidates:
        raise ValueError('PATH 上没有兼容 Herdr >= 0.9.3 / protocol 22。\n' + '\n'.join(reasons))
    return max(candidates, key=lambda item: item[0])[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        # LF and UTF-8 on every host: the Rust side reads these records, and
        # Windows text mode would write CRLF in the ANSI code page.
        with os.fdopen(fd, 'w', encoding='utf-8', newline='\n') as stream:
            stream.write(data)
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


IGNORE_BLOCK = '# BEGIN UML-VIEWER\n.uml-viewer/\numl-viewer-log.txt\n# END UML-VIEWER\n'


def ensure_ignore_block(root):
    """Keep the project's `.gitignore` block intact without moving it.

    Other tools (magpie) also append their own block last; rewriting an
    intact block to the end would make the two trade places on every run.
    Only a missing or altered block is removed and re-appended."""
    ignore = root / '.gitignore'
    text = ignore.read_text(encoding='utf-8') if ignore.exists() else ''
    if re.search(r'(?m)^' + re.escape(IGNORE_BLOCK), text):
        return
    text = re.sub(r'(?ms)^# BEGIN UML-VIEWER\n.*?^# END UML-VIEWER\n?', '', text)
    atomic_write(ignore, text.rstrip() + '\n' + IGNORE_BLOCK)


# Paths this installer creates inside a tool checkout. They must not read as
# the user's own work, otherwise the second install would refuse its own venv.
# Filtering happens here rather than in a git pathspec: glob semantics differ
# between git versions and a mismatch silently reintroduces the false refusal.
PROVISIONED_DIRECTORIES = ('.venv', '__pycache__', '.pytest_cache')


EXTRACT = """
import pathlib, sys
import py7zr
archive, target, face = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
with py7zr.SevenZipFile(archive, 'r') as bundle:
    names = [n for n in bundle.getnames()
             if pathlib.Path(n).name == face and not pathlib.Path(n).is_absolute()
             and '..' not in pathlib.Path(n).parts]
    if len(names) != 1:
        sys.exit(f'Sarasa archive must contain exactly one {face}, found {len(names)}')
    bundle.extract(path=target, targets=names)
    print(names[0])
"""


def extractor(directory):
    """An interpreter that can read 7z: the host's, else the installer's own.

    py7zr is the only dependency the installer needs beyond the stdlib; using
    the host interpreter when it already has the module avoids creating a venv
    for nothing.
    """
    probe = run([sys.executable, '-I', '-c', 'import py7zr'], check=False,
                capture_output=True, text=True)
    if probe.returncode == 0:
        return sys.executable
    override = os.environ.get('UML_VIEWER_FONT_EXTRACTOR')
    if override:
        return override
    environment = directory.parent / 'installer'
    python = environment / '.venv' / VENV_PYTHON
    if not python.is_file():
        run([sys.executable, '-I', '-m', 'venv', environment / '.venv'])
        run([str(python), '-I', '-m', 'pip', 'install', '--disable-pip-version-check',
             '--quiet', PY7ZR])
    return str(python)


def stalled(url, timeout):
    return ValueError(f'下载 Sarasa 字体超时：{url} 超过 {timeout:g} 秒没有数据；'
                      '未写入字体，请检查网络后重新运行 get-uml-viewer。')


def curl_download(curl, url, archive, timeout):
    """Windows http(s) download through curl.exe (Windows 10 1803+).

    Python's urllib stalls on GitHub release downloads on Windows hosts where
    curl.exe completes. The stall rule is the same: no connection (TLS
    included) within `timeout`, or less than one byte per second for
    `timeout` seconds, is curl exit 28.
    """
    seconds = str(max(1, round(timeout)))
    result = run([curl, '-fsSL', '--connect-timeout', seconds, '--speed-limit', '1',
                  '--speed-time', seconds, '-o', str(archive), url], check=False,
                 capture_output=True, text=True, encoding='utf-8', errors='replace')
    if result.returncode == 28:
        raise stalled(url, timeout)
    if result.returncode:
        raise ValueError(f'下载 Sarasa 字体失败：{url}：{result.stderr.strip()}')


def download_font_archive(url, archive, timeout):
    """Write url to archive; a stall becomes a refusal naming the URL and timeout.

    Connection-phase timeouts arrive wrapped in URLError, later ones bare. Other
    failures keep their own error.
    """
    curl = shutil.which('curl.exe') if WINDOWS and url.startswith(('http://', 'https://')) else None
    if curl:
        return curl_download(curl, url, archive, timeout)
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response, archive.open('wb') as output:
            shutil.copyfileobj(response, output)
    except (TimeoutError, urllib.error.URLError) as error:
        if not isinstance(getattr(error, 'reason', error), TimeoutError):
            raise
        raise stalled(url, timeout) from error


def font(directory):
    """Provision the pinned Sarasa face, verified by SHA256.

    Hashes come from #82 (the archive and the face), so a substituted download
    cannot reach the viewer. Upstream publishes the TTF release only as 7z, so
    extraction happens in an interpreter that has py7zr. The UML_VIEWER_FONT_*
    variables override the pin and timeout for fixture tests, exactly as the
    retired installer's -Pin parameter did.
    """
    url = os.environ.get('UML_VIEWER_FONT_URL', FONT_URL)
    archive_hash = os.environ.get('UML_VIEWER_FONT_ARCHIVE_SHA256', FONT_ARCHIVE_SHA256)
    face_hash = os.environ.get('UML_VIEWER_FONT_SHA256', FONT_SHA256)
    timeout = float(os.environ.get('UML_VIEWER_FONT_TIMEOUT', FONT_TIMEOUT))
    target = directory / FONT_FACE
    if target.is_file() and digest(target) == face_hash:
        return
    python = extractor(directory)
    with tempfile.TemporaryDirectory(prefix='uml-sarasa-') as temporary:
        base = Path(temporary)
        archive = base / url.rsplit('/', 1)[-1]
        download_font_archive(url, archive, timeout)
        if digest(archive) != archive_hash:
            raise ValueError('Sarasa font archive SHA256 mismatch')
        extracted = run([python, '-I', '-c', EXTRACT, str(archive),
                         str(base / 'expanded'), FONT_FACE], capture_output=True, text=True)
        if extracted.returncode != 0:
            raise ValueError(extracted.stderr.strip() or 'Sarasa font archive extraction failed')
        face = base / 'expanded' / extracted.stdout.strip()
        if not face.is_file() or digest(face) != face_hash:
            raise ValueError(f'{FONT_FACE} SHA256 mismatch')
        directory.mkdir(parents=True, exist_ok=True)
        shutil.copy2(face, target)


# What the LOVE-era installer put in `.uml-viewer/cimgui-love-<release>/`
# (#167): the native library or the Lua binding's entry module.
CIMGUI_MARKS = ('cimgui.so', 'cimgui.dll', 'cimgui/init.lua')


def remove_legacy_runtime(state):
    """Delete the cimgui provisioning a LOVE-era install left behind (#167).

    Decided by what exists, not by install.json, so every install is an
    idempotent cleanup. Only real directories named `cimgui-love-*` directly
    inside `.uml-viewer/` that carry a cimgui mark go; symlinks, files and
    unmarked directories stay, and rmtree never follows a link out of the
    tree. Removed paths and the manual leftovers are reported on stderr.
    """
    if is_link(state) or not state.is_dir():
        return
    removed = []
    for path in sorted(state.glob('cimgui-love-*')):
        if is_link(path) or not path.is_dir():
            continue
        if not any((path / mark).is_file() and not is_link(path / mark)
                   for mark in CIMGUI_MARKS):
            continue
        shutil.rmtree(path)
        removed.append(path)
    if removed:
        for path in removed:
            print(f'已删除旧 LOVE 运行时残留：{path}', file=sys.stderr)
        print('提示：~/.local/share/love/uml-viewer/ 以及 apt 安装的 love / lua 软件包'
              '不再需要，可手动删除或卸载。', file=sys.stderr)


def provisioned(path):
    name = path.rstrip('/').rsplit('/', 1)[-1]
    return name in PROVISIONED_DIRECTORIES or name.endswith('.egg-info')


def local_changes(directory):
    """The checkout's uncommitted work, excluding what this installer creates."""
    result = run(['git', '-C', directory, 'status', '--porcelain', '-z'],
                 capture_output=True, text=True)
    changes = []
    for entry in result.stdout.split('\0'):
        if not entry:
            continue
        # Renames carry the original path in the following NUL-separated field.
        if provisioned(entry[3:]):
            continue
        changes.append(entry)
    return '\n'.join(changes)


def fetch(directory, tool):
    url = os.environ.get(('UML_VIEWER' if tool == 'uml-viewer' else tool.upper()) + '_REPO_URL',
                         f'https://github.com/qinyuanj/{tool}.git')
    ref = os.environ.get(('UML_VIEWER' if tool == 'uml-viewer' else tool.upper()) + '_REF', 'main')
    if is_link(directory):
        # A link (or junction) is a development checkout; fetching through it
        # detaches that repository's HEAD.
        raise ValueError(f'拒绝通过链接更新开发副本：{directory}；'
                         '开发安装请设置 UML_VIEWER_TOOL_ROOT 指向链接目标。')
    if directory.exists():
        if not (directory / '.git').exists():
            raise ValueError(f'拒绝替换非 Git 目录：{directory}')
        if local_changes(directory):
            raise ValueError(f'安装副本有本地修改，已保留，请先处理：{directory}')
    else:
        run(['git', 'init', '-q', directory])
    run(['git', '-C', directory, 'fetch', '--depth', '1', url, ref])
    run(['git', '-C', directory, 'checkout', '-q', '--detach', 'FETCH_HEAD'])
    return {'source': url, 'ref': ref, 'commit': run(['git', '-C', directory, 'rev-parse', 'HEAD'],
                                                  capture_output=True, text=True).stdout.strip()}


class _ProcBsdInfo(ctypes.Structure):
    """macOS <sys/proc_info.h> struct proc_bsdinfo."""
    _fields_ = [*((name, ctypes.c_uint32) for name in (
        'pbi_flags', 'pbi_status', 'pbi_xstatus', 'pbi_pid', 'pbi_ppid', 'pbi_uid', 'pbi_gid',
        'pbi_ruid', 'pbi_rgid', 'pbi_svuid', 'pbi_svgid', 'rfu_1')),
        ('pbi_comm', ctypes.c_char * 16), ('pbi_name', ctypes.c_char * 32),
        *((name, ctypes.c_uint32) for name in (
            'pbi_nfiles', 'pbi_pgid', 'pbi_pjobc', 'e_tdev', 'e_tpgid')),
        ('pbi_nice', ctypes.c_int32),
        ('pbi_start_tvsec', ctypes.c_uint64), ('pbi_start_tvusec', ctypes.c_uint64)]


def darwin_start(pid):
    """macOS 创建身份：内核启动时刻的微秒数（十进制文本），与 Rust 端一致。"""
    libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    info = _ProcBsdInfo()
    size = ctypes.sizeof(info)
    written = libc.proc_pidinfo(ctypes.c_int(pid), ctypes.c_int(3), ctypes.c_uint64(0),
                                ctypes.byref(info), ctypes.c_int(size))
    if written != size:
        return None
    return str(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)


def windows_start(pid):
    """Windows 创建身份：`GetProcessTimes` 创建时刻 FILETIME 的十进制文本。

    与 Rust `companion::posix::identity`、`viewer::identity::starttime` 同一文本。
    句柄已 signalled（进程已退出，哪怕仍有句柄打开）或打不开时返回 None。
    """
    from ctypes import wintypes
    kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.WaitForSingleObject.restype = wintypes.DWORD
    kernel32.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel32.GetProcessTimes.restype = wintypes.BOOL
    kernel32.GetProcessTimes.argtypes = (wintypes.HANDLE, *(ctypes.POINTER(wintypes.FILETIME),) * 4)
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
    query_limited, synchronize, signalled = 0x1000, 0x00100000, 0
    if not 0 < pid <= 0xFFFFFFFF:
        return None
    handle = kernel32.OpenProcess(query_limited | synchronize, False, pid)
    if not handle:
        return None
    try:
        if kernel32.WaitForSingleObject(handle, 0) == signalled:
            return None
        times = [wintypes.FILETIME() for _ in range(4)]
        if not kernel32.GetProcessTimes(handle, *(ctypes.byref(t) for t in times)):
            return None
        return str((times[0].dwHighDateTime << 32) | times[0].dwLowDateTime)
    finally:
        kernel32.CloseHandle(handle)


def proc_identity(pid):
    """读取 PID 的创建身份；进程不存在返回 None。"""
    if WINDOWS:
        start = windows_start(pid)
        return None if start is None else {'pid': pid, 'start': start}
    if platform.system() == 'Darwin':
        start = darwin_start(pid)
        return None if start is None else {'pid': pid, 'start': start}
    try:
        text = Path(f'/proc/{pid}/stat').read_text()
    except (FileNotFoundError, ProcessLookupError):
        return None
    fields = text.rsplit(')', 1)[-1].split()
    # fields[0] 是 state（第 3 字段）；starttime 是第 22 字段，即此处 index 19。
    return {'pid': pid, 'start': fields[19]}


# recovery_domain::terminal: phases after which no recovery runs.
TERMINAL_RECOVERY = ('failed', 'cancelled', 'normal')


def live(record):
    """Whether a runtime record still describes a running session.

    A normal close leaves a terminal recovery record and the identity of an
    exited viewer behind; those are history. Held ownership, an unknown
    phase and an unreadable identity count as live.
    """
    text = record.read_text(encoding='utf-8', errors='replace').strip()
    if text in ('', '{}', 'nil'):
        return False
    if record.name == 'recovery.edn':
        phase = re.search(r':phase\s+:([\w-]+)', text)
        return not (phase and phase.group(1) in TERMINAL_RECOVERY)
    if record.name == 'viewer-process-identity.json':
        try:
            identity = json.loads(text)
        except ValueError:
            return True
        if not isinstance(identity, dict) or type(identity.get('pid')) is not int:
            return True
        observed = proc_identity(identity['pid'])
        return observed is not None and observed['start'] == identity.get('started')
    return True


def install(args):
    if args == ['--help'] or args == ['-h']:
        print('usage: get-uml-viewer [--install-only]')
        return 0
    if args not in ([], ['--install-only']):
        raise ValueError('usage: get-uml-viewer [--install-only]')
    host()
    git_command()
    herdr = select_herdr()
    cargo = cargo_command()
    root = Path.cwd().resolve()
    state = root / '.uml-viewer'
    state.mkdir(exist_ok=True)
    for name in ('companion.edn', 'recovery.edn', 'viewer-process-identity.json'):
        record = state / name
        if record.exists() and live(record):
            raise ValueError(f'保留已有运行态并拒绝安装：{record}；请先通过原有入口正常关闭会话并归档。')
    development = os.environ.get('UML_VIEWER_TOOL_ROOT')
    versions = {}
    if development:
        source = Path(development).resolve(strict=True)
        if not (source / 'Cargo.toml').is_file() or not (source / 'scripts/uml_posix.py').is_file():
            raise ValueError('UML_VIEWER_TOOL_ROOT 必须指向完整开发仓库')
        destination = state / 'uml-viewer'
        if destination.exists() or is_link(destination):
            if not is_link(destination) or destination.resolve() != source:
                raise ValueError('开发源码与现有安装副本不同，拒绝替换。')
        else:
            link_directory(destination, source)
        versions['uml-viewer'] = {'source': str(source), 'development': True,
            'commit': run(['git', '-C', source, 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip(),
            'dirty': bool(local_changes(source))}
    else:
        versions['uml-viewer'] = fetch(state / 'uml-viewer', 'uml-viewer')
    for tool in ('crapper', 'mutator', 'dryer'):
        versions[tool] = fetch(state / tool, tool)
    # Provenance/font provisioning stays; LOVE and cimgui are gone (#144).
    # Provisioned after the tool environments exist: extraction uses the
    # installer's own venv, which Python 3.12 provides without system packages.
    font(state / 'fonts')
    for tool in ('crapper', 'mutator', 'dryer'):
        directory = state / tool
        python = directory / '.venv' / VENV_PYTHON
        if not python.exists():
            run([sys.executable, '-I', '-m', 'venv', directory / '.venv'])
        packages = ['-e', str(directory)]
        if tool == 'mutator':
            packages += ['-e', str(state / 'crapper')]
        run([python, '-I', '-m', 'pip', 'install', '--disable-pip-version-check', *packages])
        run([python, '-I', '-m', 'pip', 'check'])
    # The Rust build comes last: the checkout must be complete, and a refusal
    # in any earlier step must not leave a half-built tree behind. `target/`
    # is gitignored, so a rebuild never reads as a local modification.
    binary = rust_build(cargo, state / 'uml-viewer')
    for warning in x11_warnings():
        print(f'提示：{warning}', file=sys.stderr)
    ensure_ignore_block(root)
    name, marker = ENTRY
    entry = root / name
    shipped = state / 'uml-viewer/scripts' / name
    if entry.exists() and entry.read_bytes() != shipped.read_bytes():
        if marker not in entry.read_bytes():
            raise ValueError(f'已有自定义 {name}，拒绝覆盖；请先改名保存。')
    shutil.copy2(shipped, entry)
    if not WINDOWS:
        entry.chmod(0o755)
    # After every refusal check, so a refused install deletes nothing; it
    # runs on every install and needs no record, so a rerun still cleans.
    remove_legacy_runtime(state)
    atomic_write(state / 'install.json', json.dumps({'version': 1, 'projectRoot': str(root),
                 'rust': str(binary), 'herdr': str(herdr), 'repositories': versions}, ensure_ascii=False, indent=2) + '\n')
    shown = name if WINDOWS else './uml'
    print(f'已安装 {shown}；使用 {shown} ir 生成 EDN。')
    return 0 if args else command(root, [])


NON_LIFECYCLE = ('ir', 'scene', 'crap', 'mutate', 'dry')


def refuse_coding_agent_outside_lifecycle(args):
    """--coding-agent 只属于 viewer 生命周期入口；其值由 Rust preflight 校验。"""
    if '--coding-agent' not in args:
        return
    rest, index = [], 0
    while index < len(args):
        if args[index] == '--coding-agent':
            index += 2
        else:
            rest.append(args[index])
            index += 1
    if rest and rest[0] in NON_LIFECYCLE:
        raise ValueError(f'--coding-agent 只用于 viewer 生命周期入口（./uml 与 ./uml --restart），'
                         f'{rest[0]} 不接受该选项。')


def command(root, args):
    root = root.resolve()
    os.chdir(root)
    if args in (['--help'], ['-h']):
        print('usage: uml [--runtime rust] [--backend herdr] [--coding-agent claude|codex]\n'
              '           [--restart | ir | scene | crap | mutate | dry] [args]\n'
              '  rust 是唯一运行时；love 与 JVM 已移除。\n'
              '  --coding-agent 只用于 fresh/restart 生命周期，缺省 claude；Codex 用 CODEX_BIN 或 PATH 上的 codex。')
        return 0
    host()
    args = take_runtime(args)
    refuse_coding_agent_outside_lifecycle(args)
    # Empty means unset, as in the sh entry and the installer.
    tool_root = Path(os.environ.get('UML_VIEWER_TOOL_ROOT') or root / '.uml-viewer/uml-viewer').resolve()
    if args and args[0] == 'ir':
        values = args[1:]
        has_policy = '--policy' in values
        index = 0
        while index < len(values):
            if values[index] in ('--root', '--output', '--policy'):
                index += 2
            else:
                has_policy = has_policy or not values[index].startswith('--')
                index += 1
        if not has_policy:
            preferred = root / 'examples' / (root.name + '.policy.edn')
            choices = sorted((root / 'examples').glob('*.policy.edn'))
            policy = preferred if preferred.is_file() else choices[0] if choices else root / 'policy.edn'
            if not policy.is_file():
                raise ValueError('没有 policy；请创建 policy.edn 或 examples/*.policy.edn。')
            args = ['ir', str(policy.relative_to(root)), *args[1:]]
        return native_exit(run([*runtime_argv(tool_root), *args], cwd=root, check=False).returncode)
    if args and args[0] == 'scene':
        return native_exit(run([*runtime_argv(tool_root), *args], cwd=root, check=False).returncode)
    if args and args[0] in ('crap', 'mutate', 'dry'):
        # 质量分发实现与安装器共用入口，独立模块保留协议边界。
        from uml_quality import dispatch
        return dispatch(root, args[0], args[1:])
    # 其余路径都是 viewer 生命周期：默认 fresh、--restart 替换、--backend herdr。
    from uml_launch import launch
    return launch(root, tool_root, args, restart='--restart' in args)


def utf8_streams():
    """Windows: diagnostics in UTF-8 whatever the ANSI code page.

    A piped stream otherwise uses the code page, which cannot encode the
    Chinese messages on a Western system; a console is UTF-8 already.
    """
    if WINDOWS:
        for stream in (sys.stdout, sys.stderr):
            stream.reconfigure(encoding='utf-8', errors='backslashreplace')


def main():
    utf8_streams()
    try:
        if sys.version_info < (3, 11):
            raise ValueError('需要 Python 3.11+')
        if sys.argv[1] == 'install':
            return install(sys.argv[2:])
        if sys.argv[1] == 'font':
            # The font step alone, for the viewer's real-font test.
            return font(Path(sys.argv[2]))
        return command(Path(sys.argv[2]), sys.argv[3:])
    except subprocess.CalledProcessError as error:
        print(f'uml: 命令失败（退出 {error.returncode}）：{error.cmd[0]}', file=sys.stderr)
        return native_exit(error.returncode) if error.returncode > 0 else 128 - error.returncode
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        print(f'uml: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    # -I 排除项目目录；只加入受信任的入口脚本目录。
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    sys.exit(main())
