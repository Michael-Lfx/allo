# 前端

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6`

前端是位于 [`ui/`](../../ui/) 的一个 React 19 SPA。两个宿主 —— Tauri 桌面外壳与 `nomifun-web` —— 都加载同一份 Vite 构建产物（`ui/dist`）。渲染进程从不使用 Electron IPC；在两个宿主中它都通过普通的 HTTP 与 WebSocket 与后端通信。

## 技术栈

| 关注点 | 选择 |
| --- | --- |
| 框架 | React 19 + TypeScript |
| 打包工具 | Vite 6 |
| UI 库 | Arco Design（`@arco-design/web-react`）—— 主色 `#4E5969` |
| 样式 | UnoCSS（utility 类）+ `ui/src/renderer/styles/themes/` 下按主题划分的 CSS |
| 路由 | `react-router-dom` v7 + **`HashRouter`**（对 `file://` 风格宿主与刷新安全至关重要） |
| 数据获取 / 缓存 | SWR |
| 状态 | React Context（auth、theme、feedback、preview、conversation history）—— 不使用 Redux |
| i18n | `i18next` + `react-i18next`，语言包 `zh-CN`、`en-US` |
| 编辑器 | Monaco（设置、代码预览）、CodeMirror（更轻量的输入） |
| Markdown | `react-markdown` + `remark-gfm` + KaTeX + mermaid |
| 终端 | `xterm.js`（含 `@xterm/addon-fit`、`@xterm/addon-web-links`、`@xterm/addon-webgl`） |
| Service worker | Web 宿主注册了 PWA service worker（参见 [`registerPwa.ts`](../../ui/src/renderer/services/registerPwa.ts)）；Tauri 外壳显式跳过它 |

## 三层结构：`common/`、`platform/`、`renderer/`

`ui/src/` 内的目录划分是承担约定职责的关键。

```
ui/src/
├── common/      shared library code (no React)
│   ├── adapter/            the bridge factory: HTTP + WS + Tauri shim
│   │                       （httpBridge / ipcBridge / tauriShell / tauriRuntime / browser）
│   ├── browser/            browser-use 相关共享逻辑
│   ├── chat/               chat library helpers (rendering hooks, types)
│   ├── config/             constants, configService (settings cache), i18n-config
│   ├── protocolBindings/   协议绑定辅助
│   ├── types/              TypeScript mirrors of nomifun-api-types DTOs
│   ├── update/             self-update flow helpers
│   ├── utils/              shared utilities (date, hash, ...)
│   └── index.ts
├── platform/    runtime substrate（仅两件事）
│   ├── bridge event hub（pub/sub + RPC 的 host 桥）
│   └── theme tokens
└── renderer/    the React app
    ├── pages/         feature pages (conversation, terminal, settings, ...)
    ├── features/      feature-scoped modules
    ├── components/    reusable UI components and layout
    ├── hooks/         hooks and React Contexts (Auth, Theme, Feedback, ...)
    ├── services/      i18n, FileService, PasteService, SpeechToTextService, registerPwa
    ├── styles/        Arco overrides and theme variables
    ├── utils/         renderer-specific utilities
    └── main.tsx       entry point (createRoot)
```

这种划分是有意设计的：`common/` 不知道 DOM 或 React 的存在；`platform/` 只承载宿主桥与主题 token 这一小块基板；`renderer/` 才是真正的应用。这让桥接逻辑可以脱离 React 进行测试，并且如果将来出现第二个客户端目标，可以共享 `common/`。

## 适配层（桥接层）

文件位置：[`ui/src/common/adapter/`](../../ui/src/common/adapter/)。

适配层是前端可移植性故事的核心。它对外暴露一个稳定的形状 —— `provider/invoke` 用于请求—响应，`on/emit` 用于事件 —— 渲染进程的其余部分都消费这个形状。该形状之下，它根据宿主把调用路由到三种传输之一：

| 适配文件 | 传输 | 用途 |
| --- | --- | --- |
| [`httpBridge.ts`](../../ui/src/common/adapter/httpBridge.ts) | HTTP `fetch` + 单例 WebSocket | 默认 —— 所有 `/api/*` 与 `/ws` 流量。 |
| [`tauriShell.ts`](../../ui/src/common/adapter/tauriShell.ts) | Tauri JS API 与插件（`@tauri-apps/api`、`tauri-plugin-*`） | 仅用于操作系统外壳：窗口控制、对话框、OS 路径、开机启动、通知、深链接、自更新。由 `isTauriRuntime()`（定义于 [`tauriRuntime.ts`](../../ui/src/common/adapter/tauriRuntime.ts)）守护。 |
| [`browser.ts`](../../ui/src/common/adapter/browser.ts) | 进入 platform 事件中心的旧版 WebSocket 桥接 | 把 `platform/` 的 `bridge.emit` 调用接到同一个 `/ws` 端点，并处理 auth 过期重定向。 |

