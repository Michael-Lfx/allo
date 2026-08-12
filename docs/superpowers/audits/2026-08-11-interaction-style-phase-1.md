# 交互样式优化：第一阶段验证记录

## 交付边界

本记录只覆盖全局视觉基线、应用壳、主侧栏和首页首屏。对话、聊天记录、设置、
定时任务以及视频模块专项样式均未进入实施。

## 已实现

- 新增 Canvas、Panel、Raised、Interactive 表面及正文、边界、阴影、焦点和选择状态语义层。
- 旧主题变量继续作为新语义层的来源，未删除旧 token，未改变自定义 CSS 加载与恢复路径。
- 标题栏、应用画布和侧栏统一层级；主入口、模型入口和设置入口共享一个移动选中指示器。
- 首页标题与 Composer 收紧为一个首要任务区，推荐任务改为三列入口。根据第一轮实机反馈，
  Composer 聚焦改为仅显示边界与结构阴影，位置始终稳定。
- Hover、Press、Focus 和 Selected 只以即时颜色变化与 `transform`/`opacity` 动效反馈。
- Reduced Motion 下移除位移与过渡，但保留可见的状态颜色、边界和焦点环。

## 自动化证据

| 检查 | 结果 | 边界 |
| --- | --- | --- |
| `bun run check:theme` | 通过 | 六套内置主题 Light/Dark；正文对 Canvas、Panel、Interactive、Raised 均 >= 4.5:1 |
| 四组聚焦 Bun 测试 | 9/9 通过 | token 映射、120/180/240ms、Reduced Motion、侧栏指示器、首页结构与 Composer 行为接缝 |
| i18n / icons | 通过 | 未新增裸用户文案或图标导入问题 |
| CodeMirror / process / browser boundary | 通过 | 未破坏既有前端运行边界 |
| `bun run help --check` | 通过 | 根脚本与文档入口一致 |
| `bun run dev:web` | 通过 | `nomifun-web` 启动于 8787，Vite 启动于 5173，首页开发版完成加载 |
| 1440×900 浏览器冒烟 | 通过 | 首页、单一活动入口、侧栏指示器移动、设置入口、折叠/展开、Composer contenteditable 键入并清空 |
| 1280×800 布局冒烟 | 通过 | Composer 可见、单一活动入口、无横向溢出 |
| `bun run typecheck` / `bun run check` | 基线阻塞 | 未修改的 reasoning effort 类型漂移、视频画布依赖和 Blob 类型错误 |
| `bun run build:ui` | 基线阻塞 | 11,582 个模块完成转换后，视频画布缺少 `@mediapipe/tasks-vision` |
| `bun run check:agent-vocabulary` | 基线阻塞 | 未修改的两个 Rust 注释仍包含退休词汇引用 |

## 人工体验检查点

自动化不能证明视觉喜好、动画手感、主题气氛或桌面 WebView 性能。进入第二阶段前，
仍需在开发版完成以下人工矩阵：

- 六套主题的 Light/Dark，1440×900 与 1280×800。
- 侧栏入口切换、底部设置入口、折叠态和滚动区稳定性。
- 推荐任务 Hover/Press/Focus，Composer 聚焦、输入、粘贴、模型和工作区选择。
- Reduced Motion、键盘焦点、弹层可读性、无布局跳动和可打断反馈。

用户明确确认第一阶段风格前，不进入第二阶段。

## 第一轮反馈修正（阶段 1.1）

- 移除 Composer 在 Hover、Focus、Active 下的纵向位移，保留主题焦点边界和结构阴影。
- 账号入口固定用户名与套餐两行行盒；套餐缺失或加载时保留不可见占位，展开态 Footer 固定为 40px。
- 积分刷新按钮不再显示 Hover Tooltip；无障碍名称、键盘操作、加载状态和冷却倒计时保持不变。
- 本轮不修改输入、粘贴、发送、模型选择、工作区选择或积分刷新业务逻辑。

### 阶段 1.1 验证

| 检查 | 结果 | 边界 |
| --- | --- | --- |
| 1.1 聚焦 Bun 测试 | 12/12 通过 | Composer 稳定聚焦、账号双行槽位、固定 Footer、积分按钮无 Tooltip 与键盘契约 |
| `bun run check:theme` | 通过 | 六套内置主题 Light/Dark 均通过语义颜色与对比度契约 |
| `bun run check:i18n` / `check:icons` | 通过 | 未引入裸文案或非法图标导入 |
| Composer 开发版采样 | 通过 | Focus 前后 top 与 height 差值均为 0px |
| 首页 → 设置 → 首页三次循环 | 通过 | 账号、套餐、展开 Footer 的 top 与 height 三次完全一致，差值 0px |
| `bun run typecheck` / `bun run check` | 基线阻塞 | 仍为 reasoning effort、视频画布依赖和 Blob 类型错误 |
| `bun run build:ui` | 基线阻塞 | 9,575 个模块转换后仍因缺少 `@mediapipe/tasks-vision` 停止 |

阶段 1.1 已停在独立体验检查点，尚未进入顶部栏阶段 1.2。

### 设置返回首页瞬态修正

