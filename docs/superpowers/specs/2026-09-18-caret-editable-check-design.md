# 光标可见性与位置解耦设计（2026-09-18）

## 问题

1. 浏览器点击网页正文（非输入框）时指示器误显示：`uia_selection` 级只判断"焦点元素有文本选区"就返回位置，而网页正文焦点是 Document，有选区但不能输入。
2. 把可编辑性校验塞进位置管线后，网页编辑框（焦点元素非 Edit 型，如 contenteditable 是 Document 型）被连带拒掉，光标在输入框里也不显示了。

## 设计：两条并行的线

- **位置线**：原多级检测管线（gui_info → uia_selection → msaa）原样保留，只管"光标在哪"，不做任何可编辑性判断。
- **可见性线** `is_focused_editable()`：单独裁决"焦点能不能输入"。
  - `ControlType == Edit` → 可编辑
  - `ControlType == Document` → 查 `ValuePattern.IsReadOnly`，非只读才可编辑（Word/contenteditable）
  - 其余（网页正文、按钮等）与查询失败一律不可编辑（外部数据，失败即不可信）
  - 注：曾试过 Edit-only，实测连网页输入框都会误杀，故保留 Document 分支。

显示条件 = 位置线有结果 **且** 可见性线可编辑 **且**（中文模式或配置允许英文显示）。

## 效果预期

- 网页正文：可见性线判不可编辑 → 不显示；从输入框点走后 ~100ms 内消失（msaa 残留的旧位置被可见性线拦住）
- 网页输入框/contenteditable：位置来自 uia_selection，可见性线放行 → 正常显示
- 记事本等原生控件：位置来自 gui_info，焦点 Edit → 正常显示
- VS Code / Word / 终端类待实测；若 Document 分支误放行（如某些正文也暴露非只读 ValuePattern），再收紧判据

## 改动范围

`rust_indicator/src/caret_detector.rs`（新增 `is_focused_editable`，位置管线还原）、`rust_indicator/src/main.rs`（显示条件接入）、`python_indicator/`（参考实现同步）。无配置项、无新依赖。