复合体 —— 由 [`ipcBridge.ts`](../../ui/src/common/adapter/ipcBridge.ts) 导出 —— 才是应用其余部分引入的对象。在渲染进程看来，每次操作都长得一样，无论它最终走的是 HTTP、WS 还是 Tauri-IPC。

### 解析后端 URL

渲染进程需要知道与之对话的 URL，而答案因宿主而异：

```ts
// ui/src/common/adapter/httpBridge.ts (excerpted)
function getBackendPort(): number {
  if (typeof window !== 'undefined' && window.__backendPort) {
    return window.__backendPort;             // desktop (Tauri): injected by init script
  }
  return globalThis.__backendPort ?? 13400;  // last-resort fallback
}

function isWebUiBrowserMode(): boolean {
  return typeof window !== 'undefined' && !window.__backendPort;
}

export function getBaseUrl(): string {
  if (isWebUiBrowserMode()) return '';                              // same-origin (browser)
  return `http://127.0.0.1:${getBackendPort()}`;                    // desktop
}
```

在桌面外壳中，Tauri 主进程在任何页面脚本执行之前通过**初始化脚本**注入 `window.__backendPort`（参见 [`apps/desktop/src/main.rs`](../../apps/desktop/src/main.rs)）—— 因此渲染进程的第一次调用就能看到正确的端口，无竞争。在 Web 宿主中不会注入端口；`getBaseUrl` 返回 `''`，`fetch` 把 URL 解析到页面自身的来源。

### CSRF 双提交

当宿主以认证模式运行（即未带 `--insecure-no-auth` 的 Web 宿主），后端会签发非 HttpOnly 的 cookie `nomifun-csrf-token`。在状态变更请求（POST / PUT / PATCH / DELETE）上，桥读取该 cookie 并把它回显到 `x-csrf-token` 头里。桌面外壳使用 `TrustLocalToken`：WebView 会在请求中带上 `window.__nomiLocalTrust` 注入的本地信任 secret，而不是关闭所有鉴权。

## 路由 —— `HashRouter`

[`ui/src/renderer/components/layout/Router.tsx`](../../ui/src/renderer/components/layout/Router.tsx) 是唯一的路由组件。它使用 **`HashRouter`**（形如 `/#/conversation/abc123` 的 URL），原因有两个：

1. Tauri 外壳通过 `tauri://` / `file://` 协议加载 SPA；`BrowserRouter` 在该协议下经历的页面重新加载（如深链接或应用内导航）后无法保留状态。
2. Web 宿主通过 `tower_http::services::ServeDir` 提供 SPA，并启用 `append_index_html_on_directories(true)`。Hash 路由意味着浏览器访问的任何路径都返回 `index.html`，由 SPA 完成其余工作 —— 静态服务器无需自定义 catch-all。

路由表的顶层条目涵盖会话运行时（`/guid`、`/conversation/:id`）、模型（`/models`）、设定（`/presets`）、技能（`/skills`）、MCP（`/mcp`）、开放能力（`/open-capabilities`）、终端（`/terminal-new`、`/terminal/:id`）、需求/AutoWork（`/requirements/*`、`/autowork` redirect）、定时任务（`/scheduled`、`/scheduled/:cron_job_id`）、桌面伙伴（`/nomi` 配置页、`/companion` 桌面窗口）、知识库（`/knowledge`、`/knowledge/:id`）、开发者模式评测（`/eval`）、云登录（`/cloud-login`）、浏览器管理（`/browser`）、客服域（`/customer-service*`）、学习引擎（`/learn`、`/learn/:id`）、视频生成与 Canvas 模式（`/video-generation*`，含 `/video-generation/briefing/:id` 资讯播报工作台）、计费（`/billing`），以及认证（`/login`）。旧 settings 路径只作为重定向保留。Agent 协作不建立独立路由或单独页面；AgentExecution 投影直接显示在所属 Conversation 内，避免导航层再产生一个产品对象。开发者模式开启时，会话列可滑到会话日志（`AgentTraceInspector`）：采集始终写盘，List/Turn/Call HTTP 仍要开发者模式。详见 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md)。