首轮几何采样只在导航稳定后执行，因此没有捕捉到过渡帧。逐帧检查确认，全局选中指示器
在设置页会定位到 Footer 设置按钮；返回首页时，指示器先切换为首页入口宽度，再从 Footer
移动到顶部，经过侧栏底边裁切后形成横条。Footer 设置按钮现改为静态选中态，不再参与跨区域
指示器移动；主导航内部的选中移动效果保持不变。

## 顶部栏收敛（阶段 1.2）

本阶段只收敛应用顶部栏，不进入设置页主体改造。桌面顶部栏现划分为全局导航、
会话上下文、居中标题与平台控制四项职责，并以三列 Grid 保证上下文标题不受左右按钮数量影响。

- 全局导航组固定承载侧栏、后退和前进；新建对话作为全局桌面快捷入口，在设置及其他模块中也保持可见。
- 新建对话采用气泡加号图标，直接组合“对话”与“新建”语义，替代语义过宽的单独加号；
  点击仍进入首页并重置预设。
- 中央标题由单一路由解析 Hook 提供；对话名称异步加载具有取消保护，旧请求不会覆盖新页面标题。
- 顶部栏语言按钮已删除，快速语言切换迁移到账户菜单，并继续复用既有语言持久化与双 RAF 同步流程。
- 应用按钮统一为 32x32px 点击区，Hover 使用语义 Interactive 表面，Press 只缩放不位移；
  Reduced Motion 下取消缩放和过渡。
- 顶部栏应用按钮的鼠标 Tooltip 延迟为 400ms；键盘 Focus 立即显示，离开、失焦和卸载均取消待显示计时器。
- Windows/Linux 系统窗口控制、macOS 交通灯安全区、Web 无窗口控制和移动端 48px 布局继续保持平台边界。
- 设置 Footer 的瞬态横条问题确认了一项结构规则：移动指示器只能在同一语义容器内工作，
  不允许借动画跨越主导航、Footer、顶部栏或其他区域。

### 路由显隐与标题矩阵

| 路由类别 | 中央标题 | 新建对话（桌面） |
| --- | --- | --- |
| 首页 `/guid` | Flowy | 显示 |
| 对话 `/conversation/:id` | 会话名称，加载或失败时为“会话” | 显示 |
| 终端 `/terminal-new`、`/terminal/:id` | 终端 | 显示 |
| 设置、知识库、学习、定时任务、视频生成 | 对应模块名称 | 显示 |
| 模型、MCP、设定、技能、桌面伙伴、工作区 | 对应模块名称 | 显示 |
| 未识别路由 | Flowy | 显示 |

### 阶段 1.2 验证

| 检查 | 结果 | 边界 |
| --- | --- | --- |
| 顶部栏相关聚焦 Bun 测试 | 28/28 通过 | 全局新建对话、标题解析与竞态取消、语言迁移、Tooltip 延迟和既有 1.1 契约 |
| `bun run check:theme` | 通过 | 六套内置主题 Light/Dark 继续满足语义颜色与对比度契约 |
| `bun run check:i18n` / `check:icons` | 通过 | 中英文标题成对存在，图标导入符合规则 |
| Web 路由矩阵 | 通过 | 12 类核心路由标题和桌面中央标题几何偏差不超过 0.01px；后续反馈将新建对话改为全局桌面入口 |
| Tooltip 开发版检查 | 通过 | 200ms 不显示、超过 400ms 显示、提前移开取消、Focus 立即显示 |
| 账户语言菜单 | 通过 | 原生语言名称和当前项 Check 正确；选择当前语言只关闭菜单 |
| 800px / 720px 收敛检查 | 通过 | 应用在既有移动断点切入移动结构，无横向溢出；移动结构优先于桌面 720px 标题隐藏规则 |
| CodeMirror / process / browser boundary / `help --check` | 通过 | 未破坏前端运行边界和根脚本入口 |
| `bun run typecheck` / `bun run check` | 基线阻塞 | 仍为 reasoning effort、视频画布依赖及 Blob 类型错误，未出现顶部栏新增错误 |
| `bun run build:ui` | 基线阻塞 | 9,938 个模块转换后因缺少 `@mediapipe/tasks-vision` 停止 |
| `bun run check:agent-vocabulary` | 基线阻塞 | 未修改的两个 Rust 注释仍包含退休词汇引用 |

Web 开发版已完成结构和交互验证；Windows 桌面 WebView 的拖拽、双击最大化及窗口按钮，
以及 macOS 76px 交通灯安全区仍属于目标平台人工验收边界，不能由浏览器自动化替代。

阶段 1.2 在此停止，等待用户体验确认后才进入设置阶段 2A。

设置阶段的增量范围、实施结果与独立体验门槛记录在
[`2026-08-11-settings-ui-ux-phase-2.md`](./2026-08-11-settings-ui-ux-phase-2.md)。本记录不被覆盖，
仍是第 0/1 阶段的历史决策与验证边界。

首页与设置页的最终收敛证据见
[`2026-08-12-home-settings-visual-contract-phase-2d.md`](./2026-08-12-home-settings-visual-contract-phase-2d.md)。
