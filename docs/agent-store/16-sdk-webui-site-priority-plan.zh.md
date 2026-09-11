# SDK / 站点 / 插件市场 / WebUI：方向与执行计划

> 状态：计划（2026-09-09）。**先定方向与验收口径，不含实现**。
> **四个方向**：① SDK + 站点（配对执行） · ② 插件与市场规范（收尾） · ③ WebUI 功能 · ④ 待立项（生产化硬指标）。
> 优先级：**① ③ 第一优先级；② 高优先级（已完成 90%）；④ 未排期；WP-5 协议词汇与概念对齐 延后。**
> 上游依据：`19-webui-codex-alignment.zh.md`（方向三子计划）、`17-plugin-spec.zh.md`、`18-marketplace-spec.zh.md`（方向二交付）、`15-store-chain-and-protocol-vnext-plan.zh.md`（WP-5 顺延）、`11-webui-production-readiness.md`、`12-sdk-packaging.md`、`07-typescript-sdk.md`。
> 口径：排期为范围值、按实测校准，不构成承诺；结论区分「已验证事实 / 推断 / 待定」。
> **2026-09-11 重排**：§5.2 的任务表按完成度分为 **✅ 全部完成 / 🟡 部分完成 / ⏸ 未完成** 三类（分组视图，逐项明细与证据原文原地保留）。
> **2026-09-11 决定（用户）**：**站点部署卡点相关问题整体延后**——R7（域名 / HTTPS）、R32（镜像代价，其大头同源）、R6②（市场数据刷新纳入发布流程）三项不再作为「等拍板/待推进」项挂在计划上；Q2 保持延后。重开条件见 §5.2 各组行与「卡点决策」表。
> **2026-09-11 C 档复核（卡点三分法）**：C 档原以「维持不动」统一处置 5 条，复核后判定**混杂了三种不同性质**，改按性质分别处置——① **已获批、未排期**（R23 / R24：`21` D6=A 已批准按规范实现阻断，卡的不是决策而是排期）→ 移出「等外部条件」，标「已批准待排期」；② **技术依赖**（R20 / R15：一条挂死两条，依赖粒度太粗）→ 拆条，把不依赖的部分先做；③ **触发条件**（R6① / R33：R33 的唯一承诺动作「开立项页」当时并未兑现）→ 补度量并立刻做零风险项。逐条依据见 §5.3「卡点处置四档」的 C 档重写与「卡点决策」表。
> **2026-09-11 版本框架订正**：协议 / 插件规范 / 市场规范在正式发版前均只有一个版本，统一称 v1，**不设 v1 / v1.1 / v2 之分**（`16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）。故本文中：
> - 「协议 vNext」一律改称「协议**词汇与概念对齐**」（工作包编号 WP-5 不变）——它属 v1 内部调整，不是"下一版"；
> - **下文若出现 "v1.1" / "v2"，一律是「被废止措辞的历史引述」**，不代表存在该版本；凡引用 `21` D6 原文「作为 v1.1 的破坏性变更写进 changelog」处，其**版本框架部分已被 §7 决策 4 取代**（规范未发版 ⇒ 实现即生效，不是破坏性变更、无需公告）；
> - **D6 落地口径随之翻转**：由「逃生口默认关（保兼容）」改为「**阻断默认开 + 逃生口**」。
> - **不受影响**：`upgrade` / `changelog` 两页与 D10=A 的公告规则——那是**已发布 npm 包**的发行版本口径，不是规范 / 协议的版本。

---

## 0. 方向一览

| 方向 | 范围 | 现状 | 主文档 |
| --- | --- | --- | --- |
| **① SDK + 站点** | SDK 加固（A1–A5；A6 已决策延后）+ 站点事实修正与开发者文档（C1–C5） | **退出条件已达成（2026-09-10）**：A1 / A2 修完、已发 `0.1.0-beta.3`、C1 三处事实硬伤 + T5 已修。A3–A5 与 C2–C4 转维护模式 / 未排期（见 §5.2「剩余任务总表」R1–R6）—— ✅ 已由 `21` D1 解除，纳入**批 2** | 本文 §3.1 / §3.2 |
| **② 插件与市场规范** | D1 规范正文 ✅ / D2 机器可校验 Schema ✅（T19） / 反向验证 ✅（T20） / 转现行正文 ✅（T21） | **已收口（2026-09-10）**：`17`/`18` 标为「现行正文（未发版，可改）」，偏差全部登记 | `17-plugin-spec.zh.md`、`18-marketplace-spec.zh.md` |
| **③ WebUI 功能** | W1–W14（Codex app 体验对齐，四层） | **三闭环已落地并验收（2026-09-10）**：W1 命令面板 V1 / W5 产物面板（收敛版）/ W13 市场管理，外带 W8 的 Toast + 断线横幅；其余 12 项在待反馈批（见 §5.2「剩余任务总表」R8–R21）—— ✅ 已由 `21` D1 解除，纳入**批 3 / 批 4** | `19-webui-codex-alignment.zh.md` |
| **④ 待立项** | `11` 号未纳入的 8 项 + WP-5 协议词汇与概念对齐 | 未排期 | `11-webui-production-readiness.md`、`15-...zh.md` |

---

## 1. 为什么这样排序

- **四类资产闭环已收口**：专家 / 专家团 / 技能 / 连接器「下载 → 安装 → 使用」全链路已验收（四链路 24/24、P0-A/B、OAuth 26/26），继续在协议层大改的收益低于把 SDK 与 UI 打磨到可用。
- **npm beta 已发布，第三方开始长期驻留使用**：`0.1.0-beta.2` 四包 + `runtime-win32-x64` 已上线，SDK 的进程与传输健壮性直接决定第三方是否踩坑——本轮已确认一个会冻死服务端的缺陷（F10 / A1）。
- **站点是开发者的第一触点，且存在已确认的事实性错误**（F1–F5）：下载按钮对非 Windows 访客 404、兼容性矩阵声称 5 平台而实际只有 1 个平台有产物。
- **SDK 与站点是同一条链的两端**：站点 SDK 文档是 SDK 公共面的投影，公共面每改一处即欠一笔文档债（§3.2 C0），因此合并为一个方向、配对执行。
- **WebUI 存在「协议已就绪、界面未接」的成片空白**（F19–F21）：`run/steer`、产物、全局通知均已具备后端能力却零使用（对账见 `19` §7）。
- **协议词汇与概念对齐 属重构型工作**：在 SDK 公共面与 webui 功能尚未稳定时动工，会把返工风险带进破坏性重命名，故延后。

---

## 2. 现状核查（2026-09-09 实测，按方向分组）

> ⚠️ 本节是 2026-09-09 的快照。2026-09-10 已就 F20 / F21 回写（行内以 ✅ / 🟡 标注）；F19 / F22 / F23 / F24 仍成立；D-W13-2 / D-STREAM-1 / D-STREAM-2 见 §5.2「已知偏差」。

### 2.1 站点与开发者体验

| # | 事实 | 证据 | 影响 |
| --- | --- | --- | --- |
| F1 | 下载目录**只有 2 个产物**，均为 Windows x86_64 | `curl http://111.170.173.22:10014/downloads/` → `flowy-agent-store-latest-windows-x86_64.zip`、`flowy-agent-store-v1.0.11-windows-x86_64.zip` | 非 Windows 访客无产物 |
| F2 | 首页主下载按钮**按访客系统直接拼 URL**，未校验是否已发布 | `DownloadCTA.tsx`：`detectedUrl = releaseAssetUrl("latest", detected)`；仅手动列表限定 Windows | macOS/Linux/ARM 访客点击 → **404** |
| F3 | 兼容性矩阵声称 **5 个平台「支持」** | `content/docs/zh-CN/compatibility.md` 平台表 | 与 F1 冲突，属对外过度承诺。**已决策（2026-09-09）：仅 Windows**，矩阵收敛为「仅 Windows x64 已发布」 |
| F4 | npm 侧仅发布 `runtime-win32-x64` | `sdk/package.json` optionalDependencies 预列 5 平台，实际只发 1 个 | 非 Windows 的 `launchClient` 找不到二进制 |
| F5 | 站点部署口径漂移 | `README.md` 称 GitHub Pages；`deploy-site.yml` 的 `push:` 触发**已注释**，仅剩 `workflow_dispatch` | 文档与真实部署不一致；线上是 VPS 裸 IP + HTTP |
| F6 | 三个包均**无 `engines`**、无 `repository`、无 `sideEffects` | `protocol/client/sdk` 的 `package.json` | 未声明 Node 版本下限 |
| F7 | SDK 文档无**版本与 beta 状态**标注 | `typescript-sdk.md` §1 只有 `bun add`，无版本号 / dist-tag / 平台矩阵 | 读者无法判断自己装的版本与支持范围 |
| F8 | 站点市场数据**停在 2026-09-04** | `content/market.json` 的 `updatedAt` | 首页/市场页展示的数据已过期 |
| F9 | 中英文档行数完全一致 | 7 篇 × 2 语言逐篇比对 | 双语同步靠人工，无校验脚本（漂移风险） |
| F25 | **文档债未制度化** | 没有「SDK 公共面变更 → 文档同批更新」的机制；F7 是首次欠债的证据 | 文档将持续滞后于实现 |

### 2.2 SDK

| # | 事实 | 证据 | 影响 |
| --- | --- | --- | --- |
| F10 | SDK 子进程 **stdout 背压**会冻死运行时 | `spawn.ts` 收到就绪行后 `lines.close()` → Node `readline.close()` 会 `pause()` 输入；实测合成子进程写满管道后 8s 未退出 | 长会话（多轮 turn / 扫市场树）静默卡死，表现为「请求超时」 |
| F11 | `transport.close()` **不清通知监听器** | `transport.ts` `close()` 仅关 socket、清 pending | 订阅对象与闭包泄漏；若重连，旧订阅游标过期 → 静默丢事件 |
| F12 | Conversation 与 Run 订阅**成熟度不对称** | Run：去重集 + gap 检测 + `autoResync` + `onError`；Conversation：仅 `sequence <= lastSeen` 丢弃 | 主要 UX 面缺追平能力，webui 只能自行重做（`conversation-events.ts` 289 行） |
| F13 | 包面**无 HTTP 绑定** | `client/src/transport.ts` 只导出 `WebSocketTransport`；`index.ts` 无 http 导出 | webui 自建 5 个 fetch 辅助；第三方用一次性 HTTP 需自己实现 |
| F14 | `ConversationEvent.payload` 无类型化，`event_type` 带 `\| string` 转义 | `protocol.ts` | 每个消费者都要重写解码层（webui 的 `activity.ts` 145 行）；穷尽性检查失效 |

### 2.3 插件与市场规范

| # | 事实 | 证据 | 影响 |
| --- | --- | --- | --- |
| F15 | **插件与市场的规范主体只存在于代码** | `market_source.rs` 头注释（源类型 / staging→校验→原子晋升→last-good / ETag 短路）、`app_server_marketplace.rs`（清单发现、条目解析）、`market_fetch.rs`（git vs HTTP 获取策略） | 无权威正文可依，行为变更无法评审；第三方无法按规范实现市场 |
| F16 | **`_files.txt` 目录枚举格式只在脚本注释里** | `scripts/serve-agent-store-market.mjs`（逐行相对路径、无头、`--emit-listings` 预生成） | 发布方只能读脚本反推；HTTP 市场条目树镜像行为无契约 |
| F17 | **无 Agent Store 原生插件格式规范** | `02-...import-spec.md` 只定义「导入源」映射；`plugin.json` / `marketplace.json` 语义全部继承 CodeBuddy | 插件作者不知道该按什么写；原生格式演进无据可依 |
| F18 | **无机器可校验的 Schema** | 全仓仅 `flowy-web/evaluation/corpus.schema.json`（无关）；`plugin.json` / `marketplace.json` / `_files.txt` 均无 schema | 市场内容只能靠运行时校验，发布方无法自检 |

### 2.4 WebUI

| # | 事实 | 证据 | 影响 |
| --- | --- | --- | --- |
| F19 | **`run/steer` 协议与 SDK 已实现，webui 零使用** | `web/src` 搜 `steer` 无命中；`RunClient.steer` 已导出（REQ-PAR-05a，含 live 验证） | 用户无法中途纠偏运行，只能等待或取消 |
| F20 | **产物无任何展示** | `web/src` 搜 `output_files` / `artifact` 无命中；`TurnResult.output_files` 已聚合、`artifact.created` 已发 | 跑完看不到产出文件，「使用」环节缺一环 —— ✅ **已修（2026-09-10，T12）**：按 D-W5-1 收敛为**宿主面文件服务**（`/api/fs/list`、`/api/fs/read`、`/api/fs/metadata`），`ArtifactPanel` + Topbar 入口交付「列表 / 预览 / 下载 / 评论入草稿」；**残留**：按 Run 归属、接受 / 回退（待 Artifact Phase），列表项无 size / type / mtime |
| F21 | **无全局通知层；连接状态仅一个小圆点** | 无 toast / notification 组件（仅 i18n 文案）；`composer-model-dot ${phase}` 是唯一连接指示 | 断线、后台 Run 完成、导入完成均静默；与 F11/A2 叠加时表现为「莫名其妙不能用」—— ✅ **已修（2026-09-10）**：W8 的 Toast 层（`ToastHost` + `pushToast`）与断线横幅 + 一键重连；**批 3 R13 补齐全余项**——D4=A 多标签选主（桌面通知 / 声音 / 后台 Run 提醒跨标签只触发一次）+「安装 / 刷新完成」Toast +「后台 Run 终态」Toast（后台时另发通知与声音，点通知回前台） |
| F22 | **`RunDetail` / `RunPanel` 是调试视图** | raw JSON dump + `seq/type/payload` 表格；文案硬编码英文（未走 i18n） | 界面不像产品；英文与中文界面混排 |
| F23 | **无统一重试入口** | `isRetryableError` 全仓仅 1 处使用（`CatalogView.tsx:344`）；`11` §2.1 未做 | 失败即失败，用户无自助恢复路径 |
| F24 | 审批（Approvals）无 UI，且被 `approvals: false` 阻塞 | `initialize` 硬编码 `approvals: false`；webui 无审批界面 | `approval.required` 事件无法消费 |

> WebUI 的完整对账（协议已就绪未接 / `11` 号清单 20 项核实）见 `19-webui-codex-alignment.zh.md` §7。

---

## 3. 工作包

### 3.1 方向一 · SDK（A）

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

**A6 · 平台矩阵（✅ 已决策：仅 Windows，延后）**
- **决策（2026-09-09）**：本轮只支持 **Windows x64**。站点口径随之收敛（C1-2 只列 Windows x64 为「已发布」，其余写「未提供」）。
- **延后条件**：出现真实非 Windows 需求（外部 issue / 客户要求）再立项；届时的路径是 GitHub Actions 各 runner 各自构建发布（需仓库 secret）+ `sdk/package.json` 的 optionalDependencies 已预列五平台。
- 不做：在无需求前预先构建 Linux/macOS 产物——避免维护无人使用的构建矩阵。
- 关联：`12-sdk-packaging.md` §6（分发方式定案为 npm optionalDependencies）。

### 3.2 方向一 · 站点（C）

**C0 · A ↔ C 派生关系（文档是 SDK 公共面的投影）**

站点的 SDK 文档不是独立创作，而是 **SDK 公共面的投影**：公共面每改一处，文档就欠一笔债。因此 A 与 C 必须**配对执行**，不能把文档攒到最后。

| SDK 工作包 | 需同步的站点文档章节 |
| --- | --- |
| A1 进程生命周期（stdout 排空、`onLog`/`onExit`、`env`/`cwd`） | `typescript-sdk.md` §5.1（`launchClient` 选项）、§5.2（底层原语）、§5.4（运行契约）、§5.5（错误与清理） |
| A2 传输层（重连、超时、`close` 语义） | §4.2（`Transport` 接口） |
| A3 事件面（Conversation 追平、解码器） | §4.4（子客户端）+ 新增「事件与追平」章节 |
| A4 HTTP 绑定公共面 | §4.2 + §6（浏览器接入场景） |
| A5 包元数据（`engines` / `repository`） | §1（安装：Node 版本要求、beta 状态、版本固定示例） |
| A6 平台矩阵（已延后） | `compatibility.md` + `quick-start.md` + §5.3（二进制定位）——随 C1-2 收敛口径，无构建产物需同步 |
| D 插件 / 市场规范 | `configuration.md`（`default_marketplaces` / `source_kind`）、`plugins-market.md` |

**已发生的债（需一并偿还）**：包已发布 `0.1.0-beta.2`，但 `typescript-sdk.md` §1 **未标注 beta 状态、未写 Node 版本要求**（三包均无 `engines`），只在 §4.2 顺带提了一句「Node 22+/Bun」。

> 反向价值：**文档是 SDK 的验收面**——写不出文档的 API，通常也没设计好。C3 的逐方法参考应与 A 的公共面设计同步推进，而不是事后补。

**C1 · 事实性硬伤（P0，先修）**
1. **下载 404**（F1+F2）：主 CTA 只在已发布平台给直链，其余平台引导到发布页并明确说明「当前仅 Windows x64 已发布」。
2. **兼容性矩阵与事实对齐**（F3+F4）：**仅列 Windows x64 为「已发布」**；其余平台明确写「未提供」——不写「待发布」（避免暗示已排期），并覆盖 zip 与 npm runtime 包两个维度。
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

### 3.3 方向二 · 插件与市场规范（D）

**D1 · 规范缺口收口（正文）** — ✅ **已完成（2026-09-09）**
- 交付：`17-plugin-spec.zh.md`（插件规范，兼容层）+ `18-marketplace-spec.zh.md`（市场规范，兼容层）。
- 定位：按 Q5 决策，**只定义兼容层**——明确「当前接受 CodeBuddy / WorkBuddy 格式，非 Agent Store 原生格式」，原生格式待生态起量后再定。
- 验收（已满足）：规范覆盖源类型与地址解析、清单发现优先级、`_files.txt` 格式、获取与晋升不变式、注册表字段、发布自检清单、客户端契约。

**D2 · 机器可校验 Schema（P2）** — ✅ **已完成（2026-09-10，T19）**
- 交付：`docs/agent-store/schemas/plugin.schema.json`（`17` §3 + `02` 的必填/默认/宽容形态）、`docs/agent-store/schemas/marketplace.schema.json`（`18` §3/§4，覆盖 `plugins` / `skills` / `connectors` 三种条目数组）；校验器 `scripts/check-agent-store-market.mjs`（① `18` §3 清单发现优先级 ② 清单字段 ③ 条目 `source` 相对性 + 镜像模式下的存在性 + 条目重名 ④ `_files.txt` 全规则与覆盖自检），**每条发现都带 `文件#/指针` 字段级定位**。
- 接入：`scripts/serve-agent-store-market.mjs --emit-listings` 写完清单后**立即校验**（不合格 exit 1）；`package.json` 新增 `check:market`（自检，已并入 `check`）与 `test:market`；测试 `scripts/check-agent-store-market.test.mjs`。
- 验收证据（2026-09-10）：真实市场 `experts / skills / connectors` → **`3 market(s), 0 error(s)`**；自检 **17/17**（14 个非法样例全部被拒且带定位）；单测 **6 pass / 0 fail**；发布脚本连续两次 `--emit-listings` 均 `listings validated`。过程中发现并修掉两处**工具侧**缺陷：schema 一开始把 `owner` 写成 `string`（真实市场是 `{name,email}`，已按 `author` 同形放宽），发布脚本 `listFiles` 会把 `_files.txt` 自身写进清单（违反 `18` §6，已修）。

### 3.4 方向三 · WebUI（子计划）

**WebUI 部分已独立为 `19-webui-codex-alignment.zh.md`**（Codex app 体验对齐基线，W1–W14 四层）。

- 原 B1–B12 全部迁入该文档并按四层重组（编号映射见其 §8）；现状对账两张表随迁至其 §7。
- 本节只保留跨方向依赖：

| 工作包 | 依赖 |
| --- | --- |
| W2 审批卡 | 后端解冻 `capabilities.approvals` + 安全评审（第 1 层唯一有后端改动者） |
| W8 通知与重连 | SDK A2（传输层重连） |
| W12 技能管理 | 协议增量（`skill/create|update|delete`），建议与 D2 同批 |
| W14 消费 SDK 新能力 | A3（事件面补齐） |
| W1 / W10 | 同批做（都改 composer 输入模型，避免两次重写） |

### 3.5 方向四 · 待立项（生产化硬指标）

**性质：不是「不做」，而是「未排期」。** 这些是 `11-webui-production-readiness.md` 中未被方向一/三纳入的项，需单独立项与排期，否则会变成「以为做过、其实没做」。

| 项 | 内容 |
| --- | --- |
| §1.1 正式认证与令牌管理 | 当前无正式认证 |
| §1.2 Origin / CSP / CSRF 策略 | 未做 |
| §2.2 归档 / 回收站 | 仅删除，无归档 |
| §2.3 批量操作 | 未做 |
| §3.1 统一 Request ID / 错误上报 | 未做 |
| §3.2 健康检查 | 未做 |
| §3.3 配额 / 限流 | 未做 |
| §4.1 只读 / 不可访问标识 | 未做 |
| §4.2 工作区重命名 | 会话重命名有，工作区无 |
| §6.4 无障碍与 E2E 回归 | 无 E2E |
| WP-5 协议词汇与概念对齐 | 方法 / 事件 / 概念映射表与边界拍板；待 SDK/UI 稳定后启动 |

---

## 4. 待拍板

| # | 问题 | 选项 |
| --- | --- | --- |
| Q1 | 平台矩阵策略 | ✅ **已定（2026-09-09）：仅 Windows**——站点口径收敛（C1-2），A6 延后至出现真实非 Windows 需求 |
| Q2 | 站点托管与域名 | ⏸ **延后（2026-09-11，用户决定：站点部署卡点相关问题延后）**——选项 VPS + 自定义域名 / EdgeOne / GitHub Pages **不拍板**；R7 / R32 / R6② 随之延后（不再挂在计划上），重开条件＝用户提出站点部署 / 域名 / 分发可达性需求 |
| Q3 | 附件图片输入时机 | ✅ **已定（2026-09-10）：等协议词汇与概念对齐**——W10 属「待反馈」批，不进本轮三闭环（`19` §3 W10） |
| Q4 | SDK 发版节奏 | ✅ **已定（2026-09-09）：A1+A2 先发 `0.1.0-beta.3`**——已于 2026-09-10 发布 |
| Q5 | 插件格式策略 | ✅ **已定（2026-09-09）：先只做兼容层** |
| Q6 | 设置里 provider 的写入目标 | ✅ **已定（2026-09-10）：① 写 `~/.agent-store/config.toml`**（与「唯一来源」一致）——解锁 W11 provider 分区 |
| Q7 | `auto_update` 默认值 | ✅ **已定（2026-09-10）：① 改实现以区分官方/第三方**（按 `02` §8：官方默认开、第三方默认关，V1 不自动更新第三方来源）——解锁 T14 |

---

## 5. 批次与顺序

| 批次 | 内容 | 出口 |
| --- | --- | --- |
| **第 1 批** | A1（进程生命周期 P0）+ `typescript-sdk.md` §5.1/5.2/5.4/5.5 同步 + C1（站点三处事实硬伤） | 长会话不再卡死；站点不再 404 / 过度承诺 |
| **第 2 批** | A2（传输止血）+ `typescript-sdk.md` §4.2 同步 + 补 `engines` + 发 `0.1.0-beta.3` | **SDK 转维护模式** |
| **第 3 批** | W5（产物面板）+ W1（命令面板 V1：`/` + `@`）+ W13（市场管理） | **webui 三闭环验收全绿** |
| **第 4 批（已收口）** | 规范 4 处修补 + D2（Schema 与校验器，✅ T19）+ 规范反向验证（✅ T20）+ 转现行正文（✅ T21） | ✅ `17`/`18` 转现行正文（2026-09-10） |
| 待反馈 | 其余 webui：W2 / W3 / W4 / W6 / W7 / W8 / W9 / W10 / W11 / W12 / W1b / W14 | ✅ **已解除（`21` D1=A）**：纳入批 3 / 批 4 |
| 待决策 | C5（域名 / HTTPS，等 Q2） | ✅ **已决策（`21` D2=B）**：自定义域名 + HTTPS；落地需域名与 DNS 访问权 |
| 维护模式待议 | A3 / A4 / A5 | ✅ **已解除（`21` D1=A）**：纳入批 2 |
| 未排期 | 方向四全部 | ✅ **已解除（`21` D1=A）**：纳入批 5（仍需单独立项页） |

> 第 3 批全部是「后端/client 已就绪、只差前端接线」，不碰协议，风险最低、见效最快。

### 5.1 退出条件（✅ 已采纳 2026-09-09）

三条线现在都缺「什么算完成」，这是无限打磨的根源。建议为每个方向写死退出条件：

| 方向 | 退出条件 | 退出后状态 |
| --- | --- | --- |
| ① SDK + 站点 | A1 + A2 修完；发 `0.1.0-beta.3`；C1 三处事实错误修正 | **维护模式**：新需求须有真实外部 issue 才排期（A3/A4/A5 不再主动做） |
| ② 插件与市场规范 | 4 处真缺陷修补 + 「已知偏差」小节落地 | **现行正文**：等第一个外部发布者来挑战 |
| ③ WebUI | 本轮只做三个闭环（W5 产物面板 / W1 命令面板 / W13 市场管理）验收全绿 | 其余按真实使用反馈排，不按「对齐 Codex 的完整性」排 |

> 依据：npm 下载量 API 对四包**均无可观测数据**（发布仅数小时 / 无外部下载）——目前没有可观测的第三方使用，SDK 的 A3/A4/A5 收益依赖尚未出现的用户。

> **⛔ 本节的限制已于 2026-09-10 由 `21` D1=A 全部解除**：A3/A4/A5、其余 WebUI、方向四重新纳入执行（R1–R33），改按 `21` 文末的**批 0–批 5** 顺序推进。本节保留为「当时的退出条件与理由」，**不再作为排期约束**。

### 5.2 开发计划（任务级，按优先级）

**排序原则**：**P0 = 已确认的错误或会阻断使用**（修完即消除误导/卡死）；**P1 = 直接产生用户或开发者价值**；**P2 = 收尾与加固**。同优先级内按依赖顺序——被依赖者先做。

#### P0 · 立刻（错误与阻断）

| # | 任务 | 交付物 | 验证 |
| --- | --- | --- | --- |
| T1 | SDK stdout 背压修复 | `spawn.ts` 就绪后持续排空 stdout（或 `onLog` / `logFile`） | 合成子进程写 > 1MB 日志不被阻塞；真机 3 轮 turn + 市场树扫描不卡 |
| T3 | 站点下载 CTA 平台守卫 | `DownloadCTA.tsx` 只在 Windows x64 给直链 | 非 Windows UA 下按钮指向发布页并显示「仅 Windows x64 已发布」 |
| T4 | 兼容性矩阵收敛 | `compatibility.md`（zh/en） | 只列 Windows x64「已发布」，其余「未提供」；中英一致 |
| T6 | 文档同步（A1） | `typescript-sdk.md` §5.1 / §5.2 / §5.4 / §5.5 | 文档描述与 `spawn.ts` 公共面逐项对应（与 T1 同批，不攒债） |

> ✅ **P0 批次已执行（2026-09-10）：T1 / T3 / T4 / T6 完成**，见下方「已完成」表。

