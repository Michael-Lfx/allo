# SDK / 站点 / 插件市场 / WebUI：方向与执行计划

> 状态：计划（2026-09-09）。**先定方向与验收口径，不含实现**。
> **四个方向**：① SDK + 站点（配对执行） · ② 插件与市场规范（收尾） · ③ WebUI 功能 · ④ 待立项（生产化硬指标）。
> 优先级：**① ③ 第一优先级；② 高优先级（已完成 90%）；④ 未排期；WP-5 协议 vNext 延后。**
> 上游依据：`19-webui-codex-alignment.zh.md`（方向三子计划）、`17-plugin-spec.zh.md`、`18-marketplace-spec.zh.md`（方向二交付）、`15-store-chain-and-protocol-vnext-plan.zh.md`（WP-5 顺延）、`11-webui-production-readiness.md`、`12-sdk-packaging.md`、`07-typescript-sdk.md`。
> 口径：排期为范围值、按实测校准，不构成承诺；结论区分「已验证事实 / 推断 / 待定」。

---

## 0. 方向一览

| 方向 | 范围 | 现状 | 主文档 |
| --- | --- | --- | --- |
| **① SDK + 站点** | SDK 加固（A1–A5；A6 已决策延后）+ 站点事实修正与开发者文档（C1–C5） | 均未开工。**两者是派生关系，必须配对执行**（§3.2 C0） | 本文 §3.1 / §3.2 |
| **② 插件与市场规范** | D1 规范正文 ✅ / D2 机器可校验 Schema ✅（T19） / 反向验证 ✅（T20） / v1 冻结 ✅（T21） | **已收口（2026-09-10）**：`17`/`18` 标为「v1 冻结」，偏差全部登记 | `17-plugin-spec.zh.md`、`18-marketplace-spec.zh.md` |
| **③ WebUI 功能** | W1–W14（Codex app 体验对齐，四层） | 未开工 | `19-webui-codex-alignment.zh.md` |
| **④ 待立项** | `11` 号未纳入的 8 项 + WP-5 协议 vNext | 未排期 | `11-webui-production-readiness.md`、`15-...zh.md` |

---

## 1. 为什么这样排序

- **四类资产闭环已收口**：专家 / 专家团 / 技能 / 连接器「下载 → 安装 → 使用」全链路已验收（四链路 24/24、P0-A/B、OAuth 26/26），继续在协议层大改的收益低于把 SDK 与 UI 打磨到可用。
- **npm beta 已发布，第三方开始长期驻留使用**：`0.1.0-beta.2` 四包 + `runtime-win32-x64` 已上线，SDK 的进程与传输健壮性直接决定第三方是否踩坑——本轮已确认一个会冻死服务端的缺陷（F10 / A1）。
- **站点是开发者的第一触点，且存在已确认的事实性错误**（F1–F5）：下载按钮对非 Windows 访客 404、兼容性矩阵声称 5 平台而实际只有 1 个平台有产物。
- **SDK 与站点是同一条链的两端**：站点 SDK 文档是 SDK 公共面的投影，公共面每改一处即欠一笔文档债（§3.2 C0），因此合并为一个方向、配对执行。
- **WebUI 存在「协议已就绪、界面未接」的成片空白**（F19–F21）：`run/steer`、产物、全局通知均已具备后端能力却零使用（对账见 `19` §7）。
- **协议 vNext 属重构型工作**：在 SDK 公共面与 webui 功能尚未稳定时动工，会把返工风险带进破坏性重命名，故延后。

---

## 2. 现状核查（2026-09-09 实测，按方向分组）

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
| F20 | **产物无任何展示** | `web/src` 搜 `output_files` / `artifact` 无命中；`TurnResult.output_files` 已聚合、`artifact.created` 已发 | 跑完看不到产出文件，「使用」环节缺一环 |
| F21 | **无全局通知层；连接状态仅一个小圆点** | 无 toast / notification 组件（仅 i18n 文案）；`composer-model-dot ${phase}` 是唯一连接指示 | 断线、后台 Run 完成、导入完成均静默；与 F11/A2 叠加时表现为「莫名其妙不能用」 |
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
| WP-5 协议 vNext | 方法 / 事件 / 概念映射表与边界拍板；待 SDK/UI 稳定后启动 |

