# GPUI-PRE + GPUI-KIT 使用指南

> 本文档记录 `flowy18_wip` 从 gpui-ce 迁移到 gpui-pre + gpui-kit 后的
> 实际 API 形态。写代码前先读这里，能避开本文列出的全部坑。

## 依赖（已精确锁定）

```toml
gpui = { package = "gpui-pre", version = "=0.3.14", features = ["hot-reload"] }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.14" }
gpui-kit = "=0.11.0"
```

**三者必须同时锁死**。`gpui-kit 0.11.0` 内部依赖 `gpui-pre =0.3.14`（精确），
任何一方漂移都会让 workspace 解析失败。不要用 `"0.11"` 这种 caret 范围。

## 模块结构

`gpui-kit` 是三个 crate 的聚合层：

| 路径 | 实际来源 | 内容 |
| --- | --- | --- |
| `gpui_kit::*` | `gpui-pre` | 与 `gpui` 相同的 API |
| `gpui_kit::base::*` | `gpui-base` | 输入引擎、测试支持 |
| `gpui_kit::component` | `gpui-component` | 60+ 现成组件 |

输入相关类型的**规范**路径是 `gpui_kit::base::input`。`Input` 和 `Textarea`
在 `gpui_kit::base` 根也做了 re-export，但状态和事件类型只能从 `input` 取：

```rust
use gpui_kit::base::input::{
    Input, InputState, Textarea, TextareaState,
    InputAccessibility, InputEditorStyle, InputEvent,
};
```

`InputState` 和 `TextareaState` 是**不同类型**：

```rust
InputState     = InputBaseState<InputMode>       // 单行
TextareaState  = InputBaseState<TextareaMode>    // 多行
```

不能互换。`flowy-ui` 为此提供了两个控件：`TextField`（单行）和
`TextAreaField`（多行）。

## 必须传 window 的构造

```rust
InputState::new(window, cx)
TextareaState::new(window, cx)
```

**没有**不带 `window` 的构造，也没有 `Default`。

`Context` 拿不到 `&mut Window`（只有 `window_handle()` 返回 `AnyWindowHandle`），
所以必须从调用方把 window 一路传下来。本仓库的做法：

1. `FlowyApp::new(store, system_dark, window, cx)`
2. `StudioPanel::new(config, ui, window, cx)`
3. 测试用 `cx.add_window_view(|window, cx| ...)`

## 程序化设置文本：必须排队

`set_value` 需要 `&mut Window`，而事件回调、异步回调里都拿不到。本仓库用
「排队 + 下一帧 render 落地」：

```rust
// flowy-app
app.set_input_text(&state, "text", cx);      // InputState
app.set_textarea_text(&state, "text", cx);   // TextareaState

// flowy-studio
panel.prompt_text(cx)      // 读（含未落地的 pending）
panel.set_prompt_text(..)  // 写（入队）
```

队列在 `Render::render` 顶部排空。

### 两个必须知道的语义

**`set_value` 不发 `InputEvent`。** 旧 ce 的 `emplace` 会 `emit_text_changed`，
新的不会。所以「写值 → 等事件 → 落盘」这条链会**静默失效**。正确写法是
「算出最终值 → 排队写输入框 → 直接用最终值提交」。

**同一帧内 pending 会盖过用户输入。** 排空在构建视图之前，且 `set_value`
无条件替换并 `reset_selection`。所以不要用 pending 去写「用户正在编辑的字段」，
只用于「程序化清空」或「规范化刚收到的值」。

## gpui-pre 有、gpui-ce 也有（用法基本一致）

- `Hsla::from(rgba)` —— 替代 ce 的 `rgb_to_hsla`
- `window.layout_bounds(layout_id)` —— 在 `Element::prepaint` 里拿元素坐标
- `.debug_selector(|| "...".to_owned())` —— 给元素挂调试 ID（release 编译为 no-op）
- `VisualTestContext::debug_bounds("...")` —— 测试里按调试 ID 查坐标

## gpui-pre **没有**（ce 独有，删掉或换实现）

