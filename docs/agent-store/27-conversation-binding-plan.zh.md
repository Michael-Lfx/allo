# 会话绑定：每轮技能 + 会话级专家 / 专家团 · 实现方案

> 状态：**🔧 阶段 1 / 2a / 2b 均已落地（2026-09-23），仅剩真机实测**——逐层改动、
> 验证读数与偏差见 §9.1。日期按本仓台账延续（最近一轮是 `26` 的 2026-09-22）。
> 前置：`05-flowy-agent-store-app-server-protocol.md` §4.7/§4.8、`07-typescript-sdk.md`、
> `16-sdk-webui-site-priority-plan.zh.md` §7 决策 3/4、`20-tool-injection-policy.zh.md`、
> `24-external-agent-skill-and-mcp-access.zh.md`、`26-connector-schema-and-grant-policy.zh.md`、
> `web/AGENTS.md` §5（指纹与跨仓同步）。
> 用途：回答「能不能在 `send` 上指定专家 / 专家团 / 技能 / 连接器」——**定下能做的两件**
> （技能每轮、专家与专家团在会话开始粘性指定），**记下不做的两件及理由**（每轮连接器、每轮专家），
> 免得下一轮再重新论证一遍。

---

## 1. 结论先行

| 目标 | 现状 | 本方案 | 阶段 |
|---|---|---|---|
| 每轮挂**技能** | 载体早已存在（`inject_skills` 是每轮载荷），只是公开面写死空 | `conversation/send` 收 `mentions`（只认 `skill`） | 阶段 1 |
| 会话开始指定**专家**（粘性） | 无入口；App Server 会话刻意 presetless | `conversation/create` 收 `agent_id` | 阶段 2 |
| 会话开始指定**专家团**（粘性） | 只有 `team/run` 会建 Leader Conversation | `conversation/create` 收 `team_id`，复用 team 编排前半段 | 阶段 2 |
| 每轮指定**连接器** | 工具面在运行时实例构建期固定 | **不做**（§2 非目标、§8 将来两条路） | — |
| 每轮指定**专家** | 会话身份在 create 期冻结 | **不做**（同上） | — |

**一句话**：「这一轮挂什么」和「这个会话是谁」是两件事——前者（技能）放 `send`，后者（专家 / 专家团）
放 `create`；连接器要「一轮一换」就得动运行时工具面，不在本轮。

---

## 2. 边界（非目标）

- **不做每轮连接器**，也**不在本阶段开放「客户端直接指定连接器」**（连接器栅栏仍只由 create 期的
  专家 / 团队绑定间接决定）。理由与将来两条路见 §8。
- **不做每轮专家**：专家的身份是 Preset（提示词 + `NOMI_RUNTIME_AGENT_ID` 绑定），中途更换等于换人。
- **不给 `conversation/update` 开绑定口**：服务层已有明文拒绝（`service.rs:6089-6111`
  「preset, skill and MCP snapshots are immutable post-creation」）。**换专家 = 新建会话**，这是本方案
  刻意保留的语义，不是欠账。
- **不做「装连接器就连带挂它的技能」**：镜像里大多数连接器自带 `SKILL.md`（见 §3 最后两行），但
  那是**安装期**的两条腿（技能进 skills 根、连接器进 MCP 行），不在本方案的绑定面里。
- 不改 `team/run` 的对外语义：它继续「建 Leader 会话 + 发 `goal` 首轮 + 返 `run_id`」。
- 不改 `conversation/send` 的附件语义、幂等语义、订阅语义。

---

## 3. 设计依据（既有事实 + 证据位置）