页面通过 `React.lazy` 加载，使用 `<AppLoader>` 作为 fallback，使初始包保持精简。

## 状态与数据

- **SWR** 是主要的数据层。约定是任何列表或详情视图都声明一个 SWR key 字符串及一个 fetcher；HTTP 响应到达后变更操作会调用 `mutate(key)`。`ipcBridge.*.invoke` 的返回值直接喂给 SWR。
- **React Context** 承载不属于 SWR 的应用形态状态：认证（`AuthProvider`）、主题（`ThemeProvider`）、反馈 toast（`FeedbackProvider`）、文件预览（`PreviewProvider`，位于 `pages/conversation/Preview/context/PreviewContext.tsx`），以及对话历史列表（`ConversationHistoryProvider`）。
- **`configService`**（`ui/src/common/config/configService.ts`）缓存后端设置；[`main.tsx`](../../ui/src/renderer/main.tsx) 中的入口点会在 i18n / theme 代码加载前启动 `configService.initialize()`，因此这些子系统在首次渲染时读到的是权威设置。

## 应用内通知

渲染进程通知由独立模块
[`ui/src/renderer/components/notifications`](../../ui/src/renderer/components/notifications/) 负责。
[`NotificationHost`](../../ui/src/renderer/components/notifications/NotificationHost.tsx)
只在 `Layout` 中挂载一次，并 Portal 到 `document.body`。模块级 store 会在 Host
挂载前缓存早期通知；它不修改后端数据，也不接管原生系统通知、权限或权限请求。

运行时 Arco `Message` / `Notification` 统一通过 `AppMessage`、
`AppNotification` 门面调用。`useArcoMessage` 的每个实例拥有独立 scope 和
`maxCount` 策略，静态调用使用共享 scope。新代码应使用稳定的
`appNotifications.show()` 门面，它返回带有 `dismiss()` 和 `update()` 的 handle；
Arco 形状的门面仅用于兼容迁移。同一 scope 内重复的活动 `id` 会原地更新。
返回的 `handle.update()` 是部分更新：未传入的 `title`、`icon`、`action`、
`onClose`、`announce` 等可选字段保持不变，明确传入的值才会替换原值。

通知固定在右下角：桌面端距边 24px，窄屏距边 16px，并叠加安全区。收起态最多
展示 3 张临时通知，最新通知位于底部；计数控制器显示因收起而隐藏的活动临时通知数量，文案为“还有 N 条通知”。
`duration: 0` 的持久通知始终保留；持久通知超过临时通知上限时，卡片区域改为有界滚动。
计数卡展开全部活动/退出中的通知，最新通知仍在底部；点击外部或在通知区域按 Escape
会收起，若收起前焦点在通知区域，Escape 会将焦点恢复到计数按钮。

通知在悬浮、获得焦点或展开期间暂停计时。只有会话页的 `SendBox` 和已打开的
`MobileActionSheet` 注册真实 DOM 避让元素。居中的 `/guid` 首页
`GuidInputCard` 不注册为底部遮挡物，通知会继续贴近视口右下角，不会被抬到首页输入卡顶部。
共享的 `ComposerSurface` 默认不注册通知避让，只有底部固定的调用方可以显式开启。
`ResizeObserver`、窗口/Visual Viewport 缩放、后代滚动和操作面板过渡状态会更新已注册
避让元素的位置。只有关闭按钮、调用方提供的操作按钮和计数控制可交互。
`passthrough` 通知保持点击穿透。独立的 polite/assertive live region 负责播报通知内容，
普通通知进入按创建时间排序的队列；相同 revision 去重，待播报的更新只保留最新版本，
轮到播报前已关闭的通知会被移除；错误通知进入 assertive 通道，优先打断当前正在展示的
polite 播报，但 assertive 通知自身仍按创建时间排序；展开历史不会重复播报。
入场、收起和堆叠位移只使用透明度/位移动画：空间 FLIP 位移为 240ms，入场 180ms、退出
120ms，并支持 reduced-motion。通知主色来自当前主题的 `--primary`，成功、警告、错误和信息
分别使用主题语义色，并通过低强度 tint 表达，不使用渐变或模糊。通知卡片和计数控制器统一以
不透明的 `--flowy-surface-1` 作为混色底，带透明度的主题 popup token 不得作为通知覆盖面，避免
页面内容穿透。

通知模块行为和语言包的核心契约由以下测试覆盖：