| 缺失能力 | ce 的写法 | 本仓库的替代 |
| --- | --- | --- |
| 元素 prepaint 钩子 | `.on_prepaint(..)` | 自定义 `Element` wrapper（见 `overlays.rs` 的 `PrepaintBounds`） |
| ARIA 属性 | `.aria_live` `.aria_disabled` `.aria_modal` `.aria_hidden` | 无替代，生产代码中已删 |
| ARIA 展开态 | 弹层/折叠触发按钮加 `.aria_expanded(...)` | **禁止添加**：`gpui-pre 0.3.14` 丢弃 AccessKit `Action::Expand`，会导致 Windows UIA 点击失效；保持普通按钮即可 |
| 颜色 trait | `gpui::ColorExt`（`blend` / `fade_out`） | `Hsla::from` + 手动 alpha |
| 颜色函数 | `gpui::rgb_to_hsla(x)` | `gpui::Hsla::from(x)` |
| Enter action | `gpui::Enter` | `gpui_kit::base::input::Enter { secondary, shift }` |
| 元素级样式 | `AnyElement` 可链 `.min_h` / `.bg` 等 | `AnyElement` 无 `Styled`，须包 `div()` |
| 文本替换 | `state.emplace(text, cx)` | `state.set_value(text, window, cx)` |
| 字母间距 | `letter_spacing(px(..))` | 无 |
| RTL 根方向 | `.rtl()` / `.ltr()` | 只有全局 `LayoutDirection` |
| 输入框构造上下文 | `"EditableText"` | `"Input"` |
| 修饰键 | `secondary-` | `ctrl-`（`gpui-pre` 的 `Keystroke::parse` 不认 `secondary`） |
| 可视细滚动条与 UIA `ScrollPattern` | 原生滚动条 + 默认 UIA `Scroll` | **使用 [`flowy-ui/src/scroll.rs`](../crates/flowy-ui/src/scroll.rs) 的 `scroll_area` / `scroll_list_host`**（基于显式着色的 `gpui_kit::base::Scrollbar` + `ScrollA11yHost`），并由 [`patches/accesskit_windows`](../patches/accesskit_windows/FLOWY_PATCH.md) 通过 `[patch.crates-io]` 注入 Windows UIA `IScrollProvider`；`gpui-pre 0.3.14` 本身无补丁 |

系统托盘**不在缺口里**：`gpui-tray 0.3.0` 用 `gpui-kit` feature 桥接类型图，其真包就是
`gpui-pre`，图标 / 菜单类型一致，托盘已恢复。三个平台各有宿主：Windows 通知区域、
macOS 菜单栏、Linux 的 StatusNotifier（需要 session bus 上有 AppIndicator 宿主）。

## 键上下文：按深度取最深匹配，祖先赢不了焦点元素

这条单独拎出来，因为它是本项目踩过最贵的一个坑，症状是「功能写完、编译过、测试过，但按键毫无反应」。

`gpui-base` 给单行和多行编辑器都用同一个 context：

```rust
// gpui-base/src/input/base/state.rs
pub(super) const CONTEXT: &str = "Input";
```

编辑器自己持有焦点，所以**焦点元素上的 context 是最深的**。你在外层容器上再加一个
`.key_context("MyContext")`，得到的是 `[MyContext, Input]`：
`MyContext` 在**祖先**上，深度更浅。

keymap 的派发规则（`gpui-pre/src/keymap.rs` 按 depth 降序，第一个被消费就 return）：

- `Some("Input")` → 深度 2
- `Some("MyContext")` → 深度 1
- `None`（无 context 限制）→ 按最深算，深度 2

所以外层 context 的绑定**永远轮不到**。

### 正确做法

**同深度、后注册者优先**。要覆盖某个按键在输入框里的行为，就绑在同一个
`"Input"` context 上，写在原有绑定**之后**：

```rust
KeyBinding::new("tab", FocusNext, Some("Input")),   // 先注册
KeyBinding::new("tab", InsertIndent, Some("Input")), // 后注册 → 赢
```

handler 里自己判断当前焦点是不是多行框，**不是就兜底执行原本的行为**，否则单行框
的那个键会被静默吞掉：

```rust
let focused = window.focused(cx);
for field in [&about, &instructions, &import_paste, &knowledge_body] {
    let presentation = field.read(cx).presentation();
    if Some(presentation.focus_handle()) == focused.as_ref() {
        field.update(cx, |state, cx| state.insert("    ", window, cx));
        return;
    }
}
window.focus_next(cx);   // 兜底
```

`InputPresentation` 上有 `focus_handle()`、`is_disabled()`、`is_readonly()`、
`is_multi_line()`，够判断。

