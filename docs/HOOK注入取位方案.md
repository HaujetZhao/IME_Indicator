# HOOK 注入取位方案（调研记录，暂不实现）

> 状态：**已验证可行，暂缓产品化**。本文档记录原理、实测数据、风险与产品化设计，供未来实现时参考。
> 验证工具：`rust_indicator/examples/caret_cross.rs`（可视化对比探针，2026-09-23）。

## 1. 要解决的问题

微信 4.x（Weixin.exe）聊天输入框在**没有文字、只有占位文字和闪烁光标**时，现有两条检测链全部失效：

| 通道 | 空输入框 | 有文字 |
|------|---------|--------|
| `gui_info`（GetGUIThreadInfo.hwndCaret） | ❌ 漏报 | ✅ |
| `msaa_caret`（OBJID_CARET） | ❌ | ✅ |
| UIA TextPattern | ❌ | ✅（且位置偏下，即 InputTip 用户观察到的"偏移"） |
| **HOOK（本文）** | ✅ 唯一有效 | ✅ |

而 InputTip（abgox/InputTip）在同一场景下能正常显示——它的默认检测链是 `GUI > HOOK > UIA > MSAA`，微信空输入框走的是 **HOOK**。

## 2. 为什么需要注入

- **插入符（caret）是 USER32 按线程维护的私有状态**：`GetCaretPos` 只有拥有插入符的线程调用才有效，跨进程调用直接失败。
- `GetGUIThreadInfo` 虽然跨进程，但 `hwndCaret` 字段的填充条件没有文档保证；微信自绘 UI 在空输入框状态下就是漏报（实测，微软未解释）。
- 结论：要在微信空输入框拿到 caret，只能**把代码送进目标线程里执行**。

## 3. 原理与完整流程

