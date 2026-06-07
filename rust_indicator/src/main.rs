#![windows_subsystem = "windows"]

mod caret_detector;
mod config;
mod cursor_detector;
mod fullscreen_detector;
mod ime_detector;
mod overlay;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, LoadIconW, IDI_APPLICATION};

use caret_detector::CaretDetector;
use cursor_detector::CursorDetector;
use fullscreen_detector::is_fullscreen_app;
use ime_detector::{is_caps_lock_on, is_chinese_mode};
use overlay::IndicatorOverlay;
use tray::TrayManager;

fn main() {
    // 初始化 GDI+ (IndicatorOverlay 内部会处理，但这里为了可能需要的图标加载，我们显示初始化)
    // 实际上 IndicatorOverlay::new 内部会调用 GdiplusStartup
    
    // 配置高 DPI 感知
    set_dpi_awareness();

    // 设置运行状态
    let running = Arc::new(AtomicBool::new(true));
    let r_clone = running.clone();

    // 在子线程启动检测逻辑
    thread::spawn(move || {
        run_detector_loop(r_clone);
    });

    // 主线程：根据配置决定是否创建系统托盘
    unsafe {
        // 先手动初始化一下 GDI+，因为托盘图标加载可能需要它
        let mut token = 0;
        let input = windows::Win32::Graphics::GdiPlus::GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        let _ = windows::Win32::Graphics::GdiPlus::GdiplusStartup(&mut token, &input, std::ptr::null_mut());

        if config::tray_enable() {
            // 尝试加载图标
            let h_instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            // 这里的 1 对应 resource.rc 中的 ID
            let mut icon = LoadIconW(h_instance, windows::core::PCWSTR(1 as _)).unwrap_or_else(|_| {
                LoadIconW(None, IDI_APPLICATION).unwrap()
            });
            
            // 1. 尝试从可执行文件同目录加载外部 icon.png (允许用户自定义)
            let mut loaded = false;
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(dir) = exe_path.parent() {
                    let external_icon = dir.join("icon.png");
                    if let Some(h) = TrayManager::load_icon_from_file(&external_icon) {
                        icon = h;
                        loaded = true;
                    }
                }
            }

            // 2. 如果没找到外部图标，使用内嵌在 EXE 资源中的 ICO (ID 为 1)
            if !loaded {
                // 我们在 main 顶部已经尝试加载了基于资源的 icon，此处保持逻辑一致。
                // 实际上由于 main 已经做好了，这里直接传进去即可。
            }

            let tray = TrayManager::new(icon);
            
            // 这将阻塞直到用户退出（主窗口收到 WM_QUIT）
            tray.run_message_loop();
            
            // 退出后清理
            running.store(false, Ordering::SeqCst);
            tray.destroy();
        } else {
            // 不创建托盘，主线程进入等待循环
            // 用户只能通过任务管理器结束进程
            while running.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        
        windows::Win32::Graphics::GdiPlus::GdiplusShutdown(token);
    }
}