### 另一个绕过方式

`Enter` 用的是另一条路：不绑键位，而是把「从编辑器冒泡上来的 action」在根元素上
`on_action` 接住再处理。注释里的原话：

> The field's own key context sits on an ancestor, so it loses to the focused
> element on keymap depth.

同一个规则，两种正确写法。



1. **`w_full()` 在 `justify_center` 的行里会塌成内容宽**。改用 `flex_1().min_w_0()`。
2. **`scrollbar_width(px(0.))` 会让滚动彻底失效**，不是隐藏滚动条。新界面与重构视口统一使用 [`flowy-ui/src/scroll.rs`](../crates/flowy-ui/src/scroll.rs) 的 `scroll_area`（支持 `.horizontal()` / `.both_axes()` / `.max_h_viewport()` / `.scrollbar_inset()`）或 `scroll_list_host`（`gpui::list` 宿主），提供仅溢出显示的 4px→6px 主题细胶囊滑块、正常流 `scrollbar_inset` 包装与 Windows UIA `ScrollView` + 合成 `ScrollBar` 暴露；无障碍标签统一使用带「滚动区 / scroll area」区分的 `a11y.scroll_*` 键避免 `AMBIGUOUS_TARGET`，`accesskit_windows` 升级至原生支持 `IScrollProvider` 后即可移除本地 `[patch.crates-io]` 补丁（退出判据见 [`FLOWY_PATCH.md`](../patches/accesskit_windows/FLOWY_PATCH.md)）。
3. **`viewport_size()` 缩窗后不更新**，不要拿它算响应式断点。
4. **`absolute().left().bottom()` 定位不可靠**。提示文字这类叠层要用
   flex 容器 + padding，不要用绝对定位。
5. **`ScrollHandle` 首帧的 `offset()` / `max_offset()` 都是 0**，
   要在 `on_children_prepainted` 之后读。
6. **多行编辑器的根是 `h_full()`**，父容器必须给**确定高度**（`.h(px(..))`），
   给 `min_h` 会让百分比高度解析成 auto，塌成一行。

## 测试

`test_support` 是 opt-in 的，注册表只记录**显式调用**过 `.test_support()` 的元素。

```toml
[dev-dependencies]
gpui-kit = { workspace = true, features = ["test-support"] }
```

```rust
use gpui_kit::base::TestSupportExt as _;

interactive(..).test_support()   // 只能加在链尾
```

`ObservedElement<E>` 在**没开 feature 时等于 `E`**，所以加到生产代码零成本。
`flowy-ui` 用类型别名把这件事藏起来：

```rust
pub type InteractiveDiv = gpui_kit::base::ObservedElement<Stateful<Div>>;
```

查节点：

```rust
use gpui_kit::test::TestQueryExt as _;

window.find_all_by_label("xxx")   // 重名会 panic，用 find_all_*
window.find_all_by_role(Role::Button)
```

**三个坑**：

1. `find_all_by_label` **包含不可见注册**。ce 的 a11y 树只含已绘制节点，
   所以断言写过「必须渲染」的地方要加 `.visible()` 过滤。
2. `find_by_label` 遇重名会 **panic**。
3. 拿不到 label 的元素（`Button::render` 返回 `AnyElement`）用
   `.debug_selector()` + `debug_bounds()` 定位。

## 热补丁