#### P1 · 核心价值

| # | 任务 | 交付物 | 验证 | 依赖 |
| --- | --- | --- | --- | --- |
| T2 | 运行时生命周期回调 | `onExit` / `exited`、`env` / `cwd` 透传 | 异常退出时 `exited` 正常 resolve；`env` 生效 | T1 |
| T7 | 传输层健壮性 | `transport.ts`：`close` 清监听、`connect` 超时、并发竞态、结算挂起 promise | 并发 / 超时 / 断线单测 | — |
| T8 | 重连游标重置 | 订阅对象重连后重置 `lastSeenSequence` | 重连后事件不丢 | T7 |
| T9 | 包元数据 | 三包补 `engines` / `repository` / `sideEffects` | `npm view` 字段齐全 | — |
| T10 | 发 `0.1.0-beta.3` | npm 四包 | 第三方干净目录 `npm i` 后 `launchClient` 成功 | T7, T9 |
| T11 | 文档同步（A2） | `typescript-sdk.md` §4.2 | 与 `Transport` 公共面一致 | T7 |
| T12 | 产物面板（W5） | 按会话 / 按 Run 的产物列表 + 预览 + 下载 | AC-5 | — |
| T13 | 命令面板 V1（W1） | `/` + `@` + 键盘导航 + IME 兼容 | AC-1 | — |
| T14 | 市场管理补全（W13） | auto-update 开关 + 条目导入 + 级联确认 + 注册表字段 | 切换后 `market/list` 回读一致；导入带溯源；级联移除前列出影响面 | Q7 决策 |
| T15 | 三闭环验收 | 验收记录 | AC-1 / AC-5 / W13 全绿 | T12, T13, T14 |
| T5 | 部署口径对齐 | `site/README.md` | 与 `deploy-site.yml` 实际触发方式一致 | — |

#### P2 · 收尾与加固

| # | 任务 | 交付物 | 验证 | 依赖 |
| --- | --- | --- | --- | --- |
| T19 | D2：Schema 与校验器 — ✅ **已完成（2026-09-10）** | `docs/agent-store/schemas/{plugin,marketplace}.schema.json` + `scripts/check-agent-store-market.mjs`（+ `.test.mjs`） | ✅ 三个真实市场 `0 error`；14 个非法样例被拒且带 `文件#/指针`；已接入 `--emit-listings` 与 `check` | — |
| T20 | 规范反向验证 — ✅ **已完成（2026-09-10）** | 验证记录：`17` §10 P1–P4、`18` §11 D3–D8 | ✅ 逐条核对 `18` §5/§7 与 `17` §4/§6/§7 + `18` §9，并对三个真实市场做字段普查（`--census`）；**新登记 8 条**，其中 2 条已修（`18` D5 清单抓取漏 15s 超时、`17` P2 依赖丢名），其余按建议「改规范 / 列待办」择一 | T19 |
| T21 | 转现行正文标记 — ✅ **已完成（2026-09-10）** | `17` / `18` 状态行改为「**现行正文（未发版，可改）**」+ 覆盖范围与已接受缺口清单 | ✅ 21 个批次任务全部收口 | T20 |

#### 已完成

| # | 任务 | 完成 |
| --- | --- | --- |
| T1 | SDK stdout 背压修复（`spawn.ts` 就绪后持续排空 + 回归用例） | 2026-09-10 |
| T2 | 运行时生命周期回调（`exited` / `onExit`、`env` / `cwd` 透传 + 用例） | 2026-09-10 |
| T3 | 站点下载 CTA 平台守卫（`DownloadCTA` 仅 Windows x64 给直链） | 2026-09-10 |
| T4 | 兼容性矩阵收敛（仅 Windows x64「已发布」，中英一致） | 2026-09-10 |
| T5 | 部署口径对齐（`site/README.md` 与 `deploy-site.yml` 手动触发一致） | 2026-09-10 |
| T6 | 文档同步（A1）：`typescript-sdk.md` §5.4 / §5.5 | 2026-09-10 |
| T7 | 传输层健壮性（并发 `connect` / `connectTimeoutMs` / `close` 结算与清监听 / 陈旧 socket 隔离 + 6 用例） | 2026-09-10 |
| T8 | 重连游标重置（`Transport.onLifecycle` + `rearm()`，游标归零 + 重订阅 + 全量重放 + 7 用例） | 2026-09-10 |
| W8 | 通知与连接状态层（Toast 层 + 断线横幅 + 一键重连；**余项已于批 3 补齐**：D4=A 多标签选主与 catalog / 后台 Run 通知，见 §5.2 R13 行与 `19` §3 W8「进度」） | 2026-09-10 |
| T9 | 包元数据（`protocol`/`client`/`sdk` 补 `engines` / `repository` / `sideEffects`） | 2026-09-10 |
| T10 | 发 `0.1.0-beta.3`（四包发布至 npmjs，`tag=beta`，`latest` 未动） | 2026-09-10 |
| T11 | 文档同步（A2）：`typescript-sdk.md` §4.2 实现契约 | 2026-09-10 |
| T12 | W5 产物面板（⚠️ 按 D-W5-1 收敛为宿主面文件服务：会话 workspace 的列表 / 预览 / 下载 / 评论入下一条消息；**无**「按 Run 归属」与「接受 / 回退」） | 2026-09-10 |
| T13 | W1 命令面板 V1（`/` 命令 + `@` 提及 + ↑↓/Enter/Esc + IME 守卫 + `Cmd/Ctrl+K`；无 `+` 菜单抢占） | 2026-09-10 |
| T14 | W13 市场管理补全（auto-update 开关回读 / 条目级导入 / 注册表字段 / 级联移除确认列出影响面）+ Q7 ① 后端官方-第三方 `auto_update` 区分（⚠️ `revision`、上次刷新未做 → D-W13-1） | 2026-09-10 |
| T15 | 三闭环验收（✅ **全绿**：协议级 live 验收 11/11，脚本 `web/scripts/sdk-live-w13-acceptance.ts`；UI 点击与键盘级 = 用户人工审查通过，期间排掉 3 个阻塞项 D-W13-2 / D-STREAM-1 / D-STREAM-2；记录见 `19` §5） | 2026-09-10 |
| T19 | D2：Schema 与校验器（`docs/agent-store/schemas/*.json` + `scripts/check-agent-store-market.mjs`；三个真实市场 `0 error`、自检 17/17、单测 6 pass；已接入发布脚本与 `check`） | 2026-09-10 |
| T20 | 规范反向验证（新登记 8 条偏差：`18` D4–D8、`17` P1–P4；已修 D5 / P2；其余按建议「改规范 / 列待办」择一；普查模式 `check-agent-store-market.mjs --census`） | 2026-09-10 |
| T21 | `17`/`18` 转现行正文（状态行 + 覆盖范围 + 已接受缺口清单：`17` P1/P3/P4、`18` D4/D8） | 2026-09-10 |
| T16 | 17 字段必填 / 默认 + 偏差小节 | 2026-09-09 |
| T17 | 18 `marketplace_id` / `content_digest` / 跨市场命名 + 偏差小节 | 2026-09-09 |
| T18 | 19 过时引用修正 + W1 拆分 | 2026-09-09 |

> **第 2 批已收口**：T2 / T5 / T7 / T8 / T9 / T10 / T11（+ W8）全部完成；`0.1.0-beta.3` 于 2026-09-10 发布。
> **T8 的重连策略（已实现）**：T7 让 `close()` 清空 `onNotification` 监听器，故「只重置 `lastSeenSequence`」无法恢复投递；实现为 `Transport.onLifecycle` 广播连接丢失 → `EventSubscription.rearm()` / `ConversationSubscription.rearm()` 重建订阅（`run/subscribe`）并全量重放，宿主持有的 `AppServerClient` 负责重挂通知桥并重跑 `initialize`。
> **发布环节要求**：npm 账号开启 2FA，`npm login` 的 token 不带 bypass-2FA（首次发布返回 403），必须用 granular token（bypass 2FA）经临时 npmrc 注入；临时 npmrc 只写 `_authToken=${NODE_AUTH_TOKEN}` 变量引用，token 只入进程环境变量。

#### 任务总表（按完成度分组：✅ 全部完成 / 🟡 部分完成 / ⏸ 未完成，2026-09-11 重排）

**口径**：本表是 `16` §0–§5 与 `17` §10 / `18` §11 / `19` §2–§4 的**汇总视图**，单一事实源仍是各文档原文；已完成项见上方「已完成」表与各文档的「已知偏差」登记。

**类别**：A = SDK 与站点（方向①）· B = WebUI（方向③）· C = 规范待补齐项 · D = 运行时与流式收尾 · E = 待立项。

**分组口径**：**✅ 全部完成** ＝ 该行承诺的范围全部落地且已验证；**🟡 部分完成** ＝ 主体已落地、仍有登记在案的缺口（缺口本身也算未做，故不计入 ✅）；**⏸ 未完成** ＝ 一行未动（含「有意不做」「等外部条件」「已划入待排期 / 未排期」三种，理由与解锁条件见「卡点决策」表）。分组只是**视图**：逐项明细与证据原文一律保留下方，不因重排而丢失。

##### ✅ 全部完成（22 项）

| # | 类别 | 任务 | 完成 | 关键交付 / 证据（一句话） |
| --- | --- | --- | --- | --- |
| R1 | A | A3 事件面补齐 | 2026-09-10 批 2 | 前置环形缓冲 + gap 自动追平 + `onError` + `event_type` 收窄 + 解码器进包 |
| R2 | A | A4 HTTP 绑定公共面 | 2026-09-10 批 2 | 包内 `HttpTransport`（现口径 45 / 63 的来源）；只收编传输，不把 `fs/browse` 变协议方法 |
| R3 | A | A5 余项（protocol 单测） | 2026-09-10 批 2 | 3 文件 / 17 例；`event_type` 封闭联合用 `@ts-expect-error` 钉死 |
| R4 | A | C2 余项（升级与迁移指引） | 2026-09-11 | 新增站点 `upgrade` 页（双语）+ dist-tag 语义 + 逐版本升级步骤（以实测 ground truth 为准） |
| R5 | A | C3 开发者文档深度 | 2026-09-10 批 2 | 双语 §7–§11；示例代码做真实类型检查，Node 示例对真实二进制端到端跑通 |
| R8 | B | W2 审批卡（D3=B） | 2026-09-10 批 3 | `run/answer-decision` + 三路 CAS + 能力位派生；e2e 8 passed，红线（`always_allow` 等）被参数量拒 |
| R9 | B | W3 中断与引导 | 2026-09-10 批 3 | `steerAvailability` 先判后发 + 「先读版本再 CAS」；12 例 |
| R10 | B | W4 计划与待办树 | 2026-09-11 批 3 | additive `run/plan` 快照（结案 D-W6-1），与 W6 双向互跳 |
| R11 | B | W6 Run 状态树 | 2026-09-10 批 3 | `run-tree` 投影 + `RunDetail` 重写 + 原始事件降级为 `details`（缺口由 R10 补齐） |
| R12 | B | W7 重试 / 编辑 / 重新生成 | 2026-09-10 批 3 | 幂等键策略分开建模：`resend` 复用旧键，`retry`/`regenerate`/`edit` 必须新键 |
| R13 | B | W8 余项（多标签与通知） | 2026-09-10 批 3 | `navigator.locks` 选主 + 四档降级；断线窗口内终态通知已补正 |
| R18 | B | W14 消费 SDK 新能力 | 2026-09-10 批 2 | `activity.ts` 只留展示策略，事件全部经包内解码器（已 grep 核实无直读） |
| R19 | B | W1b 命令面板 V2 | 2026-09-10 批 3 | `palette-model` 分组 + 禁用行带原因；**刻意不做**协议里不存在的「归档」 |
| R21 | B | W13 余项 | 2026-09-10 批 1 | 移除前受影响快照改由服务端前置投影（`market/get` 的 `installed_count`） |
| R25 | C | `18` D8 条目安装快照 | 2026-09-10 批 1 | `market/get` 条目带 `snapshot.installed_count`（前端消费并入 R21） |
| R26 | C | `18` D4 ① 条件请求 | 2026-09-10 批 1 | 迁移 `057_*` 增 `source_etag` / `source_last_modified` + 真条件请求 |
| R27 | C | `auto_update` 执行逻辑 | 2026-09-10 批 1 | 调度器只轮询「开关为开且官方源」的市场，逐轮走 revision / ETag 短路 |
| R28 | C | `18` D7 ① 本地化变体 | 2026-09-10 批 1 | 通用 `localized` 映射三处 additive + 客户端 D8=A 回退链 |
| R29 | D | D-STREAM-1 ① | 2026-09-10 | 兜底转发改用派生 id，不再顶掉本轮首段思考 |
| R30 | D | D-STREAM-2 ① | 2026-09-11 批 5 | `Finish` 不再等待蒸馏 child（D9=A 后台 spawn，取消语义不变） |
| R31 | D | D-STREAM-2 ② | 2026-09-11 | 忙态文案分级为「正在收尾…」（纯 reducer，`isProcessing` 语义不变） |
| R34 | D | D-TEST-1 ② 编译门 | 2026-09-11 批 7 | token 仓储测试语义化改造；整仓 `cargo check --tests --workspace` 由 exit 101 → **exit 0** |

##### 🟡 部分完成（5 项：主体已落地，缺口登记在案）

| # | 类别 | 任务 | ✅ 已完成 | 🟡 未完成（缺口本身也算未做） | 解锁条件 / 下一步 |
| --- | --- | --- | --- | --- | --- |
| R6 | A | C4 站点机制 | 中英同步校验脚本（`check-docs-sync`）· changelog 页（批 4） | ① **站内搜索**；② **市场数据刷新纳入发布流程——⏸ 已延后（2026-09-11 用户决定：站点部署卡点相关问题延后）** | ① **订正（2026-09-11 实测）**：原「会引入构建期索引这一新守卫面」**不成立**——`site/app/lib/docs.ts:10-14` 已用 `import.meta.glob(..., { query: "?raw", eager: true })` 把 docs **全文内联进 bundle**，客户端本来就拿得到全文，故搜索可以是**纯客户端、零构建步骤、零新守卫面**；剩下的只是「做不做」的产品取舍（范围建议仅 docs，市场侧已有自己的客户端过滤 `site/app/pages/Market.tsx:66-71`）；② **不再挂计划**；重开条件＝用户提出站点部署 / 发布流程需求 |
| R14 | B | W9 模型能力与用量 | 费率 / 上下文 / 视觉投影 · 发送前校验 · 接近上限建议 · **按 turn 的 token + 金额**（批 6） | **逐轮 usage 未持久化**：重载后不显示、新一轮开始即清空。**2026-09-11：B 档 ③ 已定设计但未实现**（见 §5.3「本轮落地记录」未做表） | **决策已定**（D-W9-1 取①）：给 `app_server_context_usage` **加两列**（`last_turn_input_tokens` / `last_turn_output_tokens`，可空，走**新迁移**、不改既有迁移与 `057_*`）；写入点＝`nomifun-conversation/src/stream_relay.rs:6167` `persist_app_server_context_usage(metrics)`（已持有本轮 token）；读回＝`get_app_server_context_usage` + App Server 投影 + 前端重载显示。**未做的原因**＝这是跨 5 crate + 前端的链，只加列不接读回就是半条链路；解锁＝单开一轮按该顺序落地，验收＝重载后仍显示上一轮 token 与金额 |
| R16 | B | W11 设置 Dialog | provider 分区（`config/get` / `config/set`）+ nav 八→二（批 4）+ **`agent` 分区（记忆蒸馏开关，2026-09-11，见 §5.3「本轮落地记录」A 档 ②）** | **其余五分区不渲染**（account / plugin / advanced / lab / archived）——判定为「没有可自证的真实面」，非缺数据 | 逐分区解锁条件见「卡点决策」R16 行；`agent` 已解锁（宿主**已消费** `[memory].distill_enabled`：`apps/agent-store/src/main.rs:318`） |
| R17 | B | W12 技能 / 专家管理 | **后端写面** `skill/create \| update \| delete` + `skill/copy` + `origin` / `writable` + 归属与卸载语义 + **WebUI 管理面**（列表按来源区分 / 新建 / 编辑 / 复制 / 删除，只读来源给原因）——**2026-09-11 全部落地，见 §5.3「本轮落地记录」** | ✅ **完成（2026-09-11）** | 无——原三项缺口全部闭合：① 编辑面改为 `skill/update` **服务端字段级合并**（正文按「替换」语义，故不需要 `skill/source`）；② `skill/copy` + 目录级复制原语已落地（含链接拒绝与目标占用判定）；③ UI 已接（含 `skills.*` 双语文案） |
| R20 | B | W5 余项 | 产物 list 的 size / type / mtime（2026-09-11） | **R20a「按 Run 归属」**（可做） · **R20b「接受 / 回退」**（等 Artifact Phase）——**2026-09-11 拆条**，不再一条挂死 | R20a：**订正**——归属所需数据**已存在**，`run/plan`（批 3 已落地，`nomifun-app-server/src/lib.rs:3873-3884`）已投影 attempt 的 `output_files`（`nomifun-agent-execution/src/runtime_adapter.rs:141`），故「产物 → 所属 Run/Step」**不需要新协议能力**即可表达，宿主面文件服务（`/api/fs/list`）只需补一层映射；R20b：仍需协议能力（`capabilities.artifacts` 硬编码 `false`，`nomifun-app-server/src/lib.rs:401`）+ Run 归属数据 + 接受/回退写面三件齐备（D-W5-1 已定：当前收敛为宿主面文件服务） |

##### ⏸ 未完成（7 项：一行未动，含有意不做 / 等外部条件 / 已划队列）

| # | 类别 | 任务 | 状态 | 为什么没做（卡点） | 解锁条件 |
| --- | --- | --- | --- | --- | --- |
| R7 | A | C5 部署与可达性（域名 / HTTPS） | ⏸ **延后（2026-09-11 用户决定：站点部署卡点相关问题延后）** | 入口仍是裸 IP + HTTP 的 `irm \| iex`；卡**外部资源**（域名与 DNS 访问权） | **已延后，不再挂计划**；重开条件＝用户提出站点部署 / 域名 / 可达性需求（届时按 Q2 三选一执行） |
| R32 | D | D-SDK-1 ④ 缩短镜像代价 | ⏸ **延后（2026-09-11 用户决定：站点部署卡点相关问题延后）** | 大头（HTTPS + CDN）与 Q2 同源，已随之延后；**订正**：其**本地子项**（`market_source.rs:166` 的 `BATCH = 32`、并发与首载超时口径）**不受此延后阻塞**，只是本轮不做；「增量跳过」已由 R26 的条件请求落地 | **已延后**；重开条件＝用户提出分发 / 镜像带宽需求。本地子项如需做，属独立小改动（不依赖域名） |
| R15 | B | W10 附件 / 图片输入 | ⏸ **等「载体选型」拍板**（原写「等协议词汇与概念对齐」，2026-09-11 订正） | **订正（2026-09-11 实测）**：`content` 图片载体**并非破坏性变更**——桌面 API 已有 `files: Vec<String>` 载体，协议的 `attachments` 可 **additive** 落地；真实卡点是**载体选型决策**（路径引用需先定宿主侧文件准入与生命周期 vs 内联 base64 的体积/幂等）＋ 词汇对齐 的 `thread/turn/item` 重命名会返工（Q3 已定） | **订正（2026-09-11）**：解锁条件拆成两件——① **载体选型**是**独立决策、不依赖 词汇对齐**（附录见 `22-webui-productionization.zh.md` §5），拍掉即可做 additive `attachments`；② `thread/turn/item` 重命名**只影响字段命名、不影响载体语义**，即「现在做、词汇对齐 时改一次名」与「等 词汇对齐 再做」可比较。**当前处置：单独拍载体选型**（倾向路径引用：复用会话 workspace 既有准入与生命周期，避免 base64 新开体积/幂等两个面），不再笼统写「等 词汇对齐」 |
| R22 | C | `17` P1 MCP 连接器 `env` 明文写入快照 | 🔴 **安全阻塞** | 修法是「值不入快照、只留键名 + 注入引用」。**订正（2026-09-11 实测）**：加密原语**已有**（`nomifun-common/src/crypto.rs` 的 `encrypt_string`/`decrypt_string` + workspace `aes-gcm`），值侧存放位置**已拍板**（`21` **D5 = C：config.toml + env，不落库**）；真正缺的是**「键名 → 来源」的引用语法 + MCP spawn 时的解析注入点 + 导入侧改写**（实测 `nomifun-mcp` 内无任何 env 展开 / `secret_ref` 机制）。只改一半（值删了但无处填）＝导入后连接器**静默不可用**，故两步须同批 | 引用语法 + 解析注入点 + 填写入口（见 §5.3 D 档） |
| R23 | C | `17` P3 依赖 SemVer 范围解析与阻断 | **已批准待排期**（原「v1.1 队列」） | **两处订正（2026-09-11 实测，二次核对）**。**（a）决策不存在卡点**：`21` 速览 D6 = A「按规范实现阻断 + 配置逃生口，作为 v1.1 的破坏性变更写进 changelog」，**已批准**；原写「`17` 已冻结，现在改会让冻结状态失效」是把**排期**误写成**决策阻塞**。**（b）「调用点全在自身单测」不成立**：`validate_dependencies` **已接进真实加载路径**——`nomifun-extension/src/registry_helpers.rs:46-61`（`load_and_validate` 在 `:56` 调用，并在 `:57` 用 `load_order` **真的排序**）← `registry.rs:111`（`initialize_with_scan_paths`）/ `registry.rs:166`（`hot_reload`）；`dependency.rs:112-129` 的 `=`/`^`/`~` 语义均有用例。**真缺的只有「阻断」**：`registry.rs:136-138` 只 `warn!`，`dep_result.valid` **从未被用作门**（且 `hot_reload` 在 `:166` 连结果都丢弃）。**（c）层级必须分开**：`17` §10 P3「全仓无 semver 解析」说的是**另一层**——导入层 `nomifun-importer/src/import.rs:664-681` 只把依赖登记为组件、从不参与安装决策；与 extension 层的 `LoadedExtension.manifest.dependencies` 是两个不同消费面，原来把两层并成一句，导致「已实现」与「未实现」互相打脸 | **解锁条件订正**：不再是「等 v1.1」（D6 已满足；版本框架本身已由 `16` §7 决策 4 废止）。落地形式＝**阻断默认开 + 逃生口**（`[import] strict_dependencies` 默认阻断，临时放行需显式配置）：规范未发版、无既有消费者，故实现即生效、不是破坏性变更、不需要公告窗口。工作量＝**一个策略选择 + 改一处判定**（extension 层），不是「实现解析」 |
| R24 | C | `17` P4 `strict` 完全未实现 | **已批准待排期**（同 R23） | `21` D6=A 已批准（含 `strict` 阻断）。**实测确认**：extension crate 内搜 `strict` → 0 命中（字段与阻断路径均无）。**归属订正**：`17` §3/§7 承诺的 `strict=true` 且缺 `plugin.json` 即阻断，是**导入期**语义（`02` §11.1），应挂在 **importer 层**、与 `PluginManifest` 同级（`17` §10 P4 原文已如此指向），**与 extension 层不是同一物**——原 R24 行写「同 R23」会把两层混为一谈 | 与 R23 同批（D6=A），但**归属 importer 层**；落地需打通导入期阻断路径（与 `MissingIdentity` 同级） |
| R33 | E | 方向四 + WP-5 协议词汇与概念对齐 | ⏸ **本体未排期（立项页已建）**——C 档承诺的「只开立项页」已兑现（2026-09-11） | 11 项体量超过「继续完成任务表」（原判定成立，不是「不做」的理由），且 词汇对齐 属重构型工作（SDK / UI 稳定后再动） | ✅ 立项已兑现——`22-webui-productionization.zh.md` 把 11 项拆成**安全类 / 可观测类 / 功能类 / 协议词汇与概念对齐** 四组，逐条给「可验收条目 + 边界 + 依赖」，并写死**不做假保护**红线（未实现即不呈现、不做禁用占位、`capabilities.*` 每个 `true` 须指到实现点）。**余项**：11 项的优先级与 4 条「未验证」的取证见该页 §6 未决（V1–V4） |

> **计数**：✅ 22 项 · 🟡 5 项 · ⏸ 7 项 = **34 项**（R1–R34，无遗漏、无重复；**R20 于 2026-09-11 拆为 R20a / R20b 两个子项，不新增顶层编号，故总数不变**）。**待拍板项：无**——Q2 已于 2026-09-11 按用户决定「站点部署卡点相关问题延后」（连带的 R7 / R32 / R6② 一并延后、不再挂计划）；其余决策（Q1 / Q3–Q7、D1–D12）均已定，见 `21-open-decisions.zh.md`。
>
> **2026-09-11 C 档复核后的归属变化（不改变上表计数，只改「为什么没做」的定性）**：
> - **R23 / R24**：⏸ 未完成 **不变**（确实未做），但定性由「等外部条件（v1.1 解冻）」改为「**已批准待排期**」——`21` D6=A 已批准实现阻断，卡的是排期不是决策（见 §5.3 C 档重写）。
> - **R20**：拆为 **R20a 归属**（可做，数据已由 `run/plan` 提供）与 **R20b 接受 / 回退**（等 Artifact Phase）；原「一条挂死」不成立。
> - **R15**：解锁定性由「等协议词汇与概念对齐」收窄为「**载体选型独立拍板**」——选型不依赖 词汇对齐。
> - **R33**：立项页已建（`22-webui-productionization.zh.md`），C 档唯一承诺动作已兑现；R33 本体仍未排期。
> - **R6①**：阻塞理由订正——docs 全文已内联进 bundle（`site/app/lib/docs.ts:10-14`），不存在「构建期索引守卫面」；是否做改为纯产品取舍。

#### 逐项明细（R1–R34，证据原文，按编号）

> 分组只在上表；本节保留每一行的**完整未完成内容 / 状态 / 前置**与证据链（含各批实测输出），是「单一事实源」，改口径请改这里。

