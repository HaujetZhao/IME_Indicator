import time
import ctypes
from win32_api import user32, wintypes
import config
from ime_detector import is_chinese_mode
from caret_detector import CaretDetector
from cursor_detector import CursorDetector
from overlay import IndicatorOverlay

def main():
    # 1. 初始化检测器
    caret_detector = CaretDetector()
    cursor_detector = CursorDetector(config.MOUSE_TARGET_CURSORS)
    
    # 2. 初始化悬浮窗
    caret_overlay = None
    if config.CARET_ENABLE:
        caret_overlay = IndicatorOverlay(
            "Caret", config.CARET_SIZE, config.CARET_COLOR_CN, 
            config.CARET_COLOR_EN, config.CARET_OFFSET_X, config.CARET_OFFSET_Y
        )
        
    mouse_overlay = None
    if config.MOUSE_ENABLE:
        mouse_overlay = IndicatorOverlay(
            "Mouse", config.MOUSE_SIZE, config.MOUSE_COLOR_CN, 
            config.MOUSE_COLOR_EN, config.MOUSE_OFFSET_X, config.MOUSE_OFFSET_Y
        )
    
    print("IME Indicator (输入法综合状态提示) 已启动。")
    if config.CARET_ENABLE: print(f" - 文本光标提示: 已启用 (大小:{config.CARET_SIZE})")
    if config.MOUSE_ENABLE: print(f" - 鼠标跟随提示: 已启用 (大小:{config.MOUSE_SIZE})")
    print("按下 Ctrl+C 停止运行。")
    
    last_state_check_time = 0
    chinese_mode = False
    
    # 运行状态
    caret_active = False
    mouse_active = False
    
    try:
        while True:
            current_time = time.time()
            
            # --- A. 状态检测 (100ms) ---
            if current_time - last_state_check_time >= config.STATE_POLL_INTERVAL:
                chinese_mode = is_chinese_mode()
                
                # Caret 状态判断
                if config.CARET_ENABLE:
                    caret_pos_data = caret_detector.get_caret_pos() # (x, y, h)
                    # 可见性线（黑名单制）：位置有、且焦点不在只读正文中才显示
                    should_caret = (caret_pos_data is not None
                                    and not caret_detector.is_readonly_document_focus()
                                    and (chinese_mode or config.CARET_SHOW_EN))
                    if should_caret != caret_active:
                        caret_active = should_caret
                        if caret_active: caret_overlay.show()
                        else: caret_overlay.hide()
                
                # Mouse 状态判断
                if config.MOUSE_ENABLE:
                    target_cursor = cursor_detector.is_target_cursor()
                    should_mouse = target_cursor and (chinese_mode or config.MOUSE_SHOW_EN)
                    if should_mouse != mouse_active:
                        mouse_active = should_mouse
                        if mouse_active: mouse_overlay.show()
                        else: mouse_overlay.hide()
                
                last_state_check_time = current_time
            
            # --- B. 坐标追踪 ---
            
            # 1. 追踪文本光标
            if config.CARET_ENABLE and caret_active:
                cp = caret_detector.get_caret_pos()
                if cp:
                    caret_overlay.update(cp[0], cp[1], chinese_mode, cp[2])
                    
            # 2. 追踪鼠标
            if config.MOUSE_ENABLE and mouse_active:
                m_pt = wintypes.POINT()
                if user32.GetCursorPos(ctypes.byref(m_pt)):
                    mouse_overlay.update(m_pt.x, m_pt.y, chinese_mode)
            
            time.sleep(config.TRACK_POLL_INTERVAL)
            
    except KeyboardInterrupt:
        print("\n正在停止...")
    finally:
        if caret_overlay: caret_overlay.cleanup()
        if mouse_overlay: mouse_overlay.cleanup()

if __name__ == "__main__":
    # 配置高 DPI 感知
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2) # PROCESS_PER_MONITOR_DPI_AWARE
    except:
        user32.SetProcessDPIAware()
    main()
