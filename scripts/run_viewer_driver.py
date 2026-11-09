#!/usr/bin/env python3
"""在真实 viewer 里运行一个 GUI 行为 driver，断言可观察结果。

driver 是声明式 EDN 脚本（`scripts/drivers/<name>.edn`），由 Rust egui
viewer 重放（#139；Lua 实现已随 #144 删除）。driver 通过
`UML_VIEWER_TEST_DRIVER` 拿脚本路径、写入证据文件，并遵守同样的退出契约。
原先每个 driver 配一个包装器（设环境变量、启动 viewer、检查证据）；那些
PowerShell 包装器已随 Windows 原生支持退役（ADR 0007），本脚本是它们唯一的
POSIX 替代，因此断言本身不丢失。

用法：
    python3 -I scripts/run_viewer_driver.py metrics
    python3 -I scripts/run_viewer_driver.py navigation
    python3 -I scripts/run_viewer_driver.py metrics --evidence DIR --seconds 60
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

REPO = Path(__file__).resolve().parents[1]
WINDOWS = sys.platform == 'win32'


def edn_string(text):
    """`text` as an EDN string literal: a Windows path's backslashes escaped."""
    return '"' + str(text).replace('\\', '\\\\').replace('"', '\\"') + '"'


def write_text(path, text):
    """UTF-8 with LF on every host, as the POSIX runs wrote it."""
    with open(path, 'w', encoding='utf-8', newline='\n') as output:
        output.write(text)


def wait_for(test, timeout, interval=0.05, message='condition'):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = test()
        if value:
            return value
        time.sleep(interval)
    raise AssertionError(f'timed out waiting for {message}')


# What each driver proves, expressed as the evidence files it must publish.
# These lists came from the retired PowerShell wrappers, so retiring a wrapper
# did not silently drop a behavioural assertion.
# Each driver asserts against specific project content; the retired PowerShell
# wrappers wrote that content before launching the viewer. These fixtures replace
# exactly that setup, so retiring a wrapper dropped no behavioural assertion.
def prepare_metrics(project):
    (project / '.metrics').mkdir(parents=True, exist_ok=True)
    write_text(
        project / 'diagram.edn',
        '{:packages [{:label "Adapters" :classes [{:id :ffi :ns "ffi"}'
        ' {:id :imgui :ns "imgui"}]}] :edges [{:from :ffi :to :imgui}]}')
    write_text(
        project / '.metrics/crap.edn',
        '{:entries [{:namespace "ffi" :name "draw" :complexity 1'
        ' :coverage 100 :crap 1}]}')


def prepare_navigation(project):
    children = ' '.join(f'{{:id :source.child{n}}}' for n in range(1, 21))
    ops = ' '.join(f'"operation{n}"' for n in range(1, 71))
    write_text(
        project / 'diagram.edn',
        '{:hierarchical true :title "Navigation acceptance" :order [:source :other]'
        f' :classes [{{:id :source}} {{:id :source.lua}} {{:id :source.lua.parser}} {children}'
        f' {{:id :other :ops [{ops}]}}] :edges []}}')


def prepare_search(project):
    write_text(
        project / 'diagram.edn',
        '{:hierarchical true :classes [{:id :alpha.deep.parser :name "解析器"'
        ' :ns "demo.alpha.deep.parser"} {:id :beta.parser :name "解析器"'
        ' :ns "demo.beta.parser"}] :edges [{:from :alpha.deep.parser'
        ' :to :beta.parser :kind :dependency}]}')


def prepare_review_fixes(project):
    # Two independent source windows whose selections must not leak into each
    # other, and a text field elsewhere in the UI that must keep its own copy.
    write_text(project / 'diagram.edn', '{:packages [{:id :p :classes [{:id :a}]}]}')
    write_text(project / 'first.lua', 'local FIRST = 1\nreturn FIRST\n')
    write_text(project / 'second.lua', 'local SECOND = 2\nreturn SECOND\n')


def prepare_source_jump(project):
    # The retired wrapper named the source file absolutely; the driver asserts
    # the row it lands on, so the path must resolve exactly as written.
    source = project / 'source.lua'
    write_text(source, "local M = {}\n" + ''.join(f'-- preceding line {n}\n' for n in range(1, 81))
               + "\tfunction M.target() local 变量 = 'target ## %'; return 变量 end\nreturn M\n")
    write_text(project / 'diagram.edn',
               '{:hierarchical true :title "Source jump acceptance" :classes'
               ' [{:id :app.some_module :name "some_module" :ns "app.some_module" :lang :lua'
               f' :file {edn_string(source)} :ops [{{:name "M.target"}}]}}] :edges []}}')