| 事实 | 位置 |
|---|---|
| 技能是**每轮**载荷：`SendMessageData.inject_skills` + `loaded_skill_snapshots` | `nomifun-ai-agent/src/types.rs:32-59` |
| 技能正文是**提示词文本**（贴进本轮 prompt），不带来任何工具 | 同上 `inject_loaded_skill_context`（`:70`） |
| 显式技能必须解析成不可变快照；失败早于「占用 durable receipt」 | `nomifun-conversation/src/service.rs:1513-1534`、`:9019-9023` |
| 只有 `status == "pending"`（首轮）才把 preset 的 catalog 技能并入本轮 | `service.rs:9006-9017` |
| 内部幂等指纹已包含 `inject_skills` | `service.rs:2518-2528`（`turn_delivery_request_payload`） |
| `conversation/send` 在协议层把技能写死成空 | `nomifun-app-server/src/lib.rs:4537` |
| send 请求是严格形状（`deny_unknown_fields`），WS / HTTP 各一处 | `lib.rs:6904-6913`（`WsConversationSend`）、`lib.rs:3368-3381`（`ConversationSendRequest`） |
| HTTP 路由表的 body 字段清单也要同步 | `web/packages/client/src/http-transport.ts:111-116` |
| `create` 的 WS 臂与 HTTP 臂**共用** `ConversationCreateRequest` | `lib.rs:5983-5989`（WS）、`lib.rs:3299-3315`（DTO） |
| App Server `create` 目前传**空绑定** | `lib.rs:3717-3720`（`AppServerChatBindings::default()`） |
| 绑定 seam 已存在，且 team 已在用 | `service.rs:4850-4882`（`AppServerChatBindings` / `AppServerTeamLeaderBindings`） |
| `create_app_server_chat` 硬编码 `preset_id: None`（会话刻意 presetless） | `service.rs:5040` |
| 专家解析已有实现：`agent/get` → `preset_id`、至多一个、未装即拒、与显式 agent_id 冲突即拒 | `lib.rs:5234-5263`（`apply_mentions` 的 agent 分支） |
| 专家的 preset 还要过白名单与 enabled 校验 | `lib.rs:3076-3088`（`validate_agent_store_preset_source` / `preset_disabled`） |
| 团队编排已有实现（解析成员 → ensure 模板 → 建 Leader 会话） | `nomifun-app-server/src/team_run.rs:490-589` |
| MCP 工具面在**运行时实例构建期**装载并被实例持有 | `nomifun-ai-agent/src/factory/nomi.rs:408-439`、`manager/nomi/agent.rs:151` |
| MCP 栅栏 create 期冻结在 `extra.mcp_server_ids`（工厂读它），junction 另写一份 | `service.rs:5554-5566`、`service.rs:5476-5539` |
| 会话快照 create 后不可变（含 `mcp_*` / `skills` / `preset_*`） | `service.rs:6089-6111`、`routes.rs:107-171`（open JSON 剥离） |
| WebUI 已能选技能 / 连接器 mention，但普通聊天路径把它们**丢掉**（只处理 `agent`） | `web/src/components/Composer.tsx:143-148`、`web/src/store/appStore.ts:1385-1400`、`submitTurn` 调用处 `:1430` |
| 连接器镜像自带技能：241 个条目里 **215 个**带 `SKILL.md`，结构是 `mcp.json` + `skills/<slug>/SKILL.md`（有的一条目带 2 个技能） | 本机实测 `~/.workbuddy/connectors-marketplace/connectors`（2026-09-23 扫描）；安装期两条腿分开落地见 `nomifun-app/src/app_server_installer.rs:408-458` |
| 协议指纹**现行 `fp-3`**（本轮由 `fp-2` bump） | `web/packages/protocol/src/protocol.ts:38`、`lib.rs:134`、`http-transport.ts:43` |

---

## 4. 阶段 1：`conversation/send` 每轮技能

### 4.1 协议形状

`conversation/send` 增加一个可选字段，**复用 `agent/run` 的 `MentionRef`**（`{kind, id}`，
`lib.rs:5161-5179`，TS 侧 `MentionRef` 已在 `protocol.ts:245-250`）：

```jsonc
{
  "conversation_id": "…",
  "content": "…",
  "idempotency_key": "…",
  "attachments": ["…"],                                  // 既有，可选
  "mentions": [{ "kind": "skill", "id": "release-notes" }] // 本阶段新增，可选
}
```

- **阶段 1 只接受 `kind: "skill"`**；`agent` / `connector` 一律 `invalid_request`。理由：本仓的既有口径是
  「拒绝而不是静默忽略」（`05` §5.2 对 `team/run` 的 `planning` 字段就是这么定的），让客户端知道
  「这个 kind 在 send 上还不生效」比悄悄丢掉好。
- `id` 用 `skill/list` 公布的 id（**即技能名**），不是 `install/status` 的组件 id——传组件 id 会因
  `SkillId::parse` 失败而被降级并静默不挂载（`05` §4.8 的订正）。
- 复用 `mentions` 而不是新造 `skills: string[]`：一套词汇、一处解析，且阶段 2 的 `create` 侧能沿用同一
  类型（虽然 `create` 用的是 `agent_id` / `team_id`，见 §5.1）。**备选方案**（更简单但会多一套词汇）
  记在 §8。
