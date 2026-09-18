# 光标可见性：黑名单制设计（2026-09-18）

## 演进过程

1. **问题**：浏览器网页正文（非输入框）点击时指示器误显示——检测管线判断"焦点元素有文本选区"就返回位置，网页正文焦点是 Document，有选区但不能输入。
2. **白名单尝试（失败）**：把"焦点必须是 Edit（或可编辑 Document）"作为显示前提——塞进位置管线会连带拒掉 contenteditable 等焦点非 Edit 的真输入框；作为独立可见性线后，记事本等应用因焦点元素类型不确定而不显示。白名单要求枚举所有"可输入"形态，枚举不全就误杀。
3. **最终设计（黑名单制）**：默认显示，只在**确认**焦点位于不可输入位置时隐藏。

## 最终设计

显示条件 = 位置管线有结果 **且非** `focus_is_readonly_document()` **且**（中文模式或允许英文显示）。

`focus_is_readonly_document()`：焦点元素 ControlType 为 Document，且（无 ValuePattern 或 `IsReadOnly == true`）→ 命中黑名单。精确覆盖已知唯一误显示来源：浏览器网页正文。Word / contenteditable 是非只读 Document，不命中；Edit、按钮、终端等其他类型与任何查询失败都不命中。

## 位置管线收缩（2026-09-18 追加）

删除 `uia_selection` 级（UIA TextPattern GetSelection 路线）：

- **删除原因**：在 VS Code 中像素映射错误（指示器落到行首）；而浏览器/VS Code 的光标 `msaa` 级（OBJID_CARET）已实测覆盖。它是净伤害兼死代码。
- **连带删除**：其全部专用辅助——空字段 U+FFFC 识别（`rect_covers_element`）、坏像素映射检测（`doc_start_rect`）、选区折叠逻辑。
- 位置管线只剩两级：`gui_info`（记事本等原生）→ `msaa`（浏览器/VS Code）。
- 可见性线仍用 UIA（GetFocusedElement），UIA 实例保留。

若日后发现 msaa 覆盖不到的场景，再按需引入新的检测级，而不是复活这条复杂且不可靠的路线。

## 改动范围

`rust_indicator/src/caret_detector.rs`、`rust_indicator/src/config.rs`、`rust_indicator/src/main.rs`、`python_indicator/`（参考实现同步）。无新依赖。
