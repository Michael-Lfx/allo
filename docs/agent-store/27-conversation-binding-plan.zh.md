# 会话绑定：每轮技能 + 会话级专家 / 专家团 · 实现方案

> 状态：**📋 已立项（2026-09-23 起草，未动工）**——本文只登记范围、非目标、验收口径与实施顺序；
> 动工后的逐层读数与偏差回写 §9.1。日期按本仓台账延续（最近一轮是 `26` 的 2026-09-22）。
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
| 协议指纹现行 `fp-2` | `web/packages/protocol/src/protocol.ts:38`、`lib.rs:134`、`http-transport.ts:43` |

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
  记在 §8 待定。
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
  （备选：create 也收 `mentions`，见 §8 待定）。

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
  已绑模板决定）。这条语义**需要在实现时实测确认**（§8 待定 1）。
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
| 粘性 team 会话「首轮由谁发」不确定 | §8 待定 1：实现时实测「create 带 team_id → 用户发一条 → 是否正常委派」；不成立就把 create 的 team 变体退回 `team/run` |
| 两阶段合并提交导致一次指纹囊括两次变更 | 允许（指纹只需「与上一次不同」），但站点文档与 `05` 必须一次写全，否则下一轮对账不了 |

---

## 8. 待定与不做的事

1. **待定：粘性 team 会话的首轮语义**（§5.3）——create 不发 `goal` 时，Leader 会话的 pending 状态与
   用户首条消息的委派是否成立，必须实测。若不成立，只保留 `agent_id` 的 create 变体，团队继续走
   `team/run`。
2. **待定：`send` 用 `mentions` 还是 `skills: string[]`**（§4.1）。`mentions` 的好处是一套词汇、阶段 2
   可扩展；代价是 `agent` / `connector` 两个 kind 在本阶段「存在但必然 400」。若评审认为这个味道更糟，
   改用 `skills: string[]`，代价是协议里多一套词汇。
3. **待定：`create` 用 `agent_id` / `team_id` 还是也收 `mentions`**（§5.1）。前者与 `agent/run` /
   `team/run` 对齐；后者统一词汇但需要给 `MentionKind` 加 `team`。
4. **不做：每轮连接器**（本方案的核心取舍）。两道障碍（`extra.mcp_server_ids` 是 create 期冻结的快照，
   且 `service.rs:6089-6111` 明令不可变；MCP manager 在运行时实例构建期建立并被实例持有）。将来两条路：
   - **(a) 每轮重建运行时实例**：改动小但代价高（冷启动 + in-flight / 流式 turn 的处理）。
   - **(b) 给运行时加「本轮启用 / 停用一组 MCP server」的工具面 API**：正确但跨层——`SendMessageData`
     要加字段，且按 `AGENTS.md` 的边界必须经 `nomifun-ai-agent` 桥接，不能给 backend 直接加 `nomi-*` 依赖。
   两条都要单独立项 + 真机验证「轮 A 挂、轮 B 不挂」不被实例复用带过去。
5. **不做：每轮专家**（会话身份 create 期冻结，见 §2）。

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
    → 单测：Leader 行形状正确且无首轮；§8 待定 1 的实测
 8. 指纹 bump + 正文 + 站点（typescript-sdk / examples-sdk / changelog）
    → 全链路绿
```

### 9.1 落地记录

（未动工。实施后逐层回写：实际改动 / 验证读数 / 与方案的偏差及理由。）
