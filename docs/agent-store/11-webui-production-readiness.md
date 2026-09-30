# Allo WebUI 生产化工程技术方案（Production Readiness Specification）

> 状态：设计规范（已校准）  
> 适用范围：`web/` 独立 App Server WebUI、React 客户端及其与 App Server 双向通信层  
> 关联设计：[`05-flowy-agent-store-app-server-protocol.md`](file:///c:/workspace/allo/docs/agent-store/05-flowy-agent-store-app-server-protocol.md)、[`09-release-readiness.md`](file:///c:/workspace/allo/docs/agent-store/09-release-readiness.md)、[`19-webui-codex-alignment.zh.md`](file:///c:/workspace/allo/docs/agent-store/19-webui-codex-alignment.zh.md)、[`22-webui-productionization.zh.md`](file:///c:/workspace/allo/docs/agent-store/22-webui-productionization.zh.md)  
> 核心原则：**以公开 App Server 协议为唯一边界，服务端强制隔离多租户上下文，前端实现零凭据明文持久化与确定性状态自愈。**

---

## 1. 背景与核心挑战

Allo WebUI 是面向终端开发者与独立部署环境的聊天与智能体交互界面。在早期原型验证阶段，WebUI 依赖本地开发模式，采用了临时性实现策略（如 URL 携带 Token、全量消息一次性拉取、纯文本消息展示、单标签单向 WebSocket 连接等）。

随着系统向生产级交付（Production Ready）推进，面临四大核心挑战：
1. **安全性降级风险**：明文凭据暴露在 URL 或 `localStorage` 中极易遭受 XSS 窃取；缺少 Origin/CSRF 校验无法抵御跨站会话劫持；富文本直接渲染易导致 DOM 注入。
2. **连接抖动与多标签竞争**：长连接在移动端休眠或网络闪断后容易陷入“假死”或幽灵“处理中”状态；同用户开启多个标签页时，各标签状态漂移且产生重复写请求。
3. **海量历史消息的性能瓶颈**：随着会话轮次增长，全量拉取不仅导致白屏时间拉长，更引发 DOM 节点暴增与内存泄漏。
4. **工作区与权限漂移**：底层工作区目录可能在外部被删除、重命名或切为只读，若缺乏主动感知，会导致用户执行时抛出底层不可读错误。

---

## 2. 总体架构与交互拓扑

WebUI 生产化架构围绕“安全凭据层”、“连接与同步总线”、“虚拟化渲染流水线”、“可观测性网关”四层展开：

```mermaid
flowchart TD
    subgraph Browser["浏览器客户端 (Browser Client)"]
        subgraph TabA["活跃标签页 (Leader Tab)"]
            AuthStore["内存凭据管理器 (短期 Token)"]
            SyncBusA["多标签广播总线 (BroadcastChannel)"]
            WSManager["自愈 WS 管理器 (指数退避+抖动)"]
            VirtList["增量虚拟滚动列表 (Keyset Pagination)"]
            SafeRender["安全 Markdown 净化流水线 (DOMPurify)"]
        end
        subgraph TabB["次要标签页 (Follower Tab)"]
            SyncBusB["多标签广播总线"]
            FollowerStore["只读镜像状态"]
        end
    end

    subgraph Boundary["安全防护与网关边界"]
        SecHeaders["CSP / Origin / Host 校验器"]
        RateLimiter["租户级自适应限流 (Token Bucket)"]
        ReqIdMiddleware["TraceId / RequestId 注入器"]
    end

    subgraph AppServer["App Server 核心层 (Rust Backend)"]
        AuthService["会话鉴权与 HttpOnly Cookie 交换"]
        MsgRepo["消息持久化与 Keyset 游标引擎"]
        WorkspaceGov["工作区可达性探测与规范化 (Canonicalize)"]
        HealthChecker["只读探针 (/healthz, /readyz)"]
    end

    TabA <-->|跨标签同步| TabB
    TabA -->|1. 证书与短期令牌交换| Boundary
    TabA <-->|2. 双向事件流 (带 RequestId)| Boundary
    Boundary --> AppServer
```

---

## 3. 核心技术模块与实施方案

### 3.1 安全防护与凭据生命周期

1. **凭据双轨制管理**：
   - 生产环境禁止在 URL（`?token=`）与 `localStorage` 中存放持久化明文凭据。
   - **主认证凭据**：采用 `HttpOnly`、`SameSite=Lax`、`Secure` Cookie 存储长效会话 Session，脚本不可直接读取。
   - **操作级短期令牌**：客户端仅在内存（`AuthContext`）中保留短期 Access Token（TTL ≤ 15 分钟），每次页面刷新通过 Cookie 静默交换。
2. **防重放与 CSRF 双重提交**：
   - 所有非幂等写操作（工作区注册、会话删除、模型切换）均要求请求头携带 `X-CSRF-Token`（从服务端元数据注入的防伪 Nonce）。
   - 服务端严格校验 HTTP `Origin` 与 `Host` 头，跨源非白名单请求直接返回 `403 Forbidden`。
3. **内容安全策略（CSP）与富文本安全流水线**：
   - 服务端返回标准 CSP 头：
     `default-src 'self'; script-src 'self'; connect-src 'self' wss://*; img-src 'self' data: https:; style-src 'self' 'unsafe-inline';`
   - 前端 Markdown 渲染执行严格的白名单标签与属性过滤（禁止 `<script>`, `<iframe>`, `onerror`, `javascript:` 伪协议），链接默认注入 `rel="noopener noreferrer"` 并做协议净化。

### 3.2 弹性长连接与多标签状态自愈

1. **指数退避与抖动重连**：
   - 断线后采用带有 Jitter 的指数退避重连算法（间隔公式：$T_{wait} = \min(T_{max}, T_{base} \times 2^{retry}) \pm \text{jitter}$，上限 30 秒）。
   - 重连成功后，客户端**不假设本地增量无损**，必须向服务端发送 `conversation/sync` 请求或根据最新 sequence id 补齐增量。
   - 收到服务端 `conversation/resync-required` 事件时，强制触发权威全量快照拉取，清空幽灵“进行中（processing）”状态。
2. **多标签页广播同步（BroadcastChannel）**：
   - 利用浏览器 `BroadcastChannel('allo_sync_bus')` 协调多标签状态。
   - 采用轻量级 Leader 竞选机制：优先由 Leader 标签页维持单一 WebSocket 物理长连接，其余 Follower 标签页通过内部事件总线同步渲染更新，避免对 App Server 造成连接风暴与重复写冲突。

### 3.3 增量消息虚拟滚动与游标分页

1. **Keyset 游标增量加载**：
   - 摒弃基于 `offset` 的低效分页，采用基于消息物理序号或时间的 Keyset 游标机制：`GET /conversation/messages?before_id={cursor}&limit=50`。
   - 客户端维护已加载消息的双向区间索引 `[head_id, tail_id]`，向上滚动触发触顶拉取历史，实时增量追加至尾部，依据全局唯一 `message_id` 实现幂等去重。
2. **视口虚拟化（Virtualized Viewport）**：
   - 对超长会话采用视口高度虚拟化，DOM 中仅保留当前视口及上下 5 屏的节点，其余节点以空占位块（Placeholder Spacer）撑开滚动条。
   - 万级消息场景下首屏渲染时间保持在 100ms 以内，内存占用降至 50MB 以下。

### 3.4 工作区治理与容灾降级

1. **可达性主动探测与状态标记**：
   - 服务端为每个注册的工作区定期或在操作前执行 `std::fs::metadata` 探测，状态分为：`Active`（正常）、`ReadOnly`（只读）、`Missing`（路径不存在）、`PermissionDenied`（权限不足）。
   - `workspace/list` 协议显式返回 `status` 与 `reason`。
2. **前端交互降级**：
   - 当工作区处于 `Missing` 或 `PermissionDenied` 状态时，WebUI 将该工作区高亮警示，禁用新建会话与发送动作，并提供“重新定位路径”或“从列表注销”指引。
   - 路径展示由服务端提供经过规范化（`canonicalize`）的跨平台展示文案，统一处理 Windows 反斜杠与 Unix 斜杠。

### 3.5 全链路可观测性与健康探测

1. **统一分布式上下文透传**：
   - 客户端发起的每个 HTTP 与 WebSocket 请求均生成唯一 `X-Request-Id`（UUIDv4），并在收到响应后与错误日志关联。
   - 客户端异常时将结构化 `AppServerError`（含错误码、租户上下文、Request ID）上报至监控端点。
2. **分级健康检查端点**：
   - `/healthz`：轻量级进程存活探针（Liveness），仅检查 WebUI 与后端基础 HTTP 服务是否能响应，不触发数据库与外部 I/O。
   - `/readyz`：就绪探针（Readiness），探活 SQLite 数据库连接池与关键目录可读写性。

---

## 4. 关键架构决策与权衡矩阵

| 决策点 | 备选方案 A | 备选方案 B | 最终决策 | 决策依据与权衡 |
| :--- | :--- | :--- | :--- | :--- |
| **凭据持久化模式** | `localStorage` 保存长效 Token | `HttpOnly` Cookie + 内存短效 Token | **方案 B** | 彻底杜绝 XSS 直接读取凭据的风险；即使前端存在富文本渲染漏洞，也不会泄漏鉴权凭证。 |
| **多标签同步方式** | 各标签独立创建 WS 长连接 | `BroadcastChannel` 协调单主连接 | **方案 B** | 减少 App Server 连接数与并发锁竞争，避免多标签同时编辑同一会话引起写冲突。 |
| **超长历史分页** | 全量加载 + 本地假分页 | 服务端 Keyset 游标 + 虚拟 DOM 滚动 | **方案 B** | 杜绝万级消息导致的白屏与渲染假死，网络带宽消耗降低 80% 以上。 |
| **Markdown 渲染** | 直接利用 `dangerouslySetInnerHTML` | 严格白名单过滤流水线 (DOMPurify) | **方案 B** | 聊天交互包含不可信的外部输入与智能体生成内容，必须强制执行安全净化。 |

---

## 5. 验收标准与测试用例

### 5.1 自动化与端到端测试矩阵

| 用例编号 | 模块 | 验证场景 | 预期行为 |
| :--- | :--- | :--- | :--- |
| **TC-SEC-01** | 安全凭据 | 模拟 XSS 尝试从 `document.cookie` / `localStorage` 读取凭据 | 返回 `undefined`，无法提取长效会话 Token。 |
| **TC-SEC-02** | CSRF 防护 | 伪造不带 `X-CSRF-Token` 的写操作请求 | 服务端返回 `403 Forbidden`，操作被拦截。 |
| **TC-SYNC-01** | 连接自愈 | 模拟弱网断线 15 秒后恢复 | WS 自动指数退避重连，拉取增量补丁，无幽灵“处理中”状态。 |
| **TC-SYNC-02** | 多标签协作 | 标签 A 发送消息，标签 B 处于打开状态 | 标签 B 几乎同时实时收到消息与气泡渲染，不重复发起网络拉取。 |
| **TC-PERF-01** | 滚动渲染 | 加载包含 5000 条消息的长会话 | 首屏渲染耗时 < 150ms，滚动帧率稳定在 60 FPS，无卡顿。 |
| **TC-WS-01** | 工作区异常 | 物理删除工作区对应本地目录后刷新 WebUI | 界面显示“工作区丢失”警示标识，发送按钮置灰并提示重新关联。 |

---

## 附录：原始技术底稿与历史归档 (Historical & Technical Reference Archive)

> **归档说明**：以下完整保留重构前的原始技术底稿、历次讨论与历史记录全文，供历史追溯、协议字段详细对照与技术审计。

---

# Allo WebUI 生产化补齐清单（Production Readiness）

> 状态：规划中（**清单原件**；各项当前状态见 `19-webui-codex-alignment.zh.md` §7 逐项对账，立项拆项见 `22-webui-productionization.zh.md`）
> 日期：2026-08-26
> 适用范围：`web/` 的 App Server 聊天 WebUI 及其协议消费面
> 关联：`05-flowy-agent-store-app-server-protocol.md`、`web/README.md`、`09-release-readiness.md`、`19-webui-codex-alignment.zh.md`、`22-webui-productionization.zh.md`
> 前置：会话重命名/删除、按工作区分组、上下文占用、多行输入框等基础能力已落地。

本文档记录让 Allo WebUI 达到“生产环境标准”仍需补齐的工作项。每一项都有背景、目标、验收要点和依赖，供后续按优先级排期。

---

## 0. 总体原则

- 全部能力以公开 App Server 协议为边界增量扩展，不直接调 allo 内部 API。
- 涉及安全、数据库迁移、公共协议字段的改动，先行完成设计和评审再实施。
- 遵循 owner 隔离：任何跨 owner 展示（路径、会话、指标）都必须由服务端裁剪，不依赖前端自觉。

---

## 1. 安全

### 1.1 正式认证与令牌管理

- **背景**：当前 WebUI 通过 `?token=`/`Authorization: Bearer` 直连 App Server；开发模式禁用了鉴权，生产上需要真实的身份与会话边界。
- **目标**：
  - 支持正式登录/令牌换取，令牌不落 `localStorage` 明文（优先 `HttpOnly` Cookie + CSRF token，或短期内存令牌 + 刷新机制）。
  - 令牌到期、失效、被吊销时有明确的状态与重连策略。
  - 敏感请求（删除、工作区注册、模型切换）做额外确认或权限校验。
- **验收**：无明文凭据持久化；断连/401 时有可恢复的重新认证流程；刷新后令牌不泄露给脚本。

### 1.2 Origin / CSP / CSRF 策略

- **背景**：Web 界面需要防止跨站请求伪造与脚本注入。
- **目标**：
  - 服务端校验 `Origin`/`Host`，拒绝跨源请求；配置合理的 CSP 响应头（限制 `script-src`、`connect-src` 只允许受信端点）。
  - 写操作请求携带 CSRF 防护（同源 Cookie + 双重提交或服务端 nonce）。
- **验收**：跨站发起连接/删除被拒绝；安全头在响应中可见；CSP 不阻断 UI 正常运行。

### 1.3 WS 断线重连与多标签协调

- **背景**：Chat UI 依赖长连接实时事件，网络抖动/服务重启会导致事件丢失或状态漂移。
- **目标**：
  - 指数退避自动重连（带抖动和上限），重连后以权威 `conversation/list`/`conversation/get` 恢复状态。
  - `conversation/resync-required` 后自动补拉历史。
  - 多标签页协调：同一用户的多个标签共享状态变更（BroadcastChannel/`storage` 事件），避免各自为政造成冲突或重复请求。
- **验收**：断线后恢复不卡死、无幽灵“处理中”；两个标签对同一会话的变更保持一致。

---

## 2. 会话工作流

### 2.1 消息重试 / 编辑 / 重新生成

- **背景**：消息失败或生成不符合预期时，目前只能整段重发。
- **目标**：
  - 失败消息支持“重试”（复用幂等机制或新消息）。
  - 用户消息支持“编辑并重新生成”，复用后端 edit/resubmit 语义。
  - Assistant 回复支持“重新生成/换模型再试”。
- **验收**：visible 反馈（正在重试/生成中），失败有稳定错误码，不影响消息顺序一致性。

### 2.2 归档 / 回收站

- **背景**：删除是强语义，无法恢复，用户误删成本高。
- **目标**：
  - 支持“归档”（从主列表隐藏）与“回收站”（软删除、可恢复）。
  - 回收站内显示删除时间，过期自动清理或在界面明确提示。
- **验收**：归档/回收不破坏会话引用；恢复后分组与上下文快照仍在。

### 2.3 批量操作

- **背景**：大量会话时逐个删除/归档效率低。
- **目标**：支持多选会话后进行批量删除、归档、导出。
- **验收**：批量操作有进度与部分失败的明确反馈；可安全中断。

### 2.4 历史分页 / 虚拟列表

- **背景**：长会话一次性拉全量消息会导致首屏慢、内存高。
- **目标**：
  - 使用 `conversation/messages` 分页/keyset cursor 增量加载历史（向上滚动加载更早消息）。
  - 列表渲染虚拟化或节流，避免大量 DOM。
- **验收**：万级消息不卡顿；滚动加载无重复、无缺口；实时增量与历史加载去重一致。

---

## 3. 可观测性

### 3.1 统一 Request ID / 错误上报

- **背景**：生产故障排查需要能串联一次请求的全链路。
- **目标**：
  - 每个 WS/HTTP 请求携带并透传 `request_id`，服务端返回、日志与错误均带上。
  - 前端把结构性错误（`AppServerError`）汇总上报到可观测后端，携带会话/操作上下文。
- **验收**：一次失败能定位到客户端请求与服务端日志；错误不携带敏感数据。

### 3.2 健康检查

- **背景**：部署与运维需要服务存活/依赖可用状态。
- **目标**：提供 `/healthz` 只读端点（进程、DB、连接状态），不触发副作用。
- **验收**：健康检查在依赖故障时返回非 2xx 与原因，不影响业务流量。

### 3.3 配额 / 限流

- **背景**：防止滥用与突发负载（尤其工作区注册、消息发送、模型调用）。
- **目标**：按 owner 对关键写操作限流（消息、删除、工作区注册），超限返回稳定、可重试的错误码。
- **验收**：突发时请求被优雅拒绝并提示稍后重试，不出现级联故障。

---

## 4. 工作区治理

### 4.1 只读 / 不可访问标识

- **背景**：注册的目录可能被删除、权限变更或变为只读。
- **目标**：
  - 服务端在每个工作区上维护可达性/只读状态，`workspace/list` 返回 `status` 与可读原因。
  - UI 对只读/不可访问工作区做视觉与交互降级（禁止新建会话或提示）。
- **验收**：目录不可访问时不会静默创建/写出失败；状态能随刷新更新。

### 4.2 重命名注册

- **背景**：当前显示名由目录 basename 派生，用户无法自定义标签。
- **目标**：允许 owner 为工作区设置自定义显示名（服务端持久化、规范化）。
- **验收**：重命名只改标签不改真实路径；跨 owner 不可见；刷新保持一致。

### 4.3 跨平台路径显示

- **背景**：Windows/Unix 路径分隔符与大小写差异，跨机迁移后路径可能失效。
- **目标**：统一路径显示与规范化（服务端 canonicalize），路径变化时给出迁移/重新注册提示。
- **验收**：同路径在不同平台被识别为同一工作区或给出明确冲突提示。

---

## 5. 模型与成本

### 5.1 token/费用按 turn 汇总

- **背景**：上下文指示器只展示最近一次实测占用，缺少累计成本视图。
- **目标**：按会话/turn 汇总 token 与估算费用，提供只读用量视图或导出。
- **验收**：汇总与 provider 上报一致；不暴露凭据与内部 ID。

### 5.2 接近上限的压缩 / 新会话建议

- **背景**：上下文接近窗口上限时回复质量下降或无响应。
- **目标**：达到阈值时提示“建议开启新会话/压缩历史”，可选自动压缩。
- **验收**：提示时机准确，不误报；触发压缩与建议不互相冲突。

### 5.3 模型健康 / 能力限制

- **背景**：provider 可能不可用或模型不支持附件/长文本。
- **目标**：在模型选择器中展示可用性与能力限制，发送前校验附件与模型兼容性。
- **验收**：不可用模型不可选；兼容性错误在发送前提示。

---

## 6. 产品质量

### 6.1 正式 i18n

- **背景**：当前为硬编码中文提示。
- **目标**：引入 i18n 框架与 `zh-CN`/`en-US` 语言包，全部用户可见文案走 key，支持运行时切换。
- **验收**：切换语言后一致性覆盖所有界面与错误；无遗漏硬编码文本。

### 6.2 Markdown / XSS 安全渲染

- **背景**：`message-text` 目前展示纯文本；需要富文本时须防注入。
- **目标**：引入安全的 Markdown 渲染（白名单标签/属性），对链接做 `rel="noopener"` 与协议净化。
- **验收**：外部内容不执行脚本；危险标签/事件属性被过滤；无障碍语义保留。

### 6.3 附件上传 / 预览

- **背景**：当前发送只支持纯文本，无文件附件。
- **目标**：支持上传（受控路径/大小/类型校验）、消息内预览与下载。
- **验收**：服务端对附件做大小与类型限制，预览不泄露路径。

### 6.4 无障碍与 E2E 回归

- **背景**：对话框、菜单、上下文指标需要完整键盘导航与屏幕阅读器支持。
- **目标**：关键路径（新建/重命名/删除/切换/发送）键盘操作完整、焦点管理正确；建立 E2E 回归（含桌面 + 移动视口）。
- **验收**：无需鼠标可完成核心流程；E2E 覆盖关键路径并通过。

---

## 7. 建议实施顺序（优先级）

1. **P0（先做）**：安全 —— 正式认证/令牌、Origin/CSP/CSRF、WS 重连与多标签协调。
2. **P0**：可观测性 —— 统一 request ID、健康检查。
3. **P1**：会话工作流 —— 历史分页/虚拟列表、消息重试/重新生成、归档/回收站。
4. **P1**：模型与成本 —— 接近上限建议、token/费用汇总。
5. **P2**：工作区治理 —— 只读/不可访问标识、重命名注册、跨平台路径。
6. **P2**：产品质量 —— i18n、Markdown 安全渲染、附件、无障碍与 E2E。

> 优先级可随真实反馈调整；每完成一项，在本节对应子项标记 `[x]` 并补充验证证据。

---

## 8. 交接与验证

- 每项实施遵循最小验证：前端 `bun run typecheck`；后端受影响 crate `cargo check -p <crate>`；行为变更补 mock/smoke 或 Rust 测试。
- 涉及数据库迁移/公共协议的项额外运行 `nomifun-db` 迁移测试与协议文档更新。
- 完成一轮后回归整体：`cargo check --workspace` + `bun run build` + `bun run smoke`。