- 三处 DTO 落点：`WsConversationSend`、`ConversationSendRequest`（都要 `#[serde(default)]`）、
  `http-transport.ts` 的路由表 `body` 清单；另需同步 `web/scripts/mock-server.ts` 与
  `web/scripts/smoke.ts` 的 mock 分支。

### 4.2 服务端接线

1. `lib.rs:6037-6054`（`conversation/send` 臂）：把 `mentions` 交给
   `send_conversation_message_for_user`（或先在臂里做 kind 白名单校验）。
2. `send_conversation_message_for_user`（`lib.rs:4505-4547`）：现在写死
   `inject_skills: Vec::new()`（`:4537`），换成 `mentions` 里的 skill id 列表。
3. **不新增技能解析**：`service.rs` 的 `resolve_requested_skill_snapshots` 已经承担
   「canonical id → 不可变快照 / 缺失或超限即失败」的全部语义，本方案只是终于有调用方喂它。
4. **失败必须早于 receipt**：既有不变量（`:9019-9023`）——无效技能不得占用幂等键、不得把会话推成
   Running。实现时保留这个顺序，并加断言。
5. **幂等指纹无需改形状**：`turn_delivery_request_payload` 已含 `inject_skills`，所以「同键不同技能」
   天然是不同请求；反过来，不带技能的老调用 payload 逐字节不变，旧 receipt 重放不受影响。

### 4.3 客户端与 WebUI

- **TS 签名**：`conversations.ts:73` 现在是 4 个位置参数
  (`conversationId, content, idempotencyKey, attachments = []`)。建议第 4 参改为联合类型：

  ```ts
  send(id, content, key, options?: string[] | { attachments?: string[]; mentions?: MentionRef[] })
  ```

  数组形态保持向后兼容（现有调用方与 `attachments` 语义逐字不变）。
- **WebUI 接回已存在的空洞**：composer 已经收集 `composerMentions`，但只有 `kind === "agent"` 被转成
  `runs.agent`，技能 / 连接器被丢（`appStore.ts:1385-1400`）；`submitTurn` 也不收 mentions。
  本阶段把 **skill** mention 接到 `send`，**connector** mention 仍明确提示「本阶段不支持」
  （不静默丢）。

### 4.4 错误码

- kind 不在白名单 → `invalid_request`（新增的判定，措辞点名那个 kind）。
- 未知 / 超限技能 → **沿用既有 `inject_skills` 失败路径的码与语义，不新增码**；验收里钉死
  「与既有路径同码」。

### 4.5 验收

| 层 | 用例 |
|---|---|
| `nomifun-app-server` | `mentions` 缺省 → 行为逐字不变；`kind:"skill"` → 透传；`kind:"agent"/"connector"` → `invalid_request`；空 id / 非规范 id → `invalid_request` |
| `nomifun-conversation` | 带技能的首轮：技能进 `SendMessageData.inject_skills` 且快照被解析；**未知技能在写 receipt 之前失败**（断言无 receipt 行、会话未进 Running）；同幂等键 + 不同技能 → `Conflict` |
| 既有回归 | 首轮把 preset catalog 技能并入的既有行为不变（`:9006-9017`） |
| web | `cd web && bun run typecheck && bun run test`；`mock-server.ts` 支持新字段；`smoke.ts` 加一条断言 |
| 指纹 | `bun run check:fingerprint` 绿（见 §6） |

---

## 5. 阶段 2：`conversation/create` 会话开始指定专家 / 专家团（粘性）

### 5.1 协议形状

`ConversationCreateRequest`（WS / HTTP 共用，`lib.rs:3299-3315`）增加两个**互斥**的可选字段：

```jsonc
{
  "name": "…",
  "model": { "provider_id": "…", "model": "…" },
  "workspace": { "id": "…" },
  "reasoning_effort": "high",
  "agent_id": "wb-demo-software-architect",   // 新：专家（AgentDefinition 的 agent/list id）
  "team_id":  "…"                            // 新：专家团（team/list id）
}
```

- 两个都给 → `invalid_request`；都不给 → 现行行为（presetless 普通会话）逐字不变。
- 命名随 `agent/run` / `team/run` 的既有词汇（`agent_id` / `team_id`），**不用** `mentions`：send 上的
  mention 是「这一轮挂什么目录产物」，create 上的 id 是「这个会话是谁」——两件事用两套名字更清楚
  （备选：create 也收 `mentions`，见 §8）。