def prepare_source_window(project):
    prepare_source_jump(project)


def prepare_inspector(project):
    write_text(
        project / 'diagram.edn',
        '{:title "中文 Inspector" :hierarchical true :prefix "demo" :classes'
        ' [{:id :a :name "中文类" :ns "demo.a" :ops [{:name "work" :text "work()"}]}]'
        ' :proposals [{:id :plan :name "中文方案" :layers []}'
        ' {:id :keep :name "保留方案" :layers []}]}')
    # Proposal write-back (#141) must keep the other proposal and every other
    # policy key, so the IR has a sibling policy carrying both.
    write_text(
        project / 'diagram.policy.edn',
        '{:lang :rust :src "crates" :prefix "demo" :omit [:zzz]'
        ' :proposals [{:id :plan :name "中文方案" :layers []}'
        ' {:id :keep :name "保留方案" :layers []}]}')


def prepare_detail_scroll(project):
    ops = ' '.join(f'{{:name "op{n}" :text "op{n}()"}}' for n in range(1, 41))
    write_text(
        project / 'diagram.edn',
        '{:title "Scroll Inspector" :hierarchical true :prefix "demo" :classes'
        f' [{{:id :a :name "A" :ns "demo.a" :ops [{ops}]}}]'
        ' :proposals [{:id :plan :name "Plan A" :layers []}]}')


def prepare_fonts(project):
    write_text(
        project / 'diagram.edn',
        '{:hierarchical true :prefix "demo" :classes [{:id :a :name "中文类名"'
        ' :ns "demo.a"}] :proposals [{:id :plan :name "中文提案" :layers []}]}')


def prepare_font_parity(project):
    write_text(
        project / 'diagram.edn',
        '{:hierarchical true :prefix "demo" :classes [{:id :demo.detail'
        ' :name "中文详情类名" :ns "demo.detail" :level 2 :stereotype :abstract'
        ' :fields ["中文字段"] :ops [{:name "读取" :text "读取中文数据()" :crap 2'
        ' :coverage 0.9 :killed 3 :survived 0}]}]}')


def prepare_dpi_window(project):
    write_text(
        project / 'diagram.edn',
        '{:title "DPI 角标" :hierarchical true :prefix "demo" :classes'
        ' [{:id :a :name "中文类" :ns "demo.a" :level 2 :stereotype :interface'
        ' :ops [{:name "work" :text "work()"}]} {:id :b :name "Beta" :ns "demo.b"'
        ' :stereotype :abstract :fields ["count"]}]'
        ' :edges [{:from :a :to :b :kind :dependency}]}')


def prepare_standalone(project):
    # No `.uml-viewer/` state at all: the window alone.
    write_text(
        project / 'diagram.edn',
        '{:title "Standalone" :packages [{:label "P" :classes'
        ' [{:id :target :ns "app.target"}]}]}')


FIXTURES = {
    'metrics': prepare_metrics,
    'navigation': prepare_navigation,
    'search': prepare_search,
    'review_fixes': prepare_review_fixes,
    'source_jump': prepare_source_jump,
    'source_window': prepare_source_window,
    'inspector': prepare_inspector,
    'detail_scroll': prepare_detail_scroll,
    'fonts': prepare_fonts,
    'font_parity': prepare_font_parity,
    'dpi_window': prepare_dpi_window,
    'standalone': prepare_standalone,
}


def sarasa_font():
    """The provisioned Sarasa font, when the installer has put it in place."""
    path = REPO / '.uml-viewer/fonts/SarasaMonoSC-Regular.ttf'
    return path if path.is_file() else None


def provision_sarasa(project):
    font = sarasa_font()
    if not font:
        raise AssertionError('Sarasa is required by this driver: provision '
                             '.uml-viewer/fonts/SarasaMonoSC-Regular.ttf')
    target = project / '.uml-viewer/fonts'
    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(font, target / 'SarasaMonoSC-Regular.ttf')
    return font


