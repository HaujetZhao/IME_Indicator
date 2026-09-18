//! 文本光标位置检测模块 - 多级检测策略


use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextPattern2, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::Win32::UI::Input::Ime::{
    CFS_POINT, COMPOSITIONFORM, ImmGetCompositionWindow, ImmGetContext, ImmReleaseContext,
};
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

/// Chromium 把空输入框表示为单个 U+FFFC 对象替换字符，此时选区/插入单元的
/// 边界矩形等于整个元素矩形，不是光标位置（实测：空字段时二者完全相等，
/// 有字符时为零宽竖线）。拒收这种假矩形，让检测落到下一级。
fn rect_covers_element(focused: &IUIAutomationElement, l: f64, t: f64, w: f64, h: f64) -> bool {
    const TOL: f64 = 2.0;
    match unsafe { focused.CurrentBoundingRectangle() } {
        Ok(r) => {
            (l - r.left as f64).abs() <= TOL
                && (t - r.top as f64).abs() <= TOL
                && (l + w - r.right as f64).abs() <= TOL
                && (t + h - r.bottom as f64).abs() <= TOL
        }
        Err(_) => false,
    }
}

/// 取文档起点（offset 0）折叠后的矩形。Chromium 地址栏（OmniboxViewViews）的
/// 像素映射残缺：任意 offset 的折叠 range 都返回这个矩形（实测 offset 0..N 全等），
/// 配合 caret offset 做矛盾检测。
fn doc_start_rect(text_pattern: &IUIAutomationTextPattern) -> Option<(f64, f64)> {
    unsafe {
        let doc = text_pattern.DocumentRange().ok()?;
        let probe = doc.Clone().ok()?;
        probe
            .MoveEndpointByRange(TextPatternRangeEndpoint_End, &probe, TextPatternRangeEndpoint_Start)
            .ok()?;
        let rects = probe.GetBoundingRectangles().ok()?;
        if rects.is_null() {
            return None;
        }
        let lower = SafeArrayGetLBound(&*rects, 1).ok()?;
        let upper = SafeArrayGetUBound(&*rects, 1).ok()?;
        if upper - lower + 1 < 2 {
            return None;
        }
        let mut p = std::ptr::null_mut();
        if SafeArrayAccessData(&*rects, &mut p).is_err() {
            return None;
        }
        let d = std::slice::from_raw_parts(p as *const f64, 2);
        let pt = (d[0], d[1]);
        let _ = SafeArrayUnaccessData(&*rects);
        Some(pt)
    }
}

/// 检测来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionSource {
    GuiInfo,
    UiAutomation,
    UiaCaretRange,
    Ime,
    MsaaFallback,
    None,
}

