# SDK / WebUI / 站点开发者体验：优先级调整与执行计划

> 状态：计划（2026-09-09）。**先定方向与验收口径，不含实现**。
> 方向调整：**SDK 与 WebUI 功能 = 第一优先级；站点（开发者体验）与插件 / 市场规范 = 高优先级；WP-5 协议 vNext 延后。**
> 上游依据：`15-store-chain-and-protocol-vnext-plan.zh.md`（其 WP-5 顺延，WP-6/WP-7 已完成部分继续有效）、**`19-webui-codex-alignment.zh.md`（WebUI 子计划，取代本文档原 §3 B 章）**、`11-webui-production-readiness.md`、`12-sdk-packaging.md`、`07-typescript-sdk.md`、`17-plugin-spec.zh.md`、`18-marketplace-spec.zh.md`。
> 口径：排期为范围值、按实测校准，不构成承诺；结论区分「已验证事实 / 推断 / 待定」。

---

## 1. 为什么调整

- **四类资产闭环已收口**：专家 / 专家团 / 技能 / 连接器「下载 → 安装 → 使用」全链路已验收（四链路 24/24、P0-A/B、OAuth 26/26），继续在协议层大改的收益低于把 SDK 与 UI 打磨到可用。
- **npm beta 已发布，第三方开始长期驻留使用**：`0.1.0-beta.2` 四包 + `runtime-win32-x64` 已上线，SDK 的进程与传输健壮性直接决定第三方是否踩坑——本轮已确认一个会冻死服务端的缺陷（F10 / A1）。
- **站点是开发者的第一触点，且当前存在已确认的事实性错误**（F1–F5）：下载按钮对非 Windows 访客 404、兼容性矩阵声称 5 平台而实际只有 1 个平台有产物。
- **WebUI 存在「协议已就绪、界面未接」的成片空白**（F19–F21）：`run/steer`、产物、全局通知均已具备后端能力却零使用；同时 `11` 号就绪清单中多项仍为未做（对账见 `19` §7）。
- **协议 vNext 属重构型工作**：在 SDK 公共面与 webui 功能尚未稳定时动工，会把返工风险带进破坏性重命名。故延后，待 SDK/UI 稳定后再启动。

---

## 2. 现状核查（2026-09-09 实测）