# Per-driver process environment. The retired wrappers set these; the drivers
# read them, so losing them would silently weaken the assertion.
def env_fonts(project, evidence):
    """Three lookup cases, as the retired wrapper ran them."""
    font = provision_sarasa(project)
    return {'UML_CJK_FONT': '', 'UML_FONT_TEST_EXPECT': 'sarasa',
            'UML_FONT_TEST_EXTRA': '中文类名提案'}


def env_fonts_override(project, evidence):
    font = provision_sarasa(project)
    return {'UML_CJK_FONT': str(font), 'UML_FONT_TEST_EXPECT': 'env',
            'UML_FONT_TEST_EXTRA': '中文类名提案'}


def env_fonts_fallback(project, evidence):
    return {'UML_CJK_FONT': '', 'UML_FONT_TEST_EXPECT': 'system',
            'UML_FONT_TEST_EXTRA': '中文类名提案'}


def env_inspector(project, evidence):
    provision_sarasa(project)
    return {'UML_CJK_FONT': '', 'UML_FONT_TEST_EXPECT': 'sarasa'}


def env_font_parity(project, evidence):
    font = provision_sarasa(project)
    return {'UML_CJK_FONT': str(font), 'UML_PARITY_SCENE': 'chinese'}


def env_font_parity_baseline(project, evidence):
    font = provision_sarasa(project)
    return {'UML_CJK_FONT': str(font), 'UML_PARITY_SCENE': 'baseline'}


def env_source_jump(project, evidence):
    return {'UML68_EVIDENCE': str(evidence), 'UML68_SOURCE_FILE': str(project / 'source.lua')}


def env_review_fixes(project, evidence):
    return {'UML78_EVIDENCE': str(evidence), 'UML78_THROW': '0'}


def env_dpi(project, evidence, phase='first', tag='run', scale='1'):
    provision_sarasa(project)
    return {'UML_CJK_FONT': '', 'UML_VIEWER_TEST_DPI_SCALE': scale,
            'UML_DPI_TEST_PHASE': phase, 'UML_DPI_TEST_TAG': tag}


class DriverSkipped(Exception):
    """The driver's precondition is absent on this machine (not a failure)."""


# Drivers the retired wrappers ran several times, each in its own project. A
# scenario states its own fixture (when it differs from the shared one), the
# environment the driver reads, the viewer arguments and its pass markers.
def scenarios_fonts(project, evidence):
    return [
        {'tag': 'provisioned', 'needs_sarasa': True, 'env': env_fonts},
        {'tag': 'override', 'needs_sarasa': True, 'env': env_fonts_override},
        # The fallback case must run where no Sarasa is provisioned at all.
        {'tag': 'fallback', 'env': env_fonts_fallback},
    ]


def scenarios_font_parity(project, evidence):
    return [
        {'tag': 'baseline', 'needs_sarasa': True, 'env': env_font_parity_baseline,
         'diagram': str(REPO / 'examples/uml-viewer.edn'),
         'script': 'font_parity_baseline'},
        {'tag': 'chinese', 'needs_sarasa': True, 'env': env_font_parity},
    ]


def scenarios_review_fixes(project, evidence):
    return [{'tag': 'clipboard', 'env': env_review_fixes, 'markers': ['clipboard-pass.txt']}]


def scenarios_dpi_window(project, evidence):
    # Scale 1 is the deterministic case; other scales need a real display.
    # On Windows the live process must also be per-monitor DPI aware.
    return [{'tag': 'first-1', 'needs_sarasa': True,
             'env': lambda p, e: env_dpi(p, e, 'first', 'first-1', '1'),
             'observe': observe_dpi,
             'markers': ['first-1-pass.edn']}]


def scenarios_terminate(project, evidence):
    return [{'tag': 'terminate', 'observe': observe_terminate,
             'markers': ['terminate-ready.txt', 'terminate-pass.txt']}]


# Environments a single-run driver needs. Missing here means the retired
# wrapper's setup would be lost.
DRIVER_ENV = {
    # Force the undecorated Wayland window (the libxkbcommon-x11 fallback).
    'window_controls': lambda p, e: {'UML_VIEWER_TEST_UNDECORATED': '1'},
    'inspector': env_inspector,
    'detail_scroll': env_inspector,
    'source_jump': env_source_jump,
    'source_window': env_source_jump,
}


SCENARIOS = {
    'fonts': scenarios_fonts,
    'font_parity': scenarios_font_parity,
    'review_fixes': scenarios_review_fixes,
    'dpi_window': scenarios_dpi_window,
    'terminate': scenarios_terminate,
}