impl DetectionSource {
    fn name(&self) -> &'static str {
        match self {
            DetectionSource::GuiInfo => "GuiInfo",
            DetectionSource::UiAutomation => "UiAutomation",
            DetectionSource::UiaCaretRange => "UiaCaretRange",
            DetectionSource::Ime => "Ime",
            DetectionSource::MsaaFallback => "MsaaFallback",
            DetectionSource::None => "None",
        }
    }

    /// 从配置名解析（配置里用 snake_case）
    fn from_name(s: &str) -> Option<Self> {
        match s {
            "gui_info" => Some(DetectionSource::GuiInfo),
            "uia_caret_range" => Some(DetectionSource::UiaCaretRange),
            "uia_selection" => Some(DetectionSource::UiAutomation),
            "ime" => Some(DetectionSource::Ime),
            "msaa" => Some(DetectionSource::MsaaFallback),
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
    /// 上一次写入日志的来源，用于变化检测
    logged_source: Option<DetectionSource>,
}

/// 追加一行来源变化日志到 exe 同目录 caret_source.log
fn log_source_change(source: DetectionSource, pos: Option<CaretPos>, err: &str) {
    use std::io::Write;
    use windows::Win32::System::SystemInformation::GetLocalTime;

    let t = unsafe { GetLocalTime() };
    let pos_str = match pos {
        Some((x, y, h)) => format!("({x},{y},h{h})"),
        None => {
            if err.is_empty() {
                String::new()
            } else {
                format!(" err={err}")
            }
        }
    };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {} {}\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds,
        source.name(), pos_str
    );
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("caret_source.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
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

        // 会话分隔行 + 生效的方法管线，便于区分多次运行与对照测试
        {
            use std::io::Write;
            let path = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("caret_source.log");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "---- session start ----");
                let _ = writeln!(f, "methods: {}", crate::config::caret_methods().join(" -> "));
            }
        }

        Self {
            automation,
            last_source: DetectionSource::None,
            last_uia_error: String::new(),
            logged_source: None,
        }
    }

    /// 核心：多级检测光标位置，来源变化时写日志
    pub fn get_caret_pos(&mut self) -> Option<CaretPos> {
        let pos = self.detect();
        if self.logged_source != Some(self.last_source) {
            log_source_change(self.last_source, pos, &self.last_uia_error);
            self.logged_source = Some(self.last_source);
        }
        pos
    }

    /// 多级检测：按配置的 methods 顺序依次尝试
    fn detect(&mut self) -> Option<CaretPos> {
        for name in crate::config::caret_methods() {
            let Some(method) = DetectionSource::from_name(name) else { continue };
            let pos = match method {
                DetectionSource::GuiInfo => self.get_pos_via_gui_info(),
                DetectionSource::UiaCaretRange => self.get_pos_via_uia_caret_range(),
                DetectionSource::UiAutomation => self.get_pos_via_uia_selection(),
                DetectionSource::Ime => self.get_pos_via_ime(),
                DetectionSource::MsaaFallback => self.get_pos_via_msaa_fallback(),
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

    /// 通过 UI Automation TextPattern2 GetCaretRange 获取光标位置 (支持 VS Code)
    fn get_pos_via_uia_caret_range(&mut self) -> Option<CaretPos> {
        use windows::Win32::UI::Accessibility::TextUnit_Character;
        
        let automation = self.automation.as_ref()?;
        
        // 清空错误信息（这是第一个 UIA 方法）
        self.last_uia_error.clear();

        unsafe {
            // 获取焦点元素
            let focused = match automation.GetFocusedElement() {
                Ok(f) => f,
                Err(e) => {
                    self.last_uia_error = format!("Car:Focus:{:X}", e.code().0 as u32);
                    return None;
                }
            };

            // 尝试获取 TextPattern2 (更新版本，支持 GetCaretRange)
            let pattern_obj = match focused.GetCurrentPattern(UIA_TextPattern2Id) {
                Ok(p) => p,
                Err(e) => {
                    self.last_uia_error = format!("Car:Pat2:{:X}", e.code().0 as u32);
                    return None;
                }
            };
            
            let text_pattern2: IUIAutomationTextPattern2 = match pattern_obj.cast() {
                Ok(t) => t,
                Err(e) => {
                    self.last_uia_error = format!("Car:Cast:{:X}", e.code().0 as u32);
                    return None;
                }
            };

            // 获取光标范围
            let mut is_active = windows::Win32::Foundation::BOOL::default();
            let caret_range = match text_pattern2.GetCaretRange(&mut is_active) {
                Ok(r) => r,
                Err(e) => {
                    self.last_uia_error = format!("Car:Range:{:X}", e.code().0 as u32);
                    return None;
                }
            };

            // 先尝试直接获取边界矩形
            let rects = match caret_range.GetBoundingRectangles() {
                Ok(r) => r,
                Err(e) => {
                    self.last_uia_error = format!("Car:Rect:{:X}", e.code().0 as u32);
                    return None;
                }
            };

            if rects.is_null() {
                self.last_uia_error = "Car:Null".to_string();
                return None;
            }

            // 使用 SafeArray API 访问数据
            let lower = SafeArrayGetLBound(&*rects, 1).ok()?;
            let upper = SafeArrayGetUBound(&*rects, 1).ok()?;
            let elem_count = (upper - lower + 1) as usize;

            // 如果为空，尝试扩展范围后再获取
            if elem_count < 4 {
                // 扩展到字符单元
                if caret_range.ExpandToEnclosingUnit(TextUnit_Character).is_ok() {
                    // 再次尝试获取边界矩形
                    if let Ok(rects2) = caret_range.GetBoundingRectangles() {
                        if !rects2.is_null() {
                            let lower2 = SafeArrayGetLBound(&*rects2, 1).ok()?;
                            let upper2 = SafeArrayGetUBound(&*rects2, 1).ok()?;
                            let elem_count2 = (upper2 - lower2 + 1) as usize;
                            
                            if elem_count2 >= 4 {
                                let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                                if SafeArrayAccessData(&*rects2, &mut data_ptr).is_ok() {
                                    let doubles = std::slice::from_raw_parts(data_ptr as *const f64, elem_count2);
                                    let _ = SafeArrayUnaccessData(&*rects2);
                                    if rect_covers_element(&focused, doubles[0], doubles[1], doubles[2], doubles[3]) {
                                        self.last_uia_error = "Car:EmptyField".to_string();
                                        return None;
                                    }
                                    return Some((doubles[0] as i32, doubles[1] as i32, doubles[3] as i32));
                                }
                            }
                        }
                    }
                }
                
                self.last_uia_error = format!("Car:Cnt:{}", elem_count);
                return None;
            }

            // 访问数据
            let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
            if SafeArrayAccessData(&*rects, &mut data_ptr).is_err() {
                self.last_uia_error = "Car:Access".to_string();
                return None;
            }

            let doubles = std::slice::from_raw_parts(data_ptr as *const f64, elem_count);
            let left = doubles[0] as i32;
            let top = doubles[1] as i32;
            let height = doubles[3] as i32;

            let _ = SafeArrayUnaccessData(&*rects);

            return Some((left, top, height));
        }
    }

    /// 通过 UI Automation TextPattern GetSelection 获取光标位置 (支持 Chrome)
    fn get_pos_via_uia_selection(&mut self) -> Option<CaretPos> {
        let automation = self.automation.as_ref()?;
        
        // 追加错误信息的辅助闭包
        let append_error = |s: &mut String, new: String| {
            if !s.is_empty() {
                s.push_str(" | ");
            }
            s.push_str(&new);
        };

        unsafe {
            // 获取焦点元素
            let focused = match automation.GetFocusedElement() {
                Ok(f) => f,
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Focus:{:X}", e.code().0 as u32));
                    return None;
                }
            };

            // 尝试获取 TextPattern
            let pattern_obj = match focused.GetCurrentPattern(UIA_TextPatternId) {
                Ok(p) => p,
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Pat:{:X}", e.code().0 as u32));
                    return None;
                }
            };
            
            let text_pattern: IUIAutomationTextPattern = match pattern_obj.cast() {
                Ok(t) => t,
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Cast:{:X}", e.code().0 as u32));
                    return None;
                }
            };

            // 获取选区
            let selection = match text_pattern.GetSelection() {
                Ok(s) => s,
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Sel:{:X}", e.code().0 as u32));
                    return None;
                }
            };
            
            let count = selection.Length().unwrap_or(0);
            if count == 0 {
                append_error(&mut self.last_uia_error, "Sel:NoSel".to_string());
                return None;
            }

            // 获取第一个选区范围
            let range = match selection.GetElement(0) {
                Ok(r) => r,
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Range:{:X}", e.code().0 as u32));
                    return None;
                }
            };

            // 非零宽选区（如 Ctrl+L 全选地址栏、拖选文本）的边界矩形是选中内容的
            // 整体矩形，不是光标位置；折叠到选区起点（打字替换发生的位置）再取竖线
            match range.GetText(1) {
                Ok(t) if !t.is_empty() => {
                    if let Err(e) = range.MoveEndpointByRange(
                        TextPatternRangeEndpoint_End,
                        &range,
                        TextPatternRangeEndpoint_Start,
                    ) {
                        append_error(&mut self.last_uia_error, format!("Sel:Collapse:{:X}", e.code().0 as u32));
                        return None;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Text:{:X}", e.code().0 as u32));
                    return None;
                }
            }

            // 获取边界矩形
            let rects = match range.GetBoundingRectangles() {
                Ok(r) => r,
                Err(e) => {
                    append_error(&mut self.last_uia_error, format!("Sel:Rect:{:X}", e.code().0 as u32));
                    return None;
                }
            };

            if rects.is_null() {
                append_error(&mut self.last_uia_error, "Sel:Null".to_string());
                return None;
            }

            // 使用 SafeArray API 访问数据
            let lower = SafeArrayGetLBound(&*rects, 1).ok()?;
            let upper = SafeArrayGetUBound(&*rects, 1).ok()?;
            let elem_count = (upper - lower + 1) as usize;

            if elem_count < 4 {
                append_error(&mut self.last_uia_error, format!("Sel:Cnt:{}", elem_count));
                return None;
            }

            // 访问数据
            let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
            if SafeArrayAccessData(&*rects, &mut data_ptr).is_err() {
                append_error(&mut self.last_uia_error, "Sel:Access".to_string());
                return None;
            }

            let doubles = std::slice::from_raw_parts(data_ptr as *const f64, elem_count);
            // caret offset（文本模型，地址栏也正确）；只有像素映射是坏的
            let caret_offset = text_pattern
                .DocumentRange()
                .ok()
                .and_then(|doc| {
                    range.CompareEndpoints(TextPatternRangeEndpoint_Start, &doc, TextPatternRangeEndpoint_Start).ok()
                })
                .unwrap_or(0);
            let result = if rect_covers_element(&focused, doubles[0], doubles[1], doubles[2], doubles[3]) {
                append_error(&mut self.last_uia_error, "Sel:EmptyField".to_string());
                None
            } else if caret_offset > 0 {
                match doc_start_rect(&text_pattern) {
                    // offset>0 却落在文本起点：像素映射矛盾（Chromium 地址栏），拒收
                    Some((sl, st)) if (doubles[0] - sl).abs() <= 2.0 && (doubles[1] - st).abs() <= 2.0 => {
                        append_error(&mut self.last_uia_error, "Sel:BrokenPixelMap".to_string());
                        None
                    }
                    _ => Some((doubles[0] as i32, doubles[1] as i32, doubles[3] as i32)),
                }
            } else {
                Some((doubles[0] as i32, doubles[1] as i32, doubles[3] as i32))
            };

            let _ = SafeArrayUnaccessData(&*rects);

            result
        }
    }

    /// 通过 IME 组合窗口获取光标位置
    fn get_pos_via_ime(&self) -> Option<CaretPos> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }

            let h_imc = ImmGetContext(hwnd);
            if h_imc.0.is_null() {
                return None;
            }

            let mut comp_form = COMPOSITIONFORM::default();
            let mut pos = None;

            if ImmGetCompositionWindow(h_imc, &mut comp_form).as_bool() {
                if (comp_form.dwStyle & CFS_POINT) != 0 {
                    let mut pt = POINT {
                        x: comp_form.ptCurrentPos.x,
                        y: comp_form.ptCurrentPos.y,
                    };
                    let _ = ClientToScreen(hwnd, &mut pt);
                    pos = Some((pt.x, pt.y, 20));
                }
            }

            let _ = ImmReleaseContext(hwnd, h_imc);
            pos
        }
    }

    /// MSAA：使用 AccessibleObjectFromWindow(OBJID_CARET) + IAccessible::accLocation
    fn get_pos_via_msaa_fallback(&mut self) -> Option<CaretPos> {
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
                // 继续尝试 GUITHREADINFO 回退
            } else if p_acc.is_none() {
                append_error(&mut self.last_uia_error, "MSAA:NoAcc");
                // 继续尝试 GUITHREADINFO 回退
            } else if let Some(acc) = p_acc {
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
                            return Some((x, y, h));
                        } else {
                            append_error(&mut self.last_uia_error, "MSAA:Zero");
                        }
                    }
                    Err(e) => {
                        append_error(&mut self.last_uia_error, &format!("MSAA:Loc:{:X}", e.code().0 as u32));
                    }
                }
            }

            // 回退到 GUITHREADINFO
            let mut gui_info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };

            if GetGUIThreadInfo(0, &mut gui_info).is_ok() {
                let target_hwnd = if !gui_info.hwndCaret.0.is_null() {
                    gui_info.hwndCaret
                } else if !gui_info.hwndFocus.0.is_null() {
                    gui_info.hwndFocus
                } else if !gui_info.hwndActive.0.is_null() {
                    gui_info.hwndActive
                } else {
                    return None;
                };

                if gui_info.rcCaret.left != 0 || gui_info.rcCaret.top != 0 {
                    let mut pt = POINT {
                        x: gui_info.rcCaret.left,
                        y: gui_info.rcCaret.top,
                    };
                    let _ = ClientToScreen(target_hwnd, &mut pt);
                    if pt.x > -1000 && pt.y > -1000 {
                        let h = gui_info.rcCaret.bottom - gui_info.rcCaret.top;
                        return Some((pt.x, pt.y, h));
                    }
                }
            }
        }
        None
    }
}

impl Default for CaretDetector {
    fn default() -> Self {
        Self::new()
    }
}
