//! 全屏应用检测模块
//!
//! 用于检测是否有应用程序处于全屏状态（如视频播放、游戏等），
//! 在全屏时自动隐藏 IME 指示器，避免遮挡视频内容。

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN,
};

/// 检测是否有应用程序处于全屏状态
///
/// 判断逻辑：
/// 1. 获取当前活动窗口
/// 2. 检查窗口是否覆盖整个屏幕
/// 3. 排除桌面窗口（Progman）
///
/// # Returns
/// - `true` - 检测到全屏应用
/// - `false` - 无全屏应用
pub fn is_fullscreen_app() -> bool {
    unsafe {
        // 获取当前活动窗口
        let hwnd = GetForegroundWindow();

        if hwnd.is_invalid() {
            return false;
        }

        // 获取窗口矩形
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }

        // 获取屏幕尺寸
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);

        // 检查窗口是否覆盖整个屏幕
        // 允许小幅偏差（某些播放器可能会有几像素的边框）
        let tolerance = 2;
        let covers_screen = (rect.left <= tolerance)
            && (rect.top <= tolerance)
            && (rect.right >= screen_width - tolerance)
            && (rect.bottom >= screen_height - tolerance);

        if !covers_screen {
            return false;
        }

        // 排除桌面窗口
        // 获取窗口类名来判断是否是桌面
        let mut class_name = [0u16; 256];
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut class_name);

        let class_name_str = String::from_utf16_lossy(
            &class_name[..class_name.iter().position(|&c| c == 0).unwrap_or(0)],
        );

        // 排除桌面和某些系统窗口
        if class_name_str.contains("Progman") || class_name_str.contains("WorkerW") {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fullscreen_detection() {
        // 基本的功能测试
        let result = is_fullscreen_app();
        println!("Fullscreen status: {}", result);
    }
}
