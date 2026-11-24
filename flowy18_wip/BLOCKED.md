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
它的真包就是 `gpui-pre`，所以 `MenuItem` / `App` / `Action` 类型一致，托盘代码已接回：
图标、Show / Quit 菜单，`flowy_shell::tray_available()` 报告是否可用。

**但运行时建不出来**：启动时 `Shell_NotifyIconW operation 0 failed`，`tray_available()`
为 false，所以本机实际仍是关窗即退出。见文末「运行时与遗留」。

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
| 系统托盘 | `gpui-tray` | 代码已接回（`gpui-kit` → `gpui-pre`），运行时 `Shell_NotifyIconW` 失败 |

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
- 已处理：`Cargo.toml` 的 `[workspace.dependencies]` 里那条 `flowy-skills` 没有任何
  成员依赖，每次 `cargo check` 都会报 `unused_workspace_dependencies`。该行已删除；
  `crates/flowy-skills` 仍是 workspace 成员，随时可以被别的 crate 以
  `flowy-skills = { path = "..." }` 或重新登记的 workspace 依赖接上。

## 2026-11-24 · 运行时与遗留（实测）

- **托盘运行时不可用**：启动日志 `WARN flowy.boot: system tray unavailable
  stage="shell::tray::build" error=native tray error: Shell_NotifyIconW operation 0 failed`。
  代码路径已接回、编译通过，但本机建不出图标，`tray_available()` 为 false，于是关窗
  直接退出，托盘菜单与最小化都走不到。原因未查，需单独排查（explorer 通知区域、
  图标数据、会话权限都可能是因）。
- **`flowy-agent` 测试目标 45 条 clippy**：`cargo clippy -p flowy-agent --all-targets
  --locked -- -D warnings` 退出 101，报 45 条，全部落在 lib test；`--lib` 已清零。
  CI 门禁命令不选 `flowy-agent`，所以这 45 条不进流水线。要清得先决定是否把
  `-p flowy-agent` 加进门禁，再另开一轮。
- **资料库导入被跳过（环境问题）**：日志 `artifact import skipped ...
  publisher directory is a link or reparse point`。本机 `%APPDATA%\Flowy` 是指向
  `.absorb_tmp/flowy-appdata` 的 junction，导入逻辑拒绝这类路径。属本机数据目录
  形态，不是代码缺陷。