---

## 4. 待拍板

| # | 问题 | 选项 |
| --- | --- | --- |
| Q1 | 平台矩阵策略 | ✅ **已定（2026-09-09）：仅 Windows**——站点口径收敛（C1-2），A6 延后至出现真实非 Windows 需求 |
| Q2 | 站点托管与域名 | ⏸ **已延后（2026-09-10）**——VPS + 自定义域名 / EdgeOne / GitHub Pages；待 C5 与 D-SDK-1 的带宽问题需要一并解决时再拍 |
| Q3 | 附件图片输入时机 | ✅ **已定（2026-09-10）：等协议 vNext**——W10 属「待反馈」批，不进本轮三闭环（`19` §3 W10） |
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
| **第 4 批（已收口）** | 规范 4 处修补 + D2（Schema 与校验器，✅ T19）+ 规范反向验证（✅ T20）+ v1 冻结（✅ T21） | ✅ `17`/`18` v1 冻结（2026-09-10） |
| 待反馈 | 其余 webui：W2 / W3 / W4 / W6 / W7 / W8 / W9 / W10 / W11 / W12 / W1b / W14 | 按真实使用反馈排期 |
| 待决策 | C5（域名 / HTTPS，等 Q2） | — |
| 维护模式待议 | A3 / A4 / A5 | 须有真实外部 issue |
| 未排期 | 方向四全部 | 需单独立项 |

> 第 3 批全部是「后端/client 已就绪、只差前端接线」，不碰协议，风险最低、见效最快。

### 5.1 退出条件（✅ 已采纳 2026-09-09）

三条线现在都缺「什么算完成」，这是无限打磨的根源。建议为每个方向写死退出条件：

| 方向 | 退出条件 | 退出后状态 |
| --- | --- | --- |
| ① SDK + 站点 | A1 + A2 修完；发 `0.1.0-beta.3`；C1 三处事实错误修正 | **维护模式**：新需求须有真实外部 issue 才排期（A3/A4/A5 不再主动做） |
| ② 插件与市场规范 | 4 处真缺陷修补 + 「已知偏差」小节落地 | **v1 冻结**：等第一个外部发布者来挑战 |
| ③ WebUI | 本轮只做三个闭环（W5 产物面板 / W1 命令面板 / W13 市场管理）验收全绿 | 其余按真实使用反馈排，不按「对齐 Codex 的完整性」排 |