`gpui-pre` 有 `hot-reload` feature（基于 Dioxus subsecond），每个 view 的
`render` 都在 `subsecond::HotFn` 边界里。Windows 启动方式（在 `flowy18_wip/` 下）：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev-hotpatch.ps1
```

**Windows 已跑通（2026-11-26 实测）**：此前 `dx serve --hot-patch` 强制
`-Clinker=dx.exe` 失败（`exit 1` / `LNK1104`），根因是 `dioxus-cli 0.7.10` 在
Windows 下的五处联动缺陷，现已通过脚本与本地 wrapper 全部规避：

1. **响应文件空格路径未加引号**：`dx` 写 `@link_args.json` 时不加引号，`gpui-pre`
   传递的 `C:\Program Files\...\atlmfc\lib\x64` 被链接器按空格拆成 `Files\Microsoft.obj`
   报 `LNK1104` → `dx-link-wrapper` 将含空白行重写到临时 UTF-16LE BOM `.rsp`。
2. **`NoLink` 旁路失效与捕获环境污染**：`dx` 将 `custom_linker` 透传进 fat/thin
   阶段，且 thin 重放把捕获的 `envs` 覆盖在最后 → `dx-capture-wrapper` 过滤临时
   `RUSTC_WRAPPER` / `DX_RUSTC` / `DX_LINK*` / `CARGO_MAKEFLAGS`；`dx-link-wrapper`
   在 `DX_WRAPPER_NO_LINK=1` + `DX_LINK=1` + `@<DX_LINK_ARGS_FILE>` 时写出 20 字节
   COFF 占位桩，恢复 `dx` 原本的 `NoLink` 行为。
3. **MSVC `link.exe` 隐式 `/INCREMENTAL` 与 `LNK4003`**：MSVC `/DEBUG` 默认开增量链接，
   导出 `@ILT` 跳转桩令 subsecond ASLR 偏移错位崩溃，且不认 GNU-ar `libdeps-*.a` →
   `dev-hotpatch.ps1` 优先使用 rustup 自带 `lld-link.exe` 作为 `DX_REAL_LINKER`，
   仅在缺失时回退 MSVC `link.exe` 并由 `dx-link-wrapper` 追加 `/INCREMENTAL:NO`。
4. **x64 `stub.o` 的 `__chkstk` ABI 破坏**：`dx 0.7.10` 给所有未定义函数（含
   `__chkstk`）生成 `movabs rax, addr; jmp rax`，踩毁 Windows x64 栈探测入参
   `RAX` 导致热补丁生效即栈溢出 → `dx-link-wrapper` 在 `/DLL` 补丁链接时复制
   `stub.o` 并将 `__chkstk` 等符号由 `EXTERNAL` 降为 `STATIC`，改由 `msvcrt.lib`
   原生 `chkstk.obj` 解析。
5. **1-hop 文件监听范围**：`dx 0.7.10` 忽略 `[web.watcher] watch_path`，只递归监听
   `flowy-desktop` 的直接依赖 → `crates/flowy-desktop/Cargo.toml` 直接声明
   `flowy-studio` / `flowy-ui` / `flowy-i18n`（均已是 `flowy-app` 传递依赖），使
   修改这些 UI crate 也能自动触发拓扑级联热补丁。

**边界不变**：渲染、布局、文案与绘制改动可原位热补丁（进程 PID 与窗口/登录状态保持）；
修改 struct 字段、静态初始化器或依赖增删仍需按 `r` 重建或重启。

## 动效原语与无障碍总闸（2026-11-26 实测增补）

1. **严禁硬编码 `cx.set_reduce_motion(true)`**：
   - `flowy-app` 早期曾在窗口启动时硬编码 `cx.set_reduce_motion(true)`，导致所有 `with_spring`、旋转动画与过渡在开发机上全部静默跳过。
   - 正确做法是按系统/主题的 `MotionPolicy` 驱动（`Full` 启用平滑变换，`Reduced` 降级为 0 位移直达态）。
2. **`Icon` 零重排 GPU 变换（`with_transformation` / `with_timed_transformation`）**：
   - `gpui-pre 0.3.14` 原生支持在 `svg()` 上挂 `Transformation::rotate(radians)` 与 `with_animation(..., Animation::new(dur).repeat(), ...)`，完全走 GPU 变换而不触发布局抖动。
   - 仓库已通过 `flowy-ui/src/icons.rs` 封装透出，用于 `+ → ×`（45° 旋转）与 `loader_circle` / `refresh_cw` 持续旋转。
3. **叶级 `with_spring` 与 `#[gpui::test]` 多帧断言**：
   - 浮层（Popover / Modal）与右侧抽屉的弹簧入场过渡仅挂在**叶级内容节点**（`flowy-ui/src/{motion,overlays}.rs`），绝不包裹外层布局壳，避免破坏 `flex_1().min_w_0()` 收缩或圆角裁切。
   - 在 `#[gpui::test]` 中验证动效与布局边界时，必须显式调用 `window.draw(cx).clear(cx)` 并推进测试时钟后，再断言 `debug_bounds()` 或状态变化；跳过绘制调用会让 `debug_bounds()` 断言失真。

## 相关文档

- `DESIGN.md` —— 界面规则与布局陷阱
- `BLOCKED.md` —— 已验证不可行的方向与能力缺口对照表
- `PROGRESS.md` —— 迁移进度与遗留清单