### 5.2 专家（`agent_id`）

1. 解析复用 `apply_mentions` 的 agent 分支语义：`agent/get(agent_id)` → `preset_id`；未安装 →
   `agent_not_installed`；被 disable → `preset_disabled`；来源白名单 → `validate_agent_store_preset_source`
   （`lib.rs:3076-3088`、`5234-5263`）。区别只在**没有冲突对象**（create 不接受显式 preset）。
2. 把解析出的 preset **冻进会话行**：`create_app_server_chat`（`service.rs:4985-5050`）现在把
   `preset_id` 传成 `None`（`:5040`），改成 `Option<String>` 并由调用方给出。preset 的
   `preset_snapshot` / `preset_revision` 由既有的 `project_preset_runtime_context`
   （`build_runtime_options` 内，`:14233`）在每次构建运行时投影，因此**不需要新的运行时通路**。
3. **连带效应（要写进 `05`）**：专家的 preset 自带技能与连接器引用，因此「指定专家」会一并冻结它的
   技能与连接器栅栏——这正是「会话开始粘性指定」的应有之义，但必须写明：**换专家只能新建会话**
   （PATCH 被 `service.rs:6089-6111` 拒绝）。
4. 模型：`create` 现有 `resolve_app_server_model`（`lib.rs:3702-3706`）保持优先；专家 preset 未绑模型时
   沿用既有的 owner 默认模型回退语义（`agent/run` 的 `default_run_model` 同源），不发明第二套。

### 5.3 专家团（`team_id`）

把 `execute_team_run`（`team_run.rs:490-589`）的前 3 步抽成一个函数：

```rust
async fn prepare_team_leader_conversation(
    state, user, team_id, team_version, workspace,
) -> Result<(ConversationResponse /* leader */, ProviderWithModel), AppServerError>
// 内部：resolve_team_members → team_connector_fence → resolve_app_server_model
//       → ensure_team_template → create_app_server_team_leader_chat
```

- `team/run` 改成「调它 → 发 `goal` 首轮 → 反查 `lead` execution link → 返 receipt」，行为与
  错误码**逐字不变**（`team_run_not_started` 等既有码保持）。
- `conversation/create` 带 `team_id` 时只调它、**不发首轮 goal**：返回的 Leader 会话处于 pending，
  用户自己的第一条 `conversation/send` 就是 Leader 的首轮（委派由 `delegation_policy=automatic` +
  已绑模板决定）。这条语义**需要在实现时实测确认**（§8 第 3 条）。
- 成员的 `agent_not_installed` / `agent_disabled` / `connector_unavailable` /
  `team_member_model_unbound` 校验时机不变（每次调用都查，`team_run.rs:509-519` 的既有决定）。

### 5.4 验收

| 层 | 用例 |
|---|---|
| `nomifun-app-server` | 不带新字段 → 现行行为不变；`agent_id` 未装 / disabled / 非白名单来源 → 对应码；`agent_id` + `team_id` 同时给 → `invalid_request` |
| `nomifun-conversation` | 带 `agent_id` 的会话行 `preset_id` 非空且 `preset_snapshot` 在运行时投影可见；`extra.mcp_server_ids` / `extra.skills` 按该专家冻结；PATCH 改这些键仍被拒 |
| team 回归 | `team/run` 的既有测试**全绿且不修改**（这是抽取是否成功的第一判据）；带 `team_id` 的 create 产出 Leader 行（`execution_template_id` + `delegation_policy=automatic`）且**不发首轮** |
| 真实链路 | 一次 live：create 带 `agent_id` → send 两轮 → 两轮都在专家 preset 下（读运行时日志的 preset/agent 名） |

---

## 6. 指纹与跨仓同步

- 两次 wire 变更（阶段 1 的 send DTO、阶段 2 的 create DTO）**都是「现有 DTO 加字段」= 明文触发项**，
  各自 bump 一次指纹：现行 `fp-2` → 阶段 1 取 `fp-3` → 阶段 2 取 `fp-4`。
  **`fp-<n>` 是计数器，不得复用历史值**；若两阶段合成一次提交，则只取 `fp-3`。
- 每个阶段按 `web/AGENTS.md` §5 走：改两个常量 → **旧值全仓 grep** → `bun run check:fingerprint`
  （现行落点：本仓 7 文件 10 处 + 站点 2 处；新增落点要进 `MIRRORS` / `SITE_MIRRORS`）。