fn run_detector_loop(running: Arc<AtomicBool>) {
    // 初始化检测器
    let mut caret_detector = CaretDetector::new();
    let cursor_detector = CursorDetector::new(config::mouse_target_cursors());

    // 初始化悬浮窗
    let caret_overlay = if config::caret_enable() {
        Some(IndicatorOverlay::new(
            "Caret",
            config::caret_size(),
            config::caret_color_cn(),
            config::caret_color_en(),
            config::caret_color_en_upper(),
            config::caret_offset_x(),
            config::caret_offset_y(),
        ))
    } else {
        None
    };

    let mouse_overlay = if config::mouse_enable() {
        Some(IndicatorOverlay::new(
            "Mouse",
            config::mouse_size(),
            config::mouse_color_cn(),
            config::mouse_color_en(),
            config::mouse_color_en_upper(),
            config::mouse_offset_x(),
            config::mouse_offset_y(),
        ))
    } else {
        None
    };

    let state_interval = Duration::from_millis(config::state_poll_interval_ms());
    let track_interval = Duration::from_millis(config::track_poll_interval_ms());

    let mut last_state_check_time = Instant::now();
    let mut chinese_mode = false;
    let mut caps_lock_on = false;
    let mut caret_active = false;
    let mut mouse_active = false;
    let mut is_fullscreen = false;

    // 主线程负责消息循环，子线程负责检测
    while running.load(Ordering::SeqCst) {
        let now = Instant::now();

        // A. 状态检测 (100ms)
        if now.duration_since(last_state_check_time) >= state_interval {
            // 检测全屏状态
            is_fullscreen = is_fullscreen_app();

            chinese_mode = is_chinese_mode();
            caps_lock_on = is_caps_lock_on();

            // Caret 状态判断
            if config::caret_enable() {
                if let Some(ref overlay) = caret_overlay {
                    // 全屏时强制隐藏
                    if is_fullscreen {
                        if caret_active {
                            caret_active = false;
                            overlay.hide();
                        }
                    } else {
                        // 非全屏时正常检测
                        let caret_pos = caret_detector.get_caret_pos();

                        let should_caret = caret_pos.is_some() && (chinese_mode || config::caret_show_en());
                        if should_caret != caret_active {
                            caret_active = should_caret;
                            if caret_active {
                                overlay.show();
                            } else {
                                overlay.hide();
                            }
                        }
                    }
                }
            }

            // Mouse 状态判断
            if config::mouse_enable() {
                if let Some(ref overlay) = mouse_overlay {
                    // 全屏时强制隐藏
                    if is_fullscreen {
                        if mouse_active {
                            mouse_active = false;
                            overlay.hide();
                        }
                    } else {
                        // 非全屏时正常检测
                        let target_cursor = cursor_detector.is_target_cursor();
                        let should_mouse = target_cursor && (chinese_mode || config::mouse_show_en());
                        if should_mouse != mouse_active {
                            mouse_active = should_mouse;
                            if mouse_active {
                                overlay.show();
                            } else {
                                overlay.hide();
                            }
                        }
                    }
                }
            }

            last_state_check_time = now;
        }

        // B. 坐标追踪
        // 全屏时不追踪位置
        if !is_fullscreen {
            // 1. 追踪文本光标
            if config::caret_enable() && caret_active {
                if let Some(ref overlay) = caret_overlay {
                    if let Some((x, y, h)) = caret_detector.get_caret_pos() {
                        overlay.update(x, y, chinese_mode, caps_lock_on, h);
                    }
                }
            }

            // 2. 追踪鼠标
            if config::mouse_enable() && mouse_active {
                if let Some(ref overlay) = mouse_overlay {
                    let mut pt = POINT::default();
                    unsafe {
                        if GetCursorPos(&mut pt).is_ok() {
                            overlay.update(pt.x, pt.y, chinese_mode, caps_lock_on, 0);
                        }
                    }
                }
            }
        }

        std::thread::sleep(track_interval);
    }
}

/// 设置高 DPI 感知
fn set_dpi_awareness() {
    unsafe {
        // 尝试使用 SetProcessDpiAwareness (Windows 8.1+)
        let shcore = windows::Win32::System::LibraryLoader::LoadLibraryW(
            windows::core::w!("shcore.dll"),
        );
        if let Ok(h) = shcore {
            if let Some(func) = windows::Win32::System::LibraryLoader::GetProcAddress(
                h,
                windows::core::s!("SetProcessDpiAwareness"),
            ) {
                let set_dpi: extern "system" fn(i32) -> i32 = std::mem::transmute(func);
                let _ = set_dpi(2); // PROCESS_PER_MONITOR_DPI_AWARE
                return;
            }
        }
        // 回退到 SetProcessDPIAware (使用动态加载)
        let user32 = windows::Win32::System::LibraryLoader::LoadLibraryW(
            windows::core::w!("user32.dll"),
        );
        if let Ok(h) = user32 {
            if let Some(func) = windows::Win32::System::LibraryLoader::GetProcAddress(
                h,
                windows::core::s!("SetProcessDPIAware"),
            ) {
                let set_dpi: extern "system" fn() -> i32 = std::mem::transmute(func);
                let _ = set_dpi();
            }
        }
    }
}