| # | 事实 | 证据 | 影响 |
| --- | --- | --- | --- |
| F1 | 下载目录**只有 2 个产物**，均为 Windows x86_64 | `curl http://111.170.173.22:10014/downloads/` → `flowy-agent-store-latest-windows-x86_64.zip`、`flowy-agent-store-v1.0.11-windows-x86_64.zip` | 非 Windows 访客无产物 |
| F2 | 首页主下载按钮**按访客系统直接拼 URL**，未校验是否已发布 | `DownloadCTA.tsx`：`detectedUrl = releaseAssetUrl("latest", detected)`（`platform.ts` 拼 `flowy-agent-store-latest-<os>-<arch>.zip`）；仅手动列表限定 Windows | macOS/Linux/ARM 访客点击 → **404** |
| F3 | 兼容性矩阵声称 **5 个平台「支持」** | `content/docs/zh-CN/compatibility.md` 平台表 | 与 F1 冲突，属对外过度承诺 |
| F4 | npm 侧仅发布 `runtime-win32-x64` | `sdk/package.json` optionalDependencies 预列 5 平台，实际只发 1 个 | 非 Windows 的 `launchClient` 找不到二进制 |
| F5 | 站点部署口径漂移 | `README.md` 称 GitHub Pages；`.github/workflows/deploy-site.yml` 的 `push:` 触发**已注释**，仅剩 `workflow_dispatch` | 文档与真实部署不一致；线上是 VPS 裸 IP + HTTP |
| F6 | 三个包均**无 `engines`**、无 `repository`、无 `sideEffects` | `protocol/client/sdk` 的 `package.json` | 未声明 Node 版本下限；文档也未写 |
| F7 | SDK 文档无**版本与 beta 状态**标注 | `content/docs/zh-CN/typescript-sdk.md` 第 1 节只有 `bun add`，无版本号 / dist-tag / 平台矩阵 | 读者无法判断自己装的版本与支持范围 |
| F8 | 站点市场数据**停在 2026-09-04** | `content/market.json` 的 `updatedAt` | 首页/市场页展示的数据已过期 5 天 |
| F9 | 中英文档行数完全一致 | 7 篇 × 2 语言逐篇比对，行数相同 | 双语同步机制目前靠人工，无校验脚本（漂移风险） |
| F10 | SDK 子进程 **stdout 背压**会冻死运行时 | `spawn.ts` 收到就绪行后 `lines.close()` → Node `readline.close()` 会 `pause()` 输入；实测合成子进程写满管道后 8s 未退出 | 长会话（多轮 turn / 扫市场树）静默卡死，表现为「请求超时」 |
| F11 | `transport.close()` **不清通知监听器** | `transport.ts` `close()` 仅关 socket、清 pending | 订阅对象与闭包泄漏；若重连，旧订阅游标过期 → 静默丢事件 |
| F12 | Conversation 与 Run 订阅**成熟度不对称** | Run：去重集 + gap 检测 + `autoResync` + `onError`；Conversation：仅 `sequence <= lastSeen` 丢弃 | 主要 UX 面缺追平能力，webui 只能自行重做（`conversation-events.ts` 289 行） |
| F13 | 包面**无 HTTP 绑定** | `client/src/transport.ts` 只导出 `WebSocketTransport`；`index.ts` 无 http 导出 | webui 自建 5 个 fetch 辅助；第三方用一次性 HTTP 需自己实现 |
| F14 | `ConversationEvent.payload` 无类型化，`event_type` 带 `\| string` 转义 | `protocol.ts` | 每个消费者都要重写解码层（webui 的 `activity.ts` 145 行）；穷尽性检查失效 |
| F15 | **插件与市场的规范主体只存在于代码** | `market_source.rs` 头注释（源类型 / staging→校验→原子晋升→last-good / ETag 短路）、`app_server_marketplace.rs`（清单发现、条目解析）、`market_fetch.rs`（git vs HTTP 获取策略） | 无权威正文可依，行为变更无法评审；第三方无法按规范实现市场 |
| F16 | **`_files.txt` 目录枚举格式只在脚本注释里** | `scripts/serve-agent-store-market.mjs`（逐行相对路径、无头、`--emit-listings` 预生成） | 发布方只能读脚本反推；HTTP 市场条目树镜像行为无契约 |
| F17 | **无 Agent Store 原生插件格式规范** | `02-codebuddy-workbuddy-import-spec.md` 只定义「导入源」映射；`plugin.json` / `marketplace.json` 语义全部继承 CodeBuddy | 插件作者不知道该按什么写；原生格式的演进无据可依 |
| F18 | **无机器可校验的 Schema** | 全仓仅 `crates/agent/flowy-web/evaluation/corpus.json` 的 schema（无关）；`plugin.json` / `marketplace.json` / `_files.txt` 均无 schema | 市场内容只能靠运行时校验，发布方无法自检 |
| F19 | **`run/steer` 协议与 SDK 已实现，webui 零使用** | 全仓 `web/src` 搜 `steer` 无命中；`RunClient.steer` 已导出（REQ-PAR-05a，含 live 验证） | 用户无法中途纠偏运行，只能等待或取消 |
| F20 | **产物无任何展示** | `web/src` 搜 `output_files` / `artifact` 无命中；`TurnResult.output_files` 已聚合、`artifact.created` 事件已发 | 跑完看不到产出文件，「使用」环节缺一环 |
| F21 | **无全局通知层；连接状态仅一个小圆点** | `web/src` 无 toast / notification 组件（仅 i18n 文案）；`Composer` 的 `composer-model-dot ${phase}` 是唯一连接指示，无横幅、无重连动作 | 断线、后台 Run 完成、导入完成均静默；与 F11/A2 叠加时表现为「莫名其妙不能用」 |
| F22 | **`RunDetail` / `RunPanel` 是调试视图** | raw JSON dump + `seq/type/payload` 表格；文案硬编码英文（未走 i18n） | 界面不像产品；英文与中文界面混排 |
| F23 | **无统一重试入口** | `isRetryableError` 全仓仅 1 处使用（`CatalogView.tsx:344`）；`11` §2.1 消息重试/编辑/重新生成未做（仅「复制错误」按钮） | 失败即失败，用户无自助恢复路径 |
| F24 | 审批（Approvals）无 UI，且被 `approvals: false` 阻塞 | `initialize` 硬编码 `approvals: false`；webui 无审批界面 | `approval.required` 事件无法消费 |