| # | 类别 | 任务 | 未完成内容 | 状态 / 前置 |
| --- | --- | --- | --- | --- |
| R1 | A | A3 事件面补齐 | ~~`ConversationSubscription` 对齐 Run；payload 解码器进包；`event_type \| string` 逃逸；前置事件缓冲~~ ✅ **已完成（2026-09-10，批 2）**：①**前置环形缓冲**（默认 256，溢出丢最旧并计数经 `onError` 上报，首监听器注册时按序 flush）；②**gap 检测**（`sequence > lastSeen + 1 && lastSeen > 0`，服务端 `conversation_event_sequence` 对单会话自增故连续）→ `onResync("gap")`+**自动追平**（注入 `fetchMessages` 拉最新一页 transcript 经 `onBackfill` 交付，并发信号合并为一次取数，`autoResync:false` 可关）；③`onError`（追平/`rearm` 失败不再静默）；④**`event_type` 逃逸已修**（`ConversationEventType` 封闭联合）；⑤**解码器进包**：`@flowy-agent-store/protocol` 新增 `decodeConversationEvent`（判别联合 + 归一化「activity kind=thinking」→ `message.thinking` + `unknown` 兜底保留原始类型）与 `thinkingData`/`tipsData`/`toolCallData`/`parseContextUsage`/`contentToText`。**验证**：client 47 例（含新增 8 例订阅行为）、protocol 26 例（含 9 例解码器）、web 全量 **91 passed / 1 skipped**、typecheck 0 错 | ✅ 完成 |
| R2 | A | A4 HTTP 绑定公共面 | ✅ **已完成（2026-09-10，批 2）**：包内新增 `HttpTransport`（`web/packages/client/src/http-transport.ts`），把协议方法名映射到服务端手写 REST 路由，**覆盖 43 / 56 个方法**（R8 加 `run/answer-decision` 后为 **44 / 57**；未覆盖 13 个：`initialize`/`initialized`、`workspace/create`、`conversation/model-options`、`conversation/update`、`conversation/subscribe|unsubscribe`、`run/subscribe|unsubscribe`、`agent/list|get`、`team/list|get`）。**映射表逐条读 handler 函数体核实**（不按路由清单反推），每条自带 `source` 记录证据（如 `conversation/send` 与 WS 臂共用 `send_conversation_message_for_user`、`run/steer` 共用 `execute_steer_run`、目录/市场/导入/安装类共用 `*_impl`）。三条语义边界按定案执行：①**请求-响应面**——`onNotification` 返回空订阅、`notify` 抛错，类注释写明**不等价于 WS**，实时事件与订阅仍必须走 `WebSocketTransport`；②**无绑定方法抛 `TransportError`**，其中 `*/subscribe|unsubscribe` 单独标注 WebSocket-only，不静默降级；③**`/api/fs/*` 留宿主**（`05` §2.1.1 明写它不是协议方法）不进包。④握手按 `05` §2.1.1「每次调用独立握手」实现（`connect()` 为 no-op），另公开 `openConnection()` 供宿主侧路由（`/api/fs/*` 与 HTTP-only 的 `POST /workspaces`）借用就绪连接 id；导出 `appServerErrorFromWire` 统一 wire-error 映射，webui 的 `httpHandshake`/`httpError` 已收编（`/api/fs/*` 与 register 仍留本地）。**新发现偏差**：`POST /workspaces`（`workspace_register`：无 body、服务端分配 id、返回 `{id}`）与 WS `workspace/create`（`{path}` 入参、返回 `WorkspaceView`）**不是同一操作**，`05` §2.1.1「workspace/* 一一对应」的表述已修正。验证：`http-transport.test.ts` **13 例**（握手次数与顺序、路径/查询/body 拆分、缺路径参数报错、WS-only 与无绑定方法报错、wire-error 映射、baseUrl 归一、路由表自带 source）；client 包 10 文件 **58 passed**；web 全量 vitest **104 passed / 1 skipped**；`bun run typecheck` 0 错 | ✅ 完成 |
| R3 | A | A5 余项 | ~~`protocol` 包单测（类型守卫 / 错误分类）~~ ✅ **已完成（2026-09-10，批 2）**：包内从 0 个测试文件到 **3 个 / 17 例**——`localized.test.ts`（回退链 / 族优先级 / 基线兜底）、`errors.test.ts`（`AppServerError` 的 code/request_id/retryable/details 与 details 默认 `{}`、`TransportError` 默认不可重试、`ProtocolError` 三种 kind、`RequestTimeoutError`、跨类不误判、`formatError` 单一渲染）、`wire-types.test.ts`（`ConversationEventType` **封闭联合**用 `@ts-expect-error` 锁定——有人再加 `\| string` 时 `typecheck` 立即报「Unused '@ts-expect-error' directive」；通知联合按 `method` 收窄）。元数据（`engines` / `repository` / `sideEffects`）已由 T9 补 | ✅ 完成 |
| R4 | A | C2 余项 | ~~两条分发路径（安装包 / npm）互引用~~ ✅ **已做（2026-09-10，批 0）**：`quick-start` 与 `typescript-sdk` 中英各加一条互引用；~~升级与迁移指引（D10=A 已解锁）、dist-tag 说明~~ ✅ **已完成（2026-09-11）**：**新增站点 `upgrade` 页**（`site/content/docs/{zh-CN,en-US}/upgrade.md`，中英各 172 行、结构对称；`site/app/lib/docs.ts` 的 `DOC_ORDER` + `DocSections` 与两语言 `site/app/i18n/*.ts` 的 `docs.sections.upgrade` 同步），内容＝兼容性口径（beta 期不承诺向后兼容 / 破坏性走 minor + changelog 明示）· **发布事实表**（版本 / 发布时间 UTC / 当前 dist-tag / 与上一版的实质差异）· dist-tag 语义与两个陷阱 · 裸安装与范围解析实测 · 固定确切版本 · **逐版本升级步骤**（beta.2→beta.3、0.1.0→beta.*）· 自查命令 · 未发布差异（`event_type` 收窄尚未随任何版本发布）· changelog 边界（明写「独立 changelog / release notes 页尚未建设，见 R6」，不发明版本历史）；`typescript-sdk.md` §1「版本状态」两语言同步改成与实测一致（原先只写「固定版本如 beta.2」、不提 dist-tag 语义）并指向新页。**实测 ground truth**：`versions` = `0.1.0-beta.2` / `0.1.0-beta.3` / `0.1.0`，`dist-tags` = `{beta: 0.1.0-beta.3, latest: 0.1.0-beta.2}`（`latest` **不是**最新版；无 tag 指向 `0.1.0`）；同一范围 `^0.1.0-beta.2` **npm 解析到 beta.3、bun 解析到 beta.2**，`npm view '@flowy-agent-store/sdk@>=0.0.0' version` → **`0.1.0`**（无 tag 也能被范围命中），裸 `bun add` 会把 `^0.1.0-beta.2` 写进 `package.json`；tarball 核对（`npm pack`，只读）：`0.1.0` 与 `beta.2` 三包的代码与声明**逐字节相同**（仅 `package.json` 异，且 `0.1.0` 的 sdk **无** `optionalDependencies`），`beta.3` 相对 `beta.2` 只增声明（client `TransportLifecycle`/`onLifecycle`/`connectTimeoutMs`/订阅 `rearm()`；sdk `assertProtocolCompatible`/`SpawnOptions.env` / `cwd` / `onExit`/`SpawnedServer.exited`/`SpawnExitInfo`）+ 元数据（`engines.node >= 22`、`repository`、`sideEffects`），三个已发布版本的 `event_type` 仍带 `\| string`（R1 收窄未发布）。**验证（真实输出）**：`C:/appexe/bun.exe scripts/check-docs-sync.mjs` → **8 page(s) in 2 language(s), 0 drift(s)**（新页另做 4 类突变反向测试：标题层级 / 代码块数量 / 表格列数 / 相对内链，**均被拒**）、`--self-test` → **9/9 as expected**；`site` 下 `tsc --noEmit` **0 错**、`react-router build` **通过**并预渲染 `/zh-CN/docs/upgrade` 与 `/en-US/docs/upgrade`；i18n 键集对齐（`docs.sections` 两语言均 8 项、含 `upgrade`）。**未做（有意）**：独立 changelog / release notes 页（R6 剩余项）。 | ✅ 完成 |
| R5 | A | C3 开发者文档深度 | ✅ **已完成（2026-09-10，批 2）**：`site/content/docs/{zh-CN,en-US}/typescript-sdk.md` 新增 §7–§11（双语各 203 行、结构对称，docs-sync 0 drift）——**§7 逐方法 API 参考**（顶层 20 个方法 + 8 个子客户端 + §7.3 HTTP 绑定，指向 `httpRouteTable()` 而非手抄）；**§8 事件参考**（9 种 `event_type` 对照解码 kind、两种「思考」拼写的归一化、`sequence` 单会话单调连续语义、gap 判定、会话 vs Run 的追平载体差异）；**§9 错误模型与重试**（4 个错误类 × `retryable` 语义、`withRetry` 7 个选项含默认值、幂等重放安全性）；**§10 示例集**（Node / 浏览器 / Electron，含凭据只留主进程）；**§11 MCP 接入指南**（D12=A：`.codebuddy-connector/connectors.json` + 插件 `mcpServers`，并给出「不要用 `env` 传密钥」的现行约束）。**验证**：①示例代码被抽出做真实类型检查（6 段全部通过，仅 stub Electron 全局；过程中修掉一处真错——`new WebSocketTransport(url, {token})` 原写成单对象）；②`§10.1` Node 示例对真实二进制端到端跑通（`launchClient` → readiness `127.0.0.1:60930` → `conversations.create` → `follow` → `list` 1 → `models.list` 5 → `close`，exit 0）；③`§8.3` 订阅接线跑通真实 `send`，收到并解码 `message.created` / `turn.status` / `message.activity`，无 resync / 无 error；④站点构建通过且产物含新章节与「43 / 56」；⑤`http-transport.test.ts` 新增 2 例把「43 已映射 / 13 未映射」钉在代码上（改映射不改文档即测试失败）；⑥site `tsc --noEmit` 0 错、web typecheck 0 错、web vitest **106 passed / 1 skipped**、`check-docs-sync` 0 drift、cargo 全绿 | ✅ 完成 |
| R6 | A | C4 站点机制 | ~~中英文档同步校验脚本~~ ✅ **已做（2026-09-10，批 0）**：`scripts/check-docs-sync.mjs`（比对标题层级 / 代码块 / 表格列数 / 相对内链的结构骨架，已并入 `check` 链，自检 9/9）；~~changelog / release notes 页~~ ✅ **已完成（2026-09-11，批 4）**：**新增站点 `changelog` 页**（`site/content/docs/{zh-CN,en-US}/changelog.md`，slug ＝ `changelog`；`site/app/lib/docs.ts` 的 `DOC_ORDER` + `DocSections` 与两语言 `site/app/i18n/*.ts` 的 `docs.sections.changelog` 同步），内容＝**只写已发布事实**（三个版本的条目 + 逐字节差异结论）+ D10=A 的**破坏性变更公告规则**（本页是唯一公告面 / 走 minor 号 / 条目永不改写 / 写错追加「更正（日期）」/ dist-tag 移动不产生条目）+ **未发布边界**（`event_type` 收窄、wire 方法增量、release 重建与 `beta.4` 挂起，不写成「即将发布」）；`upgrade` 页两语言的三处「changelog 页尚未建设」已改为指向新页的内链（详见下方「R6 剩余项落地记录」） | 批 0 已完成；changelog 页 ✅ 批 4 已完成；余两项归批 4 尾部（详见卡点决策表 R6 行）——站内搜索可单独立项（**2026-09-11 C 档复核：阻塞理由已改判为「产品取舍」，见卡点决策表 R6 行**）；**市场数据刷新纳入发布流程：⏸ 已延后（2026-09-11 用户决定：站点部署卡点相关问题延后），不再挂计划** |
| R7 | A | C5 部署与可达性 | 域名 / HTTPS（入口仍是裸 IP + HTTP 的 `irm \| iex`） | ⏸ **延后（2026-09-11 用户决定：站点部署卡点相关问题延后）**——不再挂计划 |
| R8 | B | W2 审批卡（D3=B） | ✅ **已完成（2026-09-10，批 3）—— 六步全落地**。**① adapter**：`AgentRuntimeAdapter::answer_decision`（`nomifun-agent-execution/src/runtime_adapter.rs`）＝ owner 作用域**直通**包装（`AgentExecutionActor::user(owner)` → `AgentExecutionEngine::answer_decision` → 回读 `AgentRunView`），自身不加任何策略、不放宽任何判定。**② 协议方法**：新增 additive `run/answer-decision` —— WS 臂与 HTTP `POST /api/app-server/run/{run_id}/answer-decision` 共用 `execute_answer_decision`（只有它做 public run_id → internal execution 的 owner 内解析）；params = `run_id`+`step_id`+`attempt_id`+`answer`+三个 CAS 版本，两处结构均 `deny_unknown_fields`。**③ 事件断链修复**：`AgentRunEvent` 补投 `step_id`/`attempt_id`，并在 `list_events` 为 `approval.requested` 投影三个 CAS 版本。**侦察漏点（本次补齐）**：引擎 `DecisionRequested` payload 只有 `{question, stop_turn_operation_id}`，客户端拿不到 step/attempt 版本就无法构造合法回答；现由 adapter 在读取时从权威行投影——CAS 语义不变（读到之后任何并发移动 → `Conflict`，绝不静默覆盖），且 `run/events` 与 WS 实时 `event` 推送共用同一投影。**④ 能力位**：`Capabilities.approvals` 改为**派生自 runtime**（与 `run_notifications` 同款写法），不再硬编码 false——也不需要新增 `CapabilityAvailability` 字段：派生写法让所有构造路径都不可能漏设，且无 runtime 的连接不会宣告一个只会回 `runtime_unavailable` 的能力。**⑤ TS/WebUI**：`protocol` 新增 `AnswerDecisionInput` + `RunEvent` 投影字段（含三个 CAS、无 `always_allow`）；`RunClient.answerDecision`；`HttpTransport` 路由表 +1（**43 / 56 → 44 / 57**）；WebUI 审批卡 `web/src/components/ApprovalCard.tsx` 挂在**已挂载的**会话页 composer 上方，选择逻辑抽在 `web/src/lib/approvals.ts`（最新未回答决策 + CAS 不完整时**不渲染可提交卡片**），store 新增 `followRun`/`answerRunDecision`（`@agent` mention 起 Run 后跟随，`runEvents` 用 `mergeRunEvents(按 sequence 去重)`）。**⑥ 文档**：本行、`05` §5.2 方法表与语义、site 双语 §7.2/§7.3/§7.4、handoff §3.1；`http-transport.test.ts` 的 43/56 防漂移护栏同步改 44/57。**红线守住**：`always_allow`/`approve_all`/`yolo`/`skip_cas` 被两处参数结构 `deny_unknown_fields` 拒绝（单测钉住）；三路 CAS + 仅 `WaitingInput` + 非空 answer 原样保留、无绕过；能力位在 ①–③ 落地且测试通过后才翻真值。**验证（真实输出）**：`cargo test -p nomifun-app --test agent_execution_decision_e2e` **7 passed**（非 owner→`NotFound`、execution/step/attempt 三路版本过期→`Conflict`、非 `WaitingInput`→`Conflict`、空 answer/非法 id→`BadRequest`、成功回答后按同一事件重放→`Conflict`、投影含 ids+三版本、`initialize` 宣告 approvals）；`cargo test -p nomifun-app-server --lib` **66 passed**（params 契约、WS 臂 ready→runtime 次序、approvals 跟随 runtime）；`web` vitest **120 passed / 1 skipped**、`bun run typecheck` 0 错。**已知边界**：WebUI 的 Run 面目前只有 composer 上方这张卡（Run 状态树 / 侧栏归属归 R11）；卡片**有意**不提供 any-way approve-all 记忆化开关。 | ✅ 完成 |
| R9 | B | W3 中断与引导 | ✅ **已完成（2026-09-10，批 3）**。**判定层**：`web/src/lib/run-steer.ts` —— `steerAvailability`（`available` / `busy` / `terminal` / `no-run`，终态与无 Run 在**发请求之前**拒绝，不制造必然失败又容易被误读成「引导已生效」的请求）+ `isStaleRunWrite`（结构化判定 `code === "conflict"`，不依赖具体错误类实例）。**store**：`steerRun(text)` **先 `run/get` 读服务端当前版本**再带 `expectedVersion` 提交（CAS 令牌绝不本地猜，与 `answerRunDecision` 同口径）；陈旧 → 回读 `run/events` 补齐后提示「状态已变，本次引导未生效」，不硬重试；成功走既有 Toast（`run.steerAccepted`）。另加 `cancelRun()`（同「先读版本再 CAS」），与 steer 明确分工。**Composer**：有在跑 Run 时切「引导输入」形态——placeholder 与发送按钮文案切换、Enter 与发送键都走 `steerRun`、`runSteerBusy` 期间禁用、`runSteerError` 行内呈现（key 走 i18n，原始消息用 `defaultValue` 回显）；`cancel` 入口留在 Run 面头部，两者不混。**验证（真实输出）**：`run-steer.test.ts` 6 例 + `appStore.steer.test.ts` 6 例（假 client 钉住「终态零请求」「版本先读后用」「conflict 回读 + 提示」「成功 toast」「cancel 同口径」）；`cd web && bun run test` → **196 passed / 1 skipped（33 文件）**；`bun run typecheck` → 0 错。 | ✅ 完成 |
| R10 | B | W4 计划与待办树 | ✅ **已完成（2026-09-11，批 3）—— 由 additive `run/plan` 解锁（D-W6-1 结案）**。**① 协议**：新增 `run/plan`（WS 臂 + HTTP `GET /api/app-server/run/{run_id}/plan`，共用 `get_run_plan_for_user`＝owner 作用域与 `run/get` 同一 `engine.get`）返回 `AgentRunPlan`＝**计划快照**：step 带 `title`/`kind`/`status`/`role`+`model`（成员归属）/`introduced_in_revision`/`superseded_in_revision`/`created_at`/`updated_at`，attempt 带 `status`/`trigger_reason`/`question`/`error`/`output_summary`/`output_files`/`tokens`/`started_at`/`finished_at`。**② 为什么必须新增**：`run/events` 是追加式日志，`task.updated`/`attempt.updated` 只带 `{change}`/`{status}` 标记，`run.plan_changed` 不含标题，`AgentRunEvent` 无时间戳——标题、失败原因、耗时、成员归属**在事件面上不可得**（这就是 D-W6-1 的证据链）。快照与事件**互补**：事件给「发生过什么」，快照给「现在是什么」。**③ UI**：`lib/run-plan.ts` 纯投影（展示序号与引擎 `attempt_no` 解耦、耗时两端齐全才算、空错误不当错误、`superseded/skipped` 归最弱档、`planProgress` 只数未取代步骤），`RunDetail` 增「计划与待办（done/total）」块：步骤标题 + 状态徽标 + 成员 + 修订号 + 取代标记 + 每次尝试的原因/耗时/token/错误/产出，**与 W6 step 树双向互跳**（`plan-step-*` ↔ `run-step-*` 锚点 + 短暂高亮），历史计划仍由事件树的「计划修订」列表承载（可追溯）。**④ 安全面**：不新增内部标识（`participant_id`/`source_agent_id` 不上 wire），`output_files` 走既有相对路径过滤；**只读**方法，无任何写路径绕过审批门。**验证（真实输出）**：`cargo test -p nomifun-app --test agent_execution_decision_e2e` → **8 passed**（新增 `plan_snapshot_carries_titles_statuses_and_member_attribution`：title=`decide`、`kind=agent`、`waiting_input`、`builder`·`model_test`、`trigger_reason=initial`、审批问题在快照上、序列化不含 `participant_id`/`source_agent_id`、外部 owner→`not_found` 且不回显 step_id）；`cargo test -p nomifun-app-server --lib` → **67 passed**；`cargo check -p nomifun-app-server --tests` → 通过（仅既有 warning）；web：`run-plan.test.ts` 11 例 + `http-transport.test.ts` 17 例（含 45/58 防漂移守卫 + `run/plan` 路由绑定断言）。**已知边界**：快照只有「当前」计划（历史修订仍在事件面）；`superseded_in_revision` 的步骤保留展示但不计入进度。 | ✅ 完成（D-W6-1 结案） |
| R11 | B | W6 Run 状态树 | ✅ **已完成（2026-09-10，批 3），带一处已登记的线上缺字段（D-W6-1）**。**① 树投影（纯模块）**：`web/src/lib/run-tree.ts` 把 `RunEvent[]` 折成「运行头（最新状态 / 原因 / 末序号 / 事件数）→ 计划修订 → 步骤 → 尝试」；按 `sequence` 排序并对 `(run_id, sequence)` 去重（实时尽力投递 + 追赶重放会重复投递），**两个并行 step 的事件互不串线**（单测钉住）；每步带状态、重试次数（`change=retry_requested` 计数）、尝试数、事件数、会话副作用（steer / stop_turn 的 requested / delivered）与审批问题；无 attempt 作用域的事件不反推步骤状态（引擎用 `{status:"queued"}` 表达尝试排队，反推会把步骤误标）。**② UI**：`RunDetail.tsx` 从调试视图重写为 Run 面——头部状态徽标（`runStatusTone` 五档）+ 事件数 + 取消按钮（`run/get` 先读版本再 CAS）、计划修订列表、可展开的步骤 → 尝试树（默认展开：有尝试或有过引导记录）、审批问题行（待答复 / 已答复）；**原始事件降级为底部 `<details>` 调试面板（默认收起）**，其表格保留 seq / type / 作用域 / payload。文案全部走 i18n（新增 `run.*` 约 60 键 + `run.statusValue.*` / `run.statusReasonValue.*` / `run.marker.*` / `run.planChange.*` 家族，**未收录的协议值回落原值**，不假装认识）；W2 审批卡的英文硬编码一并改走 i18n。挂载点：会话页 composer 上方（与审批卡同区，不与 `is-new` 空态冲突），无跟随时返回 `null`。**③ 侧栏运行标记**：`followRun(runId, conversationId)` 记录起 Run 的会话，侧栏只在**该**会话行渲染 `.run-dot`（终态自动消失），对应「标记运行状态」；「项目 → 线程」分组本已存在（`workspaceGroups`）。**④ 死代码清理**：删除无人引用的 `RunPanel.tsx`（硬编码默认 agent ID + 英文调试表单）。**⑤ 可测性设计**：把纯展示层拆成 `RunSurface`（显式入参）——因为 store 走 `useSyncExternalStore`，服务端渲染读到的是**初始** state，用真实 store 渲染再断言 HTML 会永远得到空串。**验证（真实输出）**：`cd web && bun run test` → **196 passed / 1 skipped（33 文件）**；`bun run typecheck` → 0 错；`bun run build` → 通过。新增用例：`run-tree.test.ts`（并行步骤隔离 / 去重 / 乱序输入等幂 / 未知事件不炸 / 状态色调 5 档）、`RunDetail.render.test.tsx`（`react-dom/server` 渲染真实组件，断言计划修订 / 两步各自状态 / 重试 1 / 尝试 2 / 审批问题 + 待答复 / 引导标记 / 终态徽标 / 取消禁用 / 调试面板文本）。**未达成（线上缺字段，见 D-W6-1）**：步骤**标题**、**失败原因**、**耗时**在 wire 上不存在（`task.updated` / `attempt.updated` 只有标记字段，`RunEvent` 无时间戳）；sub-agent（member）归属同样没有 wire 事件。 | ✅ 完成（D-W6-1 为已知缺口） |
| R12 | B | W7 重试 / 编辑 / 重新生成 | ✅ **已完成（2026-09-10，批 3）**。**① 修好一条被解码器丢掉的信息**：`message.error` 在 wire 上本就带 `code` / `retryable`（`nomifun-app-server/src/lib.rs:4241` 的投影），但 `packages/protocol` 的解码器只取 `message`，于是「可重试 / 不可重试」在界面上无从判断；现补 `code` + **三态** `retryable`（`booleanValue`：非布尔一律 `null` 而不是真假猜测），webui 投影把它随错误行存下（`content: {content, code, retryable}`），`conversation/send` 回执的 `result_error_retryable` 与之同源。**② 幂等策略分开建模（关键）**：`web/src/lib/turn-actions.ts` —— `resend`（发送**没拿到回执**，用户行仍停在 `sending`/`failed`）**复用原幂等键**（服务端按 `(user, conversation, key)` 派生 operation_id 并持久化回执，`service.rs:7827` 起，因此「响应丢了但服务端已执行」不会执行第二次）；而 `retry` / `regenerate` / `edit` 是**已回执**的轮次，必须用**新**键——复用旧键只会拿到旧回执回放，按钮点了等于没点。键 = `kind-messageId-contentDigest`，故双击/重放同一次动作天然幂等，改过正文则键变化。**③ 拒绝口径（不发请求）**：正文为空 / 找不到用户轮次 / 服务端标记不可重试 / 原键已不在（刷新后）四种情形直接给原因，不制造假成功；`missing-key` 时明确提示改用「重新生成」而不是偷偷换新键重发。**④ 发送编排只写一次**：store 抽出 `submitTurn`（`send` / 重试 / 重发 / 重新生成 / 编辑重发共用），失败时保留 pending 行供重发；新增 `runTurnAction(request)` 统一入口 + `turnActionBusy` / `turnActionError`。**⑤ UI**：错误卡加「重试」按钮与「可重试 / 不可重试」徽标（`retryable === null` 时只给按钮、不标真假）；最后一条用户消息可**行内编辑后作为新一轮发送**（原消息保留、不覆写历史）；最后一条正常助手回复给「重新生成」；动作失败在会话内提示条呈现（i18n 全走 `message.*`）。**验证（真实输出）**：`turn-actions.test.ts` 17 例（幂等策略 / 拒绝口径 / digest 稳定 / 文案键齐备）、`appStore.turn-actions.test.ts` 8 例（**「失败发送重发复用原键」**、不可重试零请求、原键缺失零请求、重新生成取最后用户轮次、编辑不改原消息、在途忽略、传输失败不静默）、`packages/protocol` 解码器 1 例（三态 + 非布尔不当真），`site` 双语 §8.1 同步「解码后带 `code` 与 `retryable`」；`cd web && bun run test` → **224 passed / 1 skipped（35 文件）**、`bun run typecheck` → 0 错、`bun run build` → 通过（`✓ built in 1.06s`）、`check-docs-sync` → 0 drift。**已知边界**：历史错误行若 wire 未带 `retryable` 则显示为「未知」（给按钮但不标真假），不做本地猜测。 | ✅ 完成 |
| R13 | B | W8 余项 | ✅ **已完成（2026-09-10，批 3，按 `21` D4=A 落地）**。**① 多标签订阅归属**：新增选主层 `web/src/lib/global-effects.ts`——「每标签各自订阅各自 WS」是既成事实（store 每标签一份模块实例，`attachLifecycle` 注释明写**不做**单写者选举；服务端订阅表 `WsSubscriptions` 是**每连接**一份，见 `nomifun-app-server/src/lib.rs:4027`，故多标签各自订阅无服务端争用）；只有**逃出标签页的副作用**（桌面通知 / 声音 / 后台 Run 提醒）进选举：`navigator.locks` 抢排他锁 `allo:global-effect`（阻塞式，1.5s 上限）→ 持锁者复查跨标签去重表 → 触发一次；去重表落 `localStorage`（`allo-global-effects-v1`，上限 128 条），故**跨标签且跨刷新**只触发一次。**锁不可用时的确定性降级（代码注释写明 + 单测钉住）**：①无 `navigator.locks` → 「Claim 级」（先写共享去重表再回读自己的签名，单写者胜出：顺序场景仍去重，同一瞬时可重复 ≤1 次/标签，**绝不静默丢失**）；②锁请求抛错/超时 → 仅该次 `emit` 降级到同一 Claim 级（不让调用方失败）；③连共享存储也没有（隐私模式 / 内嵌 WebView）→ 每标签每 key 各触发一次（**宁可重复、绝不丢失**），有 `BroadcastChannel` 时靠广播再把重复压掉；④performer 拒绝（通知未授权 / 无音频上下文）时**不写「已处理」**，让能投递的标签仍有机会触发。**② 通知补齐（复用既有 Toast 机制，不新造通道）**：`Toast` 加 `params`、`ToastHost` 透传 `t(key, params)`——「刷新完成」（`catalog.marketRefreshDone` / `marketRefreshUnchanged`，带市场名 + 条目数）与「安装完成」（`catalog.storeInstallDone` / `storeInstallReused`，带条目名）走 `pushToast`；「条目导入完成」T14 已落地（`marketEntryImportDone` / `Reused`）故不重复挂。「**后台 Run 终态**」由 `web/src/lib/run-notify.ts` 从 `run/events` 投影（只认 `run.started` / `run.status_changed` 的 `{status}`，按 `sequence` 而非到达顺序取最新；终态 = `completed` / `completed_with_failures` / `failed` / `cancelled`），`followRun`（首取历史 + 实时 `onEvent`）与 `answerRunDecision`（回答后回读）三处调 `announceRunTerminal`：标签内按 `run:<id>:terminal:<status>` 幂等；**可见且聚焦只出 Toast**，**后台**（`document.hidden \|\| !hasFocus()`）才经选主层发桌面通知 + 声音一次，点通知把投递标签拉到前台（对上 AC-8「可点击通知」）。通知权限**不主动索取**（需用户手势），未授权时桌面通道拒绝、声音通道照旧（`run-reminder` 双通道，任一成功即算投递，谁都不成功也不报错）。**③ 自动可测（不依赖手工多开标签）**：`navigator.locks` / `BroadcastChannel` / `localStorage` / performer 全部注入——`global-effects.test.ts`（假锁管理器 + 假 channel hub）覆盖「两标签只触发一次 / 跨刷新去重 / 锁抛错降级 / 无任何协调 API 时每标签一次 / 拒绝不污染 key / 空 key 报错 / dispose 后不再听广播」；`notice-performers.test.ts`（假 `Notification` + 假 `AudioContext`）覆盖权限拒绝、构造抛错、增益不归零、`suspended` 时 `resume`、点击聚焦、双通道任一成功；`appStore.terminal-notice.test.ts`（假 gate + 假 client）钉住 store 侧「仅终态通知 / 后台才发全局提醒 / 同标签幂等 / 运行中沉默」。**验证（真实输出）**：`cd web && bun run test` → **164 passed / 1 skipped（29 文件）**（改动前 122 passed / 1 skipped / 25 文件）；`bun run typecheck` → 0 错；`bun run build` → 通过（`✓ built in 1.66s`）。**未做（有意）**：不新增通知中心或第二条通知通道；不主动请求通知权限（需用户手势，留作单独 UI 项）；跨标签**会话内** UI（toast / 断线横幅）**有意不协调**——D4=A 的边界就是「只协调逃出标签的副作用」；`store/list` 的自动回读刷新不发完成通知（只对接用户显式「检查更新」）。**收口补正（2026-09-11）**：重连分支原只 rearm 会话订阅，而被跟随 Run 的 `run/subscribe` 在服务端同样已被丢弃 → **断线窗口内进入终态的 Run 永远发不出通知**（正是后台提醒最该出现的场景），即「② 复用断线横幅机制」当时并未真正接上。修法：`connect()` 的 `lost` 分支在重连后一并 `await runSubscription?.rearm()`（复用 T8 既有 rearm，无新通道、无新权限）；rearm 回放的持久化事件经既有实时监听器触发通知，且按「run + 终态」幂等，故断线前已发的那次不会重复。**证据（真实输出，2026-09-11 00:13）**：R13 四文件 `bun run test src/lib/global-effects.test.ts src/lib/notice-performers.test.ts src/lib/run-notify.test.ts src/store/appStore.terminal-notice.test.ts` → **44 passed（4 文件）**，其中 `appStore.terminal-notice.test.ts` **6 passed**（新增 2 例：断线期间进终态→重连后仍提示一次；重连回放同一终态→不重复）；`cd web && bun run test` → **全量 vitest 通过**（2026-09-11 00:19 实测 **235 passed / 1 skipped / 36 文件**；该数字随并行 W7 线新增用例增长，R13 自身的 4 个文件恒为 44 passed）；`bun run typecheck` → exit 0；`bun run check:docs-sync` → 7 页 × 2 语言 0 drift；`bun run test:docs-sync` → 16 pass。**边界（不回退也不扩大）**：`localStorage` 里的共享偏好（语言 / 主题 / 设置，共 3 处写点）是 last-write-wins 的幂等写，**不进选主**——D4=A 只协调「逃出标签页的副作用」，给偏好选主还需要额外的「谁的值胜出」语义，本行不引入；横幅的「忽略」（`dismissConnectionLost`）只清 `connectionLost`、不拆链路，之后没有第二个重连入口（只剩页面重载＝新标签状态，Run 跟随按会话内内存语义丢弃）——要让「忽略后仍能恢复」需要新增重连入口，属 T8/W8 通道决策，本行不改。 | ✅ 完成 |
| R14 | B | W9 模型能力与用量 | ✅ **完成（2026-09-11，批 6）**——④/④ 项均已落地：取价路径、能力/限制标注、发送前校验、接近上限建议、**按 turn 的费用 / token**（批 3 判「不可达」的前提已被源码级侦察推翻：**不是缺链路，是投影丢数据**，见 ⑥）。**① 取价路径（D12=A 要的那条，原先缺）**：`conversation/model-options` 的每个模型条目新增 models.dev 目录事实——`cost_input` / `cost_output`（每百万 token USD）、`catalog_context_window`、`supports_vision`，全部 `skip_serializing_if` 缺席即不上 wire（`nomifun-app-server/src/lib.rs` 的 `ConversationModelOption` + `catalog_model_facts`；只用**已缓存**目录，`resolve_catalog_capabilities` 不触网，故列模型不会变成网络调用；未映射的 provider（如内置 `mimo` 的 `MergePolicy::Never`）与未知模型**整组缺席**，不给 0 站台）。Rust 侧新增用例断言「已知模型投影出费率/窗口/视觉 + 序列化形状 + 未知模型与未映射 provider 均为空」——`cargo test -p nomifun-app-server --lib` **67 passed**。**② 模型选择器的能力/限制标注**：`web/src/lib/model-facts.ts`（纯模块，14 例）负责口径——`costRateText`（`$3/M in · $15/M out`，单向缺就只显示单向，两向都缺返回 `null`）、`contextWindowTokens`（目录优先、配置兜底、都没有则 `null`）、`capabilityLabels`（**只有目录明确 `true` 才给「视觉」标签**；`false` 与未知一律不给——「目录没说支持」不等于「目录说不支持」）、`rateText`（整数不带小数、小数两位去尾零，0/NaN/∞ 视为未知）；`ModelPicker` 渲染 `费率 · 上下文 · 视觉`，**未知就整段不显示**。**③ 发送前兼容性校验**：`validateModelSelection` + store 的 `send()` 在**发请求之前**拦截「选中的模型不在已知目录里」（`models/list` ∪ 配置投影，两个命名空间各自成键），并给出原因（`common.modelUnknown`）；**目录尚未加载时一律放行**——把「我们还没拿到数据」说成「你的模型不存在」是更糟的错。store 侧 4 例（拦截且草稿保留 / 配置投影命中 / 目录命名空间命中 / 未加载放行）。**④ 接近上限的建议**：`web/src/lib/context-advice.ts`（5 例）按服务端测量值分级（阈值 80%，`percent` 缺失时用 `used/window` 现算，窗口为 0 或未测量返回 `null` 不给建议），Composer 在 `near` 时显示「上下文已用 X%，继续会更快触顶」+「新建对话」按钮——**压缩不是协议能力（`11` §5.3 未做），所以只建议真能做的事**。**⑤ 文档**：site 双语 §7.3 补 `modelOptions()` 的可选目录字段与「缺席 = 未知」口径。**⑥ 按 turn 的费用 / token（本批落地，撤销原「不可达」判断）**：源码级侦察查明**运行时早已上报逐轮 token**——`TurnCompleted` 事件带 `input_tokens` / `output_tokens`（`crates/backend/nomifun-ai-agent/src/protocol/events/mod.rs:218-256` 的 `TurnCompletedEventData`），会话中继**无条件**把它转发到用户事件总线（`crates/backend/nomifun-conversation/src/stream_relay.rs:3412` 的 `forward_to_websocket`，payload 形状见 `:3761-3787` = `message.stream{type:"turn_completed",data:{…}}`），**唯一丢数据的地方是 App Server 投影**：`project_conversation_notification`（`nomifun-app-server/src/lib.rs:4397`）把未枚举的 kind 一律降级成 `message.activity{message_id,kind}`，逐轮 token 就在这一步被丢掉。改法即在**这一层 additive 补上**：`turn_completed` 分支新增 `usage: {input_tokens, output_tokens, total_tokens}`（`TurnUsageView`，字段名对齐 Run 面 `TurnUsage`）——运行时未上报 / 缺一侧 / 两侧皆 0 时**整段不出现**（不给 0 站台，也不拿上下文占用顶替；`skip_serializing_if` 同口径：缺席即不上 wire）；`kind` 与 `message.activity` 的关系不变，R31 的「收尾中」标记与降噪规则照旧。客户端：`parseTurnUsage` **只读 `turn_completed`** 的 `usage`；reducer 把用量与「事件到达那一刻的模型键快照」一起记进 `stream.turnUsage`（`turnModelKey`：显式选择优先、会话模型兜底）；Composer 用 `turnCostUsd` / `costText` 出金额，**费率与 token 两者都在才显示**（目录无价、只有单向价、token 未知、模型键未知 → 只显示 token，金额整段不渲染）。**已知边界**：逐轮用量只在**实时**事件里（服务端不持久化历史轮次），重载后不显示、新一轮开始即清空；要回放需 DB 迁移，本轮不做（见 D-W9-1）。**验证（真实输出）**：`cargo check -p nomifun-app-server --tests` → exit 0；`cargo test -p nomifun-app-server --lib` → **79 passed**（新增 2 例：带用量的投影 + 未上报整段缺席）；`cd web && bun run test` → **320 passed / 1 skipped（43 文件）**；`bun run typecheck` → exit 0；`bun run build` → exit 0（`✓ built in 1.68s`）。 | ✅→🟡 **主体完成（2026-09-11，批 6）**——④/④ 项（费率、标注、校验、建议、按 turn token+金额）已落地；**按上述分组口径归 🟡（部分完成）**：登记在案的边界「逐轮用量仅实时、未持久化」仍属未做。 |
| R15 | B | W10 附件 / 图片输入 | content 图片载体（协议加法）+ run/turn 处理 + 拖拽粘贴 + 发送前能力校验 | ⏸ 等协议词汇与概念对齐（Q3） |
| R16 | B | W11 设置 Dialog 补全 | ✅ **provider 分区完成（2026-09-11，批 4）—— nav 八→二，其余分区按 §6「不做假开关」不渲染**。**① 协议（纯加法）**：新增 WS 方法 `config/get` / `config/set`（`nomifun-app-server/src/lib.rs` 的 `execute_config_get` / `execute_config_set`，紧邻 `models/list`），**无 HTTP 绑定、不进 SDK 包**——provider / 默认模型配置是宿主管理面（§6 判断规则），宿主自己的 Web UI 经 `web/src/lib/client.ts` 的两个 helper 走 transport 调用。**② 读口径复用**：`AppServerConfigView { exists, default_model, providers[{name,enabled,models}] }` 直接投影 `AgentStoreConfig`（与 `app_server_catalog.rs` 的 config-only provider 分支同源），**无 `api_key` / `base_url` / 路径字段**；文件缺失 = `exists:false` + 显式 `null`（正常答案，不报错），读不动 / 解析失败 = `config_unavailable`（不拿默认值把"文件坏了"伪装成"没写"）。**③ 写白名单**：`AgentStoreConfigPatch`（`#[serde(deny_unknown_fields)]`）当前只允许 `default_model`；`api_key` / `base_url` / 路径等任何其他键 → `invalid_request`（不是静默忽略）；空值 / 无 `/` / provider key 不在 `[providers.<key>]` 同样被拒——**运行时解析不了的默认值不写**（未声明的 *model* 允许，运行时按请求注册它）。**④ 最小改动写**：`AgentStoreConfig::with_default_model` 用 `toml_edit` 只重写目标键（注释 / 排版 / 其余键逐字节保留），缺失键插在文件头注释之后、第一个 `[table]` 之前（绝不落进某张表），同目录临时文件 + `rename` 原子替换；**响应 = 写后重读**，落点即磁盘内容（不做乐观回显）。**⑤ 前端**：`SettingsDialog` 拆成 store 闸门 + `SettingsPanel`（纯 props，服务端渲染可测），nav 收敛为**通用 + 供应商**；provider 分区 = 默认模型 select（选项来自 `config/get` 的 provider/model 事实，停用 provider 不供选，手改过的存量值保留）+ 保存 + 配置文件状态行，`web/src/store/settingsConfig.ts` 单一来源，失败行内报错 + 重试，成功显示**服务端回读值**；`general` 里从不回写任何地方的 `providerId` / `model` 输入框**已删除**（连无用 i18n 键含 `comingSoon` 一并清掉）。**⑥ 其余六分区判定（见文末卡点决策表）**：`agent`（唯一候选写面 `[memory] distill_enabled` 宿主只解析不消费 → 写它就是假开关）/ `account` / `plugin` / `advanced` / `lab` / `archived` 一律不渲染。**验证（真实输出）**：`cargo test -p nomifun-app-server --lib` → **77 passed**（新增：读视图不含 api_key/base_url 字样；写入只改目标键且注释 / `api_key` 行 / 其他表原样、`default_model` 只出现一次、无 `.toml.tmp` 残留；7 类越界请求全部 `invalid_request` **且文件逐字节不变**；缺失文件→`exists:false` 无错、声明 provider 后写入落盘；外部 owner→`policy_denied`、未注册连接→`not_found`、`config/get` 带 `path`→`invalid_request`、未就绪→`not_initialized`）；`cargo test -p nomifun-app --test agent_execution_decision_e2e` → **8 passed**（R8 审批门未被绕过）；web：`bun run test` → **301 passed / 1 skipped（43 文件）**（新增 `settingsConfig.test.ts` 10 例 + `SettingsDialog.render.test.tsx` 6 例）、`bun run typecheck` → 0 错、`bun run build` → 通过；`node scripts/check-docs-sync.mjs` → 0 drift；site 双语 §7.3 计数 **45 / 58 → 45 / 60**（`config/get`、`config/set` 进无 HTTP 绑定清单），`packages/client` 两道护栏（`docs-drift.test.ts` + `http-transport.test.ts` 13→15）同批改；`05` 增 §4.10 契约。 | ✅ provider 分区完成；六分区不渲染（判定见卡点决策表） |
| R17 | B | W12 技能 / 专家管理 | 创建 / 编辑 / 删除 / 复制，与市场安装产物区分 | 🟡 **后端写面已落地（2026-09-11，批 4）**：`skill/create \| update \| delete`（WS-only 宿主管理面）+ 归属语义（`origin` / `writable`）+ 方法级真断言；**未做**：WebUI 技能管理面、`skill/copy`、编辑面的全量正文回读（理由与下一步见「R17 落地记录」） |
| R18 | B | W14 消费 SDK 新能力 | ~~webui 事件层瘦身，统一走包内解码器~~ ✅ **已完成（2026-09-10，批 2）**：`src/lib/activity.ts` 只留展示策略（`NOISE_ACTIVITY_KINDS` / `isActivityMessageType`）并再导出包内解码器；`applyEvent` 全部经 `decodeConversationEvent`（**全仓已无 `event.payload.*` 直读**，已 grep 核实）；订阅改 `autoResync:false` + `onError`（shell 自持权威重载：view + 首页 + 游标） | ✅ 完成 |
| R19 | B | W1b 命令面板 V2 | ✅ **已完成（2026-09-10，批 3）**。**① 数据层独立成模块**：`web/src/lib/palette-model.ts`（React-free）—— `PaletteItem` 增 `groupKey` / `keywords` / `actionId` / `modelKey` / `effort` / `disabled` + `disabledReasonKey`；`sessionPaletteRows`（重命名 / 删除 / 分享 / 复制会话 ID）、`modelPaletteRows`（来自 `models/list` 目录，当前项标「当前」，`keywords` 收 provider id / model / 默认标记）、`effortPaletteRows`（默认 + 低/中/高/极高/超高）、`commandPaletteRows`（顺序＝优先级：命令 → 提示 → 会话 → 模型 → 思考等级）、`filterPaletteItems`（大小写不敏感，匹配文案 + `keywords` + hint）。**② 组件只做渲染**：`CommandPalette` 按 `groupKey` 变化插入分组小标题（光标仍是 `items` 上的扁平索引，键盘不需要树），新增 session / model / effort 图标（Pencil / Trash2 / Share2 / Copy、SlidersHorizontal、Brain）与**禁用行**（`disabled` + 说明为什么不可用）；按下不发请求的行即使被键盘选中也被 `pickPaletteItem` 拦掉。**③ 交互口径不变**：焦点仍留在 textarea（`/`、`@`、`Cmd/Ctrl+K` 都由草稿派生查询，IME 守卫与 Esc 清半截触发符原样）；会话动作仍是既有 store action（重命名/删除走原弹窗，不在面板里另建一套）；模型 / 等级切换复用 `chooseModel` / `chooseEffort`。**④ 刻意不做「归档」**：协议里没有归档 / 回收站（`11` §2.2 未做、`16` §3.5 方向四未排期），面板里放一个点了没反应的「归档」比不放更糟——R19 行文提到它，落地按现状收敛并在此登记。**验证（真实输出）**：`palette-model.test.ts` 11 例（分组顺序、关键词命中 provider id、无会话时四个会话行禁用且带原因、等级六档与当前标记、`palette.*` 文案键不得进搜索面）；`CommandPalette.render.test.tsx` 3 例（`react-dom/server` 渲染真实组件：四个分组标题按行序出现、禁用行带原因、mention 模式不出现命令分组）——**该渲染测试当场抓到一处真错**：等级文案键最初写成 `composer.effort*`，实际在 `modelPicker.effort*`，只跑数据层测试看不出来；`cd web && bun run test` → **238 passed / 1 skipped（37 文件）**；`bun run typecheck` → 0 错；`bun run build` → 通过（`✓ built in 1.09s`）。 | ✅ 完成 |
| R20 | B | W5 余项 | 🟡 **list 项 size / type / mtime 已补（2026-09-11）**。**2026-09-11 拆条**：**R20a「按 Run 归属」＝可做**——归属所需数据**已存在**（`run/plan` 已投影 attempt 的 `output_files`，`nomifun-agent-execution/src/runtime_adapter.rs:141` + `nomifun-app-server/src/lib.rs:3873-3884`），**零协议增量**，只差把「产物路径 → 所属 Run/Step」在宿主面文件服务上映射出来；**R20b「接受 / 回退」＝等 Artifact Phase**（需协议能力 `capabilities.artifacts`，`nomifun-app-server/src/lib.rs:401` 硬编码 `false`）。**实现**：`/api/fs/list` 只回名字与路径，元数据在宿主 `POST /api/fs/metadata`——store 新增 `artifactsMeta`（按绝对路径索引，`null`＝查不到，**缺键**才是未查询，故失败也记账以杜绝重复请求）+ `loadArtifactMetadata()`（并发上限 4、单条失败降级为 `null`、**不弹错不重试**、切会话丢弃过期结果），`refreshArtifacts` 拿到列表后自动补齐；`ArtifactPanel` 列表行与预览头渲染真实 `size · MIME · mtime`，**查不到就整段不渲染**；预览头在无元数据时才退回按文本长度估算（并在注释里写明那只是字符数、不是文件字节数）。验证：`appStore.artifact-meta.test.ts` **5 passed**（每路径只查一次 / 失败记 null 且不重发 / 显式路径只查这些 / 无 client 与无路径 no-op / 无工作区时不查）。 | 🟡 size/type/mtime 已完成；**R20a 归属可做（拆条后）**、R20b 接受/回退待 D-W5-1 |
| R21 | B | W13 余项 | ~~「移除前」的受影响快照改由服务端前置投影~~ ✅ **已完成（2026-09-10，批 1）**：`market/get` 每条带 `snapshot.installed_count`（R25）；`CatalogView` 打开移除弹窗前先刷新 `market/get`，弹窗列出 `installed_count > 0` 的条目，条目卡安装态同源（**不再从聚合 `store/list` 派生**）；live 验收脚本补断言「导入后 `installed_count = 0`」 | ✅ 完成 |
| R22 | C | `17` P1 | MCP 连接器 `env` 明文写入快照（违反 §6） | 🔴 **安全优先级最高**；前置：安全存储注入路径 |
| R23 | C | `17` P3 | 依赖 SemVer 范围解析与「不可满足即阻断」 | **已批准待排期**（原写 v1.1）。**二次订正（2026-09-11）**：**（a）决策不是卡点**——`21` D6=A 已批准按规范实现阻断 + 配置逃生口；**（b）解析 + 拓扑排序已接线、不是「调用点全在单测」**：`registry_helpers.rs:46-61`（`load_and_validate` 在 `:56` 调 `validate_dependencies`、`:57` 用 `load_order` 真排序）← `registry.rs:111` / `:166`；`dependency.rs:112-129`。**真缺的只有阻断**：`registry.rs:136-138` 只 `warn!`，`dep_result.valid` 从未当门；**（c）层级要分开**——「全仓无 semver 解析」指的是**导入层** `nomifun-importer/src/import.rs:664-681`（只登记不决策），与 extension 层是两个消费面 | **阻断默认开 + 逃生口**（规范未发版、无既有消费者 → 实现即生效，不是破坏性变更、无需公告窗口） |
| R24 | C | `17` P4 | `strict` 完全未实现（无字段、无阻断路径） | **已批准待排期**（`21` D6=A 含 `strict`）。**归属订正**：这是**导入期**语义（`17` §3/§7、`02` §11.1），应挂 **importer 层**（`PluginManifest` 同级），**与 extension 层不是同一物**——原「同 R23」把两层混为一谈；extension crate 内 `strict` 实测 0 命中（那条只说明 extension 层没有这个字段） | 与 R23 同批，但归属 importer 层；打通导入期阻断路径（与 `MissingIdentity` 同级） |
| R25 | C | `18` D8 | ~~`market/get` 未返回条目安装快照~~ —— ✅ **后端 + TS 类型已做（2026-09-10，批 1）**：条目新增可选 `snapshot`（含 `installed_count`），由新增仓储查询一次 JOIN 投影；**剩余**：前端条目列表与移除弹窗改为消费它 | ✅ 协议面完成（前端并入 R21） |
| R26 | C | `18` D4 ① | ~~持久化服务器原始 `ETag` / `Last-Modified` 并发送真条件请求~~ ✅ **已完成（2026-09-10，批 1）**：迁移 `057_*` 新增 `source_etag` / `source_last_modified`；`fetch_http_market` 发 `If-None-Match` / `If-Modified-Since` 并在刷新后写回；`resolved_revision` 标记规则不变（升级不触发重建） | ✅ 完成 |
| R27 | C | `auto_update` 执行逻辑 | ~~刷新调度器~~ ✅ **已实现（2026-09-10，批 1）**：`[marketplace] auto_update_interval_hours` 门控（不写 / `0` = 关闭），仅轮询「开关为开 **且** 官方源」的市场，逐轮走 `refresh` 的 revision / ETag 短路；失败仅当轮跳过（不做紧环重试） | ✅ 完成 |
| R28 | C | `18` D7 ① | ~~本地化变体消费（`description_zh` / `description_en` 等，真实市场 268 条）~~ ✅ **已完成（2026-09-10，批 1）**：市场条目新增**通用变体映射** `localized`（只收 `_zh`/`_en` 后缀、值为字符串或字符串数组的字段），从清单**透传**（探测 → DB 行 → `market/get` 投影三处 additive，空映射不上 wire）；前端在 `@flowy-agent-store/protocol` 新增 `pickLocalized` / `pickVariantText` / `pickEntryTags` / `pickEntryText` 纯函数落实 **D8=A 回退链** `{field}_{lang}` → `{field}_{other}` → 基线字段（`tags_*` 族优先于 `legacy_tags_*`），webui 里硬编码的 `.zh \|\| .en` 全部改为按界面语言取值（市场详情条目卡、StoreDrawer/AgentDrawer、卡片标签、`@` 提及菜单）。验证：`nomifun-common` 3 例、`nomifun-app` 探针/投影 2 例、protocol vitest 6 例全绿；`bun run typecheck` 0 错 | ✅ 完成 |
| R29 | D | D-STREAM-1 ① | ~~`_ =>` 兜底转发与 permission 等事件仍共用轮 id，排在首段思考之后仍会顶掉它~~ ✅ **已完成（2026-09-10）**：兜底分支改用 `derived_message_id("event", event_kind)`——同一 kind 每轮一行、就地更新，不再顶掉首段思考 | ✅ 完成 |
| R30 | D | D-STREAM-2 ① | 让 `Finish` 不再等待记忆蒸馏 child（回答完成 ＝ 轮次结束） | ✅ **已完成（2026-09-11，批 5，按 `21` D9=A）**：蒸馏 child 从「`Finish` 前 `await`」改为**同轮次取消域内后台 `tokio::spawn`**（`manager/nomi/agent.rs` 收尾段 + 新增 `distill::spawn_distill_exact_turn` / `spawn_exact_turn_child`）。取消语义原样保留：child 持有本轮 `turn_cancel` 的 **clone**，`await_exact_turn_child` 的「取消即丢弃 future、到不了同步 apply 阶段」契约不变；spawn 前先判 `is_cancelled()`，已取消则不建 child，`TurnStopReason::Cancelled` 分支（`cancel_active_tool_calls` + `fence_cancelled_processes`）一字未改。失败仍 best-effort（既有 tracing 出口：provider 失败由 `debug` 提为 `warn` 使其在默认日志级别可见），未新增 wire 方法 / 事件类型 / SDK 改动。验证：`cargo test -p nomifun-ai-agent --lib` → **992 passed / 7 failed / 3 ignored**，其中 7 项失败**全部**为 HEAD 基线既有（逐例在 HEAD 复现，见 D-TEST-1 ③），本项新增/改写 **7 例**（3 例 manager 级：不等待 / 取消不建 child 且仍报 `Cancelled` / 蒸馏失败不把回合变成错误；4 例 distill 级：spawn 不阻塞调用方 / 取消后到不了 apply 阶段 / 已取消的轮次不建 child / real provider 失败正常返回且不写盘，另把原「child 先于终态」用例改写为「后台 child 不阻塞调用方」）| 
| R31 | D | D-STREAM-2 ② | ✅ **已完成（2026-09-11）**：`turn_completed` 到达后忙态文案从「正在处理」分级为「正在收尾…」（`common.wrappingUp`）。实现落在纯 reducer：噪声活动 `turn_completed` 不再只是丢弃，而是置 `wrapUp` 标记（`isProcessing` 语义**不变**——输入框该禁用还是禁用，只是不再让用户以为模型还在生成）；`turn.status{running:false}` / `setProcessing(false)` / `reset` / 新回执清标记；重放里的 `turn.status{running:true}` **不会**把已定的标记打回（D-STREAM-2 的服务端此刻确实仍报 `is_processing=true`）。验证：`bun x vitest run src/lib/conversation-events.test.ts` → **10 passed**（新增 6 例：置标记不入 transcript / 其他心跳不误置 / 回合结束清除 / 重放不清 / 回执与 reset 清除 / 可见活动照常渲染）。**未做**：`Finish` 侧的真实修法仍是 R30。 | ✅ 完成 |
| R32 | D | D-SDK-1 ④ | 缩短镜像代价（加大 BATCH / 增量跳过 / 换 HTTPS + CDN） | ⏸ **延后（2026-09-11 用户决定：站点部署卡点相关问题延后）**——HTTPS+CDN 大头随之延后；本地子项（BATCH=32 / 并发）不受阻但本轮不做，「增量跳过」已由 R26 落地 |
| R33 | E | 方向四 + WP-5 | ✅ **立项页已建（2026-09-11）：`22-webui-productionization.zh.md`**。11 项拆为四组：**A 安全类**（Origin / CSP / CSRF / 认证与令牌 / 限流配额）、**B 可观测类**（统一 Request ID / 健康检查 / 错误上报）、**C 功能类**（归档 / 批量 / 工作区重命名 / 只读标识 / 无障碍 E2E）、**D 协议词汇与概念对齐**；逐条给「可验收条目 + 边界 + 依赖」。**实测口径**：Origin / CSP / CSRF / 限流配额 / 归档在 `nomifun-app-server/src` 内**零命中**（`app_server_routes` 是裸 `Router::new()`，`nomifun-app-server/src/lib.rs:880-884`，`.layer(` 命中全在测试内）；Request ID **半有**——只回显客户端提供的 `req.id`（`:532-538` / `:4468-4500`），无服务端生成贯穿面；健康检查 **半有**——`ping` 在**认证路由组内**（`:883`），无未认证 liveness；4 条「未验证」已标注（批量操作 / 只读标识 / 工作区重命名 / 无障碍 E2E），取用前须先补取证 | ✅ 立项已兑现；**余项**＝该页 §6 未决 V1–V4（优先级信号、4 条「未验证」取证、A4 是否与 `06` 合并设计、A5 作用域与部署形态）；R33 本体仍未排期 |
| R34 | D | D-TEST-1 ② · 基线 `cargo test` 编译门 | ✅ **已完成（2026-09-11，批 7）**。**① 语义先于代码**（不机械补 `None`）：`registration_id` = 铸造该 token 的 `oauth_client_registrations.id`，**逻辑链接**（v3 schema 禁物理外键，`id_schema_contract.rs` 登记为非引用 id 列），legacy 行留 NULL 且无注册可解析时按 `requires_reauthorization` 处理（迁移 `052` 注释）；`principal_id` = 「为未来多用户预留、不是当前单设备模式下的 `users` 行」（同登记注释），生产写点只有两处且**都传 `None`**（`nomifun-mcp/src/oauth_service.rs` 的 `persist_token` 与 refresh 段），refresh 段反过来把 `row.registration_id` / `row.principal_id` 原样带回。**② 6 处调用点改用最小构造器** `UpsertOAuthTokenParams::new(...)`（`sample_params()` + 5 处内联字面量）——这些用例本就测 URL-keyed 的 legacy 路径，构造器语义（两列 `None`）与文档口径一致，注释写明为什么是合法状态。**③ 新增 5 例真断言**（不是补字段了事）：`legacy_token_has_null_registration_and_principal`（legacy 行两列 NULL、`get_by_registration` 查不到、URL 仍可达）、`linked_token_round_trips_registration_identity`（`.with_registration(42)` 落库后按 registration 读回同一行，其它 id → `None`）、`principal_id_round_trips_when_supplied`（预留列的值不被仓储吞掉）、`registration_lookup_is_scoped_per_row`（三条行两条链，互不串行）、`reupsert_carries_or_clears_registration_link`（upsert 是 `ON CONFLICT(server_url) DO UPDATE ... registration_id = excluded.registration_id` 的**全行写**：refresh 式带上则保链、省略即清链，`created_at` 仍保留）。**④ 未改迁移、未改语义、未动 R8 / R22**（本项只碰一个测试文件）。**真实输出**：修前 `cargo check -p nomifun-db --tests` → **exit 101 / 6× E0063**（全部在该文件）；修后 → **exit 0**（`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 4.15s`）；`cargo test -p nomifun-db --test oauth_token_repository` → **15 passed / 0 failed**（10 原有 + 5 新增）；`cargo test -p nomifun-db --lib` → **448 passed / 0 failed / 0 ignored**（本机 1512.22s）；`rustfmt --edition 2021 --check <该文件>` → 干净。**⑤ 顺带查明（同日收口，见 D-TEST-1 ④）**：修好 ② 后全仓 `cargo check --tests` **曾仍不绿**——`crates/backend/nomifun-ai-agent/tests/acp_agent_integration.rs:189` 的 `event_type_name` 少 `AgentStreamEvent::OutputDiscarded(_)` 分支（**E0004，HEAD 既有**：该 variant 在 `HEAD:src/protocol/events/mod.rs:41` 已存在、而该测试文件在工作区未被改动）；`nomifun-mcp --tests` 编译通过（仅既有 warning）。**该处已于同日补上分支（`"OutputDiscarded"`，只改这一个测试文件）并实测收敛**：归属核验先证明无活跃 owner（该测试文件 `git status` 未被改动、mtime 停在 2026-08-12；`OutputDiscarded` 生产端 `manager/nomi/agent.rs` 的未提交改动停在 03:34 且 `capability/backend_output_sink.rs` 完全未改；`session_turn_leases` 为空、除本会话外无 allo 活会话），随后 `cargo check --tests -p nomifun-ai-agent` 修前 **exit 101 / 1× E0004** → 修后 **exit 0**（`Finished \`dev\` profile ... in 7.92s`）；四 crate 宽门 `cargo check -p nomifun-ai-agent -p nomifun-db -p nomifun-app-server -p nomifun-app --tests` → **exit 0**（2m46s）；整仓 `cargo check --tests --workspace` 当时**仍 exit 101**（剩 D-TEST-1 ⑤ 那 2 处；**已于批 7 续 2 收敛为 exit 0**）；`cargo test -p nomifun-ai-agent --test acp_agent_integration` → **1 passed / 0 failed / 11 ignored**（11 例是文件头注明的 `requires JSON-RPC mock agent` 跳过，与本次无关）。 | ✅ 完成（D-TEST-1 ②④⑤ 均已收敛，全仓 `cargo check --tests --workspace` 编译门绿；**D-TEST-1 ③ 的 7 例运行时失败亦已于 2026-09-11 收口 → `cargo test -p nomifun-ai-agent --lib` 999 passed / 0 failed / exit 0**，逐例归属与修法见卡点决策表 D-TEST-1 行 ③） |

> **⚠️ 口径缺口（✅ 已解决）**：方向① 的退出条件（§5.1）只写「A1 + A2 修完；发 `0.1.0-beta.3`；C1 三处事实错误修正」，未含 C2 / C3 / C4。R4–R6 因此既不在「维护模式（须真实外部 issue）」清单，也不在「未排期」清单—— ✅ **已解决（`21` D1=A：全解除）**：R4–R6 不再「无主」，分别归入批 0（已完成互引用与同步校验）与批 2 / 批 4。

> **📌 拍板入口**：本表里所有**需要人拍板**的事项已集中到 `21-open-decisions.zh.md`（**D1–D12**，每条带 ⭐ 默认建议与解锁范围）。回复 `D1=A D2=B …` 或一句「**全部按建议**」即可解锁连续推进；批 0（无卡点项）不依赖任何拍板。

> **R6 剩余项落地记录（2026-09-11，批 4 · changelog 页）**：新增站点 `changelog` 页（`site/content/docs/{zh-CN,en-US}/changelog.md`，slug ＝ `changelog`，命名沿用站点既有的 kebab/英文 slug 习惯）。
> - **三处同步**：`site/app/lib/docs.ts`（`DOC_ORDER` 在 `upgrade` 之后插入 `changelog` + `DocSections.changelog`）、`site/app/i18n/{zh-CN,en-US}.ts`（`docs.sections.changelog`，两语言 **9 项且顺序一致**）。渲染层无需改动——`docs/:slug` 是动态路由，侧栏与文档索引都由 `DOC_ORDER` 驱动。
> - **内容纪律（宁缺勿编）**：发布序列以**本机实测**为准（`npm view @flowy-agent-store/sdk versions dist-tags time --json` → `versions` ＝ `0.1.0-beta.2` / `0.1.0-beta.3` / `0.1.0`，`dist-tags` ＝ `{beta: 0.1.0-beta.3, latest: 0.1.0-beta.2}`，`time` ＝ `0.1.0` 2026-09-09T09:09:04.795Z / `beta.2` 09:27:36.122Z / `beta.3` 2026-09-10T04:44:34.609Z，与 `upgrade` §2/§3 **完全一致**，无差异）；每版「改了什么」**只引仓库内已有记录**（`16` R4 行的 tarball 逐字节比对结论、`21` 的 R4 落地记录），不新增判据；**未发布项**（`event_type` 收窄、wire 方法增量、release 重建与 `beta.4`）单列一节并标注「未发布 / 挂起」。
> - **D10=A 落到格式约定**：本页是破坏性变更的**唯一公告面**（不靠 commit message / 聊天记录 / release 页）；破坏性变更走 minor 号 + 本页条目标「破坏性」并给迁移做法；条目**发布后才追加**、版本号与日期**永不改写**、写错追加「更正（日期）」、**dist-tag 移动不产生条目**。
> - **交叉引用两语言同步**：`upgrade` 页 §1 两条要点、§9 表格行（`尚未建设` → ✅ 已建设 + 新页链接，并新增「本页与 changelog 的分工」行）、§10 另见，各加指向 `changelog` 的相对内链。
> - **验证（真实输出）**：`scripts/check-docs-sync.mjs` → **9 page(s) in 2 language(s), 0 drift(s)**（新增页后总数 8 → 9）、`--self-test` → **9/9 as expected**、`bun test scripts/check-docs-sync.test.mjs` → **16 pass / 0 fail**；新页另做 4 类突变反向测试（标题层级 / 代码块数量与语言 / 表格列数 / 相对内链，**均被拒**）；`site` 下 `tsc --noEmit` **0 错**、`react-router build` **通过**并预渲染 `/zh-CN/docs/changelog` 与 `/en-US/docs/changelog`。
> - **本轮顺带修（既有破损）**：`scripts/check-docs-sync.test.mjs` 的「本站页码清单」仍停在 **7 页**（上一轮加 `upgrade.md` 时漏改）→ 该用例在 HEAD 上本来就 **1 fail**（15 pass / 1 fail）；已补 `upgrade.md` + `changelog.md`，现 **16 pass / 0 fail**。
> - **守卫既有边界（登记，不改）**：表格列数只比**表头/分隔行**——数据行少写一列不会被拦（突变测试实测确认；其余三类规则对新页均正常触发）。
> - **未做（有意）+ 解锁条件**：**站内搜索**（属站点新功能，会把构建期索引一致性这一新面拉进来）与**市场数据刷新纳入发布流程**（卡在部署访问权，与 D2 域名/DNS 同源）——见下方卡点决策表 R6 行。**未动协议与 SDK 源码**（无新 wire 方法、未改 `web/packages/*/package.json` 版本号、未发布/未修改任何 npm 包、未 commit / push）。
>   - **订正（2026-09-11 C 档复核，保留原文以便追溯）**：上句「站内搜索会把构建期索引一致性这一新面拉进来」**不成立**——`site/app/lib/docs.ts:10-14` 已用 `import.meta.glob(..., { eager: true, query: "?raw" })` 把 docs **全文内联进 bundle**，搜索可**纯客户端**实现，无构建期产物、无新守卫面。该条现已改判为**产品取舍**（见下方「C 档判定（重写）」）。

> **R34 落地记录（2026-09-11，批 7 · `16` D-TEST-1 ②）**：**唯一改动文件** `crates/backend/nomifun-db/tests/oauth_token_repository.rs`（**未改迁移、未改 src、未 commit / push**）。语义来源（读代码而非猜）：`crates/backend/nomifun-db/src/repository/oauth_token.rs`（两字段注释 + `new()` / `with_registration()`）、`migrations/052_oauth_client_registrations.sql`（`registration_id` / `principal_id` 加列 + 索引 + legacy 语义注释）、`src/models/oauth_token.rs`（`OAuthTokenRow`）、`src/id_schema_contract.rs:298-301`（两条非引用 id 列登记，「`principal_id` 是未来多用户 owner、不是当前单设备模式的 `users` 行」）、生产写点 `crates/backend/nomifun-mcp/src/oauth_service.rs:1172`（refresh：原样带回 `row.registration_id` / `row.principal_id`）与 `:1208`（`persist_token`：`registration_id` 来自授权流程、`principal_id: None`）。修法：6 处字面量 → `UpsertOAuthTokenParams::new(...)`，并新增 5 例真断言（见 R34 行 ③）。**真实输出**：`cargo check -p nomifun-db --tests` 修前 **exit 101**（6× E0063，行 19/62/86/133/155/192）→ 修后 **exit 0**；`cargo test -p nomifun-db --test oauth_token_repository` → **15 passed / 0 failed / 0 ignored**（50.88s）；`cargo test -p nomifun-db --lib` → **448 passed / 0 failed / 0 ignored**（1512.22s）；`rustfmt --edition 2021 --check` → 干净。**边界**：① 注册行本身仍由 `sqlite_oauth_client_registration` 的单测覆盖，本项只钉 token 侧的链接列；② `principal_id` 目前**没有任何生产写点传非 NULL**（预留），本项只保证仓储透传、不引入多用户语义；③ 全仓 `cargo check --tests` 仍因 D-TEST-1 ④（`acp_agent_integration` 的 `OutputDiscarded` 分支）不绿——**不在本项范围**。

> **R34 落地记录（续 · D-TEST-1 ⑤ 收口，2026-09-11，批 7 续 2）**：上一条记录里的 ③ 已被兑现——**整仓编译门转绿**，`cargo check --tests --workspace` 由 **exit 101 → exit 0**（149s ＝ `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 2m 25s`，日志 `^error` 计数 0）。**改动面（只碰测试文件）**：`crates/agent/nomi-agent/tests/bootstrap_test.rs`（`:393` 断言改挂真实字段 `output_max_tokens: Option<u32>`）、`crates/agent/nomi-agent/tests/autocompact_test.rs`（`:79` mock 去掉多余 `Some`、`:704` 陈旧期望 `Some(4000)` → `Some(16_000)`）。**语义先于代码**：`Config` 的输出上限字段是 `output_max_tokens`（`max_turns` 是轮次），`LlmRequest::max_tokens` 是 `Option<u32>`（`None` ＝ 让 provider 用默认，见 `crates/agent/nomi-types/src/llm.rs:13-15` 的注释）——三处都按真实类型 / 真实公式断言，无 `assert!(true)`、无 `let _ =`、无删用例。**归属证据链（第 ③ 条陈旧期望）**：`git blame` 显示该期望随 `27742b15b`（2026-08-19）引入，当时公式为 `compact_max_output_tokens = (window/32).max(512).min(4096)`（`git show aac092873^:crates/agent/nomi-agent/src/compact/prompt.rs`，128k ⇒ 4000）；`aac092873`（2026-08-21）改用 `window_output_unit = (window/8).min(20_000)`（`crates/agent/nomi-config/src/compact.rs:169-173`，128k ⇒ 16_000）并删掉旧常量——即 **4000 是旧公式的遗留，不是生产回归**。**未做**：`crates/agent/**/src/`、迁移、`057_*`、R8 审批门、R22 凭据门一律未动；未 commit / push。

#### 已知偏差

**登记规则（强制）**：规范与实现不一致时，先在本节登记，再择一修正——要么改实现，要么改规范，不允许默默不一致。

| # | 现象 | 证据 | 影响 | 待决 |
| --- | --- | --- | --- | --- |
| D-SDK-1 | **冷启动首次 `store/list` 远超默认请求超时** | 干净目录 `npm i @flowy-agent-store/sdk@0.1.0-beta.3` 后实测：`launchClient` 2565ms、`models/list` 1ms、**首次 `store/list` 91733ms**（438 条目，全新 data-dir） | `typescript-sdk.md` 只写 `requestTimeoutMs` 默认 30s，按文档默认参数调用首次 `store/list` 必得 `RequestTimeoutError`；发布闸门 `verify-published-sdk.ts` 已硬编码 120s 绕开该问题；**SDK 默认每次新建临时 data-dir（`spawn.ts:72-82`）→ 每次 spawn 都重付**；仓库自带的 `bun scripts/smoke.ts --real` 也会被它自己的 15s 超时打爆（2026-09-10 实测冷启动运行时，第二次现场复现） | ① 把默认市场注册从 `store/list` 解耦——✅ **已修（2026-09-10）**：`ensure_default_marketplaces` 改为返回「是否全部注册成功」，新增 `warm_default_marketplaces`（`AtomicBool` 守卫 + `tokio::spawn`，不完整则清标记供下次重试），`store/list` / `market/list` / `market/get` 三个调用点不再 `await`。**实测**（重建 release 二进制 + 全新 data-dir）：`first store/list: 1ms items=0` → 130 秒后 `store/list: 133ms items=438`、`market/list count=3`。**加载态——✅ 已做（2026-09-10，用户放开协议改动）**：① 启动即预热（`nomifun-app/src/router/routes.rs` 构造 state 后调一次 `warm_default_marketplaces`，带 `Handle::try_current()` 守卫，无运行时则退回懒触发）；② `store/list` 响应新增 `markets_pending`（`#[serde(default, skip_serializing_if = "Not::not")]`，纯 additive，`false` 时不上 wire），由 `nomifun_app_server::marketplaces_warming()` 供 `AppServerStoreProvider::list` 读取；③ WebUI 空目录时显示「内置市场仍在后台载入…」而非「还没有市场」。**实测（2026-09-10，重建二进制 + 全新 data-dir）**：`first store/list: 1ms items=0 markets_pending=true` → 135 秒时**仍未完成**（`items=275`、`market/list count=2`、`markets_pending=true`，第三个源 `connectors` 未完）——真实窗口可达 **2 分钟以上**（随源站带宽波动），标记如实反映。**残留**：`markets_pending` 目前只作用于「空目录文案」，**部分目录（非空）时 UI 不额外提示**；如需提示，要把 store 面板包一层 banner（小改，未做）——**已在同日修复，见下**。
> **人工实测暴露的 3 处修正（2026-09-10）**：
> 1. **`markets_pending` 永久为 true（我实现的 bug）**：原用一个 `AtomicBool` 兼表「运行中」与「已完成」，预热成功后仍为 `true`——实测 3 个市场全在册却报 `markets_pending=true`。已拆为 `DEFAULT_MARKETPLACES_RUNNING`（跑时置位、结束清位）+ `DEFAULT_MARKETPLACES_DONE`（仅完整跑完才 latch，不完整则留空供下次重试），`marketplaces_warming()` 只读 RUNNING。
> 2. **市场列表只在挂载时拉一次、不轮询**（使用陷阱）：在预热窗口内打开页面 → `market/list` 返回空 → 列表停在空态、**没有可点的市场行**，W13 的详情字段/开关/仅导入/移除弹窗全部无从出现。已在 `CatalogView` 增补 pending 期间**自动重取**（20s × 最多 5 次，之后停止），并在市场源页非空的部分目录时显示 `.market-pending-note` 提示。
> 3. 市场源页空态文案改为复用 `catalog.storePending`（区分「仍在载入」与「还没有市场」）。② SDK 默认 data-dir——✅ **已定（2026-09-10）：不改默认**；预热后台化后「每次 spawn 重付」的痛感已消除，需要跨进程复用就自持 `dataDir`（`typescript-sdk.md` §4.3 已写明）③ 文档止血——✅ **已被 ① 取代**（2026-09-10：中英 §4.3 从「首次请传 120s」改为「首次可能为空 + 后台预热」，§5.1 注释同步）④ 缩短镜像代价（加大 BATCH / 增量跳过 / 换 HTTPS+CDN，与 Q2 同源） |
| D-W5-1 | **W5 的「artifact 协议已就绪」前提不成立** | `19` §3 / §7 原称 `artifact/list` / `artifact/get` 已定义、`artifact.created` 已发；实测 `05`:152 与 `TC-AS-008` 明确 Artifact 能力**延后实现**（`capabilities.artifacts` 恒 `false`，且规定不允许任意路径读取），TS 协议包与 `packages/client` 均无对应类型与子客户端，`artifact.created` 事件不存在 | W5 无法按「按 Run 的产物 + 接受/回退」交付；在「第 3 批不碰协议」的边界下，AC-5 的「按 Run 归属」与「回退」两项不可达 | ✅ **已定（2026-09-10）：收敛为宿主面文件服务方案**——按会话 workspace 交付列表 / 预览 / 下载（`/api/fs/list` + `/api/fs/read`，服务端已有），「按 Run 归属」与「接受 / 回退」留待 Artifact Phase；`19` §3 / §7 已按实测修正 |
| D-W13-1 | **W13 要展示的 `revision` / 上次刷新在 wire 上不存在** | `MarketplaceSummary`（`protocol.ts:526`）与 `AppServerMarketplaceSummary`（`nomifun-api-types/src/app_server.rs:542`）只有 `version` / `auto_update` / `enabled` / `entry_count` / `added_at`；`resolved_revision` 仅出现在 `market/refresh` 响应里，`last_checked_at` 只是 DB 列（`plugin_marketplace.rs:52`），两者都未上 wire。另外 `market/remove` 的受影响快照只在**移除后**返回（`MarketplaceRemoveResult.snapshots`），没有移除前投影 | W13 的注册表字段展示缺这两项；级联确认清单只能从聚合的 `store/list` 派生（已如此实现） | ① 协议增量——✅ **已做（2026-09-10，用户已放开协议改动）**：`AppServerMarketplaceSummary` 加 `resolved_revision` / `last_checked_at`（`#[serde(default, skip_serializing_if)]`，纯 additive，`Detail` 走 `#[serde(flatten)]` 自动带上），`to_summary` 投影补两字段，测试替身同步；TS `MarketplaceSummary` 补两字段，`CatalogView` 详情面板加两行 `MetaRow`（源修订 mono、上次检查本地时间）。② 不再需要。**实测注意**：`resolved_revision` 在 `market/add` 后即有值；`last_checked_at` 由**刷新**写入，`market/add` 插入时显式 NULL（DB 语义即「上次检查」），因此新注册的市场在首次 `market/refresh` 前该列为空——UI 显示显式 `—` 而非静默丢行 |
| D-W13-2 | **「市场源」与「导入记录」两个页面在 UI 上不可达** | `CatalogTab`（`CatalogView.tsx:59`）定义了 `"store" \| "sources" \| "installed" \| "imports"`，`sources` 面板（`:1261`）与 `imports` 面板（`:1474`）都有完整渲染块，但 `setTab("sources")` 只在**商店空态**按钮里出现一次、`setTab("imports")` 全文件**零次**调用；`catalog.tabStore` / `tabSources` / `tabImports` 三个文案键已存在却无控件消费（顶栏页签在某次「分类行取代页签」的改动中被删掉） | 商店非空时（实测 438 条）空态永不出现 → **W13 的全部验收项（注册表字段 / auto-update 开关 / 条目级导入 / 移除确认）在 UI 上都无法触达**，T15 的 UI 级验收卡在此处；`19` §3 W13 的「点市场进详情」路径没有入口 | ✅ **已修（2026-09-10）**：`.market-tabs` 恢复为四页签分段控件「应用商店 / 市场源 / 导入 / 已安装」（`Store` / `Globe` / `Upload` / `Users` 图标，激活态 `.market-mine.is-active`），搜索框仍只在商店/已安装显示 |
| D-STREAM-1 | **直播流里工具事件与「本轮首段思考」共用 `msg_id`，前端整条互相覆盖** | 运行时：本轮第一段 thinking 的 wire id = `root_turn_id`（`stream_relay.rs` 的 `mint_thinking_segment_id`，:2385 → :3654），而工具事件走 `forward_to_websocket(&event)` → 内部固定用 `self.msg_id`（:3698，默认即 `root_turn_id`）；App Server 投影把两者都写成同一个 `message_id`（`nomifun-app-server/src/lib.rs:4032-4036` / `4063-4073` / `4083-4090`）；前端 `mergeMessagesById` 按 `message_id` 扁平 upsert、**后写整条覆盖**（`web/src/lib/conversation-events.ts:259-263`） | 实时渲染时首段「思考过程」被紧随其后的工具事件**顶掉**；同一轮内两次工具调用也会互相覆盖；重载历史后一切正常（两条 id 本不相同）——表现为「实时少一段思考、刷新后才对」 | ✅ **已修（2026-09-10）**：`ToolCall`（含 artifact 路径共 3 处转发）/`ToolGroup`/`AgentStatus` 改为使用各自持久化行的派生 id（`tool_message_id` / `derived_message_id("tool_group", …)` / `agent_status_message_id`），直播与历史同 id；`cargo check -p nomifun-conversation --tests` + `cargo test -p nomifun-conversation --lib thinking` 9 passed。**残留**：① `_ =>` 兜底转发与其他仍用 `self.msg_id` 的事件（permission / 未列举 kind）在首段思考之后到达时仍会顶掉它；② 直播 `message.tool` 投影**不含 `args`/`output`**——✅ **已做（2026-09-10，用户拍板）**：投影补齐 `args`/`output`（`nomifun-app-server/src/lib.rs`，仍不带 `call_id`/`input`/`turn_id` 等不透明标识），前端 reducer（`web/src/lib/conversation-events.ts`）改为**按字段增量合并**，使 running → completed 两帧不会互相清空；投影测试同步改名为 `conversation_tool_projection_carries_args_and_output_but_hides_ids`。**实测**：`4431ms message.tool name=Glob args={"path":".","pattern":"*.md"} status=running` → `4438ms … status=completed output=No files matched the pattern`。**代价（已记录）**：args/output 会随每个状态帧重复上 wire，大输出工具会放大流量；历史路径本就是同一份数据、只是晚到。 |
| D-STREAM-2 | **回答可见后仍显示「正在处理」约 6–15 秒（后端收尾 child 阻塞 `Finish`）** | 实测探针（临时会话，跑完即删）：`33ms RECEIPT completed=false` / `33ms turn.status{running}` → `15079ms message.delta`（回答完成）→ `15245ms message.activity{turn_completed}` → **25s 时 `conversations.get` 仍报 `is_processing=true status=running`**。运行时日志同轮：`execute_turn() completed; closing exact post-turn effects before Finish, elapsed_ms=15178` → `StreamRelay received terminal event event_type="Finish" elapsed_ms=24996`，即**回答完成后 9.8 秒才发 `Finish`**。代码位置：`nomifun-ai-agent/src/manager/nomi/agent.rs:1965-2036` —— **post-session memory distillation**（`super::distill::run_distill_exact_turn`，一次 provider 调用；门控 = host opt-in `distill_enabled` + `distill_dir` + human origin）在 `Finish` 之前被 `await`，注释明写「Finish is forbidden until this child closes」；其后还有 `post_turn_review` hooks | 用户视角「答案已出但一直转圈、输入框被锁」；前端**没有说谎**（服务端此刻确实 `is_processing=true`），是该窗口内没有任何 wire 信号（最后可见事件为 `turn_completed`，下一个事件要等近 10 秒） | ⏸ **待拍板**：① 让 `Finish` 不再等待记忆蒸馏（改为后台 child + 已有 recovery 观测），使「回答完成」与「轮次结束」同时发生——动 durability 语义，需记忆侧确认；② 纯 UI：`message.activity{kind:"turn_completed"}` 之后把忙态文案分级为「正在收尾…」（不改语义，成本最低）；③ 关掉记忆蒸馏——✅ **已做（2026-09-10）：配置化**。`~/.agent-store/config.toml` 新增 `[memory].distill_enabled`（`AgentStoreConfig.memory`，`Option<bool>`，缺省 = 上游默认 ON；`nomifun-app-server/src/agent_store.rs`），由 agent-store 主机启动时经 `nomifun_ai_agent::manager::nomi::distill::set_distill_host_override()` 注入（**进程内原子，不改环境变量、不受线程启动顺序限制**；优先级：`NOMIFUN_MEMORY_DISTILL` 环境变量 > 本文件 > 上游默认）。文档：`site/content/docs/{zh-CN,en-US}/configuration.md` 新增 `## memory` 小节 + 顶层键行；本机 `~/.agent-store/config.toml` 已写入 `[memory] distill_enabled = false`。①（让 `Finish` 不再等待蒸馏 child）—— ✅ **已拍板（`21` D9=A）：改为不等待**；用户已确认「蒸馏延迟/失败不影响会话正确性」，**已于批 5 落地（2026-09-11，见 R30 行与「R30 落地记录」）**。 |
| D-W6-1 | **W6 验收口径里的「耗时 / 失败原因 / 步骤标题 / sub-agent 归属」在 wire 上不存在** | `run/events` 里 `task.updated` / `attempt.updated` 的 payload 实测只有标记：`{change:"retry_requested"}`、`{change:"conversation_effect_delivered",effect:"steer"\|"stop_turn",operation_id}`、`{status:"queued"\|"running"}`、`{attempt_status,step_status}`、`{reason:"process_restart"\|"queued_before_restart",reconciliation:"initial_turn_receipt"}`（`nomifun-agent-execution/src/scheduler.rs:1199/1289/1346/1559/1604/1842/1896/1974/2047`）；`run.plan_changed` 只有 `{change:"initial_plan"\|"replanned"\|"adjusted"+intent\|"steps_added"\|"delegated_steps_appended"}`，**不含步骤标题**（`engine.rs:1053/1560/1636/1713/2658`）；`AgentRunEvent` 无时间戳字段（`runtime_adapter.rs:65`），引擎 `detail` 里的 `title/error/started_at/finished_at` 从未投影到任何 `run/*` 方法（`run/get` / `run/result` 只回 `RunView`），member（sub-agent）事件也不存在 | 步骤标题、耗时、失败原因、member 归属在 UI 上**不可得**；R11 只交付了可得的部分（状态 / 重试次数 / 尝试 / 审批 / 副作用标记 + 原始事件面板），`19` §3 W6 的验收口径因此**未全满足** | ⏸ **待决策**：加 additive `run/plan`（owner 作用域，投影引擎 `detail` 的 steps/attempts：`title/kind/status/attempt status/trigger_reason/error/started_at/finished_at`，WS 臂 + HTTP 路由各一）。R11 原按文档归类为「纯 UI」，故先登记不擅自加方法；替代方案是维持「只显示事件标记」（现状） | ✅ **已解（2026-09-11，批 3）—— 按本行的建议方案实施了 additive `run/plan`**（决定权在于用户 2026-09-11 的「卡点问题你自己决定，但需要记录到文档」）。`AgentRunPlan` 投影引擎权威行：step 的 `title`/`kind`/`status`/`role`+`model`（成员归属）/修订号，attempt 的 `trigger_reason`/`question`/`error`/`output_summary`/`output_files`/`tokens`/`started_at`/`finished_at`。**逐条对上本行的缺口**：① 步骤标题 → step.`title`；② 失败原因 → attempt.`error`（+ `trigger_reason` 说明为何有这次尝试）；③ 耗时 → `finished_at - started_at`（两端齐全才算，进行中不给 0）；④ sub-agent 归属 → `role` + `model`（**有意不投影 participant_id / source_agent_id**，沿用「无公开映射的内部 id 不上 wire」规则）；⑤ `member 事件不存在` 不再是阻塞——归属改由快照表达，事件面不必新增。owner 作用域与 `run/get` 同一 `engine.get`（外部 owner→`NotFound`）。验证：`cargo test -p nomifun-app --test agent_execution_decision_e2e` → **8 passed**（新增 plan 快照用例）；`cargo test -p nomifun-app-server --lib` → **67 passed**；`run-plan.test.ts` 11 例。文档：`05` §5.2 方法清单 + 语义段、`16` R10 行、site §7.3 路由计数 44/57→45/58。
| D-W9-1 | **W9 的「按 turn 的费用汇总」原先缺 per-turn token —— 批 6 查明「不缺链路、只差一层投影」，已补（2026-09-11）** | ① 费率原先不在 wire 上：`conversation/model-options` 只回 `name` / `display_name` / `context_limit`（`nomifun-app-server/src/lib.rs:2615`），models.dev 的 `cost_input` / `cost_output` 只被运行时使用（`nomifun-ai-agent` 的 MoA 槽位价 `factory/moa.rs:147`、provider 配置 `factory/provider_config.rs:232`），从未投影给客户端——**批 3 已补**（见 R14 行 ①）。② **per-turn token 从不缺**：运行时 `TurnCompleted` 一直带 `input_tokens` / `output_tokens`（`nomifun-ai-agent/src/protocol/events/mod.rs:218-256`），会话中继也一直无条件转发（`nomifun-conversation/src/stream_relay.rs:3412` 的 `message.stream{type:"turn_completed"}`），**是 App Server 投影 `project_conversation_notification`（`nomifun-app-server/src/lib.rs:4397`）把它降级成活动标记时丢掉的**；Run 面 `TurnResult.usage`（`web/packages/client/src/turn-result.ts:15-17`）只是另一条路径，从来不是唯一来源。 | **批 3 的「算不出来」结论已撤销**：费率 + 逐轮 token 都在之后，UI 显示「本轮 token + 本轮估算金额」。金额口径不放宽：**费率与 token 两者都在才显示**，任一缺席整段不渲染，且**绝不用「上下文占用 × 费率」冒充本轮花费**。 | ✅ **已落地（2026-09-11，批 6）**：additive 投影 `message.activity.usage`（缺席即不上 wire）+ 客户端 `parseTurnUsage` / `turnUsage` / `turnCostUsd`，验证见 R14 行 ⑥（`cargo test -p nomifun-app-server --lib` 79 passed；web 320 passed）。**剩余边界（未做，需拍板）**：逐轮 usage **未持久化**——历史轮次重载后不显示（只显示本会话刚完成的那一轮）；要回放需给 `app_server_context_usage` 加列（如 `last_turn_input_tokens` / `last_turn_output_tokens`）或单开逐轮表 + 迁移，二选一需决策后再动。 |
| D-TEST-1 | **HEAD 上 `cargo test` 编译不过：多处测试未跟上类型演进（①②③④⑤ 已于 2026-09-11 全部收敛：①②④⑤ 是编译断点、③ 是 7 例运行时失败，现主门 `cargo test -p nomifun-ai-agent --lib` 与整仓编译门同时绿）** | ① `nomifun-api-types/src/mcp.rs` 的 `test_oauth_status_response` / `test_oauth_login_response` 构造 `OAuthStatusResponse` / `OAuthLoginResponse` 时缺新增字段 `state` / `error_code`；② `nomifun-db` 集成测试 `oauth_token_repository` 的 6 处 `UpsertOAuthTokenParams` 缺 `principal_id` / `registration_id`；③（第二处编译断点，见下方 ④）`nomifun-ai-agent/tests/acp_agent_integration.rs` 的 `event_type_name()` 穷尽 `match` 缺 `AgentStreamEvent::OutputDiscarded(_)` 分支 | `cargo check --tests` 与 `cargo test` 在该基线上**直接失败**——任何人都无法跑测试，且极易误判成「自己改坏了」（批 1 实现 R25 时即先撞到它） | ✅ **① 已修（2026-09-10，批 1 顺带）**：补齐字段，并让断言覆盖新增的 wire 字段（`state` / `error_code`）；② **✅ 已完成（2026-09-11，批 7，见 `16` R34 行）**：先判语义再动手（`registration_id` = 铸造该 token 的 `oauth_client_registrations.id` 逻辑链接、legacy 行留 NULL；`principal_id` = 为未来多用户预留、当前单设备模式**只有** NULL 这一合法态，两列在 `id_schema_contract.rs` 登记为非引用 id 列、迁移 `052` 注释写明 legacy 语义），6 处调用点（`sample_params()` + 5 处内联字面量）全部改用最小构造器 `UpsertOAuthTokenParams::new(...)`（这些 URL-keyed 用例本就不涉及注册/主体），并**新增 5 例真断言**（legacy 行两列 NULL 且注册查询查不到 / 带注册的行按 `get_by_registration` 读回 / `principal_id` 值透传 / 注册查询不串行 / upsert 是全行写：带上则保链、省略即清链）；③ **✅ 已收口（2026-09-11）**——`cargo test -p nomifun-ai-agent --lib --no-fail-fast` 从 **992 passed / 7 failed**（exit 101，90.53s）转 **999 passed / 0 failed / 3 ignored**（exit 0，161s 含编译、用例段 99.69s），7 例逐条 `... ok`。**动手前先做归属核验**（证明失败都在 HEAD 基线、无活跃 owner）：`factory/nomi.rs` 与 `factory/acp.rs` 在改动前与 HEAD **逐字节相同**（`git diff --quiet` exit 0）；三例 `manager/nomi` 用例的测试体在改动前与 HEAD 逐字相同（`git blame`：图片例 = `27f620d5eb` 2026-07-12、provider 错误例 = `8adfe861e6` 2026-07-16、MaxTokens 例 = `52e05c19f` 2026-07-07），工作区对 `agent.rs` 的未提交改动只有 R30 段（`@@ -1853…` / `@@ -1965…` / `@@ -2005,13 +2017`）与追加的 R30 测试块（`@@ -4301,6 +4311,240`），都不在这三例上；`session_turn_leases` **0 行**、`sessions` 中最近一条 `C:\workspace\allo` 会话停在 2026-08-27、进程表只有本轮自己的 `cargo.exe`/`rustc.exe`。**逐例归属与修法**： **（a）`manager/nomi` 3 例 = 测试期望陈旧（宿主侧续写机制已被删除）→ 改测试，场景改挂新机制**。根因是 `cbe698ff2`（2026-08-21「make an output-ceiling truncation a resumable round」）删掉宿主侧 auto-continue：`crates/agent/nomi-agent/src/round.rs:42-44` 明文写着被删的 `MAX_TRUNCATION_AUTO_CONTINUES = 2` 与「截断草稿不可续写、只能带 ledger 重试原始需求」。三例实况：① 图片例的 provider 载荷实测为 `[Text("[Context]\nCurrent date: …"), Text("What is shown?"), Image(image/png, 212 bytes)]`——附件**一直**到达 provider，只是轮次尾部的 `[Context]` 块（`engine/mod.rs:1602-1607` + `context_contributor.rs:104`）占了首块，旧断言按「恰好两块 `[Text, Image]`」定形匹配；② 用例想守的核心不变量**仍成立且被保留**（实测事件序列 `Start → OutputDiscarded(attempt=2) → TurnCompleted → Finish`，全程无 `ToolCall(nomi-call-large-write)`）；③ 被断言的那 4 句宿主提示（`Do not call Write with a full large file in one call` 等）在 HEAD 上**除该用例自身外全仓无引用**（`git grep … HEAD -- crates/` 只命中测试 4 行）；新版只在收到 `LlmEvent::ToolUseTruncated` 证据时重开一轮，纯 `ToolUseDelta` + `Done{MaxTokens}` 不再触发续写（实测 provider 调用 1 次）。修法：图片例改为「问题文本逐字 **且** 恰好一个 `Image(image/png, 非空)` 块」（等值断言，非存在性放宽）；另两例改用当前协议表达 `LlmEvent::ToolUseTruncated{…} + Done{MaxTokens}`（并注册 `Write` 使其真被 advertise）→ 引擎在同一轮内重开，**原强度全部保留**（`provider.calls() == 3`、`requests.len() == 2`、`OutputDiscarded.restart_attempt == vec![2]`、`Start` 恰好 1 次），并新增反向断言 `!contains("continue where you left off")` 与被删提示的替代文案断言（`[resumable round 2/3]` / `WHAT WAS CUT OFF:` / `Write (65536 bytes of arguments streamed, NOT executed)` / `Split any large file: write a small complete version first, then edit or append.`）；MaxTokens 例按机制改名 `max_tokens_truncated_write_restarts_without_repeating_the_large_write`。**顺带钉死一例假绿**：`provider_error_…` 原先没注册 `Write`，首轮 `is_err()` 其实由「tool progress 'Write' was not advertised」这条协议违规满足、而非用例想测的 provider 错误——现在注册后用 `expect_err` + 断言错误文本含 `malformed structured tool arguments` 把因果固定。 **（b）`factory/nomi` 3 例 = 生产行为回归 → 改 `src/` 一行**。`resolve_nomi_url_and_compat` 的 chat 全 URL 分支把 `compat.api_path` 设成 `Some("/chat/completions")`，与三处口径矛盾：同函数文档注释（「the configured URL is the request URL … with an empty `api_path`」）、紧邻的 openai-responses 分支（`Some(String::new())`，注释写明 resolver「replaces `api_path` with an empty suffix」）、以及消费端拼接 `nomi-providers/src/openai.rs:845` 的 `format!("{base_url}{api_path}")`（叠加后得到 `…/v1/chat/completions/chat/completions`）；`nomifun-api-types/src/dispatch_target.rs:11,79` 亦定义 `is_full_url` ＝「base_url is already the complete endpoint」并原样使用。**修法**：该分支改回 `Some(String::new())` 并写明理由——**模块内 2 个单测 + 220 行平台快照测试一字未改即转绿**，即期望与生产重新一致。**归属证据**：`Some("/chat/completions")` 的写法由 `a833120a9`（2026-07-07）引入（`git log -S` 唯一命中）；快照用例来自另一条并行谱系——`501899b87`（2026-07-29，用例与它同提交）及其合流点 `b35663c78` 上该分支都是 `Some(String::new())`，两条谱系汇入 HEAD 时取的是 `/chat/completions` 一侧，于是「锁定旧行为的快照」自合流起就与实现不一致（精确合流提交未逐条二分，不影响归属：该文件改动前与 HEAD 逐字节相同）。**为何长期未暴露**：HEAD 上 `cargo test` 根本编不过（①②④⑤），这些运行时断言从未被执行。 **（c）`factory/acp::tests::row_to_sdk_stdio_roundtrip` = 环境相关 → 改测试为与主机无关**。失败原文：`unexpected stdio command path: <…>/mise/shims/npx.exe`；`resolve_command_path` 按设计经宿主 PATH 解析（Windows 会带 shim 后缀，取决于谁装的 npx），旧断言只列举 `/npx` 与 `/npx.cmd`。**修法**：取 basename、剥掉 `.cmd`/`.exe`/`.ps1`/`.bat` 之一后等值断言 `npx`（仍排除任何别的可执行名），**测试与文档中不出现本机私有路径**；同仓 `factory/nomi.rs:2698-2708` 有同类旧写法（`uvx`/`/uvx.exe`），本次按同一思路把 acp 这处做成后缀无关。 **③ 真实输出（2026-09-11 实测）**：主门 `cargo test -p nomifun-ai-agent --lib --no-fail-fast` → **exit 0 / 999 passed / 0 failed / 3 ignored / 0 measured**（161s 含编译；用例段 99.69s；修前同命令 = **exit 101 / 992 passed / 7 failed / 3 ignored**）；回归门 `cargo check --tests --workspace` → **exit 0**（71s，日志 `^error` 计数 **0**）；旁证 `cargo test -p nomifun-ai-agent --tests --no-fail-fast` → **exit 0**（186s，14 个 test target 全 `test result: ok`，其中 `factory_provider_integration` 7 passed 覆盖 `factory/nomi.rs` 改动的集成面、`prompt_pipeline_integration` 8 passed）；`rustfmt --edition 2021 --check` 三个改动文件干净。**③ 改动文件**：`crates/backend/nomifun-ai-agent/src/factory/nomi.rs`（1 行语义 + 注释理由）、`src/factory/acp.rs`（测试断言）、`src/manager/nomi/agent.rs`（3 例测试，含 1 例改名）。**③ 边界**：未碰迁移与 `057_*`、未改 `crates/agent/**/src/`、R8 审批门与 R22 凭据门未动、未 commit / push。**③ 未做**：整仓 `cargo test` 全量与 `crates/agent/nomi-agent` 的 lib 门本轮未重跑（③ 的改动不触及该 crate 的 `src/`，其编译面由回归门覆盖）。④ **（2026-09-11，批 7 查明 → 同日收口）`cargo check --tests` 的全仓编译门还有第二处独立的断点**：`crates/backend/nomifun-ai-agent/tests/acp_agent_integration.rs:189` 的 `event_type_name()` 是穷尽 `match`，缺 `AgentStreamEvent::OutputDiscarded(_)` 分支 → **E0004**（`error: could not compile \`nomifun-ai-agent\` (test "acp_agent_integration")`，修前实测 `cargo check --tests -p nomifun-ai-agent` → **exit 101**）。**HEAD 既有**（非某会话引入、非工作区并行改动：`OutputDiscarded` 在 `HEAD:src/protocol/events/mod.rs:41` 已存在，而该测试文件在工作区 `git status` 中未被修改）。**✅ 已修（2026-09-11 同日）**：动手前先做**归属核验**证明无活跃 owner——① 该测试文件 `git status --porcelain` 为空、mtime 停在 2026-08-12（未被任何线改动）；② `OutputDiscarded` 生产端未提交改动只落在 `manager/nomi/agent.rs`（mtime 03:34，实测时已静止 4 小时）与 `distill.rs`（属批 5 的 R30），`capability/backend_output_sink.rs` **完全未改**（mtime 2026-08-25）；③ `session_turn_leases` **0 行**、`sessions` 中除本会话外无 `C:\workspace\allo` 活会话、无 `cargo.exe` 在跑。随后**只在该测试文件补一行** `AgentStreamEvent::OutputDiscarded(_) => "OutputDiscarded",`（未改 `src/`、未改迁移、未碰 `057_*`、未动 R8 / R22）。**真实输出**：`cargo check --tests -p nomifun-ai-agent` 修前 **exit 101 / 1× E0004** → 修后 **exit 0**（`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 7.92s`）；宽门 `cargo check -p nomifun-ai-agent -p nomifun-db -p nomifun-app-server -p nomifun-app --tests` → **exit 0**（2m46s，仅既有 warning）；**整仓** `cargo check --tests --workspace` → **exit 101**（**仍未绿**：剩 2 处**既有**断点全在 `crates/agent/nomi-agent/tests/`，不在 Agent Store 线、本次未修，已登记为 D-TEST-1 ⑤）；`cargo test -p nomifun-ai-agent --test acp_agent_integration` → **1 passed / 0 failed / 11 ignored**（11 例按文件头注明 `requires JSON-RPC mock agent` 跳过，与本次无关）；`rustfmt --edition 2021 --check <该文件>` → 干净（**未**跑 `cargo fmt -p`）。**边界**：本次只补显示名，不改 `event_type_name` 的用途、不改那条 variant 的任何语义；③ 的 7 例运行时失败与本次无关，**已于同日收口（见本行 ③）**。⑤ **（2026-09-11 同日，随整仓 `--workspace` 验证新查明；未修，不在 Agent Store 线）**：本项修好后 `cargo check --tests --workspace` **仍 exit 101**——剩下 2 处编译断点全在 `crates/agent/nomi-agent/tests/`（同一批验证命令 `cargo check --tests --workspace` 输出里只有这 2 个 `error`）：`bootstrap_test.rs:393` 的 `assert_eq!(bootstrap.config().max_tokens, 1024)` → **E0609 `no field max_tokens on type &nomi_config::config::Config`**（编译器建议 `max_turns`；该字段已按 capability-driven 口径改造）；`autocompact_test.rs:79` 的 `*self.last_max_tokens.lock().unwrap() = Some(request.max_tokens)` → **E0308 `expected u32, found Option<u32>`**（请求侧已是 `Option<u32>`，外层又多包了一层 `Some`）。**判定 HEAD 既有、与本轮无关**：`crates/agent/` 整树在工作区 `git status` 中**无任何改动**（既非 Agent Store 线在改，也不是本次引入）；两文件 mtime 均停在 2026-09-02；`HEAD` 版本即含这两处用法（`git show HEAD:<file>` 逐行可见），最后触及它们的提交是 `aac092873`（2026-08-21「refactor(agent): make output ceilings capability-driven」）。**⑤ ✅ 已修（2026-09-11，批 7 续 2）**：改动只落在两个测试文件（`crates/agent/nomi-agent/tests/bootstrap_test.rs`、`tests/autocompact_test.rs`，**未碰任何 `src/`**）。① `bootstrap_test.rs:393` → `assert_eq!(bootstrap.config().output_max_tokens, Some(1024));`（`Config::max_tokens: u32` 已在 `aac092873` 改为 `output_max_tokens: Option<u32>`；`max_turns` 是轮次上限、**不是** token 上限，故不采纳编译器的 `max_turns` 建议）。② `autocompact_test.rs:79` 的 mock 记录点改为 `= request.max_tokens`（去掉多余 `Some`；字段语义＝「该请求实际带的上限，`None` ＝ 未带、交给 provider 默认」，唯一读点仍按 `Some(值)` 等值断言，强度不变）。③ **顺带发现同文件第三条陈旧期望**（`:704` `summary_output_cap_follows_context_window`）：断言 `Some(4000)` 出自旧公式 `compact_max_output_tokens = (window/32).max(512).min(4096)`（该期望随 `27742b15b` 2026-08-19 引入），而 `aac092873` 把公式换成 `window_output_unit = (window/8).min(20_000)` → 128k 实际为 `Some(16_000)`；因该文件自 `aac092873` 起就编不过，这条陈旧断言从未被运行过。已改为 `Some(16_000)` 并在注释里写明公式归属与换算（公式本身由 `nomi-config` 的 `window_output_unit_scales_with_context_window` 单测守着）。**真实输出**：修前 `cargo check --tests -p nomi-agent` → **exit 101**（2 处真错：E0609 @ `bootstrap_test.rs:393`、E0308 @ `autocompact_test.rs:79`，另 2 行是 cargo 的 `could not compile` 汇总）；修后 → **exit 0**（`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 1.65s`）；**主门** `cargo check --tests --workspace` → **exit 0**（149s ＝ `Finished ... in 2m 25s`，日志 `^error` 计数 **0**）；`cargo test -p nomi-agent --test bootstrap_test --test autocompact_test --no-fail-fast` → **15 passed / 0 failed**（bootstrap_test）＋ **32 passed / 0 failed**（autocompact_test）；`rustfmt --edition 2021 --check <两文件>` → 干净；旁证 `cargo test -p nomi-agent --lib --no-fail-fast` → **747 passed / 0 failed**（36s，exit 0）——该 crate 的 lib 门与这两个集成测试门同时绿。**边界**：未改 `src/`、未碰迁移与 `057_*`、未动 R8 / R22、未 commit / push。**口径订正**：批 7 前文「全仓只剩 ④ 一处断点」过强——彼时实测命令是 scoped 的（`-p nomifun-mcp -p nomifun-ai-agent`），本行用 `--workspace` 才暴露这 2 处；`16` R34 行 ⑤ 与 `21` 批 7 行已同步订正。 |