> 依据：npm 下载量 API 对四包**均无可观测数据**（发布仅数小时 / 无外部下载）——目前没有可观测的第三方使用，SDK 的 A3/A4/A5 收益依赖尚未出现的用户。

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
| T20 | 规范反向验证 — ✅ **已完成（2026-09-10）** | 验证记录：`17` §10 P1–P4、`18` §11 D3–D8 | ✅ 逐条核对 `18` §5/§7 与 `17` §4/§6/§7 + `18` §9，并对三个真实市场做字段普查（`--census`）；**新登记 8 条**，其中 2 条已修（`18` D5 清单抓取漏 15s 超时、`17` P2 依赖丢名），其余按建议「改规范 / 列 v1.1」择一 | T19 |
| T21 | v1 冻结标记 — ✅ **已完成（2026-09-10）** | `17` / `18` 状态行改为「**v1 冻结**」+ 冻结范围与已接受缺口清单 | ✅ 21 个批次任务全部收口 | T20 |

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
| W8 | 通知与连接状态层（⚠️ 部分：Toast 层 + 断线横幅 + 一键重连；多标签协调与 catalog/后台 Run 通知未做，见 `19` §3 第 3 层 W8「进度」） | 2026-09-10 |
| T9 | 包元数据（`protocol`/`client`/`sdk` 补 `engines` / `repository` / `sideEffects`） | 2026-09-10 |
| T10 | 发 `0.1.0-beta.3`（四包发布至 npmjs，`tag=beta`，`latest` 未动） | 2026-09-10 |
| T11 | 文档同步（A2）：`typescript-sdk.md` §4.2 实现契约 | 2026-09-10 |
| T12 | W5 产物面板（⚠️ 按 D-W5-1 收敛为宿主面文件服务：会话 workspace 的列表 / 预览 / 下载 / 评论入下一条消息；**无**「按 Run 归属」与「接受 / 回退」） | 2026-09-10 |
| T13 | W1 命令面板 V1（`/` 命令 + `@` 提及 + ↑↓/Enter/Esc + IME 守卫 + `Cmd/Ctrl+K`；无 `+` 菜单抢占） | 2026-09-10 |
| T14 | W13 市场管理补全（auto-update 开关回读 / 条目级导入 / 注册表字段 / 级联移除确认列出影响面）+ Q7 ① 后端官方-第三方 `auto_update` 区分（⚠️ `revision`、上次刷新未做 → D-W13-1） | 2026-09-10 |
| T15 | 三闭环验收（✅ **全绿**：协议级 live 验收 11/11，脚本 `web/scripts/sdk-live-w13-acceptance.ts`；UI 点击与键盘级 = 用户人工审查通过，期间排掉 3 个阻塞项 D-W13-2 / D-STREAM-1 / D-STREAM-2；记录见 `19` §5） | 2026-09-10 |
| T19 | D2：Schema 与校验器（`docs/agent-store/schemas/*.json` + `scripts/check-agent-store-market.mjs`；三个真实市场 `0 error`、自检 17/17、单测 6 pass；已接入发布脚本与 `check`） | 2026-09-10 |
| T20 | 规范反向验证（新登记 8 条偏差：`18` D4–D8、`17` P1–P4；已修 D5 / P2；其余按建议「改规范 / 列 v1.1」择一；普查模式 `check-agent-store-market.mjs --census`） | 2026-09-10 |
| T21 | `17`/`18` v1 冻结（状态行 + 冻结范围 + 已接受缺口清单：`17` P1/P3/P4、`18` D4/D8） | 2026-09-10 |
| T16 | 17 字段必填 / 默认 + 偏差小节 | 2026-09-09 |
| T17 | 18 `marketplace_id` / `content_digest` / 跨市场命名 + 偏差小节 | 2026-09-09 |
| T18 | 19 过时引用修正 + W1 拆分 | 2026-09-09 |

> **第 2 批已收口**：T2 / T5 / T7 / T8 / T9 / T10 / T11（+ W8）全部完成；`0.1.0-beta.3` 于 2026-09-10 发布。
> **T8 的重连策略（已实现）**：T7 让 `close()` 清空 `onNotification` 监听器，故「只重置 `lastSeenSequence`」无法恢复投递；实现为 `Transport.onLifecycle` 广播连接丢失 → `EventSubscription.rearm()` / `ConversationSubscription.rearm()` 重建订阅（`run/subscribe`）并全量重放，宿主持有的 `AppServerClient` 负责重挂通知桥并重跑 `initialize`。
> **发布环节要求**：npm 账号开启 2FA，`npm login` 的 token 不带 bypass-2FA（首次发布返回 403），必须用 granular token（bypass 2FA）经临时 npmrc 注入；临时 npmrc 只写 `_authToken=${NODE_AUTH_TOKEN}` 变量引用，token 只入进程环境变量。

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
| D-STREAM-2 | **回答可见后仍显示「正在处理」约 6–15 秒（后端收尾 child 阻塞 `Finish`）** | 实测探针（临时会话，跑完即删）：`33ms RECEIPT completed=false` / `33ms turn.status{running}` → `15079ms message.delta`（回答完成）→ `15245ms message.activity{turn_completed}` → **25s 时 `conversations.get` 仍报 `is_processing=true status=running`**。运行时日志同轮：`execute_turn() completed; closing exact post-turn effects before Finish, elapsed_ms=15178` → `StreamRelay received terminal event event_type="Finish" elapsed_ms=24996`，即**回答完成后 9.8 秒才发 `Finish`**。代码位置：`nomifun-ai-agent/src/manager/nomi/agent.rs:1965-2036` —— **post-session memory distillation**（`super::distill::run_distill_exact_turn`，一次 provider 调用；门控 = host opt-in `distill_enabled` + `distill_dir` + human origin）在 `Finish` 之前被 `await`，注释明写「Finish is forbidden until this child closes」；其后还有 `post_turn_review` hooks | 用户视角「答案已出但一直转圈、输入框被锁」；前端**没有说谎**（服务端此刻确实 `is_processing=true`），是该窗口内没有任何 wire 信号（最后可见事件为 `turn_completed`，下一个事件要等近 10 秒） | ⏸ **待拍板**：① 让 `Finish` 不再等待记忆蒸馏（改为后台 child + 已有 recovery 观测），使「回答完成」与「轮次结束」同时发生——动 durability 语义，需记忆侧确认；② 纯 UI：`message.activity{kind:"turn_completed"}` 之后把忙态文案分级为「正在收尾…」（不改语义，成本最低）；③ 关掉记忆蒸馏——✅ **已做（2026-09-10）：配置化**。`~/.agent-store/config.toml` 新增 `[memory].distill_enabled`（`AgentStoreConfig.memory`，`Option<bool>`，缺省 = 上游默认 ON；`nomifun-app-server/src/agent_store.rs`），由 agent-store 主机启动时经 `nomifun_ai_agent::manager::nomi::distill::set_distill_host_override()` 注入（**进程内原子，不改环境变量、不受线程启动顺序限制**；优先级：`NOMIFUN_MEMORY_DISTILL` 环境变量 > 本文件 > 上游默认）。文档：`site/content/docs/{zh-CN,en-US}/configuration.md` 新增 `## memory` 小节 + 顶层键行；本机 `~/.agent-store/config.toml` 已写入 `[memory] distill_enabled = false`。①（让 `Finish` 不再等待蒸馏 child）仍未做，需要记忆侧确认 durability 语义。 |