来源：InputTip 的 `getCaretPosFromHook`（`src/core/var.ahk`），其又源自 Tebayaki 的
[GetCaretPosEx](https://github.com/Tebayaki/AutoHotkeyScripts/blob/main/lib/GetCaretPosEx/GetCaretPosEx.ahk)。
本质是 **不落盘的裸注入**：预编译的位置无关 shellcode + 远线程。

### 准备阶段（我们的进程）

1. 前台窗口 `hwnd` → `GetWindowThreadProcessId` 得到线程 `tid`、进程 `pid`（插入符属于该线程）
2. `OpenProcess(1082)`：`PROCESS_CREATE_THREAD | QUERY_INFORMATION | VM_OPERATION | VM_WRITE | VM_READ`
3. ToolHelp 快照枚举目标进程模块，拿到**它地址空间里** `user32.dll`、`combase.dll` 基址（ASLR 下各进程不同）
4. 解码预编译 shellcode（x64 版本约 1.5KB，机器码见第 7 节 `SHELLCODE_X64_B64`），
   向头部固定偏移打补丁：
   - `+0`  user32 基址（u64）
   - `+8`  combase 基址（u64）
   - `+16` 目标 hwnd（u64）
   - `+24` tid（u32）
   - `+28` 注册消息 ID（u32，`RegisterWindowMessageW("WM_GET_CARET_POS")`）
   - 远线程入口 `+0x4E0`，RECT 结果区 `+56`
5. `VirtualAllocEx`（RWX）→ `WriteProcessMemory` → `FlushInstructionCache` → `CreateRemoteThread(入口, 参数=内存块)`

### 注入阶段（目标进程内）

6. shellcode 远线程调 `SetWindowsHookExW(WH_CALLWNDPROC, hookProc, NULL, tid)`——**只作用于目标线程**的消息钩子，此刻回调已运行在目标线程上下文
7. 钩子需要目标线程处理消息才会触发：shellcode 调 `SendMessageTimeoutW(hwnd, WM_GET_CARET_POS, ...)` 发一条注册消息
8. 目标线程处理该消息时，Windows 先调我们的钩子；钩子识别暗号消息后**在目标线程内调 `GetCaretPos`**，把结果 RECT 写进共享内存块 `+56` 处，然后 `UnhookWindowsHookEx` 撤钩，远线程退出码归 0

### 收割阶段（回到我们进程）

9. 等待远线程结束（探针用 2 秒超时防挂死），`GetExitCodeThread` 必须为 0
10. `ReadProcessMemory` 读 `+56` 处 16 字节 RECT，即插入符屏幕坐标

shellcode 还引用了 `combase.dll` 的 `CoCreateInstance`，可能在目标线程内做了额外的 COM 查询（未逆向确认）；核心机制是第 6-8 步。

## 4. 实测结论（2026-09-23）

| 实验 | 结果 |
|------|------|
| 微信空输入框 | 只有 HOOK 十字出现且位置准确 |
| 微信有文字 | GUI/MSAA 等通道恢复工作，HOOK 依旧准确 |
| 同花顺远航版（探针无防护版） | **卡死并退出** |
| 同花顺远航版（纯 HOOK 的 InputTip） | 正常，不崩 |
| 同花顺远航版（加防护后的探针） | 正常，不崩 |

崩溃原因分析：探针初版**每次轮询无条件注入**任何前台进程，且**缺 WOW64 检查**。
InputTip 有两个我们初版没有的保护，已补进探针：

1. **WOW64 检查**：目标进程是 32 位（`IsWow64Process`）时跳过——x64 shellcode 在 32 位进程里执行必崩
2. **链序短路**：GUI 通道成功就不注入——多数程序用 `GetGUIThreadInfo` 就够，永远不碰注入

## 5. 风险清单（产品化前必须逐条应对）

| 风险 | 说明 | 缓解 |
|------|------|------|
| **杀软误报** | `OpenProcess + VirtualAllocEx + WriteProcessMemory + CreateRemoteThread` 是恶意软件标志性动作链，360/Defender 可能报毒或静默拦截 | 数字签名；文档说明；失败时静默降级 |
| **目标崩溃** | 32 位目标、注入时机撞上目标初始化/卸载、目标自我保护 | WOW64 检查；注入失败全部静默返回 None；等待带超时 |
| **目标自我保护** | 交易/安全类软件对"被打开进程 + 被创建远程线程"敏感（同花顺已实测不触发，但无法覆盖所有软件） | 进程黑名单机制（如误伤案例出现后按 exe 名排除） |
| **开销** | 每次取位 = 一次完整注入（跨进程分配/写入/远线程），10ms 轮询下不可接受 | 只在 `gui_info`、`msaa_caret` 全部失败时注入；可另加节流 |
| **许可** | shellcode 字节来自 Tebayaki/GetCaretPosEx（经 InputTip 使用），实现前需核实其许可证是否允许本项目使用 | 核查许可；必要时自行编译等价 shellcode |
| **健壮性** | 目标无响应时我们的轮询线程会被挂住 | `WaitForSingleObject` 带超时（探针为 2s），超时视为失败 |

## 6. 产品化设计（未实施）

- `DetectionSource` 增加 `HookCaret`，配置名 `hook_caret`
- 管线位置：**最后一位** `gui_info → msaa_caret → hook_caret`，即仅微信空输入框这类场景才会触发
- 内置防护：WOW64 跳过 + 注入全程失败静默（返回 `None`）+ 远线程等待超时
- 若要支持 32 位目标进程需另取 InputTip 中 x86 版 shellcode（本文方案在 WOW64 处直接跳过，不支持 32 位）
- 完整参考实现见下一节（2026-09-23 在可视化探针中验证通过后原样保留）

## 7. 参考实现代码（已验证）

验证环境：Windows 10 22H2，`windows = "0.58"`，rust 2021。
以下代码即探针中实际运行的注入部分，移植时把 `include_str!` 换成内嵌的 `SHELLCODE_X64_B64` 常量即可。

Cargo 需要追加的 features：

```toml
windows = { version = "0.58", features = [
    "Win32_System_Threading",           # OpenProcess / CreateRemoteThread / IsWow64Process
    "Win32_System_Memory",              # VirtualAllocEx / VirtualFreeEx
    "Win32_System_Diagnostics_Debug",   # WriteProcessMemory / ReadProcessMemory / FlushInstructionCache
    "Win32_System_Diagnostics_ToolHelp",# 模块基址枚举
    "Win32_Security",                   # CreateRemoteThread 签名引用 SECURITY_ATTRIBUTES
] }
```

```rust
use std::ffi::c_void;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HWND, WAIT_OBJECT_0, CloseHandle};
use windows::Win32::System::Diagnostics::Debug::{
    FlushInstructionCache, ReadProcessMemory, WriteProcessMemory,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W,
    TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, PAGE_EXECUTE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, IsWow64Process, LPTHREAD_START_ROUTINE, OpenProcess,
    WaitForSingleObject, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, RegisterWindowMessageW};

/// shellcode 内固定偏移：远线程入口 / RECT 结果区 / 头部补丁布局见 docs 第 3 节
const HOOK_THREAD_PROC_OFFSET: usize = 0x4e0;
const HOOK_RECT_OFFSET: usize = 56;

/// HOOK 取位：注入 shellcode，在目标线程内调 GetCaretPos。失败一律静默返回 None。
pub fn hook_caret_pos(hwnd: HWND) -> Option<(i32, i32)> {
    static SHELLCODE: OnceLock<Vec<u8>> = OnceLock::new();
    static WM_GET_CARET_POS: OnceLock<u32> = OnceLock::new();
    unsafe {
        let shellcode = SHELLCODE.get_or_init(|| b64_decode(SHELLCODE_X64_B64));
        let msg = *WM_GET_CARET_POS.get_or_init(|| {
            RegisterWindowMessageW(PCWSTR(b"WM_GET_CARET_POS\0".as_ptr() as *const u16))
        });

        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if tid == 0 {
            return None;
        }

        let hprocess = OpenProcess(
            PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE | PROCESS_VM_READ,
            false,
            pid,
        )
        .ok()?;

        // 防护 1：32 位(WOW64)进程无法执行 x64 shellcode，注入必崩，跳过
        let mut wow64 = windows::Win32::Foundation::BOOL::default();
        let _ = IsWow64Process(hprocess, &mut wow64);
        if wow64.as_bool() {
            let _ = CloseHandle(hprocess);
            return None;
        }

        let result = hook_inject(hprocess, hwnd, tid, msg, shellcode);
        let _ = CloseHandle(hprocess);
        result
    }
}

unsafe fn hook_inject(
    hprocess: HANDLE,
    hwnd: HWND,
    tid: u32,
    msg: u32,
    shellcode: &[u8],
) -> Option<(i32, i32)> {
    let user32 = module_base(pid_of(hwnd)?, "user32.dll")? as usize;
    let combase = module_base(pid_of(hwnd)?, "combase.dll")? as usize;

    // 头部补丁布局: +0 user32基址 | +8 combase基址 | +16 hwnd | +24 tid | +28 注册消息
    let mut code = shellcode.to_vec();
    code[0..8].copy_from_slice(&(user32 as u64).to_le_bytes());
    code[8..16].copy_from_slice(&(combase as u64).to_le_bytes());
    code[16..24].copy_from_slice(&(hwnd.0 as u64).to_le_bytes());
    code[24..28].copy_from_slice(&tid.to_le_bytes());
    code[28..32].copy_from_slice(&msg.to_le_bytes());

    let mem = VirtualAllocEx(hprocess, None, code.len(), MEM_COMMIT, PAGE_EXECUTE_READWRITE);
    if mem.is_null() {
        return None;
    }
    if WriteProcessMemory(hprocess, mem, code.as_ptr() as *const c_void, code.len(), None).is_err() {
        let _ = VirtualFreeEx(hprocess, mem, 0, MEM_RELEASE);
        return None;
    }
    let _ = FlushInstructionCache(hprocess, Some(mem), code.len());

    let thread_proc: LPTHREAD_START_ROUTINE =
        std::mem::transmute(mem as usize + HOOK_THREAD_PROC_OFFSET);
    let hthread = CreateRemoteThread(hprocess, None, 0, thread_proc, Some(mem), 0, None);
    if hthread.is_err() {
        let _ = VirtualFreeEx(hprocess, mem, 0, MEM_RELEASE);
        return None;
    }
    let hthread = hthread.unwrap();

    // 防护 2：带超时等待，目标无响应时不挂死轮询线程
    let wait = WaitForSingleObject(hthread, 2000);
    let mut exit = 0u32;
    let _ = GetExitCodeThread(hthread, &mut exit);
    let _ = CloseHandle(hthread);
    let mut rect = [0i32; 4];
    if wait == WAIT_OBJECT_0 && exit == 0 {
        ReadProcessMemory(
            hprocess,
            (mem as usize + HOOK_RECT_OFFSET) as *const c_void,
            rect.as_mut_ptr() as *mut c_void,
            16,
            None,
        )
        .ok();
    }
    let _ = VirtualFreeEx(hprocess, mem, 0, MEM_RELEASE);
    (wait == WAIT_OBJECT_0 && exit == 0).then_some((rect[0], rect[1]))
}

unsafe fn pid_of(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    (tid != 0).then_some(pid)
}

/// 用 ToolHelp 快照找目标进程内模块基址（ASLR 下各进程不同）
fn module_base(pid: u32, want: &str) -> Option<*mut u8> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid).ok()?;
        let mut me = MODULEENTRY32W::default();
        me.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut found = None;
        if Module32FirstW(snap, &mut me).is_ok() {
            loop {
                let len = me.szModule.iter().position(|&c| c == 0).unwrap_or(0);
                if String::from_utf16_lossy(&me.szModule[..len]).eq_ignore_ascii_case(want) {
                    found = Some(me.modBaseAddr);
                    break;
                }
                if Module32NextW(snap, &mut me).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

fn b64_decode(s: &str) -> Vec<u8> {
    let mut table = [255u8; 256];
    for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[*c as usize] = i as u8;
    }
    let mut acc: u32 = 0;
    let mut nbits = 0;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for b in s.bytes() {
        if b == b'=' {
            break;
        }
        let v = table[b as usize];
        if v == 255 {
            continue;
        }
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    out
}
```

x64 shellcode（base64，来源 Tebayaki/GetCaretPosEx，经 InputTip 使用；**实现前先核实许可证**）：

```text
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABrnppSh2UjT6uenH1oPjxQAeiAqiEg0hGT4ABgsGe4blNldFdpbmRvd3NIb29rRXhXAAAAVW5ob29rV2luZG93c0hvb2tFeABDYWxsTmV4dEhvb2tFeAAAAAAAAFNlbmRNZXNzYWdlVGltZW91dFcAQ29DcmVhdGVJbnN0YW5jZQAAAAAAAAAASIlcJAhIiXQkEFdIg+wgSYvYSIvyi/mFyXgjSIXbdB6LBQb///9BOUAQdRJIjQ3d/v//6JgBAACJBfL+//9Iiw3L/v//SI0VdP///+jnAgAASIXAdRBIi1wkMEiLdCQ4SIPEIF/DTIvLTIvGi9czyUiLXCQwSIt0JDhIg8QgX0j/4MzMzMzMzDPAw8zMzMzMQFNWSIPsSIvySIvZSIXJdQy4VwAHgEiDxEheW8NIi0kISI1UJGBIiVQkKEG4/////0iNVCQwSIl8JEAz/0iJVCQgiXwkYIvWSIsBRI1PAf9QKIXAeHJIOXwkMHRrOXwkYHRlSItLCEiNVCR4SIl8JHhIiwH/UEiL+IXAeDJIi0wkeEiFyXQoSIsBSI1UJHBMi0QkMEyNSxBIiVQkIIvW/1AgSItMJHiL+EiLAf9QEEiLTCQwSIsB/1AQi8dIi3wkQEiDxEheW8NIi3wkQLgBAAAASIPESF5bw8zMzMzMzMxIhcl0VEiF0nRPTYXAdEpIiwJIhcB1HUi4wAAAAAAAAEZIOUIIdCxJxwAAAAAAuAJAAIDDSbkD6ICqISDSEUk7wXXkSLiT4ABgsGe4bkg5Qgh11EmJCDPAw7hXAAeAw8xAU0iD7EBIi9lIjZHYAAAASItJCOhPAQAASIXAdQu4AQAAAEiDxEBbwzPJx0QkWAEAAABIjVQkaEiJTCRoSIlUJCBMjUt4M9JIiUwkYEiJTCQwiUwkUEiNS2hEjUIX/9CFwA+I7wAAAEiLTCRoSIXJD4ThAAAASIsBSI1UJFD/UBiFwA+IhQAAAEiLTCRoSI1UJGBIiwH/UDiFwHhxSItMJGBIhcl0bEiLAUiNVCQw/1AwhcB4WEiLTCQwSIXJdGZIjUNISIlLMEiJQyhMjUMoSI0Vyf7//0G5AwAAAEiJEEiNBdH9//9IiUNQSI1UJFhIiUNYSI0Fxf3//0iJQ2BIiwFIiVQkIItUJFD/UBhIi0wkYEiLVCQwSIXSdA5IiwJIi8r/UBBIi0wkYEiFyXQGSIsB/1AQSItMJGhIhcl0BkiLAf9QEItEJFj32BvAg+AESIPEQFvDuAQAAABIg8RAW8PMzMzMzMxIiVwkCEiJbCQQSIl0JBhIiXwkIEyL2kyL0UiFyXRwSIXSdGtIY0E8g7wIjAAAAAB0XYuMCIgAAACFyXRSRYtMCiBJjQQKi3AkTQPKi2gcSQPyi3gYSQPqD7YaRTPA/89BixFJA9I6GnUZD7bLSYvDSSvThMl0Lw+2SAFI/8A6DAJ08EH/wEmDwQREO8d20TPASItcJAhIi2wkEEiLdCQYSIt8JCDDSWPAD7cMRotEjQBJA8Lr28zMSIlcJAhIiWwkEEiJdCQYSIl8JCBBVkiD7EBIixlIjZGIAAAASIv5SIvL6Bn///9IjZfEAAAASIvLSIvw6Af///9IjZecAAAASIvLSIvo6PX+//9Mi/BIhfZ0ZUiF7XRgSIXAdFtEi08YSI0VoPv//0UzwEGNSAT/1kiL8EiFwHUFjUYC6z+LVxwzwEiLTxBFM8lIiUQkMEUzwMdEJCjIAAAAiUQkIP/VSIvOSIvYQf/WSIXbdQWNQwPrCotHIOsFuAEAAABIi1wkUEiLbCRYSIt0JGBIi3wkaEiDxEBBXsM=
```
