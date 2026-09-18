# 光标可编辑性校验设计（2026-09-18）

## 问题

1. 浏览器里点击网页正文（非输入框）时，指示器误显示。根因：`uia_selection` 检测级只判断"焦点元素支持 TextPattern 且有选区"就返回光标位置——网页正文的焦点元素是整个 Document，有文本选区但不能输入。
2. 点走后残留显示：从输入框点到网页正文后，`uia_selection` 已判定不可编辑并返回 None，但管线把 None 当"这级没拿到"继续降级，msaa 级的 GUITHREADINFO 回退用 hwndFocus 上残留的 rcCaret 返回旧光标位置，圆点不消失。

`gui_info`/`msaa` 两级有天然可输入信号（系统光标 hwndCaret 只在真正可输入时存在），自身不校验。

## 方案

- **校验**：`uia_selection` 拿到焦点元素后，只接受 `ControlType == Edit`（浏览器输入框、VS Code）。查询失败按拒绝处理（外部数据，失败即不可信）。曾实现 Document+IsReadOnly 与 off 等模式（配置 `editable_check`），实测 edit_only 已够用，其余删除。
- **终止语义**："焦点不可编辑"是"不在可输入位置"的权威答案，终止整条管线，不降级到 `gui_info`/`msaa`（它们可能返回残留的旧光标位置）。焦点确实可编辑但 uia 拿不到矩形时，仍正常降级。

## 效果预期

- 浏览器网页正文：Document → 拒绝并终止 → 不显示，且离开输入框后立即消失
- 浏览器输入框（Edit）→ 正常显示
- 记事本等原生控件（gui_info 级）→ 不受影响

## 改动范围

`rust_indicator/src/caret_detector.rs`（校验函数 + 管线终止）、`python_indicator/caret_detector.py`（参考实现同步）。无配置项、无新依赖。
