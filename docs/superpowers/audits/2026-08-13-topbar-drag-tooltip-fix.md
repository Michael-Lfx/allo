# 顶部栏拖拽与 Tooltip 交互修复：实施与验证记录

## 基本信息

- 日期：2026-08-13
- 分支：`feat/topbar-drag-style-fix`
- 范围：Windows/Linux 无边框窗口的标题栏命中区域，以及公共 `InstantHoverTooltip` 的视口边界保护。
- 关联说明：[`桌面应用开发指南`](../../guides/desktop-app.md) 与 [`前端架构说明`](../../architecture/frontend.zh.md)。

## 问题确认

修复前的桌面 Debug 实例可以复现：标题栏左右空白区域无法拖动，只有中间标题区域能够拖动；左侧和右侧边缘按钮的 Tooltip 可能被窗口边界裁切。两者都属于前端标题栏/浮层命中与定位问题，不涉及后端或 Rust 逻辑。

## 实施内容

### 标题栏拖拽区域

- 保留标题栏根节点的 `data-tauri-drag-region` 和 `-webkit-app-region: drag`，让菜单列与工具栏列的空白部分成为连续拖拽平面。
- 移除菜单列、工具栏列的整块 `no-drag` 声明；导航、新建会话、主页、更新、移动端操作插槽、窗口控制和 Tooltip 锚点继续作为显式 `data-tauri-no-drag` 交互岛。
- 双击最大化只接受位于拖拽区域、且自身及祖先不属于 `data-tauri-no-drag` 的事件目标，按钮双击不会误触发最大化。
- macOS 通过专用 CSS 规则继续让菜单列和工具栏列处于 `no-drag`，保留 Overlay 标题栏行为；Web 标题栏不启用桌面拖拽。
- 不新增全局鼠标拖拽逻辑；继续依赖 Tauri/WebView 原生窗口拖拽机制。

### 公共 Tooltip 定位

- `InstantHoverTooltip` 复用现有 `@floating-ui/react`，使用 `fixed` 策略、`offset(6)`、`flip({ padding: 8 })`、`shift({ padding: 8 })` 和 `autoUpdate`。
- Tooltip 继续通过 Portal 渲染到 `document.body`，并使用 Floating UI 的 `isPositioned` 状态在首次定位完成前保持隐藏，避免从左上角闪现。
- 最大宽度限制为 `calc(100vw - 16px)`；短文本保持单行，超长中文、英文和本地化文本允许换行。
- 保留现有延迟、主题语义、字号、圆角、阴影和高层级；允许覆盖普通内容，但不允许自身超出视口。

## 自动化验证

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| 聚焦 Bun 测试 | 14/14 通过，73 个断言 | 覆盖 Tooltip、Titlebar、更新按钮和窗口控制结构契约 |
| `bun run check:theme` | 通过 | 未引入主题语义回归 |
| `bun run check:icons` | 通过 | 未引入图标规则回归 |
| `bun run check:browser-platform-boundary` | 通过 | 未破坏浏览器/桌面平台边界 |
| `bun run build:ui` | 通过 | Vite 构建完成；保留既有动态导入和大 chunk 警告 |
| `git diff --check` | 通过 | 无空白错误 |
| `bun run typecheck` | 基线阻塞 | 未修改的 `ReasoningEffortSelector.tsx`、`ChatConversation.tsx` 和 `VideoHomeComposer.tsx` 仍有既有类型错误，未出现本次变更文件错误 |
| `bun run check` | 基线阻塞 | 在上述 `typecheck` 错误处停止，未进入后续质量门 |

## Windows 实机验收边界

本轮尝试启动 `bun run dev` 时，Vite 已启动于 `http://127.0.0.1:5173`，但 Tauri 首次 Rust 编译在未修改的 `aws-lc-sys v0.43.0` MSVC 构建阶段失败，未能打开当前分支的桌面窗口。因此自动化结果不等同于当前分支的 Windows 原生命中验收。

待构建环境恢复后，需要在普通/最大化窗口、最小宽度/宽屏以及 Windows 100%/125%/150% DPI 下验证：

- 左侧、右侧和中间空白区域拖拽；
- 空白区域双击最大化/还原；
- 最小化、最大化、关闭、更新和导航按钮点击不触发拖拽；
- 左侧第一个和右侧最后一个 Tooltip 的完整显示与自动翻转；
- 长中文、长英文、亮色/暗色/半透明主题及键盘 focus 场景。

## 关联文件

- [`InstantHoverTooltip.tsx`](../../../ui/src/renderer/components/base/InstantHoverTooltip.tsx)
- [`Titlebar/index.tsx`](../../../ui/src/renderer/components/layout/Titlebar/index.tsx)
- [`Titlebar/titlebar.css`](../../../ui/src/renderer/components/layout/Titlebar/titlebar.css)
- [`WindowControls.tsx`](../../../ui/src/renderer/components/layout/WindowControls.tsx)
- 对应的 Tooltip、Titlebar、更新按钮和窗口控制测试文件

## 非目标

本次不改变标题栏高度、按钮视觉、Tooltip 延迟、主题语义、macOS 原生标题栏、Web 行为、后端/Rust 逻辑或依赖清单。Windows 实机验收完成后，应把结果补充到本记录，而不是用浏览器测试替代原生窗口验证。
