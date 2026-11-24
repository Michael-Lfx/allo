# 迁移阻塞记录（gpui-ce → gpui-pre + gpui-kit）

## 结论（2026-11-24）

`gpui-pre =0.3.14` + `gpui-kit 0.11` 可以承载 `flowy18_wip`，但需要接受一组
能力缺口。本文件记录**已验证不可行**的方向和**已知的功能损失**，避免后来者
重复踩坑。

## 已验证不可行的方向

### 1. 移植 `editable_text`

`gpui-ce` 的 `editable_text` 是在 `gpui-pre` 的排版层之上加的一层封装
（`ShapedText`、`ShapedTextLayout`、`CaretPosition`/`CaretSelection`、
`UnicodeBidi`、`TextLayoutOptions`、`ColorExt`）。移植尝试后错误数长期在
120～182 之间震荡，无法收敛：

- `gpui-pre` 的 `shape_text` 返回 `SmallVec<[WrappedLine; 1]>`，ce 返回 `ShapedText`
- `WrappedLine` 不实现 `Clone`
- `closest_index_for_position` 返回 `Result<usize, usize>`，签名不同
- `WrappedLine::paint` 多出 `align` / `align_width` 参数

**结论**：不移植。直接用 `gpui_kit::base::input` 的 `Input` / `Textarea`，
`flowy-ui` 只保留样式壳。

### 2. 热补丁（hot-patching）

`dx serve` 强制 `-Clinker=dx.exe`，两种模式都在链接阶段失败
（exit 1 / LNK1104）。用 cargo 走同一 wrapper 构建成功，说明问题在 dx 的
调用参数，不在 wrapper / MSVC / CE。**本机不可用**。

### 3. `gpui-tray`（已解决）

`gpui-tray` 用一个 cargo feature 桥接三套 GPUI 类型图。工作区选的是 `gpui-kit`，
它的真包就是 `gpui-pre`，所以 `MenuItem` / `App` / `Action` 类型一致，托盘已恢复：
图标、Show / Quit 菜单，`flowy_shell::tray_available()` 报告是否可用。

## 能力缺口（`gpui-pre` 没有）

| 能力 | ce 的写法 | pre 的现状 |
| --- | --- | --- |
| 元素 prepaint 钩子 | `.on_prepaint(..)` | 无（popover 锚点测量需另想办法） |
| ARIA 属性 | `.aria_live` `.aria_disabled` `.aria_modal` `.aria_hidden` | 无，生产代码中已删 |
| a11y 测试 API | `window.a11y_tree()` 等 | 无，改用 `gpui-kit/test-support` |
| 颜色工具 | `gpui::ColorExt`、`rgb_to_hsla` | `Hsla::from(rgba)` 等价；`ColorExt` 无 |
| Enter action | `gpui::Enter` | `gpui_kit::base::input::Enter { secondary, shift }` |
| 元素级样式 | `AnyElement` 可链 `.min_h` 等 | `AnyElement` 无 `Styled`，须包 `div()` |
| `emplace` | `state.emplace(text, cx)` | 无；`set_value(text, window, cx)` 需要 window |
| 字母间距 | `letter_spacing(px(..))` | 无 |
| RTL 根方向 | `.rtl()` / `.ltr()` | 只有全局 `LayoutDirection` |
| 系统托盘 | `gpui-tray` | 已恢复：`gpui-kit` feature 指向 `gpui-pre` |

## 当前已知遗留（不影响编译，影响行为或体验）

1. popover / 菜单 / tooltip 的锚点测量：**已修复**，见 `flowy-ui/src/overlays.rs`
   的 `PrepaintBounds` 与 `tests/anchor_measurement.rs`（2 个回归测试）
3. 逐键硬截断改为保存时截断
4. RTL 未接线，阿语/希语下根方向与局部方向不一致
5. a11y 播报与隐藏语义退化
6. 关窗：托盘可用时最小化到托盘，不可用时退出（`gpui-pre` 没有 `set_visible`）
7. 输入框内 Tab：**多行框已改为插入 4 个空格**，单行框仍移焦点。注意首页 composer 用的是裸 `text_area`，不在那四个字段里，Tab 仍移焦点
8. 光标闪烁间隔、letter_spacing 回默认
9. 启动固定落 Home，不再自动恢复上次会话
10. `set_value` 不发 change 事件，所有「写值 → 等事件 → 落盘」的链路都需要
    改成「算出最终值 → 排队 → 直接用最终值提交」

## 2026-11-24 · Clippy 欠债清理（fix-flowy18-clippy-debt）

- 待裁决：无。
- 顺带记录（未处理，不影响门禁）：`Cargo.toml` 的 `[workspace.dependencies]` 里
  `flowy-skills` 目前没有任何成员依赖，每次 `cargo check` 都会输出一条
  `unused_workspace_dependencies` 警告。它属于 cargo 层警告，不是 clippy 门禁，
  本次未动。