DRIVERS = {
    'class_boxes': {'seconds': 20, 'evidence': ['class-boxes-pass.txt', 'render.png']},
    'detail_scroll': {'seconds': 30, 'evidence': ['inspector-pass.txt', 'inspector.png', 'inspector-empty.png']},
    'inspector': {'seconds': 30, 'evidence': ['inspector-pass.txt', 'inspector.png']},
    'navigation': {'seconds': 20, 'evidence': ['navigation-pass.txt', 'navigation-entered.edn',
                                               'navigation-zoomed.edn', 'navigation-panned.edn',
                                               'navigation-fitted.edn', 'navigation-back.edn',
                                               'navigation-selected.edn', 'navigation-wheel.edn',
                                               'render.png']},
    'search': {'seconds': 30, 'evidence': ['search-pass.txt', 'search.png']},
    'source_jump': {'seconds': 30, 'evidence': ['source-jump-pass.txt', 'module.txt', 'module.png',
                                                'member.txt', 'member.png']},
    'source_window': {'seconds': 20, 'evidence': ['source-window.txt', 'source-window.png']},
    'review_fixes': {'seconds': 30, 'evidence': []},
    'standalone': {'seconds': 20, 'evidence': ['standalone-pass.txt']},
    'metrics': {'seconds': 20, 'evidence': ['metrics-pass.txt', 'scene-1.edn', 'scene-2.edn']},
    'lod_shots': {'seconds': 30, 'evidence': ['lod-blocks.png', 'lod-frames.png',
                                              'lod-full.png', 'renderer.txt']},
    'fonts': {'seconds': 30, 'evidence': []},
    'font_parity': {'seconds': 30, 'evidence': []},
    'dpi_window': {'seconds': 30, 'evidence': []},
    'terminate': {'seconds': 60, 'evidence': ['terminate-ready.txt', 'terminate-pass.txt']},
    'window_controls': {'seconds': 30, 'evidence': ['window-controls-display.edn',
                                                    'window-controls.png']},
}


def edn_script(name):
    """The declarative script of a driver (#139)."""
    path = REPO / f'scripts/drivers/{name}.edn'
    return path if path.is_file() else None


def known_drivers():
    return sorted(path.stem for path in (REPO / 'scripts/drivers').glob('*.edn'))


def rust_executable():
    configured = os.environ.get('UML_VIEWER_RUST_EXECUTABLE')
    if configured:
        return configured
    for profile in ('release', 'debug'):
        binary = REPO / 'target' / profile / ('uml-viewer.exe' if WINDOWS else 'uml-viewer')
        if binary.is_file():
            return str(binary)
    raise ValueError('the Rust uml-viewer binary is missing: run cargo build --release '
                     '(or set UML_VIEWER_RUST_EXECUTABLE)')


WM_CLOSE = 0x0010

if WINDOWS:
    import ctypes
    from ctypes import wintypes

    _kernel32 = ctypes.WinDLL('kernel32', use_last_error=True)
    _shcore = ctypes.WinDLL('shcore', use_last_error=True)
    _user32 = ctypes.WinDLL('user32', use_last_error=True)
    _kernel32.OpenProcess.restype = wintypes.HANDLE
    _kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    _kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    _shcore.GetProcessDpiAwareness.argtypes = [wintypes.HANDLE, ctypes.POINTER(ctypes.c_int)]
    _ENUM = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    _user32.EnumWindows.argtypes = [_ENUM, wintypes.LPARAM]
    _user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    _user32.IsWindowVisible.argtypes = [wintypes.HWND]
    _user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    _user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]


def viewer_windows(pid):
    """Visible, titled UML viewer roots owned by exactly `pid` (Windows).

    Winit's empty-title Thread Event Target can also appear visible. Closing
    it breaks event delivery, so visibility and PID alone are insufficient.
    """
    found = []

    def visit(hwnd, _):
        owner = wintypes.DWORD()
        _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and _user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(512)
            _user32.GetWindowTextW(hwnd, title, len(title))
            if title.value.startswith('UML viewer'):
                found.append(hwnd)
        return True

    _user32.EnumWindows(_ENUM(visit), 0)
    return found


def close_window(pid):
    """Post one native WM_CLOSE to each viewer root; return whether found."""
    windows = viewer_windows(pid)
    for hwnd in windows:
        if not _user32.PostMessageW(hwnd, WM_CLOSE, 0, 0):
            raise ctypes.WinError(ctypes.get_last_error())
    return bool(windows)


