//! 输入指示器 - 零依赖 TOML 解析
//! 追求极致代码简洁度与二进制体积

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

// ============================================================================
// 数据结构 (扁平化，删除冗余嵌套)
// ============================================================================

pub struct Config {
    pub poll_state_interval_ms: u64,
    pub poll_track_interval_ms: u64,

    pub tray_enable: bool,
    pub debug_console: bool,

    pub caret_enable: bool,
    pub caret_color_cn: u32,
    pub caret_color_en: u32,
    pub caret_color_en_upper: u32,
    pub caret_size: i32,
    pub caret_offset_x: i32,
    pub caret_offset_y: i32,
    pub caret_show_en: bool,

    pub mouse_enable: bool,
    pub mouse_color_cn: u32,
    pub mouse_color_en: u32,
    pub mouse_color_en_upper: u32,
    pub mouse_size: i32,
    pub mouse_offset_x: i32,
    pub mouse_offset_y: i32,
    pub mouse_show_en: bool,
    pub mouse_target_cursors: Vec<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_state_interval_ms: 100,
            poll_track_interval_ms: 10,
            tray_enable: true,
            debug_console: false,
            caret_enable: true,
            caret_color_cn: parse_color("#FF0000A0"),
            caret_color_en: parse_color("#0000FFA0"),
            caret_color_en_upper: parse_color("#008000A0"),
            caret_size: 8,
            caret_offset_x: 0,
            caret_offset_y: 0,
            caret_show_en: true,
            mouse_enable: true,
            mouse_color_cn: parse_color("#FF0000A0"),
            mouse_color_en: parse_color("#0000FFA0"),
            mouse_color_en_upper: parse_color("#008000A0"),
            mouse_size: 8,
            mouse_offset_x: 2,
            mouse_offset_y: 18,
            mouse_show_en: true,
            mouse_target_cursors: vec![32513, 32512],
        }
    }
}

// ============================================================================
// 颜色与解析辅助
// ============================================================================

pub trait ConfigParseExt {
    fn parse_color(&self) -> u32;
}

impl ConfigParseExt for str {
    fn parse_color(&self) -> u32 {
        let clean = self.trim().trim_matches('"').trim_start_matches('#');
        if clean.len() >= 6 {
            let r = u32::from_str_radix(&clean[0..2], 16).unwrap_or(0);
            let g = u32::from_str_radix(&clean[2..4], 16).unwrap_or(0);
            let b = u32::from_str_radix(&clean[4..6], 16).unwrap_or(0);
            let a = if clean.len() == 8 { u32::from_str_radix(&clean[6..8], 16).unwrap_or(0xA0) } else { 0xA0 };
            (a << 24) | (r << 16) | (g << 8) | b
        } else {
            0xA0FF7800
        }
    }
}

pub fn parse_color(s: &str) -> u32 { s.parse_color() }

// ============================================================================
// 微型 TOML 解析器
// ============================================================================

fn load_config() -> Config {
    let mut config = Config::default();
    let path = get_config_path();

    if !path.exists() {
        let _ = fs::write(&path, generate_toml_template());
        return config;
    }

    if let Ok(content) = fs::read_to_string(&path) {
        let mut sections = HashMap::new();
        let mut cur_sec = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            
            if line.starts_with('[') && line.ends_with(']') {
                cur_sec = line[1..line.len()-1].to_lowercase();
            } else if let Some((k, v)) = line.split_once('=') {
                let key = k.trim().to_lowercase();
                // 智能移除行内注释：寻找 " #" (带空格的井号) 
                let val = v.split(" #").next().unwrap().trim().to_string();
                sections.entry(cur_sec.clone()).or_insert_with(HashMap::new).insert(key, val);
            }
        }

        // 映射数据 (精简写法)
        let get = |sec: &str, key: &str| sections.get(sec)?.get(key);
        
        if let Some(v) = get("poll",  "state_interval_ms") { if let Ok(n) = v.parse() { config.poll_state_interval_ms = n; } }
        if let Some(v) = get("poll",  "track_interval_ms") { if let Ok(n) = v.parse() { config.poll_track_interval_ms = n; } }
        
        if let Some(v) = get("tray", "enable") {
            match v.as_str() {
                "true" => config.tray_enable = true,
                "false" => config.tray_enable = false,
                _ => {} // 保持默认值
            }
        }
        if let Some(v) = get("tray", "debug_console") {
            match v.as_str() {
                "true" => config.debug_console = true,
                "false" => config.debug_console = false,
                _ => {}
            }
        }
        
        if let Some(v) = get("caret", "enable") { 
            match v.as_str() {
                "true" => config.caret_enable = true,
                "false" => config.caret_enable = false,
                _ => {}
            }
        }
        if let Some(v) = get("caret", "color_cn") { config.caret_color_cn = v.parse_color(); }
        if let Some(v) = get("caret", "color_en") { config.caret_color_en = v.parse_color(); }
        if let Some(v) = get("caret", "color_en_upper") { config.caret_color_en_upper = v.parse_color(); }
        if let Some(v) = get("caret", "size")     { if let Ok(n) = v.parse() { config.caret_size = n; } }
        if let Some(v) = get("caret", "offset_x") { if let Ok(n) = v.parse() { config.caret_offset_x = n; } }
        if let Some(v) = get("caret", "offset_y") { if let Ok(n) = v.parse() { config.caret_offset_y = n; } }
        if let Some(v) = get("caret", "show_en") { 
            match v.as_str() {
                "true" => config.caret_show_en = true,
                "false" => config.caret_show_en = false,
                _ => {}
            }
        }

        if let Some(v) = get("mouse", "enable") { 
            match v.as_str() {
                "true" => config.mouse_enable = true,
                "false" => config.mouse_enable = false,
                _ => {}
            }
        }
        if let Some(v) = get("mouse", "color_cn") { config.mouse_color_cn = v.parse_color(); }
        if let Some(v) = get("mouse", "color_en") { config.mouse_color_en = v.parse_color(); }
        if let Some(v) = get("mouse", "color_en_upper") { config.mouse_color_en_upper = v.parse_color(); }
        if let Some(v) = get("mouse", "size")     { if let Ok(n) = v.parse() { config.mouse_size = n; } }
        if let Some(v) = get("mouse", "offset_x") { if let Ok(n) = v.parse() { config.mouse_offset_x = n; } }
        if let Some(v) = get("mouse", "offset_y") { if let Ok(n) = v.parse() { config.mouse_offset_y = n; } }
        if let Some(v) = get("mouse", "show_en") { 
            match v.as_str() {
                "true" => config.mouse_show_en = true,
                "false" => config.mouse_show_en = false,
                _ => {}
            }
        }
        if let Some(v) = get("mouse", "target_cursors") {
            config.mouse_target_cursors = v.trim_matches(|c| c == '[' || c == ']')
                .split(',').filter_map(|s| s.trim().parse().ok()).collect();
        }
    }
    config
}

