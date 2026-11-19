# Flowy 本机助理 · 开源借鉴证据

> **状态**：已核对公开源码和文档，借鉴结论待实现验证\
> **核对日期**：2026-11-19（香港时间）\
> **Flowy 基线**：`codex/flowy18-history-rendering`，`8aae3a5fb`\
> [产品与架构方案](local-agent-design.zh.md) · [执行方案](local-agent-execution.zh.md)

## 证据与采用范围

下表的实现事实来自公开资料。对 Flowy 的用法是设计推断。上游运行表现、耗时和资源开销需要分别在目标 Windows 机器验证。

| 来源与基线 | 已核对的事实 | 对 Flowy 的设计启发 |
| --- | --- | --- |
| DeepSeek Super-agent，`8c32a9dc` | 原文把统一人机交互交给 Router，将执行交给上下文隔离的 Worker。已绑定的后续事件可直达对应 Worker。原型面向单一 GitHub 仓库，允许的请求生成工单和独立会话，结果回到原对话。 | 首页只有一个入口。续办按稳定任务 ID 直接定位。Router 汇总待确认项和结果。首版从用户主动提交开始，事件订阅留作后续扩展。 [设计原文](https://mp.weixin.qq.com/s/zoS7GlReWPhfCjENGxXCdg) · [原型 README](https://github.com/deepseek-ai/dsh-superagent-prototype/blob/8c32a9dc5d72c033816c6a8110b37f40cf94fba4/README.md#L3-L12) |
| Agent Autoloop，`ca5c0ffe` | 按统一事件接单，来源、执行器、通知和代码交付有独立插件位。等待决定时可释放 Agent 进程，状态留在存储里，回答后续办。README 将整机断电列为尚未单独测试的场景。 | 等待、执行和交付是持久状态。退出或中断后先重建待处理事项，再决定哪些操作可继续。MVP 用一个内置能力组验证这个机制。 [作者原文](https://mp.weixin.qq.com/s/T9_acg247gxzUNL4j5zrCw) · [仓库 README](https://github.com/KKKKhazix/agent-autoloop/blob/ca5c0ffe2c92be17502c4f1a75cdb71823eeafef/README.md#L149-L198) |
| Orca，`44b0d8c9` | 公开编排文档区分 Run、Task 和 Dispatch。Task 有状态和所有者，一次 Dispatch 拥有本次完成的确认权。执行器支持在独立 worktree 启动，决策门由协调者处理。 | 用户看到稳定的 Task；每次执行、重试或续办有新的 Attempt，以当前编号接收结果。Windows 文件与桌面资源另设宿主约束，工作目录承担文件组织和定位职责。 [编排文档](https://github.com/stablyai/orca/blob/44b0d8c91cc907be89eaa3448f03d0c0732329f6/docs/site/content/docs/cli/orchestration.mdx#L8-L53) |
| fintwind，`eb8a6a36` | 生成时间线时，已结束轮次的中间过程可折叠。展开或折叠后恢复原行和行内偏移；原行不可见时回到该轮折叠入口。 | 用原有 `ChatFeed` 的行 ID 和 `ListState` 加入轮次折叠规则。让正文、工具错误和结果保持可查。 [轮次投影](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/app/transcript.rs#L1203-L1252) · [阅读锚点](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/app/transcript_view.rs#L1271-L1309) |
| fintwind，同一提交 | Markdown 保留稳定前缀，后台计算变更尾部，用 generation 拒绝旧来源的结果。会话可先展示列表元数据，再在后台载入选中会话。 | 优先测量首帧、流式克隆和重排。增量 Markdown、惰性加载与缓存优化根据测量进入计划。 [增量渲染](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/virtualized.rs#L421-L466) · [会话载入](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/app/sessions.rs#L278-L352) |
| pi-rust，`300c1f3b` | 本仓库固定使用该版本的 `rpi-agent`、`rpi-ai` 和 `rpi-tools`。上游公开 loop 接受结构化上下文、工具、取消信号及流函数。普通启动和从已有上下文续跑有不同的前置条件。 | 直接沿用 `flowy-agent-core` 的库级接入。任务恢复先核对宿主保存的授权与动作记录，再为 pi-rust 构造合法上下文。 [固定版本](https://github.com/bigfish1913/pi-rust/tree/300c1f3b560b653b812a8877956dc1a1776629b7) · [本地依赖](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/Cargo.toml#L48-L51) · [现有接线](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/loop_runner.rs#L91-L143) |

fintwind 使用 `gpui-pre 0.3.8` 和项目内补丁，许可证为 GPL-3.0-only。Flowy 使用 `gpui-ce 0.3.0`。采用交互和架构思路，按 Flowy 原有组件独立实现。工具缓存数量、流式合帧间隔、CPU 数据和高度参数均留给本机测量。 [fintwind 依赖与许可证](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/Cargo.toml) · [上游性能记录](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/docs/performance.md) · [Flowy 依赖](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/Cargo.toml#L82-L84)

## fintwind Markdown 专项取证

| 源码事实 | Flowy 的采用判定 |
| --- | --- |
| fintwind 在 `Cargo.toml` 声明 `pulldown-cmark = "0.13"`。它把 CommonMark 事件转换成具有源码字节范围的块与行内样式，显式开启表格、删除线、任务列表和数学扩展。原生渲染用 GPUI 的 `StyledText` 与 `TextRun`，部分格式使用独立块。 [依赖](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/Cargo.toml#L74-L84) · [解析与块定义](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/parser.rs#L84-L147) · [GPUI 文字渲染](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/render.rs#L1-L16) | 采用事件解析、块排版和原生控件的组合。首选评估独立的 MIT 解析库 `pulldown-cmark 0.13`，P2 只开验收需要的扩展。以 Flowy 已有 GPUI 主题、控件和列表独立实现。 |
| 普通会话在渲染可见行时按消息 ID 复用 `MarkdownView`。长推理内容达到阈值才走 `ReasoningView`，其异步结果带代际校验。 [普通正文接线](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/app/transcript_view.rs#L1640-L1656) · [长推理接线](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/app/transcript_view.rs#L3085-L3115) · [代际校验](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/virtualized.rs#L421-L466) | P2 用原有 `ChatFeed` 实现按需展示。后续分别测「很多消息」和「单条超长消息」，两者的排版瓶颈不同。仅在后者有实测瓶颈时评估消息内虚拟化。 |
| 普通正文的增量解析保留稳定前缀，末尾若仍可能被后续内容改变便重算。引用定义可能改变前面的链接，源内容非追加或遇到需全局解析的输入时会整条重算。流式补全只作用于展示尾部，代码块保留字面内容。 [增量解析](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/parser.rs#L1132-L1197) · [显示尾部和稳定边界](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/parser.rs#L1199-L1276) | P2 流式正文沿用文本，结束后按完整源码解析。后续做边流边排时，用同一份完整解析结果作对照，覆盖晚到引用、跨块表格与未闭合围栏。 |
| 渲染器把格式化文字拆成带样式的文字段。缓存跟踪字体和色板。代码块提供可聚焦的复制操作，直接复制代码内容。解析器用文字显示原始 HTML。 [样式缓存](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/render.rs#L459-L477) · [代码复制](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/render.rs#L2014-L2061) · [HTML 字面显示](https://github.com/roketskiy/fintwind/blob/eb8a6a36a90bcd80e3a9b7471954f9b4d03813e9/src/md/parser.rs#L673-L702) | 用主题语义色和原生复制控件。有明确的源码复制路径。Flowy 自行把关链接、图片与本机产物引用，先验证内容在渲染时无外部动作。 |

`pulldown-cmark` 官方 API 可以直接消费事件与源范围，表格是显式扩展。审核版本的清单声明 MIT，默认特性带 `getopts` 和 `html`。P2 试接优先验证 `default-features = false` 的解析路径，并核对所选补丁版、传递依赖和 Windows 锁定构建。Flowy 当前 Cargo 清单与锁文件未找到该解析库。GPL 项目中的渲染代码、补丁和资源均采用独立实现的边界，数学、图表和高亮依赖均需各自立项后评估。 [解析库 API](https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/) · [解析库清单](https://github.com/pulldown-cmark/pulldown-cmark/blob/v0.13.0/pulldown-cmark/Cargo.toml) · [Flowy 依赖](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/Cargo.toml#L82-L84)

Flowy 的 Assistant 行目前把 `text` 直接放进 GPUI 元素。投影中累积中的正文和已落定的正文都用 `ChatLine::Assistant`。P2 需从投影取得具体正文段是否已落定的语义，同一轮含多个正文片段时保留工具调用的相对位置，避免用最后一行或整个会话的运行状态猜测。以上结论来自源码，Windows 文字选择、长文性能与辅助功能留待后续实测。 [当前助手行](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-studio/src/feed.rs#L226-L247) · [当前流式投影](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent/src/projection.rs#L138-L167)

## 官方产品对照

- Manus 2.0 Studio 公布了按工作加载专业能力，以及视频时间线、游戏和其他可继续编辑的专业交付物。Flowy 借鉴「入口收集意图、能力按任务挂载、结果可打开并继续修改」。Manus 的云端电脑和独立 Cue 属于它的产品范围。 [Manus 2.0](https://manus.im/blog/introducing-manus-2-0)
- Muse、ChatGPT dot 与 Grok Bot 的官方资料均描述云端执行环境。它们验证的是用户可以交办、离开、再回来检查产物的交互方向。Flowy 的设计基于用户电脑可用时执行，睡眠或断线后用本地记录恢复。模型调用仍遵循用户所用供应商的连接配置。 [Muse](https://ai.meta.com/muse/) · [ChatGPT dot](https://learn.chatgpt.com/docs/dots/computers-and-apps) · [Grok Bot](https://x.ai/news/introducing-grok-bot)

## 当前本地证据与待证

| 已核对 | 验证意义 |
| --- | --- |
| 首页创建空会话，首页首发和已有对话发送都进入 `StudioPanel::send_prompt`；首页和聊天有两套输入框，聊天使用独立 `ChatFeed`。 [面板入口](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-studio/src/panel.rs#L450-L506) · [首页](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-studio/src/studio.rs#L461-L494) · [聊天](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-studio/src/chat.rs#L10-L31) | 公用入口有明确接线位置。当前主题、行 ID、虚拟列表和滚动恢复可复用。 |
| Engine 有每会话单运行保护和结构化历史。当前 `session_id` 未传入 loop，历史是整文件快照，部分执行配置分次取得，流式 emitter 只带会话标识。 [Engine](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/engine.rs#L239-L311) · [配置](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/loop_runner.rs#L114-L138) · [事件](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/engine.rs#L422-L477) | 状态与工具的独立性需要并发、取消和断点测试。旧快照晚写覆盖新快照是由代码路径推出的风险，尚无复现结论。 |
| 常规文件工具有路径检查，当前通用 shell 不受这层检查约束。Registry 的 `ApprovalRequired` 目前作为工具错误返回。 [路径检查](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent/src/handle.rs#L969-L1004) · [工具策略](https://github.com/Michael-Lfx/allo/blob/8aae3a5fbf38502f1d802b11c217515a42138e79/flowy18_wip/crates/flowy-agent-core/src/tools/registry.rs#L176-L220) | 任务级权限、等待授权、外部结果不明三种行为需由宿主实现并验证。 |
| 在 Windows、锁定依赖的 `8aae3a5fb` 上复跑 fmt、原 UI Clippy、两个 agent crate 的生产库 Clippy、七 crate 测试和桌面 `cargo check`，均退出 0。测试 147 通过、3 个既有 ignored。追加的 agent 全目标 Clippy 复现 10 条既有测试代码 lint。 | 这是开工基线。本轮取证未执行新版 UI、本机真实产物或三方项目的安装实测。 [命令与分项基线](local-agent-execution.zh.md#基线和统一命令) |

可编辑 PPTX 需在目标机探测生成与预览路径。现有 GPUI 的幻灯片展示条目不足以证明存在可用的生成能力。按执行方案先完成本机清单链路，实际 PPTX 通过首个可打开的文件再进入入口。