def terminate_politely(process):
    """The platform's polite termination: `WM_CLOSE` to the viewer's window
    on Windows (the runner gives it no console for CTRL_BREAK), SIGTERM
    elsewhere. Both close it with exit 0."""
    if not WINDOWS:
        process.send_signal(signal.SIGTERM)
        return
    wait_for(lambda: close_window(process.pid), 30, message='the viewer window')


# PROCESS_DPI_AWARENESS: what winit asked for (per-monitor v2 reports 2).
DPI_AWARENESS = {0: 'unaware', 1: 'system', 2: 'per-monitor'}


def dpi_awareness(pid):
    """The DPI awareness of process `pid` (Windows)."""
    handle = _kernel32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        value = ctypes.c_int(-1)
        result = _shcore.GetProcessDpiAwareness(handle, ctypes.byref(value))
        if result != 0:
            raise OSError(f'GetProcessDpiAwareness failed: {result:#x}')
        return DPI_AWARENESS.get(value.value, str(value.value))
    finally:
        _kernel32.CloseHandle(handle)


def observe_dpi(process, project, evidence, tag):
    """Per-monitor DPI awareness, read from the live viewer once its window is up."""
    if not WINDOWS:
        return
    wait_for(lambda: viewer_windows(process.pid), 30, message='the viewer window')
    awareness = dpi_awareness(process.pid)
    write_text(evidence / f'{tag}-dpi-awareness.txt', awareness + '\n')
    assert awareness == 'per-monitor', f'{tag}: the viewer is {awareness} DPI aware'


def observe_terminate(process, project, evidence, tag):
    """After the driver is ready, terminate politely; the viewer must take the
    normal close (exit 0) promptly, not the script's `:quit 1`."""
    wait_for(lambda: (evidence / 'terminate-ready.txt').exists(), 60,
             message='the terminate driver to be ready')
    terminate_politely(process)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        raise AssertionError(f'{tag}: the viewer ignored the polite termination')
    write_text(evidence / 'terminate-pass.txt', 'closed\n')


