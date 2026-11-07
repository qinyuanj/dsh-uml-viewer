"""Native viewer-window selection and normal close for acceptance scripts."""
import ctypes
from ctypes import wintypes
import os

WM_CLOSE = 0x0010

if os.name == 'nt':
    _user32 = ctypes.WinDLL('user32', use_last_error=True)
    _ENUM = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    _user32.EnumWindows.argtypes = [_ENUM, wintypes.LPARAM]
    _user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    _user32.IsWindowVisible.argtypes = [wintypes.HWND]
    _user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    _user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]


def viewer_windows(pid):
    """Visible, titled UML viewer roots owned by exactly `pid`.

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