---

## 3. 工作包

### A. SDK（第一优先级）

**A1 · 运行时进程生命周期（P0）**
- 范围：stdout 持续排空（或提供 `onLog` / `logFile`）；`onExit` / `exited` 暴露运行时崩溃；`env` / `cwd` 透传。
- 验收：新增回归用例——合成子进程写 > 1MB 日志，断言不被阻塞且能正常退出；真机跑 3 轮 turn + 一次市场树扫描全程不卡。
- 备注：F10 已实测复现，是当前 SDK 最严重缺陷。

**A2 · 传输层健壮性（P1）**
- 范围：`close()` 清理监听器；`connect()` 超时；并发 `connect()` 竞态（旧 socket 覆盖 + 泄漏）；`close()` 结算挂起的 connect promise；重连后游标重置。
- 验收：并发连接 / 连接超时 / 断线重连单测；重连后不丢事件（序列号连续）。

**A3 · 事件面补齐（P1）**
- 范围：`ConversationSubscription` 对齐 `RunSubscription`（gap 检测 + 自动追平 + `onError`）；payload 解码器进包（以 webui `activity.ts` 为素材）；修 `event_type | string` 转义（保留联合类型 + 未知类型单独分支）；首个监听器注册前的事件缓冲。
- 验收：人为丢事件场景下自动追平到连续；webui 事件层代码量下降且行为不变（回归测试）。

**A4 · HTTP 绑定公共面（P1）**
- 范围：包内新增 `HttpTransport`，实现与 `WebSocketTransport` 同一 `Transport` 接口，复用后端 `dispatch_connection_request` 的一次性语义；webui 的 `browseDirectory` / `registerWorkspace` 收编为调用方。
- 验收：webui 不再自建 fetch 辅助；两个宿主共享同一实现。
- 边界：**只收编传输实现，不把 `fs/browse` 变成协议方法**（见 §6）。

**A5 · 包元数据与单测（P2）**
- 范围：补 `engines`、`repository`、`sideEffects: false`；`protocol` 包从 0 单测起（类型守卫、错误分类）。
- 验收：`npm view` 字段齐全；`protocol` 有可运行用例。

**A6 · 平台矩阵（P1，与站点强耦合）**
- 范围：Linux / macOS runtime 包（GitHub Actions 各 runner 各自构建发布；需仓库 secret）；Windows 包随版本重发。
- 验收：Linux x64 干净环境 `npm i` 后 `launchClient` 成功；站点兼容性矩阵与产物一致。
- 依赖：需要一次仓库 secret 配置（用户参与）。

### B. WebUI（第一优先级）

**WebUI 部分已独立为 `19-webui-codex-alignment.zh.md`（Codex app 体验对齐基线）。**

- 原 B1–B12 全部迁入该文档，并按「体验骨架 / 审查面 / 会话与反馈 / 输入与配置」四层重组为 **W1–W14**（编号映射见其 §8）。
- 现状对账两张表（协议已就绪未接 / `11` 号清单核实）随迁至该文档 §7。
- 本节只保留跨工作包依赖；批次表改用 W 编号。

