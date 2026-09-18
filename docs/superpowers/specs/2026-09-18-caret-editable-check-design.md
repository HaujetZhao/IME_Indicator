# 光标可见性：黑名单制设计（2026-09-18）

## 演进过程

1. **问题**：浏览器网页正文（非输入框）点击时指示器误显示——`uia_selection` 级判断"焦点元素有文本选区"就返回位置，网页正文焦点是 Document，有选区但不能输入。
2. **白名单尝试（失败）**：把"焦点必须是 Edit（或可编辑 Document）"作为显示前提——塞进位置管线会连带拒掉 contenteditable 等焦点非 Edit 的真输入框；作为独立可见性线后，记事本等应用因焦点元素类型不确定而不显示。白名单要求枚举所有"可输入"形态，枚举不全就误杀。
3. **最终设计（黑名单制）**：默认显示，只在**确认**焦点位于不可输入位置时隐藏。

## 最终设计

显示条件 = 位置管线有结果 **且非** `focus_is_readonly_document()` **且**（中文模式或允许英文显示）。

`focus_is_readonly_document()`：焦点元素 ControlType 为 Document，且（无 ValuePattern 或 `IsReadOnly == true`）→ 命中黑名单。这精确覆盖已知唯一误显示来源：浏览器网页正文。Word / contenteditable 是非只读 Document，不命中；Edit、按钮、终端等其他类型与任何查询失败都不命中，照常显示。

## 效果预期

- 网页正文 → 隐藏；输入框点走后 ~100ms 内消失（msaa 残留旧位置被黑名单拦住）
- 网页输入框 / contenteditable / 记事本 / 终端 / VS Code → 不受影响，照常显示
- 若日后发现新的误显示场景，往黑名单里加判据，而不是给"可输入"建白名单

## 改动范围

`rust_indicator/src/caret_detector.rs`（`focus_is_readonly_document`）、`rust_indicator/src/main.rs`（显示条件）、`python_indicator/`（参考实现同步）。无配置项、无新依赖。
