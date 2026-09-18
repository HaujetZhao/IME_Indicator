//! 文本光标位置检测模块 - 多级检测策略


use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::CUIAutomation;
use windows::Win32::UI::Accessibility::IUIAutomation;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GUITHREADINFO,
};
use windows::core::Interface;

// ============================================================================
// 常量定义
// ============================================================================

/// MSAA OBJID_CARET 常量
const OBJID_CARET: u32 = 0xFFFFFFF8u32;

/// IID_IAccessible GUID: {618736e0-3c3d-11cf-810c-00aa00389b71}
const IID_IACCESSIBLE: u128 = 0x618736e0_3c3d_11cf_810c_00aa00389b71;

// ============================================================================
// 类型定义
// ============================================================================

/// 光标位置信息 (x, y, height)
pub type CaretPos = (i32, i32, i32);

/// 检测来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionSource {
    GuiInfo,
    MsaaCaret,
    None,
}

impl DetectionSource {
    /// 从配置名解析（配置里用 snake_case）
    fn from_name(s: &str) -> Option<Self> {
        match s {
            "gui_info" => Some(DetectionSource::GuiInfo),
            "msaa_caret" => Some(DetectionSource::MsaaCaret),
            _ => None,
        }
    }
}

// ============================================================================
// CaretDetector 实现
// ============================================================================

/// 文本光标检测器
pub struct CaretDetector {
    automation: Option<IUIAutomation>,
    pub last_source: DetectionSource,
    pub last_uia_error: String,
}

impl CaretDetector {
    /// 创建新的检测器
    pub fn new() -> Self {
        // 初始化 COM 和 UI Automation
        let automation = unsafe {
            // 初始化 COM (忽略错误，可能已经初始化)
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            // 创建 UI Automation 实例
            CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL).ok()
        };

        Self {
            automation,
            last_source: DetectionSource::None,
            last_uia_error: String::new(),
        }
    }

    /// 可见性线（黑名单制）：只在确认焦点位于"不可输入的位置"时返回 true。
    /// 目前已知唯一误显示来源是浏览器网页正文——焦点元素是只读 Document
    /// （无 ValuePattern 或只读）；Word/contenteditable 是非只读 Document，正常显示。
    /// 其余类型（Edit、按钮、终端……）与任何查询失败都按不隐藏处理，默认显示。
    pub fn focus_is_readonly_document(&self) -> bool {
        use windows::Win32::UI::Accessibility::{
            IUIAutomationValuePattern, UIA_DocumentControlTypeId, UIA_ValuePatternId,
        };
        let Some(automation) = self.automation.as_ref() else {
            return false;
        };
        let Ok(focused) = (unsafe { automation.GetFocusedElement() }) else {
            return false;
        };
        let is_document = (unsafe { focused.CurrentControlType() })
            .map_or(false, |t| t == UIA_DocumentControlTypeId);
        if !is_document {
            return false;
        }
        let value_pattern = unsafe { focused.GetCurrentPattern(UIA_ValuePatternId) }
            .ok()
            .and_then(|p| p.cast::<IUIAutomationValuePattern>().ok());
        match value_pattern {
            Some(vp) => matches!(unsafe { vp.CurrentIsReadOnly() }, Ok(ro) if ro.as_bool()),
            None => true,
        }
    }

    /// 核心：按配置管线检测光标位置
    pub fn get_caret_pos(&mut self) -> Option<CaretPos> {
        self.detect()
    }

    /// 多级检测：按配置的 methods 顺序依次尝试
    fn detect(&mut self) -> Option<CaretPos> {
        for name in crate::config::caret_methods() {
            let Some(method) = DetectionSource::from_name(name) else { continue };
            let pos = match method {
                DetectionSource::GuiInfo => self.get_pos_via_gui_info(),
                DetectionSource::MsaaCaret => self.get_pos_via_msaa_caret(),
                DetectionSource::None => None,
            };
            if let Some(pos) = pos {
                self.last_source = method;
                return Some(pos);
            }
        }

        self.last_source = DetectionSource::None;
        None
    }

    /// 通过 GetGUIThreadInfo 获取光标位置
    fn get_pos_via_gui_info(&self) -> Option<CaretPos> {
        unsafe {
            let mut gui_info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };

            if GetGUIThreadInfo(0, &mut gui_info).is_ok() {
                if !gui_info.hwndCaret.0.is_null() {
                    let mut pt = POINT {
                        x: gui_info.rcCaret.left,
                        y: gui_info.rcCaret.top,
                    };
                    let _ = ClientToScreen(gui_info.hwndCaret, &mut pt);
                    let h = gui_info.rcCaret.bottom - gui_info.rcCaret.top;
                    return Some((pt.x, pt.y, h));
                }
            }
        }
        None
    }

    /// 通过 MSAA OBJID_CARET 获取光标位置（VS Code 支持，浏览器不提供此对象）
    fn get_pos_via_msaa_caret(&mut self) -> Option<CaretPos> {
        use windows::Win32::UI::Accessibility::{AccessibleObjectFromWindow, IAccessible};
        use windows::core::GUID;
        use windows::core::VARIANT;

        // 追加错误信息
        let append_error = |s: &mut String, new: &str| {
            if !s.is_empty() {
                s.push_str(" | ");
            }
            s.push_str(new);
        };

        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                append_error(&mut self.last_uia_error, "MSAA:NoHwnd");
                return None;
            }

            // 使用模块级常量 IID_IACCESSIBLE
            let iid_iaccessible = GUID::from_u128(IID_IACCESSIBLE);

            // 尝试获取 OBJID_CARET 的 IAccessible 接口
            let mut p_acc: Option<IAccessible> = None;
            let result = AccessibleObjectFromWindow(
                hwnd,
                OBJID_CARET,
                &iid_iaccessible,
                &mut p_acc as *mut _ as *mut *mut std::ffi::c_void,
            );

            if result.is_err() {
                append_error(&mut self.last_uia_error, &format!("MSAA:Err:{:X}", result.unwrap_err().code().0 as u32));
                return None;
            } else if p_acc.is_none() {
                append_error(&mut self.last_uia_error, "MSAA:NoAcc");
                return None;
            }

            let acc = p_acc.unwrap();
            // 调用 accLocation 获取位置
            let mut x: i32 = 0;
            let mut y: i32 = 0;
            let mut w: i32 = 0;
            let mut h: i32 = 0;

            // CHILDID_SELF = VARIANT with VT_I4 value 0
            // 使用 from(0i32) 创建 VT_I4 类型的 VARIANT
            let var_child = VARIANT::from(0i32);

            match acc.accLocation(&mut x, &mut y, &mut w, &mut h, &var_child) {
                Ok(_) => {
                    if x != 0 || y != 0 {
                        // 有选区时 caret 对象矩形覆盖整个选区（光标在选区末尾），取右缘
                        return Some((x + w, y, h));
                    } else {
                        append_error(&mut self.last_uia_error, "MSAA:Zero");
                        None
                    }
                }
                Err(e) => {
                    append_error(&mut self.last_uia_error, &format!("MSAA:Loc:{:X}", e.code().0 as u32));
                    None
                }
            }
        }
    }
}

impl Default for CaretDetector {
    fn default() -> Self {
        Self::new()
    }
}