pub(crate) fn get_config_path() -> PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().join("config.toml")
}

fn generate_toml_template() -> String {
    r##"# 输入指示器 (IME Indicator) 配置文件
[poll]
state_interval_ms = 100   # 状态检测间隔 (ms)
track_interval_ms = 10    # 位置追踪间隔 (ms)

[tray]
enable = true               # 是否显示托盘图标 (false 时完全后台运行，只能通过任务管理器结束)
debug_console = false       # 是否显示调试控制台 (用于开发调试)

[caret]
# 颜色格式：#RRGGBBAA
# RR=红, GG=绿, BB=蓝, AA=透明度(00=透明, FF=不透明, A0=63%不透明)
enable = true               # 是否启用文本光标提示
color_cn = "#FF0000A0"    # 中文状态颜色（红）
color_en = "#0000FFA0"    # 英文小写状态颜色（蓝）
color_en_upper = "#008000A0"  # 英文大写状态颜色（绿）
size = 8                    # 提示球大小
offset_x = 0
offset_y = 0
show_en = true              # 英文状态下是否显示

[mouse]
enable = true               # 是否开启鼠标提示
color_cn = "#FF0000A0"    # 中文状态颜色（红）
color_en = "#0000FFA0"    # 英文小写状态颜色（蓝）
color_en_upper = "#008000A0"  # 英文大写状态颜色（绿）
size = 8                    # 提示球大小
offset_x = 2
offset_y = 18
show_en = true              # 英文状态下是否显示
target_cursors = [32513, 32512]  # I-Beam, Normal
"##.to_string()
}

// ============================================================================
// 全局接口
// ============================================================================

static CONFIG: OnceLock<Config> = OnceLock::new();
pub fn get() -> &'static Config { CONFIG.get_or_init(load_config) }

pub fn state_poll_interval_ms() -> u64 { get().poll_state_interval_ms }
pub fn track_poll_interval_ms() -> u64 { get().poll_track_interval_ms }
pub fn tray_enable() -> bool { get().tray_enable }
pub fn debug_console() -> bool { get().debug_console }
pub fn caret_enable() -> bool { get().caret_enable }
pub fn caret_color_cn() -> u32 { get().caret_color_cn }
pub fn caret_color_en() -> u32 { get().caret_color_en }
pub fn caret_color_en_upper() -> u32 { get().caret_color_en_upper }
pub fn caret_size() -> i32 { get().caret_size }
pub fn caret_offset_x() -> i32 { get().caret_offset_x }
pub fn caret_offset_y() -> i32 { get().caret_offset_y }
pub fn caret_show_en() -> bool { get().caret_show_en }
pub fn mouse_enable() -> bool { get().mouse_enable }
pub fn mouse_color_cn() -> u32 { get().mouse_color_cn }
pub fn mouse_color_en() -> u32 { get().mouse_color_en }
pub fn mouse_color_en_upper() -> u32 { get().mouse_color_en_upper }
pub fn mouse_size() -> i32 { get().mouse_size }
pub fn mouse_offset_x() -> i32 { get().mouse_offset_x }
pub fn mouse_offset_y() -> i32 { get().mouse_offset_y }
pub fn mouse_show_en() -> bool { get().mouse_show_en }
pub fn mouse_target_cursors() -> &'static [u32] { &get().mouse_target_cursors }
