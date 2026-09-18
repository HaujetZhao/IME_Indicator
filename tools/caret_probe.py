# -*- coding: utf-8 -*-
"""探针：对比两路光标信息，观察有选区时各上报什么。

A 路 GetGUIThreadInfo：hwndCaret + rcCaret
B 路 OBJID_CARET：AccessibleObjectFromWindow + accLocation (x, y, w, h)

用法：`uv run python -u tools/caret_probe.py`，在目标窗口里动光标/做选区。
"""
import ctypes
from ctypes import wintypes

user32 = ctypes.windll.user32
oleacc = ctypes.windll.oleacc
ole32 = ctypes.windll.ole32
kernel32 = ctypes.windll.kernel32

OBJID_CARET = 0xFFFFFFF8
# IID_IAccessible {618736e0-3c3d-11cf-810c-00aa00389b71}
IID_IAccessible = (ctypes.c_ubyte * 16)(
    0xE0, 0x36, 0x87, 0x61, 0x3D, 0x11, 0xCF, 0x81,
    0x0C, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
)


class VARIANT(ctypes.Structure):
    _fields_ = [("vt", ctypes.c_ushort), ("reserved", ctypes.c_ubyte * 6),
                ("lVal", ctypes.c_long)]


class RECT(ctypes.Structure):
    _fields_ = [("left", wintypes.LONG), ("top", wintypes.LONG),
                ("right", wintypes.LONG), ("bottom", wintypes.LONG)]


class GUITHREADINFO(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("flags", wintypes.DWORD),
                ("hwndActive", wintypes.HWND), ("hwndFocus", wintypes.HWND),
                ("hwndCapture", wintypes.HWND), ("hwndMenuOwner", wintypes.HWND),
                ("hwndMoveSize", wintypes.HWND), ("hwndCaret", wintypes.HWND),
                ("rcCaret", RECT)]


# IAccessible vtable：accLocation 是第 10 个槽（0 起）
AccLocation = ctypes.WINFUNCTYPE(
    wintypes.LONG, ctypes.c_void_p,
    ctypes.POINTER(wintypes.LONG), ctypes.POINTER(wintypes.LONG),
    ctypes.POINTER(wintypes.LONG), ctypes.POINTER(wintypes.LONG),
    VARIANT,
)

var_child = VARIANT(vt=3)  # VT_I4, CHILDID_SELF


def read_gui_info():
    gi = GUITHREADINFO(cbSize=ctypes.sizeof(GUITHREADINFO))
    if not user32.GetGUIThreadInfo(0, ctypes.byref(gi)):
        return "GetGUIThreadInfo 失败"
    if not gi.hwndCaret:
        return "无 hwndCaret"
    r = gi.rcCaret
    return f"gui_info: hwnd=0x{gi.hwndCaret:X} rect=({r.left},{r.top},{r.right-r.left}x{r.bottom-r.top})"


def read_msaa():
    hwnd = user32.GetForegroundWindow()
    if not hwnd:
        return "无前台窗口"
    p_acc = ctypes.c_void_p()
    hr = oleacc.AccessibleObjectFromWindow(
        hwnd, OBJID_CARET, ctypes.byref(IID_IAccessible), ctypes.byref(p_acc)
    )
    if hr != 0 or not p_acc:
        return f"OBJID_CARET 失败 hr=0x{hr & 0xFFFFFFFF:08X}"
    acc = ctypes.cast(p_acc, ctypes.POINTER(ctypes.c_void_p))
    vtable = ctypes.cast(acc[0], ctypes.POINTER(ctypes.c_void_p))
    func = AccLocation(vtable[10])
    x, y, w, h = (wintypes.LONG() for _ in range(4))
    hr = func(p_acc, ctypes.byref(x), ctypes.byref(y), ctypes.byref(w), ctypes.byref(h), var_child)
    if hr != 0:
        return f"accLocation 失败 hr=0x{hr & 0xFFFFFFFF:08X}"
    return f"msaa: ({x.value},{y.value}) {w.value}x{h.value}"


def read_uia_selection():
    """UIA TextPattern：选区包围盒"""
    try:
        import uiautomation as auto
    except ImportError:
        return "未安装 uiautomation"
    try:
        focused = auto.GetFocusedControl()
        if not focused:
            return "UIA: 无焦点元素"
        pattern = focused.GetPattern(auto.PatternId.TextPattern)
        if not pattern:
            return "UIA: 焦点元素无 TextPattern"
        rects = []
        for r in pattern.GetSelection():
            for rect in r.GetBoundingRectangles():
                rects.append(str(rect))
        return "uia_selection: " + ("; ".join(rects) if rects else "空")
    except Exception as e:
        return f"UIA 异常: {e}"


last = None
last_hwnd = None
print("在目标窗口里移动光标/做选区，变化时打印，Ctrl+C 退出")
while True:
    hwnd = user32.GetForegroundWindow()
    if not hwnd:
        kernel32.Sleep(100)
        continue
    if hwnd != last_hwnd:
        last_hwnd = hwnd
        last = None
        print(f"-- 前台窗口 hwnd=0x{hwnd:X} --")
    a = read_gui_info()
    b = read_msaa()
    c = read_uia_selection()
    cur = (a, b, c)
    if cur != last:
        last = cur
        print(f"{a}\n{b}\n{c}")
    kernel32.Sleep(100)