```text
bun test --cwd ui src/renderer/components/notifications
bun test --cwd ui src/renderer/services/i18n/notificationsLocales.test.ts
```

## 主题

Arco 的 `ConfigProvider` 在根处包裹应用，主色为 `primaryColor: '#4E5969'`，并按语言提供 locale（仅 `zhCN`、`enUS` 两种，见 `main.tsx` 的 `arcoLocales` map）。主题（`light`、`dark`、品牌变体）以纯 CSS 文件叠在 `ui/src/renderer/styles/themes/index.css` 中，通过 `ThemeProvider` 切换。

UnoCSS 与 Arco 并行提供 utility 类 —— 其配置位于 [`ui/uno.config.ts`](../../ui/uno.config.ts)。Arco 的自定义覆盖位于 `ui/src/renderer/styles/arco-override.css`。

## 国际化

[`ui/src/renderer/services/i18n`](../../ui/src/renderer/services/) 用 `zh-CN` 与 `en-US` 两种语言初始化 `i18next`（语言清单见 [`ui/src/common/config/i18n-config.json`](../../ui/src/common/config/i18n-config.json)）。字符串按功能组织，解析后的语言通过 `main.tsx` 中的 `arcoLocales` map 流入 Arco。切换语言无需重新加载 —— i18next 与 Arco 都会按新语言重新计算。

## 桌面端对话流式呈现

桌面端的活跃助手回复会被拆成稳定的 Markdown 前缀与轻量的末尾区块。拆分逻辑位于
[`MessageText.tsx`](../../ui/src/renderer/pages/conversation/Messages/components/MessageText.tsx)：完整段落和已经闭合的围栏代码块通过 `MarkdownView` 渲染；最后仍在生成的段落保持纯文本。尚未闭合的围栏代码块会先显示为轻量代码预览，直到闭合后再进入完整 Markdown，避免把围栏标记暴露给用户，也避免每个 token 都完整解析 Markdown。

Markdown 在 Shadow DOM 中渲染，因此消息排版和代码控件具有明确的局部样式约定：

- 消息正文保持 `14px / 22px`；标题使用克制的 `18 / 16 / 15 / 14px` 层级；
- 链接使用主题派生的常态、hover 与键盘焦点颜色；
- 桌面端代码操作在 Shadow DOM 内通过 hover 或键盘焦点显示；
- 过程行文字使用中性的可读颜色，运行、等待、失败和取消状态主要由图标承载；reduced-motion 会关闭相关动画。

### 当前范围与后续项

本轮已经更新：

- 稳定前缀 / 末尾区块渲染，以及未闭合代码的轻量预览；
- Markdown 标题层级、主题链接和 Shadow DOM 代码工具栏状态；
- 具备键盘焦点、关联详情区和桌面端有界滚动的语义化 `MessageThinking` 开关；
- Mermaid 的 Shadow DOM 工具栏和预览 / 原文分段控件；
- 代码 footer 状态、过程状态对比度、主色焦点环与仅图标的实时步骤动效；
- 分段器、Markdown 排版、思考控件、Mermaid/代码工具栏和过程布局的聚焦测试。

本轮没有更新：

- 移动端触控热区、安全区间距和窄屏披露区布局；
- 跨桌面主题的视觉截图覆盖（当前只有源码/行为测试和常规前端质量门）；
  计划中的 1440x900、1280x800 浅色/暗色视觉验收尚未记录。

## 一点平台特定的 UX

桌面外壳在 Windows / Linux 上是**无边框**的（[`ui/src/renderer/components/layout/Titlebar/`](../../ui/src/renderer/components/layout/Titlebar/) 中的 React 标题栏通过 `@tauri-apps/api/window` 绘制最小化 / 最大化 / 关闭按钮）；macOS 通过 `TitleBarStyle::Overlay` 保留原生交通灯按钮。同一份 SPA 在浏览器中会隐藏标题栏，让浏览器外框处理它。区别在运行时通过 `isTauriRuntime()`（定义于 `common/adapter/tauriRuntime.ts`）来检测。

Windows/Linux 标题栏的空白菜单列与工具栏列属于原生拖拽平面，按钮、窗口控制和 Tooltip 锚点属于显式 `no-drag` 交互岛；公共 `InstantHoverTooltip` 通过 Portal 与 Floating UI 的 `flip`/`shift` 保证视口边界内显示。实现与验证边界见 [`顶部栏拖拽与 Tooltip 修复记录`](../superpowers/audits/2026-08-13-topbar-drag-tooltip-fix.md)。
