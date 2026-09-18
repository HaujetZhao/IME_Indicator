# 光标可编辑性校验设计（2026-09-18）

## 问题

浏览器里点击网页正文（非输入框）时，指示器误显示。根因：`uia_selection` 检测级只判断"焦点元素支持 TextPattern 且有选区"就返回光标位置——网页正文的焦点元素是整个 Document，有文本选区但不能输入。

`gui_info`/`msaa` 两级有天然可输入信号（系统光标 hwndCaret 只在真正可输入时存在），不受影响、不校验。

## 方案

在 `get_pos_via_uia_selection` 拿到焦点元素后、取选区之前，按配置模式校验可编辑性：

| 模式 | 行为 |
|------|------|
| `edit_or_document`（默认） | ControlType 为 `Edit` 放行；为 `Document` 时查 `ValuePattern.CurrentIsReadOnly()`，为 `false` 才放行；其余拒绝 |
| `edit_only` | 只接受 ControlType == `Edit` |
| `off` | 不校验（旧行为） |

拒绝时按现有惯例 `append_error("Sel:NotEditable")` 留排查线索。查询失败（ControlType/ValuePattern 出错）一律按拒绝处理——这是对外部应用数据的信任边界判断，失败即不可信。

## 配置

`[caret]` 下新增 `editable_check`，取值 `edit_or_document` / `edit_only` / `off`，默认 `edit_or_document`。非法值回退默认。

## 效果预期

- 浏览器网页正文：Document + 无 ValuePattern（或只读）→ 拒绝，不再误显示
- 浏览器输入框（Edit）→ 正常显示
- 记事本等原生控件（gui_info 级）→ 不受影响
- VS Code / Word 待实测；若误杀，配置降级 `edit_only`/`off`

## 改动范围

`rust_indicator/src/caret_detector.rs`（新增校验函数 + 调用）、`rust_indicator/src/config.rs`（新增配置项），约 50 行。无新依赖。