#### 卡点决策（2026-09-11，本轮按「卡点问题你自己决定」拍板，逐条记录理由）

> 口径：能做完的**做完**（R10 / R31 / R20 部分，见上表）；做不完的**不留悬空**——每条给出决定、理由与解锁条件。凡涉及安全面的（R22）一律不为了「有进度」而放宽门。

| # | 项 | 决定 | 理由与解锁条件 |
| --- | --- | --- | --- |
| R22 | `17` P1 MCP 连接器 `env` 明文写入快照 | **不实施；保持 🔴 阻塞，等待「安全存储注入路径」先落地** | 这是**安全**项：快照里落明文 env 的修法是「值不入快照、只留键名 + 注入引用」。**订正（2026-09-11 实测）**：原写「前提是宿主有凭据存储」过重——① 加密原语**已有**（`nomifun-common/src/crypto.rs` 的 `encrypt_string`/`decrypt_string` + workspace `aes-gcm`）；② 值侧存放位置**已拍板**（`21` **D5 = C：config.toml + env，不落库**）；③ 缺的是**引用语法 + MCP spawn 解析注入点 + 导入侧改写 + 填写入口**（实测 `nomifun-mcp` 无 env 展开 / `secret_ref`）。只改一半（值删了但无处填）＝导入后连接器**静默不可用**，故两步须同批，见 §5.3 D 档。 |
| R16 | W11 设置 Dialog 7 分区 | **provider 分区已落地（2026-09-11，批 4）；其余六分区不渲染（nav 八→二）** | 逐条判定（每条都是「没有可自证的真实面」，不是「暂时缺数据」）：**agent** —— **订正（2026-09-11 实测）：宿主已消费**。`apps/agent-store/src/main.rs:318` 启动时调用 `nomifun_ai_agent::manager::nomi::distill::set_distill_host_override(Some(enabled))`，读的就是 `[memory].distill_enabled`（D-STREAM-2 的接线已落地）→ 因此「写它就是假开关」**不再成立**；缺的只有两处：`AppServerConfigView` 未投影 `[memory]`、`config/set` 白名单只有 `default_model`。**处置：A 档执行**（补 1 个 additive 字段 + 1 个白名单键，见 §5.3 A 档）。可镜像的 models / efforts 事实仍由 composer 的 ModelPicker **单一持有**，设置里不再放第二份口径。**account** —— 本 App Server 没有账户面（唯一的登录态是连接 token；连接器 OAuth 行在市场目录页）。解锁：宿主账户 API 出现。**plugin** —— 真实面（市场源 / 安装态 / `auto_update`）**已在应用商店页（W13）**，设置里再放只读副本不新增能力；`[default_marketplaces]` / `[marketplace] cadence` 虽有真实配置，但做成写面＝再开一组白名单字段。解锁：插件分区要有**自己的**宿主级设置语义（先设计写面）。**advanced / lab / archived** —— 无宿主设置、无 feature flag、无归档 API（协议里没有归档/回收站，`19` R19 行同样按现状收敛）。**执行口径**：一律从 nav 移除，**不做禁用项、不留「即将推出」占位**（§6 原文允许「标即将推出」，但空壳正是本轮要消灭的形态：禁用项与占位同样点了没反应）。 |
| R17 | W12 技能 / 专家 CRUD | **已设计并落地后端写面（2026-09-11，批 4）；UI 面与 `skill/copy` 仍不做** | 原判定「建议单独立项」已被本批执行：归属 / 覆盖 / 卸载语义按源码布局逐条定死（`user_skills_dir/{name}/` 是**唯一可写**位置，与既有 `delete_skill` 的删除目标逐字一致，也是 `resolve_skill_source_path` 的首选位置；内置 / 市场产物 / 共享 / 伙伴 / 草稿一律只读），`create` 同名不静默覆盖（`conflict` 并点明 origin），卸载仍归 `install/uninstall`（`delete_skill` 物理上也到不了 `agent-store/<snapshot>/<slug>`）。**仍不做的三项及解锁条件**：① WebUI 技能管理面——「编辑」需要**全量正文回读**，而 `skill/get` 的 1200 字截断是刻意的公共读面口径，放开需先批准一个 WS-only 的 `skill/source`（D11 只批了 `create\|update\|delete`，故本轮不擅自加方法）；② `skill/copy`——`skill_service::copy_skill` 只支持**同名**跨 scope 复制，不满足「复制为新名字 + 重写 frontmatter name」，需先加一个目录级复制原语（连带 extension 侧测试）；③ 编辑面 UI 与 ①② 同源，做不了的就不放半截控件。 |
| R23 / R24 | `17` P3 依赖 SemVer 范围解析与「不可满足即阻断」；P4 `strict` | **✅ 已批准待排期（2026-09-11 C 档复核后改判；原写「维持 v1.1 队列，不动」）** | **原判定把排期误写成决策阻塞**：`21` 速览 D6 = A「按规范实现阻断 + 配置逃生口，作为 v1.1 的破坏性变更写进 changelog」——**已批准**，故「解锁条件＝v1.1 解冻」不成立（D6 已满足）。**归属与工作量二次订正**：① extension 层的**解析 + 拓扑排序已接线**（`registry_helpers.rs:46-61` ← `registry.rs:111` / `:166`），缺的只有**阻断**（`registry.rs:136-138` 只 `warn!`，`valid` 未当门）；②「全仓无 semver 解析」指**导入层**（`nomifun-importer/src/import.rs:664-681`），是**另一个消费面**；③ `strict` 属**导入期**语义，挂 importer 层、与 `PluginManifest` 同级，不是 extension 层字段。**处置**：按逃生口**默认关**形式落地（`[import] strict_dependencies` 默认 `false`，完全保留现状行为），语义 + 反向用例先落地；等「第一个外部发布者挑战」这一触发出现时，只翻默认值 + 发 changelog 公告——避免「已批准却没做」永久悬空。 |
| R30 | D-STREAM-2 ① `Finish` 不再等待记忆蒸馏 | **批 5 已落地（2026-09-11）**：按 `21` D9=A 改为后台 spawn；**方向未变**——仍然是「distill 归轮次生命周期、取消语义不许松」 | 见上表 R30 行 + 其后的「R30 落地记录」小节 |
| R7 / R32 | C5 域名与 HTTPS；D-SDK-1 ④ 缩短镜像代价 | **⏸ 延后（2026-09-11，用户决定：站点部署卡点相关问题延后）——不再挂计划、不再等拍板** | 原卡点是**外部资源**（域名与 DNS 访问权）；用户已决定整条「站点部署」轴延后，故 R7 / R32 与 R6② 一并移出待推进队列（不是「做不了」，是「现在不做」）。**订正**：R32 的**本地子项**（`market_source.rs:166` 的 `BATCH = 32`、并发与首载超时口径）**不受此延后阻塞**，如需要可作独立小改动推进；「增量跳过」已由 R26 的条件请求落地。重开条件＝用户提出站点部署 / 域名 / 分发带宽需求。 |
| R15 | W10 附件 / 图片输入 | **✅ 改判（2026-09-11 C 档复核）：解锁定性由「等协议词汇与概念对齐」收窄为「单独拍载体选型」** | Q3 = 等协议词汇与概念对齐 的原判定把**两件事混成一件**。**订正（2026-09-11 实测）**：原写「`content` 图片载体是破坏性面变更」**不准确**——桌面 API 已有 `files: Vec<String>`（`nomifun-api-types/src/conversation.rs:109-133`），协议的 `attachments` 可 **additive** 落地（app-server 目前把 `files: Vec::new()` 写死，`lib.rs:3677`，即「引擎侧已通、协议侧空白」）。**真实卡点拆为两件**：① **载体选型**（路径引用 vs 内联 base64）——**不依赖 词汇对齐**，是独立决策，拍掉即可做；② 词汇对齐 的 `thread/turn/item` 重命名——**只影响字段命名、不影响载体语义**，即「现在做、词汇对齐 时改一次名」与「等 词汇对齐」是可比较的选项。**倾向路径引用**：复用会话 workspace 既有准入与生命周期（`workspace_resolver`），避免 base64 新开体积 / 幂等两个面。详见 `22-webui-productionization.zh.md` §5。 |
| R33 | 方向四 + WP-5 协议词汇与概念对齐 | **✅ 立项页已建（2026-09-11）：`22-webui-productionization.zh.md`** | 11 项（认证/Origin/CSP/CSRF、归档、批量、Request ID、健康检查、配额限流、只读标识、工作区重命名、无障碍 E2E、协议词汇与概念对齐）体量确实超过「继续完成任务表」，且 词汇对齐 属重构型工作（§1 已定：SDK/UI 稳定后再动）——这是「不放进现有 R 表」的理由，**不是「不做」的理由**。立项页已按**安全类 / 可观测类 / 功能类 / 协议词汇与概念对齐** 四组拆分，逐条给「可验收条目 + 边界 + 依赖」，并写死**不做假保护**红线（安全类未实现即不呈现、不做禁用占位、`capabilities.*` 每个 `true` 须指到实现点）。**余项**：该页 §6 的 V1–V4（优先级信号、4 条「未验证」取证、A4 与 `06` 是否合并设计、A5 作用域与部署形态）。 |
| R6 | 站内搜索 / 市场数据刷新纳入发布流程（changelog 页本轮已完成；**② 已于 2026-09-11 按用户决定延后**） | **① 站内搜索：本轮不做——但理由改判（2026-09-11 C 档复核）**；② 市场数据刷新纳入发布流程：⏸ 延后（2026-09-11，用户决定：站点部署卡点相关问题延后） | **站内搜索理由订正**：原写「属站点新功能——要引入索引与构建期数据，会把『索引与文档内容一致性』这一道新的守卫面拉进来」**不成立**。实测 `site/app/lib/docs.ts:10-14` 已用 `import.meta.glob("../../content/docs/**/*.md", { query: "?raw", eager: true })` 把 **9 页 × 2 语言全文内联进 bundle**，即客户端**本来就持有全文**——因此搜索可以是**纯客户端对已内联内容做索引 / 子串扫描**，零构建期产物、零新增守卫面、零协议改动。结论：这**不是技术阻塞，是产品取舍**（建议范围仅 docs；市场侧已有自己的客户端过滤 `site/app/pages/Market.tsx:66-71`）。若仍决定不做，理由应写成「价值不足」，不是「有守卫面风险」。**市场数据刷新纳入发布流程**：⏸ **已按用户决定延后（2026-09-11）**——原卡点是「部署访问权 + D2 未定」，现整条「站点部署」轴延后（与 R7 / R32 同批），不再作为待推进项；本机无法自证端到端效果的前提不变，重开条件＝用户提出站点部署 / 发布流程需求。 |
| D-W9-1 | W9 按 turn 费用 | **取①（2026-09-11，批 6，按用户指令执行）：后端在会话流补逐轮 `usage`（additive）——已落地，见 R14 行 ⑥** | 原②「不做费用、只显示费率与窗口」的前提是「补 usage ＝ 新增一条 token 上报链路，超出 D12=A『用 models.dev 定价』的原意」；源码级侦察推翻了该前提——**运行时早已上报、中继早已转发，只是 App Server 投影丢掉了这份数据**，所以补的是**一层投影**、不是新链路，D12=A 的取价口径不变。金额仍严守「费率与 token 都在才算、任一缺席整段不渲染」。 |

