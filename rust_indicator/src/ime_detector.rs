//! IME 状态检测模块 - 检测中英文输入模式

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    SendMessageTimeoutW, GUITHREADINFO, SMTO_ABORTIFHUNG,
};

/// IME 控制消息
const WM_IME_CONTROL: u32 = 0x283;
const IMC_GETOPENSTATUS: usize = 0x5;
const IMC_GETCONVERSIONMODE: usize = 0x1;
const IME_CMODE_NATIVE: u32 = 0x0001;

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
    let hwnd = get_focused_window();
    let ime_hwnd = get_ime_window(hwnd);

    if ime_hwnd.0.is_null() {
        return false;
    }

    // 获取 IME 开启状态
    let open_status = send_message_timeout(ime_hwnd, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0);
    if open_status.unwrap_or(0) == 0 {
        return false;
    }

    // 获取转换模式并检测是否包含 NATIVE 标志 (中文)
    if let Some(conversion_mode) = send_message_timeout(ime_hwnd, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0) {
        return (conversion_mode as u32 & IME_CMODE_NATIVE) != 0;
    }

    false
}

/// 检测 Caps Lock 是否开启
pub fn is_caps_lock_on() -> bool {
    unsafe {
        // VK_CAPITAL = 0x14
        // GetKeyState 返回值的低字节为 1 表示开启
        let state = GetKeyState(0x14);
        (state & 0x0001) != 0
    }
}
