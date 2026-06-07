//! IME 状态检测模块 - 检测中英文输入模式

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, GetKeyboardLayout};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    SendMessageTimeoutW, GUITHREADINFO, SMTO_ABORTIFHUNG,
};
use windows::Win32::System::Console::{AllocConsole, SetConsoleTitleW};
use windows::core::w;

/// IME 控制消息
const WM_IME_CONTROL: u32 = 0x283;
const IMC_GETOPENSTATUS: usize = 0x5;
const IMC_GETCONVERSIONMODE: usize = 0x1;
const IME_CMODE_NATIVE: u32 = 0x0001;

/// 键盘布局 ID 常量
const LAYOUT_ZH_CN: u32 = 0x0804;  // 中文 (中国)
const LAYOUT_ZH_TW: u32 = 0x0404;  // 中文 (台湾)

/// 调试状态结构体
#[derive(Clone, PartialEq)]
struct DebugState {
    ime_hwnd: isize,
    open_status: usize,
    conversion_mode: usize,
    has_native: bool,
    layout_id: u32,
    caps_lock: bool,
}

impl Default for DebugState {
    fn default() -> Self {
        Self {
            ime_hwnd: 0,
            open_status: 0,
            conversion_mode: 0,
            has_native: false,
            layout_id: 0,
            caps_lock: false,
        }
    }
}

/// 全局调试状态
static mut LAST_STATE: Option<DebugState> = None;
static mut DEBUG_LINE_NUM: u32 = 0;
static mut CONSOLE_INITIALIZED: bool = false;

/// 初始化调试控制台
fn init_debug_console() {
    unsafe {
        if !CONSOLE_INITIALIZED && crate::config::debug_console() {
            let _ = AllocConsole();
            let _ = SetConsoleTitleW(w!("IME Indicator Debug Console"));
            CONSOLE_INITIALIZED = true;
        }
    }
}

/// 打印调试状态
fn print_debug_state(state: &DebugState, result: bool) {
    unsafe {
        DEBUG_LINE_NUM += 1;

        let layout_str = match state.layout_id {
            0x0409 => "0x0409(EN)",
            0x0804 => "0x0804(ZH)",
            0x0404 => "0x0404(TW)",
            _ => &format!("0x{:04x}(?)", state.layout_id)[..],
        };

        println!(
            "[{:03}] hwnd:{:#010x} open:{} conv:{} NATIVE:{} layout:{} caps:{} => {}",
            DEBUG_LINE_NUM,
            state.ime_hwnd,
            state.open_status,
            state.conversion_mode,
            state.has_native,
            layout_str,
            state.caps_lock,
            if result { "CN" } else { "EN" }
        );
    }
}

/// 获取当前焦点窗口
fn get_focused_window() -> HWND {
    unsafe {
        let fore_hwnd = GetForegroundWindow();
        if fore_hwnd.0.is_null() {
            return HWND::default();
        }

        let thread_id = GetWindowThreadProcessId(fore_hwnd, None);
        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };

        if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok() {
            if !gui_info.hwndFocus.0.is_null() {
                return gui_info.hwndFocus;
            }
            if !gui_info.hwndActive.0.is_null() {
                return gui_info.hwndActive;
            }
        }

        fore_hwnd
    }
}

/// 获取键盘布局 ID
fn get_keyboard_layout_id() -> u32 {
    unsafe {
        let hwnd = get_focused_window();
        let thread_id = GetWindowThreadProcessId(hwnd, None);
        let hkl = GetKeyboardLayout(thread_id);
        (hkl.0 as usize & 0xFFFF) as u32
    }
}

/// 获取 IME 默认窗口句柄
fn get_ime_window(hwnd: HWND) -> HWND {
    unsafe { ImmGetDefaultIMEWnd(hwnd) }
}

/// 带超时的消息发送
fn send_message_timeout(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> Option<usize> {
    unsafe {
        let mut result: usize = 0;
        let ret = SendMessageTimeoutW(
            hwnd,
            msg,
            windows::Win32::Foundation::WPARAM(wparam),
            windows::Win32::Foundation::LPARAM(lparam),
            SMTO_ABORTIFHUNG,
            500,
            Some(&mut result),
        );
        if ret.0 != 0 {
            Some(result)
        } else {
            None
        }
    }
}

/// 检测是否为中文输入模式
pub fn is_chinese_mode() -> bool {
    init_debug_console();

    let hwnd = get_focused_window();
    let ime_hwnd = get_ime_window(hwnd);

    // 收集调试状态
    let caps_lock = unsafe { (GetKeyState(0x14) & 0x0001) != 0 };
    let mut state = DebugState {
        ime_hwnd: ime_hwnd.0 as isize,
        caps_lock,
        ..Default::default()
    };

    let mut has_native = false;
    let mut open_status = 0;

    // 检查 IME 状态
    if !ime_hwnd.0.is_null() {
        if let Some(conversion_mode) = send_message_timeout(ime_hwnd, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0) {
            state.conversion_mode = conversion_mode;
            has_native = (conversion_mode as u32 & IME_CMODE_NATIVE) != 0;
            state.has_native = has_native;
        }

        if let Some(open) = send_message_timeout(ime_hwnd, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0) {
            open_status = open;
            state.open_status = open;
        }
    }

    let layout_id = get_keyboard_layout_id();
    state.layout_id = layout_id;

    // 检测逻辑
    let result = if has_native {
        layout_id == LAYOUT_ZH_CN || layout_id == LAYOUT_ZH_TW
    } else if open_status == 1 {
        layout_id == LAYOUT_ZH_CN || layout_id == LAYOUT_ZH_TW
    } else {
        false
    };

    // 调试输出
    unsafe {
        let should_print = match &LAST_STATE {
            None => true,
            Some(last) => state != *last,
        };

        if should_print {
            print_debug_state(&state, result);
            LAST_STATE = Some(state.clone());
        }
    }

    result
}

/// 检测 Caps Lock 是否开启
pub fn is_caps_lock_on() -> bool {
    unsafe {
        let state = GetKeyState(0x14);
        (state & 0x0001) != 0
    }
}