**WebUI 与其它工作包的依赖**

| 工作包 | 依赖 |
| --- | --- |
| W2 审批卡 | 后端解冻 `capabilities.approvals` + 安全评审（第 1 层唯一有后端改动者） |
| W8 通知与重连 | SDK A2（传输层重连） |
| W12 技能管理 | 协议增量（`skill/create|update|delete`），建议与 D2 同批 |
| W14 消费 SDK 新能力 | A3（事件面补齐） |
| W1 / W10 | 同批做（都改 composer 输入模型，避免两次重写） |

### C. 站点与开发者体验（高优先级）

**C1 · 事实性硬伤（P0，先修）**
1. **下载 404**（F1+F2）：主 CTA 只在已发布平台给直链，其余平台引导到发布页并明确说明「当前仅 Windows x64 已发布」。
2. **兼容性矩阵与事实对齐**（F3+F4）：改为「已发布 / 待发布」两栏，覆盖 zip 与 npm runtime 包两个维度。
3. **部署口径对齐**（F5）：README 与真实部署一致，明确当前线上入口与后续域名/HTTPS 计划。

**C2 · 开发者入口叙事（P1）**
- 明确两条分发路径的分工：**终端用户** → 安装包 / `install.ps1`；**开发者** → npm 包。当前 quick-start 只讲安装包、typescript-sdk 只讲 npm，互不引用。
- SDK 文档补齐：**版本与 beta 状态**、**Node 版本要求**（依赖 A5）、**平台支持矩阵**、**升级与迁移指引**、版本固定示例与 dist-tag 说明。

**C3 · 开发者文档深度（P1）**
- 逐方法 API 参考（现为导览级 7 章）；事件类型参考（含 `sequence` / resync 语义与「实时投递不保证全局有序」这一约束）；错误模型补 `retryable` 语义与 `withRetry` 用法。
- 可复制示例集：Node spawn、浏览器连已运行的 App Server、Electron 主进程。
- MCP 接入指南（MCP 是核心卖点，当前文档覆盖薄）。

**C4 · 站点机制（P2）**
- `content/docs` 与 `docs/agent-store` 的**同步校验脚本**（F9：目前靠人工，无机制）。
- changelog / release notes 页；站内搜索（文档量上来后再评估）。
- 市场数据刷新纳入发布流程（F8）。

**C5 · 部署与可达性（P1，信任问题）**
- 现状是裸 IP + HTTP 的 `irm ... | iex`，用户与安全软件都会质疑。
- 待定：站点最终域名与托管方式（VPS + 自定义域名 / EdgeOne / GitHub Pages 三选一），见 Q2。
- 已知约束：EdgeOne preset 域名带签名 `eo_token` 且按路径签名，不适合做公开源，需自定义域名。

### D. 插件与市场规范（高优先级，以文档为主）

**D1 · 规范缺口收口（正文）** — ✅ **已完成（2026-09-09）**
- 交付：`17-plugin-spec.zh.md`（插件规范，兼容层）+ `18-marketplace-spec.zh.md`（市场规范，兼容层）。
- 定位：按 Q5 决策，**只定义兼容层**——明确「当前接受 CodeBuddy / WorkBuddy 格式，非 Agent Store 原生格式」，原生格式待生态起量后再定。
- 验收（已满足）：规范覆盖源类型与地址解析、清单发现优先级、`_files.txt` 格式、获取与晋升不变式、注册表字段、发布自检清单、客户端契约。

**D2 · 机器可校验 Schema（P2）**
- 范围：`plugin.schema.json` / `marketplace.schema.json` + `_files.txt` 校验器；接入市场发布脚本与 CI。
- 验收：对现有市场数据全绿；构造的非法样例被拒绝并给出字段级定位。

