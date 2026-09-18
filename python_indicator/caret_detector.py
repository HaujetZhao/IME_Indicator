import ctypes
from ctypes import byref, sizeof, wintypes, Structure, POINTER
from win32_api import user32, oleacc, GUITHREADINFO, OBJID_CARET
import uiautomation as auto

# 禁用 uiautomation 的一些冗长输出
auto.uiautomation.DEBUG_SEARCH_TIME = False
auto.uiautomation.DEBUG_GET_PATTERN = False

class VARIANT(Structure):
    _fields_ = [
        ("vt", wintypes.WORD),
        ("wReserved1", wintypes.WORD),
        ("wReserved2", wintypes.WORD),
        ("wReserved3", wintypes.WORD),
        ("lVal", wintypes.LONG),
        ("filler", wintypes.LONG),
    ]

class CaretDetector:
    def __init__(self):
        # IID_IAccessible: {618736e0-3c3d-11cf-810c-00aa00389b71}
        self._guid_iaaccessible = (wintypes.BYTE * 16)(
            0xE0, 0x36, 0x87, 0x61, 0x3D, 0x3C, 0xCF, 0x11,
            0x81, 0x0C, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71
        )

    def is_readonly_document_focus(self) -> bool:
        """可见性线（黑名单制）：焦点是否位于只读 Document（浏览器网页正文等）。
        只隐藏确认不可输入的场景，其余类型与查询失败一律不隐藏。"""
        try:
            focus = auto.GetFocusedControl()
            if not focus: return False
            if focus.ControlType != auto.ControlType.DocumentControl: return False
            vp = focus.GetValuePattern()
            if vp is None: return True
            return bool(vp.IsReadOnly)
        except Exception:
            return False

    def get_caret_pos(self):
        """核心：多级检测光标位置"""
        try:
            # 第一级：原生 Win32 (支持记事本)
            pos = self._get_pos_via_gui_info()
            if pos: return pos

            # 第二级：MSAA (支持浏览器/VS Code)
            pos = self._get_pos_via_msaa()
            if pos: return pos
        except Exception:
            pass
        return None

    def _get_pos_via_gui_info(self):
        gui_info = GUITHREADINFO()
        gui_info.cbSize = sizeof(GUITHREADINFO)
        if user32.GetGUIThreadInfo(0, byref(gui_info)):
            if gui_info.hwndCaret:
                pt = wintypes.POINT(gui_info.rcCaret.left, gui_info.rcCaret.top)
                user32.ClientToScreen(gui_info.hwndCaret, byref(pt))
                h = gui_info.rcCaret.bottom - gui_info.rcCaret.top
                return pt.x, pt.y, h
        return None

    def _get_pos_via_msaa(self):
        hwnd = user32.GetForegroundWindow()
        if not hwnd: return None
        p_acc = ctypes.c_void_p()
        try:
            res = oleacc.AccessibleObjectFromWindow(hwnd, OBJID_CARET, byref(self._guid_iaaccessible), byref(p_acc))
            if res == 0 and p_acc:
                vtable_ptr = ctypes.cast(p_acc, POINTER(POINTER(ctypes.c_void_p))).contents
                accLocation_func = ctypes.WINFUNCTYPE(ctypes.HRESULT, ctypes.c_void_p, POINTER(wintypes.LONG), POINTER(wintypes.LONG), POINTER(wintypes.LONG), POINTER(wintypes.LONG), VARIANT)(vtable_ptr[22])
                x, y, w, h = wintypes.LONG(), wintypes.LONG(), wintypes.LONG(), wintypes.LONG()
                var_child = VARIANT(vt=3, lVal=0) # CHILDID_SELF
                if accLocation_func(p_acc, byref(x), byref(y), byref(w), byref(h), var_child) == 0:
                    release_func = ctypes.WINFUNCTYPE(wintypes.ULONG, ctypes.c_void_p)(vtable_ptr[2])
                    release_func(p_acc)
                    if x.value != 0 or y.value != 0: return x.value, y.value, h.value
                release_func = ctypes.WINFUNCTYPE(wintypes.ULONG, ctypes.c_void_p)(vtable_ptr[2])
                release_func(p_acc)
        except Exception: pass
        
        gui_info = GUITHREADINFO()
        gui_info.cbSize = sizeof(GUITHREADINFO)
        if user32.GetGUIThreadInfo(0, byref(gui_info)):
            target_hwnd = gui_info.hwndCaret or gui_info.hwndFocus or gui_info.hwndActive
            if target_hwnd and (gui_info.rcCaret.left != 0 or gui_info.rcCaret.top != 0):
                pt = wintypes.POINT(gui_info.rcCaret.left, gui_info.rcCaret.top)
                user32.ClientToScreen(target_hwnd, byref(pt))
                if pt.x > -1000 and pt.y > -1000: 
                    return pt.x, pt.y, (gui_info.rcCaret.bottom - gui_info.rcCaret.top)
        return None