> 上述决定均为**记录**而非「已完成」；`16` 的状态列仍如实标注 🟡 / ⏸ / 🔴，没有任何一条被我写成完成。（**2026-09-11 补充（站点部署轴延后）**：按用户决定「站点部署卡点相关问题延后」，R7 / R32 / R6② 三项从「等拍板 / 挂起」改为**延后、不再挂计划**；Q2 与之保持延后。分组视图的状态列已同步标注，重开条件逐行写明。）（**2026-09-11 批 5 补充**：其中 R30 一条已在批 5 兑现，见下。）（**2026-09-11 批 4 补充**：R6 行的「changelog 页」已完成——见上表 R6 行与「R6 剩余项落地记录」；同行的站内搜索与发布流程两项**仍不做**，理由与解锁条件不变。**2026-09-11 C 档复核订正**：其中「站内搜索」的**理由已改判**——不存在「构建期索引守卫面」（docs 全文已内联进 bundle），改为**产品取舍**；「市场数据刷新纳入发布流程」的理由（部署访问权）不变，仍随站点部署轴延后。）（**2026-09-11 批 6 补充**：D-W9-1 的 ① 已落地——逐轮 `usage` 走 App Server 投影 additive 上 wire，见 R14 行 ⑥ 与上表 D-W9-1 行。）

