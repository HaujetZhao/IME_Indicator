//! GDI+ 悬浮窗渲染模块

use std::ptr::null_mut;
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics,
    GdipFillEllipse, GdipSetSmoothingMode, GdiplusShutdown, GdiplusStartup,
    GdiplusStartupInput, GpBrush, GpGraphics, GpSolidFill, SmoothingModeAntiAlias,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW,
    ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::PCWSTR;

/// BLENDFUNCTION 结构体
#[repr(C)]
struct BLENDFUNCTION {
    blend_op: u8,
    blend_flags: u8,
    source_constant_alpha: u8,
    alpha_format: u8,
}

const AC_SRC_OVER: u8 = 0x00;
const AC_SRC_ALPHA: u8 = 0x01;

/// 指示器悬浮窗
pub struct IndicatorOverlay {
    hwnd: HWND,
    size: i32,
    color_cn: u32,
    color_en: u32,
    color_en_upper: u32,
    offset_x: i32,
    offset_y: i32,
    gdi_token: usize,
}

impl IndicatorOverlay {
    /// 创建新的悬浮窗
    pub fn new(
        name: &str,
        size: i32,
        color_cn: u32,
        color_en: u32,
        color_en_upper: u32,
        offset_x: i32,
        offset_y: i32,
    ) -> Self {
        let gdi_token = Self::init_gdiplus();
        let hwnd = Self::create_window(name, size);

        Self {
            hwnd,
            size,
            color_cn,
            color_en,
            color_en_upper,
            offset_x,
            offset_y,
            gdi_token,
        }
    }

    /// 初始化 GDI+
    fn init_gdiplus() -> usize {
        unsafe {
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                DebugEventCallback: 0,
                SuppressBackgroundThread: false.into(),
                SuppressExternalCodecs: false.into(),
            };
            let mut token: usize = 0;
            let _ = GdiplusStartup(&mut token, &input, null_mut());
            token
        }
    }

    /// 创建透明悬浮窗
    fn create_window(name: &str, size: i32) -> HWND {
        unsafe {
            let h_instance: HMODULE = GetModuleHandleW(None).unwrap_or_default();
            let class_name: Vec<u16> = format!("IMEIndicator_{}\0", name).encode_utf16().collect();
            let window_name: Vec<u16> = format!("Indicator_{}\0", name).encode_utf16().collect();

            extern "system" fn wnd_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                unsafe {
                    if msg == 0x0002 {
                        // WM_DESTROY
                        return LRESULT(0);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                hInstance: std::mem::transmute(h_instance),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };

            RegisterClassExW(&wc);

            let ex_style = WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOPMOST
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW;

            CreateWindowExW(
                ex_style,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(window_name.as_ptr()),
                WS_POPUP,
                0,
                0,
                size,
                size,
                None,
                None,
                h_instance,
                None,
            )
            .unwrap_or_default()
        }
    }

    /// 更新渲染内容和屏幕位置
    /// is_chinese: 是否中文模式
    /// is_upper: 是否大写锁定（Caps Lock 开启）
    pub fn update(&self, x: i32, y: i32, is_chinese: bool, is_upper: bool, caret_h: i32) {
        let color = if is_upper {
            // Caps Lock 开启，不管中英文都显示绿色
            self.color_en_upper
        } else if is_chinese {
            // Caps Lock 关闭 + 中文模式 → 红色
            self.color_cn
        } else {
            // Caps Lock 关闭 + 英文模式 → 蓝色
            self.color_en
        };

        unsafe {
            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(screen_dc);

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.size,
                    biHeight: self.size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut ppv_bits: *mut std::ffi::c_void = null_mut();
            let h_bitmap =
                CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut ppv_bits, None, 0)
                    .unwrap_or_default();

            let old_bitmap = SelectObject(mem_dc, h_bitmap);

            // GDI+ 绘制
            let mut graphics: *mut GpGraphics = null_mut();
            GdipCreateFromHDC(mem_dc, &mut graphics);
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);

            let mut brush: *mut GpSolidFill = null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillEllipse(
                graphics,
                brush as *mut GpBrush,
                0.0,
                0.0,
                self.size as f32,
                self.size as f32,
            );

            GdipDeleteBrush(brush as *mut GpBrush);
            GdipDeleteGraphics(graphics);

            // UpdateLayeredWindow
            let dest_point = POINT {
                x: x + self.offset_x - self.size / 2,
                y: y + caret_h + self.offset_y - self.size / 2,
            };
            let src_point = POINT { x: 0, y: 0 };
            let size = SIZE {
                cx: self.size,
                cy: self.size,
            };
            let blend = BLENDFUNCTION {
                blend_op: AC_SRC_OVER,
                blend_flags: 0,
                source_constant_alpha: 255,
                alpha_format: AC_SRC_ALPHA,
            };

            let _ = UpdateLayeredWindow(
                self.hwnd,
                screen_dc,
                Some(&dest_point),
                Some(&size),
                mem_dc,
                Some(&src_point),
                COLORREF(0),
                Some(&blend as *const BLENDFUNCTION as *const _),
                ULW_ALPHA,
            );

            SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(h_bitmap);
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);

            // 保持窗口在最顶层
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );

            // 处理消息
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, self.hwnd, 0, 0, PM_REMOVE).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// 显示窗口
    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
    }

    /// 隐藏窗口
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// 清理资源
    pub fn cleanup(&self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            GdiplusShutdown(self.gdi_token);
        }
    }
}

impl Drop for IndicatorOverlay {
    fn drop(&mut self) {
        self.cleanup();
    }
}