- 正文：`05`（头部指纹 + `§4.7 send` / `§4.9 create` 对应小节）、`README.md` 的本轮记录。
- 跨仓 `C:\workspace\agent-store-site`：`content/docs/{zh-CN,en-US}/typescript-sdk.md` 的 §2 常量示例
  与 `conversations` / 新 create 参数说明（**中英结构必须一致**）、`examples-sdk.md` §7 补「每轮挂技能」
  与「以专家身份开会话」两个配方、`changelog` §4 未发布台账。方法计数**不变**（没有增删方法），
  所以 `DOCUMENTED_ROUTE_SPLIT` / `check:release-sync` 不应报差异——若报差异，说明改了不该改的东西。
- 完成标准：`bun run check:fingerprint` 绿；`cargo test -p nomifun-app-server -p nomifun-conversation`
  绿；`cd web && bun run typecheck && bun run test` 绿；站点 `bun run check:docs-sync` 报 `0 drift`、
  `bun run test:docs-sync` 通过；`bun run check` 整体绿。

---

## 7. 风险与失败模式

| 风险 | 处置 |
|---|---|
| 每轮技能被误当成**工具授权** | 正文（`05` + 站点）写明：技能是提示词，工具面由连接器决定；只挂技能 = 模型知道怎么做但手里没工具 |
| 技能快照把 prompt 撑爆 | 既有上限与失败语义（单文件上限、超限即整次 send 失败）沿用，不静默截断；正文提示上下文占用 |
| 客户端以为连接器 / 专家能在 send 上换 | kind 白名单**显式拒绝** + 错误措辞点名；站点文档写「换专家 = 新建会话」 |
| 阶段 2 抽取破坏 `team/run` | 抽取的唯一验收线是 `team_run` 既有测试**不改而全绿**；行为与错误码逐字不变 |
| 粘性 team 会话「首轮由谁发」不确定 | §8 第 3 条：实现时实测「create 带 team_id → 用户发一条 → 是否正常委派」；不成立就把 create 的 team 变体退回 `team/run` |
| 两阶段合并提交导致一次指纹囊括两次变更 | 允许（指纹只需「与上一次不同」），但站点文档与 `05` 必须一次写全，否则下一轮对账不了 |

---

## 8. 已定与不做的事（三条待定已于 2026-09-23 拍板）

| 原待定 | 决定 | 落点 |
|---|---|---|
| `send` 用 `mentions` 还是 `skills: string[]` | **复用 `mentions: MentionRef[]`**，阶段 1 只认 `skill`；另外两类显式 `invalid_request` | §4.1 / §12.2（`05`） |
| `create` 用 `agent_id` / `team_id` 还是也收 `mentions` | **`agent_id` + `team_id`，互斥**；不给 `MentionKind` 加 `team` | §5.1 |
| 粘性 team 会话的首轮语义 | **先不做 `team_id`**：阶段 2a 只上 `agent_id`，team 粘性等实测「create 不发 goal 首轮」成立后再上 | §5.3 / §9 |

1. **不做：每轮连接器**（本方案的核心取舍）。两道障碍（`extra.mcp_server_ids` 是 create 期冻结的快照，
   且 `service.rs:6089-6111` 明令不可变；MCP manager 在运行时实例构建期建立并被实例持有）。将来两条路：
   - **(a) 每轮重建运行时实例**：改动小但代价高（冷启动 + in-flight / 流式 turn 的处理）。
   - **(b) 给运行时加「本轮启用 / 停用一组 MCP server」的工具面 API**：正确但跨层——`SendMessageData`
     要加字段，且按 `AGENTS.md` 的边界必须经 `nomifun-ai-agent` 桥接，不能给 backend 直接加 `nomi-*` 依赖。
   两条都要单独立项 + 真机验证「轮 A 挂、轮 B 不挂」不被实例复用带过去。
2. **不做：每轮专家**（会话身份 create 期冻结，见 §2）。
3. **仍待实测：粘性 team 会话的首轮语义**（§5.3）——create 不发 `goal` 时，Leader 会话的 pending 状态与
   用户首条消息的委派是否成立。若不成立，只保留 `agent_id` 的 create 变体，团队继续走 `team/run`。

---

## 9. 实施顺序与进度