> **R30 落地记录（2026-09-11，批 5）**：上表 R30 行的「本轮不做」已被批 5 兑现——`Finish` 不再等待记忆蒸馏 child。改动面：`crates/backend/nomifun-ai-agent/src/manager/nomi/distill.rs`（`run_distill_exact_turn` → `spawn_distill_exact_turn` + `spawn_exact_turn_child`；取消域靠 **clone** `turn_cancel`，spawn 前先判 `is_cancelled()`）与 `manager/nomi/agent.rs` 收尾段（删掉 `distill_completed` 的 `await` 与「Finish is forbidden until this child closes」）。**顺序不变**：transcript 快照仍在释放 engine 锁之前取，`emit_observation_turn_end` → `post_turn_review` spawn → `term_guard.terminalize` 的相对顺序未动，只是不再等蒸馏。

> **反向核实（无同步消费者，逐条读码确认）**：① `distill_completed` 的唯一消费者是取消分支——已随改动消失，取消判定改由 `turn_cancel.is_cancelled()` 单独承担（语义等价：旧写法里 child 未完成**只可能**因为取消）；② usage / token 记账在蒸馏段**之前**由 `agent_result.usage` 生成并随 `TurnCompleted` / 观测事件发出，蒸馏那次 provider 调用**从不**计入回合用量；③ DB / goal 持久化（`spawn_goal_persist`）与蒸馏无依赖；④ teardown（`NomiTeardownFailures::finish`、`finish_nomi_teardown`）只等 kill / MCP / process-tree / Browser owner lease / SSH，**不要求** child join；⑤ 记忆索引只在 engine bootstrap 读一次（`nomi-agent/src/bootstrap.rs` 的 `auto_memory_dir`），后台晚到的写入不会让当前会话的提示词漂移。**⑥ 轮次准入只换新 token、不 cancel 旧 token**（`agent.rs` 的 `*self.turn_cancel = token.clone()`）——所以慢蒸馏可以跨轮继续跑完，这是 D9=A 的预期语义（取消才丢弃，换轮不丢弃），也是下面并发面的直接来源。