> **D-SDK-1 根因（已定位，2026-09-10）**：`store/list` → `ensure_default_marketplaces`（`nomifun-app-server/src/lib.rs:1634` / `1500-1549`）**同步**对三个内置 URL 市场执行 `add` → `fetch_remote` → `mirror_http_tree`（BATCH=32，`market_source.rs:132-200`），从明文公网镜像 `http://111.170.173.22:10072` 整树下全量 HTTP 下载（`agent_store.rs:137-155`）。就绪行在该工作**之前**发出（`apps/agent-store/src/main.rs:373-384`），故代价全落在首个 `store/list`；`initialize` / `models/list` 不触市场，仅毫秒。`add` 的幂等短路（`app_server_marketplace.rs:420-441`）是 DB 查询，只在**同一 data-dir** 生效；单源上游超时 **600s**（`lib.rs:1543`），故「调高默认值」的正确取值无上界——只能靠 ①/④ 解决，③ 仅止血。

#### 阻塞决策（拍板即解锁任务）

| 决策 | 内容 | 解锁 |
| --- | --- | --- |
| Q7 | ✅ **已定（2026-09-10）：① 改实现以区分官方/第三方** | T14 已解锁（登记见 `18` §11 D1） |
| Q6 | ✅ **已定（2026-09-10）：① 写 `~/.agent-store/config.toml`** | W11 provider 分区（仍属待反馈批） |
| Q2 | ⏸ **已延后（2026-09-10）** | C5 继续挂起；D-SDK-1 的镜像带宽大头一并等它，但**不阻塞** ③（已做）与 ① |
| Q3 | ✅ **已定（2026-09-10）：等协议 vNext** | W10 留在「待反馈」批，不进本轮三闭环 |

> **本轮已无待拍板项**：Q1 / Q3 / Q4 / Q5 / Q6 / Q7 已定，Q2 延后——带决策部分按上表执行。

---

## 6. 边界与不做什么

- **不把宿主管理面收编进协议**：进程生命周期、数据目录、provider / MCP 配置、`fs/browse`、资产直链保持为宿主管理面。判断规则：*第三方 SDK 消费者是否应该能调用它？* 不能 → 不进包。
- **不为了「纯粹」牺牲权限边界**：`fs/browse` 若进入公共协议，等于给远程客户端文件系统枚举能力。
- **不在 SDK 稳定前动协议重命名**（WP-5 延后）。
- **不做假开关**：设置分区、市场开关等一律要有真实数据源与回写路径，未实现的继续标「即将推出」而非留占位控件。
- **不让文档滞后于公共面**：A 与 C 配对同批（§3.2 C0），F25 的债机制化。
- **不承诺排期**：本计划只给批次与验收口径，实际节奏按实测校准。