```text
阶段 1（每轮技能）
 1. 协议：WsConversationSend / ConversationSendRequest / 路由表 body / mock / smoke
    → cargo check -p nomifun-app-server；cd web && bun run typecheck
 2. 服务端：send 臂透传 + send_conversation_message_for_user 替换 Vec::new()
    → 单测：kind 白名单 / 未知技能早失败 / 幂等冲突
 3. 指纹 fp-2 → fp-3 + 正文（05 / README）+ 站点两语言
    → bun run check:fingerprint；站点 check:docs-sync = 0 drift
 4. TS 客户端 4 参联合类型 + WebUI 接回 skill mention（connector 明确提示不支持）
    → cd web && bun run typecheck && bun run test

阶段 2（会话级专家 / 专家团，可合并进同一次指纹）
 5. 抽取 prepare_team_leader_conversation()，team/run 改为调用它
    → team_run 既有测试不改而全绿（抽取的唯一判据）
 6. create DTO + agent_id 解析（复用 apply_mentions 语义）+ create_app_server_chat 的 preset 参数
    → 单测：冻结可见 / PATCH 仍被拒 / 互斥校验
 7. create DTO + team_id（只调第 5 步、不发首轮）
    → 单测：Leader 行形状正确且无首轮；§8 第 3 条的实测
 8. 指纹 bump + 正文 + 站点（typescript-sdk / examples-sdk / changelog）
    → 全链路绿
```

### 9.1 落地记录（2026-09-23：阶段 1 + 阶段 2a + 阶段 2b 均已落地；仅剩真机实测）

| 层 | 实际改动 | 验证读数 |
|---|---|---|
| 协议（阶段 1） | `ConversationSendRequest` / `WsConversationSend` 各加 `#[serde(default)] mentions: Vec<MentionRef>`；WS 臂透传 | `cargo test -p nomifun-app-server --lib mention` → **7 passed / 0 failed**（含本批 2 例新增） |
| 准入（阶段 1） | 新增 `send_mention_skills`：只认 `skill`（去重、空 id 拒绝），`agent` / `connector` → `invalid_request`（点名 kind）；`send_conversation_message_for_user` 用它替换 `inject_skills: Vec::new()` | 同上 |
| 连带 | `team_run.rs` 的 Leader 首轮显式 `mentions: Vec::new()`（它是 Team 的 `goal`，不是每轮技能选择） | 编译期强制，不补会 `E0063` |
| TS（阶段 1） | `ConversationSendOptions { attachments?, mentions? }`；`send()` 第 4 参改为 `string[] \| ConversationSendOptions`（旧数组形态保持）；路由表 `body` 补 `mentions` | `cd web && bun run typecheck` → **exit 0** |
| WebUI（阶段 1） | `submitTurn` 收 `mentions` 并透传给 `conversations.send`；聊天发送路径把 skill mention 传下去、**回执后才清空**（失败保留，与 R15 附件同口径） | 同上 |
| 协议（阶段 2a） | `ConversationCreateRequest` 加 `#[serde(default)] agent_id: Option<String>`；`docs/agent-store/05` §12.2 新增规格 | `cargo test -p nomifun-app-server` → **151 passed / 0 failed** |
| 解析与冻结（阶段 2a） | 新增 `app_server_chat_bindings_for_agent`（`agent/get` → `preset_id` → 来源白名单 / `preset_disabled` / `runtime_unavailable`；定义自带的技能与连接器作为 **resolve overrides** 进快照）与 `definition_connector_fence`（停用 ⇒ `connector_unavailable`，非法 id ⇒ `internal_error`，重复声明去重） | 新增 4 例：wire / 空绑定 / 点名拒绝 / 连接器栅栏，全绿 |
| 会话 seam（阶段 2a） | `AppServerChatBindings` 加 `preset_snapshot: Option<ResolvedPresetSnapshot>`；`create_app_server_chat` 在**有快照时走 `create_from_preset_snapshot`**，并把**本宿主 auto-inject 名单并进快照的 `excluded_auto_skills`**——否则 `create` 会用（空的）preset 值覆盖 App Server 的技能栅栏，宿主自动技能就漏进这个会话 | 新增 `app_server_expert_chat_freezes_the_snapshot_and_keeps_the_auto_inject_fence`：断言 `preset_id` / `preset_revision` / `preset_snapshot` 三列冻结，且 `extra.skills == ["bound-skill"]`（`host-auto-skill` 没漏进来） |
| 编排抽取（阶段 2b） | `team_run.rs`：`resolve_team_members` 改收 `(team_id, team_version)`；新增 `PreparedTeamLeader` + `prepare_team_leader_conversation`（`require_engine` → 成员与连接器校验 → 工作区/模型解析 → 物化或复用模板 → 建 Leader 会话）；`execute_team_run` 只保留「发 `goal` 首轮 + 反查 `lead` execution + 返回 receipt」 | `cargo test -p nomifun-app-server` → **151 passed / 0 failed**（`team/run` 的既有测试未改一字） |
| 协议（阶段 2b） | `ConversationCreateRequest` 加 `#[serde(default)] team_id: Option<String>`；`create_conversation_for_user` 先做**互斥校验**（`agent_id` + `team_id` ⇒ `invalid_request`，发生在读工作区/目录之前），再分派到 `prepare_team_leader_conversation`（**不发首轮**）；`05` §12.2 扩成「以专家 / 专家团开场」 | 新增 2 例：wire 接受 `team_id`、两者同时给 ⇒ `invalid_request`；`conversation_create` 过滤下 **5 passed** |
| 指纹 | `fp-2` → **`fp-3`**（阶段 1）→ **`fp-4`**（2a）→ **`fp-5`**（2b），本仓 7 文件 10 处 + 站点 2 处，常量注释逐版写明各自承载什么 | `bun run check:fingerprint` → `✓ "fp-5" consistent across 10 landing point(s) in 7 file(s) here and 2 file(s) in the docs site` |
| 夹具 | `mock-server.ts` 与真宿主同口径：非 `skill` 的 mention 回 `invalid_request`；`smoke.ts` 增一条「拒绝不能兑现的 mention kind」断言；`appStore.attachments.test.ts` 的假客户端改读选项对象，并新增「技能随轮发出 + 回执后清空 / 失败保留」两例 | `cd web && bun run test` → **67 文件通过（1 skipped）/ 513 例通过（1 skipped）** |
| 正文 | `05` 头部指纹 + §12.1 请求体（send/create 各加字段）+ §12.2（以专家开场）+ §12.3（每轮技能）+ §12 子节重编号；站点 `typescript-sdk`（中英 §3.3 send/create 签名与说明）、`changelog` §4 台账（fp-3 + fp-4） | 站点 `check:docs-sync` → **0 drift**；`test:docs-sync` → **16 tests / 0 fail** |