> **残余风险（未修，登记不入本轮）**：`MEMORY.md` 的 `append_index_entry` 是**读全文 → 写全文**（`nomi-memory/src/index.rs`，非追加写、无锁），而 `distill_dir` 按 **workspace** 解析（同 workspace 的所有会话，加上 `LightweightTurnReviewer` 的 `spawn_blocking` 后置钩子，都写同一目录）——并发写丢掉一行索引是**改动前就存在**的类别，R30 只是把「同一会话相邻两轮」也加进可能的并发对。定级：低（丢一行索引，memory 文件本体仍在），**不发明新机制**；若要做，最小改法是给该目录加进程内 `Mutex`（或把 append 改成真正的追加写），属独立一项。

> **D-SDK-1 根因（已定位，2026-09-10）**：`store/list` → `ensure_default_marketplaces`（`nomifun-app-server/src/lib.rs:1634` / `1500-1549`）**同步**对三个内置 URL 市场执行 `add` → `fetch_remote` → `mirror_http_tree`（BATCH=32，`market_source.rs:132-200`），从明文公网镜像 `http://111.170.173.22:10072` 整树下全量 HTTP 下载（`agent_store.rs:137-155`）。就绪行在该工作**之前**发出（`apps/agent-store/src/main.rs:373-384`），故代价全落在首个 `store/list`；`initialize` / `models/list` 不触市场，仅毫秒。`add` 的幂等短路（`app_server_marketplace.rs:420-441`）是 DB 查询，只在**同一 data-dir** 生效；单源上游超时 **600s**（`lib.rs:1543`），故「调高默认值」的正确取值无上界——只能靠 ①/④ 解决，③ 仅止血。

> **R17 落地记录（2026-09-11，批 4）**：上表 R17 行的「本轮不做；建议单独立项」**已被本批执行**——协议增量 `skill/create|update|delete`（D11=A 批准的那三个，未加第 4 个）与「与市场安装产物区分」的写面语义一起落地。**改动的 8 个 Rust / 3 个前端与站点 / 4 份文档面**：`crates/backend/nomifun-extension/src/skill_service.rs`（新增 `SkillScope::User`、`SkillOrigin{User,Shared,Companion,Draft,Marketplace,Builtin,Unmanaged}`、`skill_origin_of`、`is_writable_skill`、`skill_manifest_path`）、`crates/backend/nomifun-app-server/src/skill_admin.rs`（新文件：名字门 + `SkillWriteProvider` + `SkillAdmin`）、`crates/backend/nomifun-app-server/src/lib.rs`（`state.skill_writes`、三个 dispatch 臂、三个 `deny_unknown_fields` 参数、写后回读）、`crates/backend/nomifun-api-types/src/app_server.rs`（`AppServerSkillSummary.origin/writable` + `AppServerSkillDeleteResult`）、`crates/backend/nomifun-app/src/app_server_catalog.rs`（读面填 `origin`/`writable` + 修 manifest 读法）、`crates/backend/nomifun-app/src/router/routes.rs`（接线）、`web/packages/protocol/src/protocol.ts` + **两道防漂移守卫** + `site/content/docs/{zh-CN,en-US}/typescript-sdk.md`（60→**63** 个方法、15→**18** 个无 HTTP 绑定）、`05` §4.3/§4.11、`21` D11 行与批 4 行、交接文档 §2.8/§4。
>
> **可写性判据（读面 `writable` 与写面归属门**同源**，由 `is_writable_skill` 一处定义）**：只有 `{user_skills_dir}/{name}/`（depth-1 且目录 basename ＝ 公开 id）可写；`agent-store/<snapshot_id>/<slug>/`（市场安装产物，`nomifun-importer/src/install.rs:72` 写入）、`builtin/`（含 `auto-inject/`）、`shared/`、`companion/`、`_drafts/` 一律只读并各带固有理由。**为什么 depth-1 是规范位置**（不凭概念推断）：① `skill_service::delete_skill`（模块唯一的删除原语）删的就是 `{user_skills_dir}/{name}`；② `resolve_skill_source_path`（`materialize_skills_for_agent` → `link_workspace_skills` 的解析顺序）把 `{user_skills_dir}/{name}` 排在**第一**——用户技能按设计覆盖同名内置；③ `shared/`/`companion/`/`_drafts/` **不在**该解析顺序里（属伙伴链路的所有权）。`SkillScope::User` 同时闭合了「`create_skill` 写 `shared/`、`delete_skill` 删 depth-1」这对**既有不一致**。
>
> **规则**：`create` 遇同名（用户/内置/市场产物/共享/伙伴）→ `conflict` 并在 message 里点明 origin 与出路，**不静默覆盖**；磁盘上已有同名目录但未被目录树收录（`SKILL.md` 缺失/非法）→ 同样 `conflict`，不合并写入；`update` 的正文 frontmatter `name` 必须等于 `skill_id`（id 即名字，改名走「新建 + 删除」）；目录或 `SKILL.md` 是**符号链接** → `policy_denied`（`create_dir_all` / `write` 都会跟随链接）；**卸载市场安装产物仍归 `install/uninstall`**（`delete_skill` 物理上到不了 `agent-store/<snapshot>/**`，且 `require_writable` 在调用前就拦）。凭据门（R22）不动：三个参数都是 `deny_unknown_fields`，`api_key`/`env`/`token` 出现即 `invalid_request`。
>
> **真实输出（2026-09-11 实跑）**：`cargo test -p nomifun-app-server --lib` → **88 passed / 0 failed / 0 ignored（exit 0）**（含本项新增 9 例：写面未接线 `unsupported_operation`、只接线写面但缺读面则**写前**就被拒、owner 门与无路径参数、非法名 / 路径穿越 / 未知字段 / 凭据字段后**磁盘零变化**、同名三来源 `conflict`、`update`·`delete` 只动可写来源、`delete` 报告被揭出的内置来源）；`cargo test -p nomifun-extension --lib` → **440 passed / 0 failed / 5 ignored（exit 0）**（含新增 3 例：归属分类与可写性、manifest 解析、`SkillScope::User` 三原语同一目录）；`cargo test -p nomifun-app --lib` → **301 passed / 1 failed**——新增的读面用例 `app_server_catalog::model_catalog_tests::skill_read_face_reports_origin_and_writability_per_layout` **ok**，唯一失败是 `commands::stdio_common::tests::at_most_once_retries_undelivered_connection_failures`（与本项无关的既有项：该文件与 HEAD **逐字节相同**、`git status`/`git diff` 皆空，不在 R17 任何改动路径上，且做了系统代理对照实验——`ProxyEnable 1→0` 前后结果不变、已还原——故判定为机器级/既有问题，非本项引入）；`cargo check --tests --workspace` → **exit 0**；`cargo test -p nomifun-app --test agent_execution_decision_e2e`（R8 审批门回归）→ **8 passed / 0 failed（exit 0）**；`cd web && bun run test` → **320 passed / 1 skipped（43 files，exit 0）**、`bun run typecheck` → **exit 0**、`bun run build` → **exit 0**（两道防漂移护栏的「45 / 63」与站点 §7.3 双语在这一轮内一致）。
>
> **本轮未做（登记 + 解锁条件，不用半条链路凑）**：① **WebUI 技能管理面**——「编辑」需要**全量正文回读**，而 `skill/get` 的 1200 字截断是刻意的公共读面口径（`AppServerSkillDetail` 注释：「raw `SKILL.md` body … never returned」）；放开需先批准一个 WS-only 的 `skill/source`（或让宿主自己的文件面暴露技能根——与「内部路径不过 seam」冲突，不取）。**解锁条件：D11 之外再加一次协议增量拍板。** ② **`skill/copy`（复制为新名字）**——`skill_service::copy_skill` 只做**同名**跨 scope 复制，不满足「复制为新名字 + 重写 frontmatter `name`」；需先加一个目录级复制原语（连带 extension 侧测试），属独立改动。③ 前端 `web/src/lib/client.ts` 的写面 helper **故意不加**：没有 UI 调用方的 helper 就是投机代码，等 ① 一起做。**已就绪、等 ①** 的是：读面已能给 UI「按来源区分 + 是否可写」的全部事实（`origin`/`writable`），后端三个方法与归属语义均已可用且有测试。

#### 阻塞决策（拍板即解锁任务）

| 决策 | 内容 | 解锁 |
| --- | --- | --- |
| Q7 | ✅ **已定（2026-09-10）：① 改实现以区分官方/第三方** | T14 已解锁（登记见 `18` §11 D1） |
| Q6 | ✅ **已定（2026-09-10）：① 写 `~/.agent-store/config.toml`** | ✅ **已落地（2026-09-11，`16` R16）**：`config/get` / `config/set`（WS-only、不进 SDK 包）白名单写 `default_model`，最小改动 + 写后重读；设置里的 provider 分区是唯一可写面（旧的两个从不回写的本地输入框已删） |
| Q2 | ⏸ **延后（2026-09-11，用户决定：站点部署卡点相关问题整体延后）** | C5（R7）、D-SDK-1 的镜像带宽大头（R32）、R6② 一并随之延后、不再挂计划；**不阻塞** ③（已做）与 ①。重开条件＝用户提出站点部署 / 域名 / 分发可达性需求 |
| Q3 | ✅ **已定（2026-09-10）：等协议词汇与概念对齐** | W10 留在「待反馈」批，不进本轮三闭环 |

> **本轮已无待拍板项**：Q1 / Q3 / Q4 / Q5 / Q6 / Q7 已定；**Q2 延后（2026-09-11，用户决定：站点部署卡点相关问题整体延后）**——带决策部分按上表执行。

---

#### 卡点处置四档（2026-09-11，源码取证后定，用户已批准执行顺序；**C 档已于同日二次复核改判**）

> 取证口径：每条判定都给出**文件:行**证据，区分「已验证事实 / 推断 / 未验证」。执行顺序＝**A → B → D → C（只开页）**。
>
> **2026-09-11 C 档二次复核**：C 档原以「维持不动」统一处置，复核后按**性质**改判（详见下方「C 档判定（重写）」）——原判定把排期误写成决策阻塞（R23/R24）、把可做部分一起挂死（R20/R15）、把不存在的守卫面风险当作阻塞理由（R6①）、且唯一承诺动作未兑现（R33 立项页当时并不存在）。改判只改**定性与解锁条件**，不改变「未做」这一事实。

| 档 | 范围 | 处置 | 为什么 |
| --- | --- | --- | --- |
| **A** | ① R17 三动词（列表 / 新建 / 删除）WebUI；② R16 `agent` 分区开关；③ 三处卡点措辞订正 | **做** | ① 读面已给 `origin`/`writable`、三个写方法已落地 → **零协议增量**，不做则 R17 只能算后端完成；② 宿主**已消费** `[memory].distill_enabled`（`apps/agent-store/src/main.rs:318`）→ 补「视图投影 + 白名单」即真回读闭环，不是假开关；③ 纯文档 |
| **B** | ① R17 编辑＝`skill/update` 改**服务端字段级合并**；② R17 复制原语（`skill/copy`）；③ R14 逐轮 usage 持久化 | **做**（各含一次明确选择） | ① 不扩协议方法数（D11 只批三个），正文不进 wire，符合「读面只给 bounded summary」；UI 口径＝「正文替换」而非「编辑正文」。② 复制是从只读来源派生「我的技能」的唯一路径，无协议风险（复用已批 WS-only 框架）。③ 加列优于单开表：该表本就是「按会话唯一行」，需求是「重载后看到最近一轮」而非逐轮审计（边界写进迁移注释） |
| **C** | R20 归属/接受回退；R6① 站内搜索；R15 附件；R23/R24；R33 | **按性质分别处置**（2026-09-11 C 档复核后改判；原写「维持不动，仅 R33 开立项页」）：R23/R24 **已批准待排期**；R20 **拆 a/b**；R15 **载体选型独立拍板**；R33 **立项页已建**；R6① **改判为产品取舍** | 见下「C 档判定（重写）」 |
| **D** | R22 凭据（两步同批） | **做**（安全线，唯一次序无关但独立批次） | 缺件已收窄为「引用语法 + 解析注入点 + 导入侧改写 + 填写入口」；加密原语与存储位置（`21` D5=C）都已就位 → 不再是「等其他系统」 |

**C 档判定（重写，2026-09-11 二次复核）**

> 改判理由：原判定把 5 条统一写成「维持不动」，但复核发现它们**混了三种不同性质**——「已获批但未排期」「真技术依赖」「缺触发度量」。统一处置的代价是：**已批准的事项看起来像被否决**（R23/R24 尤其明显，`21` D6=A 早已批），以及**把可做部分一起挂死**（R20/R15）。下表按性质重排。

| # | 性质 | 改判后的判定 | 依据（二次实测） |
| --- | --- | --- | --- |
| R23 / R24 | **已获批、未排期** | **移出「等外部条件」，标「已批准待排期」**。落地形式＝逃生口**默认关**（`[import] strict_dependencies` 默认 `false`），触发出现时只翻默认值 + 发 changelog | `21` D6=A **已批准**按规范实现阻断 + 逃生口 → 原「解锁条件＝v1.1 解冻」不成立。**两处口径订正**：① 「调用点全在自身单测」**不成立**——`registry_helpers.rs:46-61`（`:56` 调 `validate_dependencies`、`:57` 用 `load_order` **真排序**）← `registry.rs:111` / `:166`，`dependency.rs:112-129` 有用例；**真缺的只有阻断**（`registry.rs:136-138` 只 `warn!`，`valid` 未当门）。② 「全仓无 semver 解析」指**导入层**（`nomifun-importer/src/import.rs:664-681` 只登记不决策），是**另一消费面**；`strict` 属导入期语义，挂 importer 层（`PluginManifest` 同级），不是 extension 层字段 |
| R20 | **技术依赖（粒度太粗）** | **拆为 R20a / R20b**：R20a「按 Run 归属」＝**可做**（零协议增量）；R20b「接受 / 回退」＝等 Artifact Phase | 归属所需数据**已存在**：`run/plan` 已投影 attempt 的 `output_files`（`nomifun-agent-execution/src/runtime_adapter.rs:141` + `nomifun-app-server/src/lib.rs:3873-3884`）→ 「产物 → 所属 Run/Step」无需新协议能力。必须等的只有写面：`capabilities.artifacts` 硬编码 `false`（`nomifun-app-server/src/lib.rs:401`）。全后端 `artifact/` / `artifact.created` / `ArtifactCreated` → **0 命中**（印证写面确实缺失） |
| R15 | **技术依赖（与 词汇对齐 混同）** | **载体选型独立拍板**（不依赖 词汇对齐），倾向路径引用 | 桌面 API 已有 `files: Vec<String>`（`nomifun-api-types/src/conversation.rs:109-133`），app-server 写死 `files: Vec::new()`（`nomifun-app-server/src/lib.rs:3677`）→「引擎侧已通、协议侧空白」，`attachments` 可 additive。词汇对齐 的 `thread/turn/item` 重命名**只影响字段命名**，不影响载体语义 → 「现在做、词汇对齐 改一次名」与「等」可比较 |
| R33 | **缺触发度量 / 承诺未兑现** | **立项页已建**：`22-webui-productionization.zh.md`（四组拆分 + 逐条验收条目 + 假保护红线） | 11 项实测：`csrf` / `Origin` / `CSP` / `rate_limit` / `quota` / 归档在 `nomifun-app-server/src` 内**零命中**（`app_server_routes` 是裸 `Router::new()`，`nomifun-app-server/src/lib.rs:880-884`）；Request ID **半有**（只回显 `req.id`，`:532-538` / `:4468-4500`）；健康检查 **半有**（`ping` 在认证组内，`:883`）。含安全与配额语义，做错即**假保护** → 必须先拆条目。**原判定「只开立项页」当时并未兑现**（文件不存在），本轮补上 |
| R6① | **缺触发度量（阻塞理由不成立）** | **不做，但理由改判为「产品取舍」** | 原理由「会引入构建期索引守卫面」**不成立**：`site/app/lib/docs.ts:10-14` 已 `import.meta.glob(..., { eager: true, query: "?raw" })` 把 docs **全文内联进 bundle** → 搜索可纯客户端实现，**零构建期产物、零新守卫面**。建议范围仅 docs（市场侧已有客户端过滤，`site/app/pages/Market.tsx:66-71`）；若不做，理由是「9 页 × 2 语言价值不足」，不是「有风险」 |



- **不把宿主管理面收编进协议**：进程生命周期、数据目录、provider / MCP 配置、`fs/browse`、资产直链保持为宿主管理面。判断规则：*第三方 SDK 消费者是否应该能调用它？* 不能 → 不进包。
- **不为了「纯粹」牺牲权限边界**：`fs/browse` 若进入公共协议，等于给远程客户端文件系统枚举能力。
- **不在 SDK 稳定前动协议重命名**（WP-5 延后）。
- **不做假开关**：设置分区、市场开关等一律要有真实数据源与回写路径，未实现的继续标「即将推出」而非留占位控件。
- **不让文档滞后于公共面**：A 与 C 配对同批（§3.2 C0），F25 的债机制化。
- **不承诺排期**：本计划只给批次与验收口径，实际节奏按实测校准。

