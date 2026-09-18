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
gdi32 = ctypes.windll.gdi32
imm32 = ctypes.windll.imm32


class LOGFONTW(ctypes.Structure):
    _fields_ = [("lfHeight", wintypes.LONG), ("lfWidth", wintypes.LONG),
                ("lfEscapement", wintypes.LONG), ("lfOrientation", wintypes.LONG),
                ("lfWeight", wintypes.LONG), ("lfItalic", wintypes.BYTE),
                ("lfUnderline", wintypes.BYTE), ("lfStrikeOut", wintypes.BYTE),
                ("lfCharSet", wintypes.BYTE), ("lfOutPrecision", wintypes.BYTE),
                ("lfClipPrecision", wintypes.BYTE), ("lfQuality", wintypes.BYTE),
                ("lfPitchAndFamily", wintypes.BYTE), ("lfFaceName", wintypes.WCHAR * 32)]


class TEXTMETRICW(ctypes.Structure):
    _fields_ = [("tmHeight", wintypes.LONG), ("tmAscent", wintypes.LONG),
                ("tmDescent", wintypes.LONG), ("tmInternalLeading", wintypes.LONG),
                ("tmExternalLeading", wintypes.LONG), ("tmAveCharWidth", wintypes.LONG),
                ("tmMaxCharWidth", wintypes.LONG), ("tmWeight", wintypes.LONG),
                ("tmOverhang", wintypes.LONG), ("tmDigitizedAspectX", wintypes.LONG),
                ("tmDigitizedAspectY", wintypes.LONG), ("tmFirstChar", wintypes.WCHAR),
                ("tmLastChar", wintypes.WCHAR), ("tmDefaultChar", wintypes.WCHAR),
                ("tmBreakChar", wintypes.WCHAR), ("tmItalic", wintypes.BYTE),
                ("tmUnderlined", wintypes.BYTE), ("tmStruckOut", wintypes.BYTE),
                ("tmPitchAndFamily", wintypes.BYTE), ("tmCharSet", wintypes.BYTE)]


class NONCLIENTMETRICSW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.UINT), ("iBorderWidth", ctypes.c_int),
                ("iScrollWidth", ctypes.c_int), ("iScrollHeight", ctypes.c_int),
                ("iCaptionWidth", ctypes.c_int), ("iCaptionHeight", ctypes.c_int),
                ("lfCaptionFont", LOGFONTW), ("iSmCaptionWidth", ctypes.c_int),
                ("iSmCaptionHeight", ctypes.c_int), ("lfSmCaptionFont", LOGFONTW),
                ("iMenuWidth", ctypes.c_int), ("iMenuHeight", ctypes.c_int),
                ("lfMenuFont", LOGFONTW), ("lfStatusFont", LOGFONTW),
                ("lfMessageFont", LOGFONTW),
                ("iPaddedBorderWidth", ctypes.c_int)]


def tm_height(hfont):
    """给定 HFONT,返回选入 DC 后的 tmHeight"""
    hdc = user32.GetDC(None)
    old = gdi32.SelectObject(hdc, hfont)
    tm = TEXTMETRICW()
    ok = gdi32.GetTextMetricsW(hdc, ctypes.byref(tm))
    gdi32.SelectObject(hdc, old)
    user32.ReleaseDC(None, hdc)
    return tm.tmHeight if ok else None


def heights_of(hwnd):
    """各来源的字体行高"""
    out = {}
    # 1) WM_GETFONT
    hf = user32.SendMessageW(hwnd, 0x0031, 0, 0)  # WM_GETFONT
    out["wm_getfont"] = tm_height(hf) if hf else None
    # 2) DEFAULT_GUI_FONT
    out["default_gui"] = tm_height(gdi32.GetStockObject(17))  # DEFAULT_GUI_FONT
    # 3) NONCLIENTMETRICS.lfMessageFont
    ncm = NONCLIENTMETRICSW()
    ncm.cbSize = ctypes.sizeof(ncm)
    if user32.SystemParametersInfoW(0x0029, ncm.cbSize, ctypes.byref(ncm), 0):  # SPI_GETNONCLIENTMETRICS
        hfont = gdi32.CreateFontIndirectW(ctypes.byref(ncm.lfMessageFont))
        out["message_font"] = tm_height(hfont)
        gdi32.DeleteObject(hfont)
    # 4) IME 合成字体(候选框定位用的就是它)
    himc = imm32.ImmGetContext(hwnd)
    if himc:
        lf = LOGFONTW()
        if imm32.ImmGetCompositionFontW(himc, ctypes.byref(lf)):
            out["ime_font"] = f"lfHeight={lf.lfHeight}"
        imm32.ImmReleaseContext(hwnd, himc)
    return out

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


def class_name(hwnd):
    buf = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(hwnd, buf, 256)
    return buf.value


def hwnd_info(hwnd):
    """类名、窗口矩形(屏幕坐标)、父窗口类名"""
    rect = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    parent = user32.GetParent(hwnd)
    return class_name(hwnd), rect, class_name(parent) if parent else "-"


def read_gui_info():
    gi = GUITHREADINFO(cbSize=ctypes.sizeof(GUITHREADINFO))
    if not user32.GetGUIThreadInfo(0, ctypes.byref(gi)):
        return "GetGUIThreadInfo 失败"
    if not gi.hwndCaret:
        return "无 hwndCaret"
    r = gi.rcCaret
    pt = wintypes.POINT(r.left, r.top)
    user32.ClientToScreen(gi.hwndCaret, ctypes.byref(pt))
    cls, wrect, parent_cls = hwnd_info(gi.hwndCaret)
    focus_buf = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(gi.hwndFocus, focus_buf, 256)
    heights = heights_of(gi.hwndFocus) if gi.hwndFocus else {}
    hs = " ".join(f"{k}={v}" for k, v in heights.items())
    return (
        f"gui_info: hwnd=0x{gi.hwndCaret:X}({cls}) rect=({r.left},{r.top},{r.right-r.left}x{r.bottom-r.top}) "
        f"→ 屏幕=({pt.x},{pt.y}) "
        f"焦点={focus_buf.value} 行高[{hs}]"
    )


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