**与方案的偏差（3 处，均为实现期发现）**：

1. **站点指纹值在我动手前已是 `fp-3`**（`content/docs/{zh-CN,en-US}/typescript-sdk.md` 的常量表行）——
   阶段 1 因此一次通过门禁。也就是说站点正文**提前**占用了 `fp-3`；本轮把它的含义补齐为「每轮技能」，
   阶段 2a 则按规则取新值 **`fp-4`**（不复用历史值）。
2. **`attachments` 的文档缺口顺带补上**：站点 §3.3 此前只写 `send(id, content, idempotencyKey)`，既没有
   第 4 个参数、也没提附件（`fp-3` 之前就存在的偏差）。本轮两语言一并补成 `send(id, content, key, options?)`。
3. **宿主 auto-inject 的排除只能由会话 seam 补**：`PresetOverrides` **没有** `exclude_auto_inject_skills`
   这一项（`exclude_skills` 只从 `included_skills` 里做减法），而 `create_inner` 会用快照的
   `excluded_auto_skills` 覆盖 App Server 写的那条栅栏。所以设计落在「App Server 解析快照 + seam 把
   宿主 auto-inject 名单并进快照」——这是本阶段唯一一处需要动 `nomifun-conversation` 的地方，
   且只在**有快照**时生效，普通会话路径逐字不变。

**真机实测（2026-09-23，脚本：`web/scripts/sdk-live-team-leader.ts`）**

环境：含本次改动的 debug 二进制（`target/debug/agent-store.exe`），由 SDK `launchClient` 起宿主
（临时 data-dir；模型来自 `~/.agent-store/config.toml` 的 `mimo/mimo-v2.5`）。
**官方默认市场里没有专家团**（实测 490 条 = 262 skill + 228 connector，`team = 0`），所以脚本回退到
仓库夹具 `software-company`，经 `import/run` + `install/run` 安装（**0 warning / 0 error**，
`team/list` → `wb-software-company-team`）。