#### A / B 档与 C 档立项页 · 本轮落地记录（2026-09-11）

> 真实读数，命令与计数一律照抄；本记录只覆盖**已实际完成并验证**的部分，未做的一律在下方「未做」逐条登记（不用半条链路凑进度）。

**A 档 ① · R17 三动词 WebUI（列表 / 新建 / 删除）→ 已完成（含 B 档的编辑与复制）**

| 层 | 改动 |
| --- | --- |
| 前端 client | `web/src/lib/client.ts`：`createSkill` / `updateSkill` / `deleteSkill` / `copySkill` 四个 host-only helper（走 transport，与 `config/*` 同口径）+ `SkillCreateInput` / `SkillUpdateInput` / `SkillDeleteResult` 类型 |
| 前端 store | `web/src/store/skillAdmin.ts`（新）：写入**不做乐观回显**，landing spot 一律是服务端回读；`busy` 是技能 id，一次只允许一笔写；`policy_denied` 原文上抛 |
| 前端组件 | `web/src/components/skills/SkillWriteSurface.tsx`（新）：列表行按 `writable` 决定是否给写按钮，只读行给**原因**（按 `origin` 映射），编辑/复制/删除三个对话框 + 新建按钮；`CatalogView.tsx` 技能页接线（卡片改 `div role=button`，避免嵌套 button；来源徽标用 `origin` 而非 `source`） |
| 样式 / 文案 | `web/src/style.css` 增 `.skill-write-actions` / `.skill-write-reason` / `.settings-row-stack` / `.skill-body-input` 等；`i18n/{zh-CN,en-US}.ts` 增 `skills.*`（含 7 种 origin 标签与只读原因） |

**B 档 ① · `skill/update` 改服务端字段级合并 → 已完成**

- 新增 extension 原语 `SkillFieldPatch` / `merge_skill_md` / `patch_skill`：只改点名键，`name` 不可改（写面已无改名路径），正文按「替换」语义；空值清除可选键，`description` 不可清空。
- 协议面：`skill/update {skill_id, description?, when_to_use?, allowed_tools?, paths?, body?}`，`deny_unknown_fields`；**旧的 `markdown` 整文形态与 `name` 一律 `invalid_request`**（不再是「整文替换」）。
- UI 口径：编辑对话框把正文写成「替换正文」并明说读面不回显全文（避免半截所见即所得）。

**B 档 ② · `skill/copy`（目录级复制原语）→ 已完成**

- extension：`copy_skill_directory`（整棵子树复制、frontmatter `name` 重写、**拒绝链接**、拒绝已存在目标）+ `rename_frontmatter_name`；`ExtensionError::SkillExists` → `AppError::Conflict`。
- app-server：`skill/copy {skill_id, new_name}`，源可为**任意来源**（内置 / 市场产物 / 共享 / 伙伴 / 草稿 / 用户），目标恒为 `{user_skills_dir}/{new_name}`；`skill_source_dir()` 同时吃「内置的 SKILL.md 文件路径」与「其他来源的技能目录」两种 `location` 形态。
- 协议计数：方法总数 **63 → 64**、HTTP 无绑定 **18 → 19**（三道防漂移护栏 + 站点双语 §7.3 同步）。

**A 档 ② · R16 `agent` 分区开关 → 已完成**

- 后端 additive：`AppServerConfigView.memory { distill_enabled: bool? }`（`null` = 未配置，与「显式关闭」可区分）+ `config/set` 白名单加 `memory.distill_enabled`（`toml_edit` 最小改动写，注释与兄弟键原样保留）；「什么都没点名」的补丁被拒（`invalid_request`），凭据类字段仍 `invalid_request`。
- 前端：设置新增「智能体」分区（`AgentSettingsSection.tsx`），状态读宿主回读值，明说「宿主启动时读取、重启生效」。

**A 档 ③ · 卡点措辞订正 → 已完成**（R16 假卡点、R22 前提、R15 措辞；R23/R24 由同日二次复核进一步订正，见上文）

**C 档 · R33 立项页 → 已完成**：`22-webui-productionization.zh.md`（四组拆分 + 逐条「可验收 / 边界 / 依赖」+ **不做假保护**红线 + §6 未决 V1–V4）；R33 本体仍未排期。

**验证（真实输出）**

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-app-server --lib --no-fail-fast` | **94 passed, 0 failed, 0 ignored**（exit 0） |
| `cargo test -p nomifun-extension --lib --no-fail-fast` | **444 passed, 0 failed, 5 ignored**（exit 0） |
| `cargo check -p nomifun-app-server -p nomifun-extension` | exit 0 |
| `cargo check --tests --workspace` | exit 0 |
| `cargo test -p nomifun-app --test agent_execution_decision_e2e --no-fail-fast` | **8 passed, 0 failed**（exit 0；R8 审批门未被动过） |
| `cd web && bun run test` | **347 passed, 1 skipped（46 files）**（exit 0） |
| `cd web && bun run typecheck` | exit 0 |
| `cd web && bun run build` | exit 0 |

**本轮未做（如实登记，不做半条链路）**

| 项 | 状态 | 原因与解锁条件 |
| --- | --- | --- |
| **B 档 ③ R14 逐轮 usage 持久化** | ❌ **未做**（设计已定） | 设计：**新迁移**给 `app_server_context_usage` 加两列（`last_turn_input_tokens` / `last_turn_output_tokens`，可空，不改既有迁移与 `057_*`），写入点＝`nomifun-conversation/src/stream_relay.rs:6167` 的 `persist_app_server_context_usage(metrics)`（该处已持有本轮 token），读回＝`get_app_server_context_usage` + App Server 投影 + 前端重载后显示。**未做的原因**：这是一条跨 5 个 crate（db model / db 仓储两实现 / conversation service / stream_relay / app-server 投影）+ 前端的链，只加列不接读回就是「半条链路」；本轮余量不足以完成并验证，故**停在这里**。解锁：单开一轮，按上面 5 个改动点顺序做，验收＝重载后仍显示上一轮 token 与金额。 |
| **D 档 R22 凭据两步** | ❌ **未做**（设计已细化） | 缺件已收窄为「引用语法（`${secret:NAME}` / `env:NAME`）+ MCP spawn 解析注入点 + 导入侧改写（`nomifun-importer/src/import.rs:1100-1112` 目前原样写 `env` 值）+ 填写入口」；加密原语（`nomifun-common/src/crypto.rs`）与存放位置（`21` D5=C）都已就位。**未做的原因**：两步必须同批（只删值不给出处＝导入后连接器静默不可用），本轮余量不足以保证「同批 + 真断言」，故**不做**。解锁：单开一轮安全批次，验收＝快照内无任何 env 值（单测钉住）+ 引用可解析 + 缺值**显式报错**。 |

---

## 6. 阶段基线与交付范围（原 `agent-store-v1-roadmap.md` §1–§6、§9）

> 本节由 `agent-store-v1-roadmap.md` 的阶段部分整体并入（2026-09-11 文档合并）：V1 目标、交付范围（必须交付 / 明确非目标）、Phase 0–6 阶段计划、依赖与风险、测试分层、测试与发布引用、排期说明。**内部小节号已按 `§6.x` 重编**（原 roadmap §1–§6、§9 对应 §6.1–§6.7，仍可与 `agent-store-v1-roadmap.md` 对照）。


> 状态：现行路线（Phase 0；发版前可改，非冻结，见 §7 决策 4）；排期待 Spike 校准
> 日期：2026-08-26
> 前置：`00-architecture-decision.md` 至 `08-flowy-web-integration.md`、`10-public-contracts.md`
> 说明：阶段按两周迭代组织；具体排期须在 Spike 和实测后校准，不构成承诺
> 进展：Phase 0 的 OAuth 注入实测已完成（`06-connector-oauth-security.md` §13）；
> Phase 1 的 Importer 已落地（TC-IMP-001~009 通过，`02-codebuddy-workbuddy-import-spec.md` §14）；
> 安装模式与市场（Phase 2 提前落地）已完成（install/*、market/* 四类源、
> @mention 解析与 run 注入 TC-INS-001~007）；
> 2026-09-04：单 Agent 真实 Run 通过（TC-RT-001 planning→running→completed，mimo-v2.5 临时实例，
> `13-p0-execution-plan.md` §14；附带修复 actor 落库 500）；
> 其余 Runtime 门禁（TC-RT-002/005/006/009/010）与最小 TeamRun 尚未执行，排期仍待校准。
> 2026-09-10：Team 触发方式修订为 Leader 模型调用 `nomi_delegate(strategy=planned)`（§7 决策 3），同步修订 `00`/`01`/`03`/`04`/`10` 与 TC-TEAM-001/002。

### 6.1 V1 目标

V1 目标不是单 Agent PoC，而是完成本地优先 Agent Store 的核心端到端闭环：

```text
CodeBuddy/WorkBuddy Importer
    ↓
PluginSnapshot / Catalog
    ↓
Agent / Team / Skill / Connector
    ↓
allo Runtime Adapter
    ↓
App Server Protocol
    ↓
TypeScript SDK
    ↓
Flowy/Web
    ↓
Run / Event / Artifact / Approval
```

V1 Team 范围：

```text
固定成员
+ AgentExecutionTemplate
+ Leader Planning Context 驱动的 planned DAG
+ 局部并行
+ retry / replan
+ Event / Artifact
```

V1 不要求：

```text
完整 Mailbox
成员自主认领
成员直连消息
长期成员会话
嵌套 Team
```

### 6.2 交付范围

#### 6.2.1 必须交付

| 领域 | V1 交付 |
|---|---|
| Runtime | allo 唯一 Runtime + Runtime Adapter |
| Import | CodeBuddy/WorkBuddy PluginSnapshot |
| Catalog | Agent/Team/Skill/Connector 定义、版本、来源、digest、状态 |
| Agent | 单 Agent Run、取消、事件、结果 |
| Team | 固定成员、Leader Planning Context、planned DAG、局部并行、retry/replan |
| Connector | 至少一个 MCP Connector、工具过滤、Probe |
| OAuth | 标准 PKCE Loopback OAuth、存储、注入、刷新、重试 |
| Protocol | Versioned App Server Protocol、WebSocket 绑定、状态查询（stdio 不纳入，见 §7 决策 2） |
| SDK | TypeScript typed client、重连与状态同步、错误处理 |
| Web | Catalog、Run、Plan/DAG、Timeline、Artifact、Approval、Connector 状态 |
| 安全 | 凭据隔离、Tool Policy、审批、脱敏审计 |

#### 6.2.2 明确非目标

```text
第二个 Runtime
云端执行
多租户与 HA
完整公开 Marketplace 审核后台
签名更新体系
全部 OAuth 变体
任意 Hook/bin/script 执行
完整 LSP Runtime
完整 CodeBuddy Team 语义
入站 MCP Server 的全部能力
```

### 6.3 阶段计划

#### Phase 0：基线与 Spike（两周迭代）

目标：先关闭 Runtime Readiness Gate，而不是并行实现完整 UI。

必须实测并留存：

- AgentDefinition → Preset/ResolvedPresetSnapshot → allo ExecutionParticipant → Runtime Agent/Driver → 单 Agent Run；
- Event Log、持久化状态查询和重启后的未完成状态标记；
- 本地 App Server AuthContext；
- Connector Token 注入、Probe、401 刷新和单次重试。

在 Phase 0 未通过前，Team、SDK、Web 只能实现 schema/mock，不得宣称端到端完成。

目标：验证 allo 现有基础能支撑 V1。

交付：

- 领域模型和四份基线文档定稿；
- Adapter 最小接口；
- App Server 协议 Schema 草案；
- `software-company` 资源扫描；
- 单 Agent 和固定成员 Team 的最小创建 Spike（含 Planning Context 构造）；
- 事件、Artifact、OAuth 注入风险清单。

验收：

- 能从导入资源得到 5 个 Agent 和 1 个 Team；
- 能确认 AgentExecutionTemplate、Planning Context、Planner、Participant Resolver 的实际调用链；
- 对尚未贯通的 Agent 注册和 OAuth transport 注入明确记录证据或失败原因；
- 不以类型存在代替端到端验证。

门禁：

- 如果单 Agent 无法创建可执行快照，先修复 Adapter 边界；
- 如果 Team planned 无法物化 DAG，调整 V1 Team 实现方案并保留 Team 公共模型；
- 如果 OAuth 注入未贯通，Connector 只能标记 partial，不得宣称 connected。

#### Phase 1：Importer 与 Catalog（两周迭代）

> 状态：✅ 主体已实现（`nomifun-importer` + `plugin_snapshots` Catalog + App Server
> `import/*`、`agent/list`、`team/list`）+ 安装模式（`install/*`，Phase 2 提前落地）+
> 市场（`market/*`，directory/github/git/url 四类源：git2 克隆 + HTTP 条件下载 +
> staging 校验 → 原子晋升 → last-good；`market/refresh` 新鲜度短路；级联卸载）；
> 自动更新后台任务、安装作用域与团队分发（阶段 C/D）留待后续

交付：

- CodeBuddy/WorkBuddy Importer；
- PluginSnapshot 不可变缓存；
- Agent/Team/Skill/Connector 标准化；
- CompatibilityReport；
- 依赖、路径、digest、版权状态；
- Catalog 查询和版本选择。

验收：

- `software-company` 导入生成 5 个 AgentDefinition、1 个 AgentTeamDefinition；
- Team 成员关系和 Lead 正确；
- 不支持组件不静默丢弃；
- 路径逃逸和 digest 冲突阻断安装；
- 凭据值不进入 Snapshot。

#### Phase 2：allo Runtime Adapter（两周迭代）

交付：

- 单 Agent Run；
- Preset/ResolvedPresetSnapshot；
- 固定 Team Participant Pool；
- AgentExecutionTemplate；
- Leader Conversation 创建与 `execution_template_id` 绑定；
- Leader 可调用的 `nomi_delegate(strategy=planned)` 入口（绑定持久 `AgentExecutionEngine`）；
- Planning Context 构造与摘要/digest；
- Step/Attempt 状态；
- Planning Context 驱动的 planned DAG、ready 调度、局部并行；
- retry/replan；
- Event/Artifact 持久化。

验收：

- 完成单 Agent Run；
- 完成 software-company 最小 TeamRun，并验证 Planning Context 摘要/digest 与成员 Prompt 隔离；
- Leader 触发的 TeamRun 能从内部 Execution 反查出公共 `run_id`，且成员池/并发上限不可被模型覆盖；
- 独立 Step 可局部并行；
- 依赖 Step 正确等待；
- 失败产生新 Attempt；
- replan 保留 Plan Revision 历史；
- Runtime 重启后保留事件，并将未完成 Run 明确标记为失败或需恢复。

#### Phase 3：App Server Protocol（两周迭代）

交付：

- initialize/capability negotiation；
- Catalog API；
- agent/run、team/run；
- run/get、run/result 与尽力而为事件通知；
- cancel/pause/resume/retry/replan；
- Approval、Artifact、Connector/OAuth API；
- stdio JSONL 和 WebSocket；
- 结构化错误和幂等键。

验收：

- SDK/CLI 可只通过 App Server 完成单 Agent 和 Team Run；
- 状态查询始终一致；通知丢失不影响最终一致；
- 相同幂等键不重复创建 Run；
- 公共协议不泄露 allo 内部 ID 和凭据。

#### Phase 4：TypeScript SDK（两周迭代）

交付：

- `@flowy-agent-store/protocol`；
- `@flowy-agent-store/client`；
- Node stdio Transport；
- Browser/WebSocket Transport；
- Catalog/Run/Event/Artifact/Approval/Connector Client；
- 重连、错误和幂等辅助；
- React hooks。

验收：

- Node 和 Browser 都能完成 initialize；
- SDK 能完成 Catalog、单 Agent Run、Team Run；
- 断线重连后状态一致且不重复展示；
- SDK 不接触真实 OAuth Token。

#### Phase 5：Flowy/Web 纵向闭环（两周迭代）

交付：

- Agent/Team Catalog；
- Run Launch；
- Plan/DAG；
- Timeline（状态 + 通知驱动）；
- Approval；
- Artifact；
- Connector/OAuth 状态；
- Flowy 主进程和 Renderer 边界。

验收：

- 用户可从 UI 导入后启动 Agent/Team Run；
- 可看到计划、Step、Attempt、事件和 Artifact；
- QA 失败导致 retry/replan 后 UI 保留历史；
- OAuth 页面不接触 Token；
- Renderer 不直接调用 allo 或上游 MCP。

#### Phase 6：安全加固与发布准入（两周迭代）

交付：

- Tool Policy 和 Approval 加固；
- MCP/STDIO 进程边界；
- 凭据泄露扫描；
- 脱敏审计；
- 连接器 Probe；
- 版权/来源审核；
- V1 回归测试和发布说明。

验收：

- 标准 OAuth Connector 通过登录、注入、刷新、Probe；
- 高风险操作有 Approval；
- 任意 CLI/脚本不能绕过 allowlist；
- 未确认版权资源不能进入公开分发。

### 6.4 依赖与风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| Preset 解析或 Runtime Agent 注册桥接不完整 | 单 Agent/Team 无法执行 | Phase 0 分别验证 AgentDefinition → Preset 快照和 Runtime Agent/Driver 实际调用；保留 Adapter 边界 |
| planned DAG 与现有执行器语义不一致 | Team 交付延迟 | 先固定最小 Step/Attempt/Event 模型，不提前承诺完整 Team |
| OAuth transport 注入未贯通 | Connector 只能登录不能调用 | 将 login 与 runtime integration 分开验收 |
| 通知丢失导致界面滞后 | 用户看到过期进度 | 以 run/get 轮询兜底；V2 再做 cursor 追平 |
| Hook/bin/script 执行风险 | 任意代码执行 | V1 默认静态导入和 manual-review |
| 来源版权未确认 | 不能公开分发 | 保留 pending-legal-review，阻断市场安装 |
| allo 内部 API 变化 | SDK/Web 返工 | 所有外部调用通过版本化 App Server |
| Runtime 重启中断 Run | 用户误认为执行成功 | 复用 allo 内部安全恢复；无法证明安全时标记 `recovery_required`，App Server 不承诺一定续跑原 Attempt |

### 6.5 测试分层

#### 6.5.1 Unit

覆盖：

```text
Manifest parser
ID mapping
Version/digest
Compatibility status
Policy intersection
DAG validation
Step readiness
Attempt state machine
Event sequence（内部序号）
Error mapping
OAuth metadata/state validation
```

#### 6.5.2 Contract

覆盖：

```text
App Server initialize
Request/Response schema
Error codes
Event envelope
Status query consistency
Idempotency
SDK generated types
```

#### 6.5.3 Integration

覆盖：

```text
Importer → Catalog
Catalog → Runtime Adapter
Adapter → allo
Runtime → App Server
App Server → SDK
Connector → OAuth → Probe
```

#### 6.5.4 E2E

主链路：

```text
导入 software-company
    → 查看 5 个 Agent + 1 个 Team
    → 启动 TeamRun
    → Planning Context 驱动的 planned DAG
    → Step 顺序/局部并行
    → 失败 retry/replan
    → Event/Artifact
    → Web 显示结果
```

### 6.6 测试与发布引用

测试用例总索引是 `agent-store-v1-test-cases.md`（§1/§2 公共口径 + §3 归属总表；`TC-*` 逐条正文归各归属文档），发布门禁唯一详细定义是 `09-release-readiness.md`。本路线图只定义阶段依赖，不复制测试正文或发布清单。

阶段与测试映射：

```text
Phase 0 → TC-RT-001/005/006/009/010、TC-API-001、TC-OAUTH-003
Phase 1 → TC-IMP-*、TC-CONN-001
Phase 2 → TC-RT-*、TC-TEAM-*
Phase 3 → TC-API-*、TC-SDK-*
Phase 4 → TC-SDK-*
Phase 5 → TC-WEB-*
Phase 6 → TC-OAUTH-*、TC-CONN-*、TC-SEC-*
```

具体用例、输入、断言和证据要求统一维护在测试总索引列出的各归属文档。

完整文档地图与权威顺序见 `README.md`。

> 注（2026-09-09）：本节原文档清单仅覆盖 `00`~`08` + roadmap + test-cases，已过期；`09`~`15`、证据页与设计记录未列入，以上方指针为准。

### 6.7 排期说明

本文使用“两周迭代”作为验收节奏，不对总工期作承诺。实际排期必须在 Phase 0 完成以下实测后校准：

- Preset/ResolvedPresetSnapshot 写入 Participant，以及 Runtime Agent/Driver 实际创建链路；
- planned DAG 实际物化和执行；
- 持久化状态一致性、事件规范化和重启后的未完成状态标记；
- MCP OAuth transport 注入和刷新；
- App Server 两种传输的稳定性；
- software-company 端到端运行时间与资源占用。


---

## 7. 决策记录（原 `agent-store-v1-roadmap.md` §10）

> 本节由 `agent-store-v1-roadmap.md` §10 整体并入（2026-09-11 文档合并）。**这是本仓库 Agent Store 线唯一的决策记录位置**；此前文档中「见 `roadmap` §10」的引用现指向本节。原决策编号 1–4 保持不变：**1** 二进制分发 · **2** 协议词汇对齐（版本框架部分被 §7 决策 4 取代）· **3** Team Run 触发方式 · **4** 发版前单一版本与契约指纹。

以下决策由用户拍板，作为后续实施依据，覆盖此前文档中的“待定/备选”表述；实施顺序见 `15-store-chain-and-protocol-vnext-plan.zh.md`。

1. **二进制分发走 npm optionalDependencies**（2026-09-09）：按平台发布 `@flowy-agent-store/runtime-<platform>-<arch>` 包，作为 `@flowy-agent-store/sdk` 的 `optionalDependencies`；`resolveAppServerBin` 查找顺序 `bin` → `AGENT_STORE_BIN` → `require.resolve` 定位 platform 包内二进制 → PATH（详见 `12-sdk-packaging.md` §6）。否决 GitHub releases + checksum 下载缓存方案。
2. **协议 vNext 完全重命名（thread/turn/item），stdio 不纳入**（2026-09-09）：V1 之后破坏性升级公共协议为 v2，概念模型对齐 Codex app-server（`run/conversation → thread`、`agent/run → turn*`、事件项归并为 `item`）；`initialize` 版本协商与 `dispatch_connection_request` 唯一分发保留；stdio 维持排除（SDK 仍走 spawn + 回环 WS）。该决策将重构 webui 事件层（`conversation-events` / `RunHandle` 等）与 SDK 方法面，需在 vNext 立项前先产出 Codex app-server spec diff（方法/事件/概念映射表）再动工；V1 冻结版本文档（`05`、`07`）标为 v1 基线。（**注**：其中「升级为 v2 / 冻结版本 / v1 基线」的**版本框架表述已被本节决策 4 取代**——该对齐属协议 v1 内部调整，不设下一版；概念对齐目标不变。）
3. **Team Run 改由 Leader 模型调用 `nomi_delegate(strategy=planned)` 触发**（2026-09-10）：撤销此前"Team Runtime 不依赖 `nomi_delegate` 工具"的表述（`00` §4.4、`04` §4.3、TC-TEAM-002）。`team/run` 由服务端创建 Leader Conversation，并把 Team 的 `AgentExecutionTemplate` 绑定为该会话的 `execution_template_id`；Leader 在该 Conversation 的 turn 内调用 `nomi_delegate(strategy=planned, goal=…)`，服务端据此构造 Planning Context 并调用内部 Planner 生成/物化 DAG。
   - 成员池、`max_parallel`、`routing_constraints` 与权限取自绑定的 Template 和服务端策略，不接受模型输入（不放松 `00` §1.2「不让模型直接决定权限、成员路由、状态迁移或审批结果」）。
   - 顶层仍不得以 `strategy=parallel` 代替 planned 流程；局部并行仍由已校验 DAG 中的独立 ready Step 表达。
   - 注册给 Leader 的必须是绑定真实 `AgentExecutionEngine`（具备持久化 Execution/Event/Attempt）的 planned 实现。仅支持 `strategy=parallel`、以同步无持久化方式投影的 embedded 实现（`nomi-agent::local_delegate_tool`）不得用于 Team Runtime；Store 会话必须关闭该实现，避免模型选中错误版本。
   - App Server 的公共 `run_id` 仍由 `AppServerRunMapping` 从内部 Execution 映射，模型不可见；Leader turn 产生的 Execution 通过 ConversationExecutionLink 反查。
   - 该决策把"Leader 必须有 Conversation/Attempt"从"暂不要求"变为"必须"（不必用户可见）。
4. **正式发版前所有文档统一为「单一现行版本」，不设版本兼容框架**（2026-09-11）：适用范围＝协议 + SDK（`05`、`07`）、插件规范（`17`）、市场规范（`18`），以及全部计划 / 决策文档。
   - **唯一冻结线是「正式发版」**：发版前这些文档均为**现行正文**，可自由调整；「冻结」一词在发版前退休。发版后才开始有版本语义（那时才可能出现 v2）。
   - 因此不设：v1 / v1.1 / v2 的版本路径、迁移窗口、弃用窗口、向后兼容承诺、别名层、破坏性变更公告义务（针对规范文本）。
   - **决策 2 被本记录取代的部分**：原写「V1 之后破坏性升级公共协议为 v2」不成立——`thread/turn/item` 词汇与概念对齐属**协议 v1 内部**调整，不是"下一版"。**目标不变**（概念模型对齐 Codex app-server、保留 `initialize` 与 `dispatch_connection_request` 唯一分发、stdio 维持排除、动工前先产出 Codex app-server 概念比对）。
   - **`17` §8「届时本规范升级为 v2」是范围问题而非版本问题**：原生格式尚未定义，将来以**新增章节或独立文档**落地，兼容层降为导入源。
   - **保留两条纪律**：① 任何变更走**显式修订 + 偏差登记**，不静默修改（原「不静默修改本冻结版本」的有效部分）；② **已发布 npm 包的版本号与 `changelog` 公告义务（D10=A）不变**——那是**发行机制**的版本，不是规范 / 协议的版本。
   - **`PROTOCOL_VERSION` 不是版本号，而是契约指纹**（`nomifun-app-server/src/lib.rs:86`、`web/packages/protocol/src/protocol.ts:8`、`web/packages/client/src/http-transport.ts:43`）：任何 wire 改动（方法增删改名、现有 DTO 加字段、事件 payload 变化）都必须同步 bump 三处。**现状是坏的**：该常量自 `450d1037c` 设下后从未更新，期间已有 `config/get|set`、`skill/create|update|delete`、`run/plan`、`market/*`、`message.activity.usage` 等 wire 变更 → 当前握手对契约漂移**无保护**。
   - **连带订正 D6 落地口径**：原「逃生口默认关（不破坏现有可安装性）」的前提是规范已发布；规范未发版、无既有消费者，故改为**阻断默认开 + 逃生口**（逃生口保留是因为它本身有产品价值，不是为兼容）。
   - **本记录不改写历史证据页**：`13-p0-execution-plan.md` §15 / `13-p0-execution-plan.md` §14 / `13-p0-execution-plan.md` / `agent-store-v1-test-cases.md` 里的「版本冻结」指的是**被安装 Agent 的版本冻结语义（TC-RT-002）**，与本记录无关，一字不动。