### E. 延后

- **WP-5 协议 vNext**（方法/事件/概念映射表与边界拍板）——待 SDK/UI 稳定后启动。
- A5 中与站点无关的部分、C4 的搜索功能。
- `11` 号清单中未被本计划纳入的项：§1.1 认证令牌、§1.2 Origin/CSP/CSRF、§3.1–§3.3 可观测性、§4.1 只读标识、§5.2 压缩建议——待基础功能稳定后单独立项。

---

## 4. 待拍板

| # | 问题 | 选项 |
| --- | --- | --- |
| Q1 | 平台矩阵策略 | ① 先补 Linux/macOS 构建（A6） ② 先把文档改成「仅 Windows 已发布」（C1-2），构建后补 |
| Q2 | 站点托管与域名 | VPS + 自定义域名 / EdgeOne / GitHub Pages |
| Q3 | 附件图片输入时机 | 现在做（W10） / 等协议 vNext 一起做 |
| Q4 | SDK 发版节奏 | A1+A2 先发 `0.1.0-beta.3` / A1–A4 一起发 |
| Q5 | 插件格式策略 | ✅ **已定（2026-09-09）：先只做兼容层** |
| Q6 | 设置里 provider 的写入目标 | ① 写 `~/.agent-store/config.toml`（与「唯一来源」一致） ② 写 DB（现状之一，需说明两套关系） |
| Q7 | `auto_update` 默认值 | ① 改实现以区分官方/第三方（按 `02` §8 表述） ② 改 `02` §8 表述以匹配实现（恒 `false`） |

---

## 5. 批次与顺序

| 批次 | 内容 | 出口 |
| --- | --- | --- |
| 第 1 批 | A1（进程生命周期 P0）+ C1（站点三处事实硬伤） | 长会话不再卡死；非 Windows 访客不再点到 404；对外文档不再过度承诺 |
| 第 2 批 | **W1（命令面板：`/` + `@`）+ W13（市场管理）+ W5（产物面板）** + C2（文档叙事） | 体验骨架第一步 + 三处「只差接线」落地 |
| 第 3 批 | A2 / A3 / A4（传输、事件、HTTP 绑定） | SDK 断线与事件追平可用；第三方不必自建 HTTP 层 |
| 第 4 批 | **W2（审批卡）+ W3（`steer`）+ W4（计划树）+ W6（Run 状态树）** | 对齐核心：审批 / 引导 / 计划 / 状态可读可控 |
| 第 5 批 | **W7（消息重试/编辑/重新生成）+ W8（通知与重连）+ W9（用量与模型能力）** | 失败可自助恢复；断线不再静默；用量可查 |
| 第 6 批 | **W10（附件）+ W11（设置）+ W12（技能管理）+ W14（消费 SDK）** + A6 + C3 + D2 | 输入与配置补全；非 Windows 用户可用；市场内容可自检 |
| 待决策 | C5（域名 / HTTPS，等 Q2） | — |
| 延后 | A5 其余、C4、WP-5、`11` 未纳入项 | — |

> 第 2 批全部是「后端/client 已就绪、只差前端接线」，不碰协议，风险最低、见效最快。

---

## 6. 边界与不做什么

- **不把宿主管理面收编进协议**：进程生命周期、数据目录、provider / MCP 配置、`fs/browse`、资产直链保持为宿主管理面。判断规则：*第三方 SDK 消费者是否应该能调用它？* 不能 → 不进包。
- **不为了「纯粹」牺牲权限边界**：`fs/browse` 若进入公共协议，等于给远程客户端文件系统枚举能力。
- **不在 SDK 稳定前动协议重命名**（WP-5 延后）。
- **不做假开关**：设置分区、市场开关等一律要有真实数据源与回写路径，未实现的继续标「即将推出」而非留占位控件。
- **不承诺排期**：本计划只给批次与验收口径，实际节奏按实测校准。