def run_viewer(project, evidence, seconds, driver, diagram, extra_env, tag='run',
               viewer_args=(), observe=None):
    """Launch the real viewer once and require success plus its pass marker.

    `observe(process, project, evidence, tag)` runs beside the live viewer;
    its AssertionError fails the scenario."""
    xdg = Path(tempfile.mkdtemp(prefix='uml-viewer-xdg-'))
    env = dict(os.environ,
               XDG_CONFIG_HOME=str(xdg / 'config'), XDG_DATA_HOME=str(xdg / 'data'),
               XDG_CACHE_HOME=str(xdg / 'cache'), XDG_STATE_HOME=str(xdg / 'state'),
               UML_VIEWER_PROJECT_ROOT=str(project), UML_VIEWER_EVIDENCE=str(evidence),
               UML_VIEWER_TEST_SECONDS=str(seconds),
               UML_VIEWER_TEST_DRIVER=str(driver))
    argv = [rust_executable(), 'viewer', *viewer_args, *diagram]
    # WSLg: without these the viewer falls back to llvmpipe (ADR 0008).
    # Native Windows keeps wgpu's own D3D12 backend.
    if not WINDOWS:
        env.setdefault('GALLIUM_DRIVER', 'd3d12')
        env.setdefault('WGPU_BACKEND', 'gl')
    env.update(extra_env)
    failures = []
    try:
        process = subprocess.Popen(argv, cwd=project, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, encoding='utf-8',
                                   errors='replace')
        watcher = None
        if observe:
            def watch():
                try:
                    observe(process, project, evidence, tag)
                except (AssertionError, OSError) as error:
                    failures.append(error)
            watcher = threading.Thread(target=watch, daemon=True)
            watcher.start()
        try:
            stdout, stderr = process.communicate(timeout=seconds + 120)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
            raise
        if watcher:
            watcher.join(timeout=30)
    finally:
        shutil.rmtree(xdg, ignore_errors=True)
    write_text(evidence / f'{tag}-stdout.txt', stdout or '')
    write_text(evidence / f'{tag}-stderr.txt', stderr or '')
    if failures:
        raise AssertionError(str(failures[0]))
    return subprocess.CompletedProcess(argv, process.returncode, stdout, stderr)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('driver', help='driver name, e.g. navigation for scripts/drivers/navigation.edn')
    parser.add_argument('--evidence')
    parser.add_argument('--seconds', type=float, default=None)
    parser.add_argument('--project-root', help='use this project instead of an isolated copy')
    parser.add_argument('--diagram', help='initial diagram, relative to the project root')
    parser.add_argument('--pass-marker', action='append', default=[],
                        help='evidence file the driver must create (repeatable)')
    arguments = parser.parse_args()

    script = edn_script(arguments.driver)
    if not script:
        print(f'unknown driver {arguments.driver!r}: no scripts/drivers/{arguments.driver}.edn; '
              f'known drivers: {", ".join(known_drivers())}', file=sys.stderr)
        return 2
    spec = DRIVERS.get(arguments.driver) or {'seconds': 90, 'evidence': []}
    seconds = arguments.seconds or spec['seconds']
    markers = list(arguments.pass_marker) or list(spec['evidence'])
    driver = script
    evidence = Path(arguments.evidence or tempfile.mkdtemp(prefix='uml-viewer-evidence-'))
    evidence.mkdir(parents=True, exist_ok=True)
    temporary_project = arguments.project_root is None
    project = Path(arguments.project_root) if arguments.project_root else \
        Path(tempfile.mkdtemp(prefix='uml viewer 项目 '))
    result = {'driver': arguments.driver, 'runtime': 'rust',
              'project': str(project), 'evidence': str(evidence)}
    try:
        if temporary_project:
            (project / '.uml-viewer').mkdir(parents=True)
        fixture = FIXTURES.get(arguments.driver)
        scenarios = SCENARIOS[arguments.driver](project, evidence) \
            if arguments.driver in SCENARIOS else [{'tag': 'run'}]
        for scenario in scenarios:
            tag = scenario['tag']
            # Each scenario starts from the same content. The provisioned font
            # directory in particular is per scenario: a "no Sarasa" case must
            # not inherit the font the previous case placed there.
            shutil.copy2(REPO / 'examples/uml-viewer.edn', project / 'diagram.edn')
            shutil.rmtree(project / '.uml-viewer/fonts', ignore_errors=True)
            if fixture:
                fixture(project)
            if scenario.get('setup'):
                scenario['setup'](project, evidence)
            if arguments.diagram:
                diagram = [arguments.diagram]
            elif scenario.get('diagram'):
                diagram = [scenario['diagram']]
            else:
                diagram = ['diagram.edn']
            try:
                extra = scenario['env'](project, evidence) if 'env' in scenario \
                    else DRIVER_ENV.get(arguments.driver, lambda p, e: {})(project, evidence)
            except AssertionError as error:
                raise DriverSkipped(str(error))
            # A scenario may name its own EDN script instead.
            scenario_driver = driver
            if scenario.get('script'):
                scenario_driver = edn_script(scenario['script'])
                assert scenario_driver, f"{tag}: no EDN script {scenario['script']}"
            process = run_viewer(project, evidence, seconds, scenario_driver, diagram, extra,
                                 tag=tag, viewer_args=scenario.get('args', []),
                                 observe=scenario.get('observe'))
            expected = scenario.get('markers', markers)
            for marker in expected:
                assert (evidence / marker).exists() or (project / marker).exists(), \
                    f'{tag}: driver did not publish {marker}'
            assert process.returncode == 0, \
                f'{tag}: viewer exited {process.returncode}: {(process.stderr or "").strip()[-800:]}'
        result['ok'] = True
    except DriverSkipped as error:
        result['skipped'] = str(error)
    except (AssertionError, subprocess.TimeoutExpired) as error:
        result['ok'] = False
        result['failure'] = str(error)
    finally:
        if (result.get('ok') or result.get('skipped')) and temporary_project:
            shutil.rmtree(project, ignore_errors=True)
        elif not result.get('ok'):
            result['project_retained'] = str(project)
        write_text(evidence / 'result.json', json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    if result.get('skipped'):
        print(f'SKIP driver {arguments.driver}: {result["skipped"]}')
        return 0
    if result.get('ok'):
        print(f'PASS driver {arguments.driver}; evidence: {evidence}')
        return 0
    print(f'FAIL driver {arguments.driver}: {result.get("failure")}', file=sys.stderr)
    print(f'evidence: {evidence}', file=sys.stderr)
    return 1


if __name__ == '__main__':
    sys.exit(main())