| # | 观测 | 读数 |
|---|---|---|
| 1 | 团绑定的连接器 | 夹具的 `.mcp.json` 装出来的 MCP 行默认 `enabled = false`，`create({ teamId })` **正确拒绝** `connector_unavailable`（与 `team/run` 同一道栅栏，真机首次验证）；用第一方 `POST /api/mcp/servers/:id/toggle` 启用后通过 |
| 2 | `create({ teamId })` | **成功**：Leader 会话（`name = "<team> (leader)"`、`status = pending`、模型已解析、workspace 已绑），**没有**发出任何首轮 |
| 3 | 客户端首轮 | **被受理并真的开始跑**（事件里有 `message.activity` / `message.tool` / `message.delta`） |
| 4 | 委派工具是否注册 | **注册且在位**：明确点名委派的指令下，Leader 先 `ToolSearch("nomi_delegate")` 找到它，再调用并拿到 `execution_id`（回执文本：`Delegated work was accepted and the host is planning it.`） |
| 5 | 自然语言下是否**自发**委派 | **不确定，由模型决定**：6 次里 2 次委派成功（拿到 `execution_id`，宿主进入 planning），4 次自己动手做（十几次工具调用，其中一次 3 分钟预算内没到终态）。**同条件下既有入口 `team/run` 也出现过 `team_run_not_started`** ⇒ 这是模型行为，不是 2b 路径的缺陷 |
| 6 | 同一会话的并发边界（新发现） | 上一轮还没结束时再发消息会被拒：委派被拒是 `Conflict: conversation already has an unfinished Agent Execution`，普通发送被拒是 `Conflict: Conversation already has an authoritative local turn owner`（脚本会先 `cancel` 再发对照轮） |
| 7 | 环境噪声 | 其中一轮出现一次**可重试**的 `USER_LLM_PROVIDER_NETWORK_ERROR`（模型侧网络），与本次改动无关 |

**结论**：阶段 2b **不需要回退**——能力具备且真机可用。但**不能**把「打开 Leader 会话后由客户端发首轮」
理解成"必然会委派"：委派与否取决于模型（`team/run` 同样如此）。需要委派时，指令里应明确点名
`nomi_delegate`；脚本与 `05` §12.2 都已按这个口径写。

**判定口径本身的两处修正（记下来，免得下次再踩）**：脚本起初把「自然语言首轮出现委派证据」和
「与 `team/run` 结论一致」当判据——两者都**假设了确定性**，实测会把模型行为误判成路径缺陷；
最终判据改为「会话建得出来 / 首轮被受理并开始跑 / 明确点名时委派工具确实可被触发」，
自发委派与 `team/run` 对照只作为 `OBSERVATION` 打印。

**顺带查清的三条既有口径（都不是本次改动引入，登记以免下次重查）**——第二个真机脚本
`web/scripts/sdk-live-mention-agent.ts`（验的是 WebUI 现在的 `@专家` 调用形状）：

- **`@专家` 真机生效**：`runs.agent({ agentId: "", goal, mentions: [{ kind: "agent", id }] })` 返回
  `run_id`（`status: planning`、`preset_revision: 1`）。它此前有一条**顺序依赖**：`agent/run` 的默认模型
  回退 `default_run_model` **只读宿主 DB 的 provider 注册表**，而 provider 是**按需注册**的
  （`resolve_app_server_model` 读 `~/.agent-store/config.toml` 后写库），于是「全新宿主、还没解析过任何
  模型」时会以 `invalid_request`（`resolved_model is required`）失败——真机复现过。
  **→ 已修（2026-09-23，同一批）**：`default_run_model` 现在**先**取 config 的 `default_model`
  （与会话 / 团同源，`ensure_agent_store_provider` 按需注册），取不到才回退到注册表里第一个启用的
  provider/model；无 config 文件的宿主行为不变（走原来的注册表回退）。这是**运行期解析口径**的修正，
  不动任何 DTO / 字段，故**不 bump 指纹**。真机验收见 `sdk-live-mention-agent.ts` 的
  `MA-001.fresh-host-agent-run`（全新宿主上第一次调用即可解析出模型）。
- **专家 id 的错误码有两种**：id **不存在** ⇒ `not_found`；id 存在但**未 install/*** ⇒ `agent_not_installed`。
  文档此前只写了后者，已按此订正（`05` §4.8 / §12.2）。
- **「`@专家团`」在协议层不存在**：`MentionKind` 只有 `agent / skill / connector`，硬塞 `kind: "team"` 被
  `deny_unknown_fields` 直接拒（真机 `invalid_request`）；UI 的 mention 面板也没有团。
