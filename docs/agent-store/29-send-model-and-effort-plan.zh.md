# 随调用指定模型与思考等级（`conversation/send` · `agent/run`）· 技术方案

> 状态：**✅ 已落地（2026-09-23 起草、定稿并实施）**——两条口径按拍板执行：**粘性（从本轮起生效）**，
> 且 **`agent/run` 同批纳入**；指纹 `fp-5` → **`fp-6`**。逐层改动、门禁读数、真机读数（`SM-001`–`SM-010`）
> 与 4 处实现期偏差见 §10.1；实施前评审改掉的两处（UI 不改行为 + 加 `ConversationView.reasoning_effort`、
> `with_default_model` → `with_model`）见 §5.5 / §6.2 / §9.4。
> 前置：`05-flowy-agent-store-app-server-protocol.md` §12.1（`conversation/*` 方法面）、§12.4（模型解析）、
> `27-conversation-binding-plan.zh.md`（会话绑定：技能每轮 / 专家与专家团粘性）、
> `28-webui-composer-connector-switch-plan.zh.md`（连接器＝宿主开关，本文不动它）、`web/AGENTS.md` §5（指纹与跨仓）。
> 用途：回答三件事——**(1)** 为什么这件事的语义只能是「从本轮起生效」而不是「只影响这一轮」；
> **(2)** 两个方法各自怎么接线、**顺序为什么必须是这样**；**(3)** `agent/run` 的思考等级在当下**没有载体**
> 的情况下该挂在哪（以及为什么不走 DB 迁移）。

---

## 1. 结论先行

| 目标 | 现状 | 本方案 |
|---|---|---|
| `conversation/send` 指定**模型** | 无（模型是会话属性，只能 create/update 改） | 新增可选 `model`（与会话级模型**同一形状、同一解析**） |
| `conversation/send` 指定**思考等级** | 无（同上，只能 create/update 改） | 新增可选 `reasoning_effort`（词表 `low`/`medium`/`high`/`xhigh`，同一函数校验） |
| 语义 | — | **粘性**：写进会话行，**从本轮起**生效，此后每轮沿用（§4） |
| `agent/run` 指定**模型** | 调用方无法指定：只有 preset 自带模型，或（仅在 preset 未绑定时）宿主默认回退（`lib.rs:3115-3127`） | 新增可选 `model`，优先级 = **显式 > preset 自带 > 宿主默认**（§6.2） |
| `agent/run` 指定**思考等级** | **无载体**：执行聚合没有开放袋，快照里也没有这个字段 | 落在 `ResolvedPresetSnapshot.reasoning_effort`（**JSON 列，免迁移**），由 attempt runner 投影进尝试会话的 `extra`（§6.3） |
| 运行时的生效时刻 | 模型变更**立即拆运行时**；思考等级变更**软回收**，下一个 turn 边界重建 | 沿用既有两条机制，不新增第三条（§4.1） |
| `conversation/update` / `create` | 已有 `model` + `reasoning_effort` | **不动**（本文不新增第二条写模型的路） |
| 会话当前的思考等级**读回** | `ConversationView` 有 `model`、**没有** effort ⇒ 等级是「只写不读」（设了拿不回） | `ConversationView` 新增 `reasoning_effort`（纯投影自 `extra`，§5.5）；与 `fp-6` **同批** |
| 协议指纹 | `fp-5` | **`fp-6`**（三个 DTO 加字段 ⇒ 触发，`web/AGENTS.md` §5） |
| 方法数 / 站点版本锁步 | — | **不变**：没有新增方法，`check:release-sync` 的计数不动（站点只需指纹常量 + changelog） |

**一句话**：模型与思考等级本来就是**会话行上的设置**、而且是**运行时构建期**才读的——所以
「随这条消息指定」在工程上等于「发送前先把设置切好，本轮就是新设置的第一轮」。真·一次性要做到
只能改引擎（§4.2）。

---

## 2. 边界（非目标）

- **不做真·一次性（per-turn override）**。理由见 §4.2；这是本方案最重要的边界，不是实现取舍。
- **不动 `conversation/create` / `conversation/update` 的既有形状与语义**。`update` 仍是权威 seam，
  `send` 只是**复用**它，不新增「设置模型」的第二种写法。
- **不动 `team/run` 与 `conversation/create(team_id)`**：那两条路现在把 Leader 模型硬编码为宿主默认
  （`team_run.rs:528`），本文不给它们加参数。**但要说清后果**：Leader 会话是普通会话，之后**每一轮**
  都可以用 `send` 改（粘性）；而**开会话那一刻**仍不能指定（§9.1）。
- **不做「思考等级按模型校验后报错」**。引擎在没有 catalog 白名单时会**静默丢弃**（`factory/nomi.rs:157`），
  与 `create`/`update` 现状一致；本文不借这次机会发明第三种口径（§9.3）。
- **不给团队**每个**成员**各自的模型/等级。成员模型在模板物化时冻结（§9.2），成员的思考等级本期仍是引擎默认。
- **不在 `agent/run` 上碰 `work_dir` / `steps` / mentions 等任何既有字段**。
- **不改 WebUI 选择器的行为**（§9.4）：`chooseModel` / `chooseEffort` 继续走立即 `update`。
  理由是这条改动会**静默改写用户没瞄准的会话**（选择器是全局设置，不是每会话的），不是"顺手改一处调用点"。
  **但本轮要补上读回字段**（§5.5）——否则新能力在协议上只写不读，真机验收只能绕开协议去读库。

---

## 3. 设计依据（既有事实 + 证据位置）

| 事实 | 位置 |
|---|---|
| 会话已有 `model` 与 `reasoning_effort`（create 与 update 都有） | `nomifun-app-server/src/lib.rs:3312-3356`（两个 DTO）、`:4009-4049`（update 实现） |
| 思考等级词表只有一个函数在管（`low`/`medium`/`high`/`xhigh`，空串＝不指定） | `lib.rs:3906-3929`（`normalize_reasoning_effort`）、`:4005`（`conversation/model-options` 的 `reasoning_efforts`） |
| 模型解析只有一条：已注册 UUID 直通 / config.toml provider key 幂等注册 | `lib.rs:4051-4099`（`resolve_app_server_model`）、`:4082`（无 `default_model` 时 `invalid_request`） |
| 思考等级**落库在** `conversation.extra.reasoning_effort` | `service.rs:5043-5045`（create）、`lib.rs:4029`（update 组 `extra`） |
| 运行时**构建期**读它，并灌进引擎会话 | `nomifun-api-types/src/agent_build_extra.rs:451-456`（字段来源写明是 `conversation.extra.reasoning_effort`）、`nomifun-ai-agent/src/manager/nomi/agent.rs:1169-1170`（`set_initial_reasoning_effort`） |
| **换模型** ⇒ 立即 `terminate_runtime_with_proof`（`ConfigurationChanged`） | `service.rs:6361-6375`、`:4034-4055` |
| **换思考等级** ⇒ 只打**软回收**标记 | `service.rs:6377-6391`（`request_turn_boundary_recycle`） |
| 软回收在**下一次** turn admission 拆旧建新，**后继运行时读最新落库的 extra** | `nomifun-ai-agent/src/runtime_registry.rs:950-968`；`manager/nomi/agent.rs:543-548`（注释即契约） |
| 拆运行时**不检查**是否有在跑的 turn | `runtime_registry.rs:774-798`（直接 teardown） |
| send 的准入会因「已有权威本地 turn owner」或「未证明的 durable running generation」拒绝 | `nomifun-conversation/src/service.rs:4125-4141`（两种 `Conflict`，`:4129` 是前者） |
| `update` 的既有守卫：preset/skill/MCP 快照冻结、执行尝试会话拒绝 | `service.rs:6118-6140`、`:3762-3777` |
| 忙的现成判定：`ConversationResponse.runtime.is_processing` | `lib.rs:3643`（`project_conversation` 就是这么算 `ConversationView.is_processing` 的） |
| `conversation/send` 现形（HTTP 与 WS 同形） | `lib.rs:3399-3419`、`:7168-7180` |
| `agent/run` 现状：preset 自带模型优先，**只在未绑定时**回退宿主默认 | `lib.rs:3100-3127`（`with_default_model` 保留 mention override 的注释即教训）、`:5563-5576` |
| `agent/run` 的模型最终变成执行的 `ExecutionModelPool::Single` | `nomifun-agent-execution/src/runtime_adapter.rs:251-315`（`:259-266` 校验、`:290-295` 落池） |
| `PresetSnapshot` **就是** `ResolvedPresetSnapshot`（别名，不是另一个类型） | `runtime_adapter.rs:23-24` |
| 尝试会话的 `extra` 由 `build_agent_extra` 现场构建，preset 快照的 mcp/skills 也是这样传过去的 | `nomifun-agent-execution/src/attempt_runner.rs:418-445`、`:447-466` |
| 执行聚合**没有**开放袋：只有类型化列 | `nomifun-db/src/models/agent_execution.rs:5-27`（`AgentExecutionRow`）、`:29-50`（参与者行） |
| `initial_plan_input` **不是**通用袋：它是 `InitialPlanningCommand` 的序列化 | `nomifun-agent-execution/src/engine.rs:483`（写入）、`:2611`（读回解析） |
| 引擎在没有 catalog 白名单时**静默丢弃**思考等级 | `nomifun-ai-agent/src/factory/nomi.rs:157`（`resolve_session_reasoning_effort`）、`:2601`（单测） |
| 团队 Leader 模型硬编码宿主默认；且它被当作成员的**回退模型** | `team_run.rs:523-530`、`:139-148`（`provider_model_preference`：`use_model` 优先） |
| WebUI 现在改模型/等级是**立即** `conversation/update` | `web/src/store/appStore.ts:1516-1549` |
| `ResolvedPresetSnapshot` 被会话/定时/伙伴/模板/网关共用 | `nomifun-api-types/src/preset.rs:380-406` + 各引用点（`conversation.rs:372`、`cron.rs:66`、`agent_execution_template.rs:41/87`、`companion/profile.rs:497`） |

---

## 4. 口径：为什么只能是「从本轮起生效（粘性）」

### 4.1 机制

调用方给的 `model` / `reasoning_effort` **写进会话行**，紧接着发这一条消息——这条消息就是新设置的**第一轮**：

```text
send(model=B, effort=high)
  └─ update(会话行: model=B, extra.reasoning_effort=high)
       ├─ 换模型：空闲运行时被立即拆掉（ConfigurationChanged）
       └─ 换等级：只打 recycle_after_turn 标记
  └─ send_message_with_idempotency_key(...)
       └─ 本轮 admission：若打了软回收标记 → 拆旧建新
            └─ 新运行时按**刚写好的行**构建 → 引擎拿到 B / high
```

三条必须写进正文（否则调用方一定会误读）：

1. **粘性**：它是**会话设置**被这一条消息顺带切换，此后每一轮沿用。要还原就再发一次带旧值的调用
   （或走 `conversation/update`）——**没有**「只这一轮」的模式。
2. **`conversation/get` / `list` 会反映新值**，且 `update` 会广播 `conversation.listChanged(updated)`
   （`service.rs:6404`）；WebUI 的列表/选择器因此天然跟上。
3. **它作用于本轮的运行时构建**，不是「发给模型的一条参数」。所以对**已被委派出去的成员**无效
   （成员的运行时来自它自己的尝试会话，§9.2）。

### 4.2 被拒绝的两条路

| 候选 | 为什么不做 |
|---|---|
| **真·一次性**：只影响本轮，之后自动还原 | 模型被烘进会话/引擎配置，运行时**不是**每轮新建（缓存 + 软回收）；要一次性就得「切过去 + 还原回来」= 每轮两次拆建，且要新增一条引擎级的 per-turn model 通道。收益（少一次约定）远小于成本与风险 |
| **不落库、每轮强制按参数重建** | 同上：每轮拆建；而且 `validate_conversation_model_authority`、`execution_model_pool`、`agent/run` 的 `model_pool` 这些权威口径都建立在「模型是会话/执行的属性」上，绕开它们等于新开一套模型权威 |

> 这一节同时给 `27` §4.1 的判据补一条：send 上「有载体 / 无载体」的划分标准是
> **运行时能否按轮重建**，而不是「字段好不好加」。技能有载体（`inject_skills` 本来就是每轮快照），
> 模型/等级没有（它们是构建期输入），所以后者只能是粘性切换。

---

## 5. `conversation/send`

### 5.1 协议形状（纯加法）

```json
{
  "conversation_id": "<会话 id>",
  "content": "消息正文",
  "idempotency_key": "<客户端幂等键>",
  "attachments": ["<会话工作区内的绝对路径>"],
  "mentions": [{ "kind": "skill", "id": "<skill/list 的 id>" }],
  "model": { "provider_id": "<已注册 UUID 或 config.toml 的 provider key>", "model": "<模型名>", "use_model": null },
  "reasoning_effort": "high"
}
```

- `model` 的形状 **逐字**与 `conversation/create` / `update` 相同（`ConversationModelRef`，`use_model` 可省）；
  解析走同一个 `resolve_app_server_model`，所以「config key → 幂等注册 → 规范 UUID」这套白拿。
- `reasoning_effort` 用同一个 `normalize_reasoning_effort`：空串/缺省＝不指定，非法值 `invalid_request`。
- 两个字段都是 `#[serde(default)]`；**都不传 = 与今天的 wire 形状逐字相同**（老客户端无感）。
  请求体仍是 `deny_unknown_fields`（`lib.rs:3400`），写错字段名会被拒而不是静默忽略。

### 5.2 服务端接线（**顺序是关键**）

`send_conversation_message_for_user`（`lib.rs:4690`）改为：

```text
1. content 非空校验                      # 既有
2. get_app_server_chat                   # 既有：必须先是 App Server 聊天
3. resolve_conversation_attachments      # 既有：可能 workspace_denied / invalid_request
4. send_mention_skills                   # 既有：可能 invalid_request
5. 解析 model/effort                     # 纯校验 + 幂等注册；不写库
6. 若两者都没给 ⇒ 直接跳到 9             # 零副作用路径
7. 忙判定：status==running 或 runtime.is_processing ⇒ Conflict，**不写库**
     # 两条都要判：前者覆盖 durable Running generation（未证明终止的那种，service.rs:4135），
     # 后者覆盖本地 turn owner（service.rs:4129）。宁可 fail-closed 多拒一次，
     # 也不要拆掉一个可能还在跑的运行时（runtime_registry.rs:774-798 不看这个）。
8. 差异判定：解析值与行上现值逐项比较
     ├─ 全相同 ⇒ 跳过 update（不写库、不广播）
     └─ 有差异 ⇒ service.update(user, id, { model, extra:{reasoning_effort} }, runtime_registry)
9. send_message_with_idempotency_key     # 既有
```

四条理由，缺一条都会留下坑：

- **为什么复用 `update`**：它已经是权威 seam（preset/skill/MCP 冻结、执行尝试会话拒绝、模型权威校验
  都在里面）。再写一条「send 专用、只改模型/等级」的路径，等于让模型有两个写入点——`27` 已经吃过一次
  「第二条解析路径会漂移」的教训（`lib.rs:3112-3114`）。
- **为什么忙判定必须在落库前**：`update` 换模型会**立即拆运行时**（`service.rs:6361`），而拆运行时
  **不看**有没有在跑的 turn（`runtime_registry.rs:774-798`）。若先落库再 send，而 send 的准入随后以
  `Conflict` 拒绝（`service.rs:4125-4141`），用户就同时失去「正在跑的回合」和「这条消息」。
  忙判定把这种情况挡在写库之前。
- **为什么必须有差异判定**：不然每次 send 都会写一次行 + 广播一次 `conversation.listChanged`。
  「纯加法」的实测标准是**不带新参数的调用不产生任何写与广播**，不是靠 `#[serde(default)]` 自己成立。
- **为什么其余拒绝都排在落库前**：附件与 mention 的准入是**可预期的**拒绝（用户会经常撞到），
  必须发生在切换之前；否则「消息没发出去，模型却变了」。

**关于原子性（如实记录）**：落库与 send 准入不在同一个 `preparation gate` 之下，所以理论上仍有
「切换成功、send 因**内部错误**失败」的残余窗口。要彻底消除，只能让 Conversation 层提供一个
「应用偏好 + 准一个 turn」的合并 seam（在同一个准备门内完成）——那是改动高风险的 turn 准入门，
**本期不做**，但作为将来的收敛方向登记在此。可预期拒绝已全部前置（上表 1–4、6、7），
残余窗口只剩内部错误。

### 5.3 错误码与幂等键

| 情形 | 结果 |
|---|---|
| `reasoning_effort` 不在词表 | `invalid_request`（`normalize_reasoning_effort` 原样），**不写库** |
| `model` 解析失败（config 无 `default_model` / provider 找不到） | `invalid_request` / `provider_not_found`，**不写库** |
| 会话在跑（`running` 或 `is_processing`）且带新值 | `conflict`，**不写库**（不沿用 `update` 的静默拆运行时） |
| 会话不是 App Server 聊天 / 不存在 | 既有 `not_found` |
| 附件越界 / mention 非法 | 既有 `workspace_denied` / `invalid_request`，**在切换之前** |
| 值与现值相同 | 无写、无广播，正常发送 |
| **同一个 `idempotency_key` 重放** | `update` 在 send 之前 ⇒ 值会被再写一遍（值相同，幂等）；receipt 仍是 `replayed: true`。**这条要写进 `05`**：重放不代表发生了一次新 turn，但配置确实已切换 |

### 5.4 验收

| 层 | 检查点 |
|---|---|
| 单测 1 | wire：两个字段可省（缺省＝空）、WS 同形、`deny_unknown_fields` 仍生效 |
| 单测 2 | **不带**新字段 ⇒ 不调用 `update`（fake 断言 + 行 `modified_at` 不变） |
| 单测 3 | 带 `model` ⇒ 行上模型变为新值；带 config key ⇒ 落注册后的 UUID |
| 单测 4 | 带 `reasoning_effort` ⇒ 行 `extra.reasoning_effort` 更新；非法值 `invalid_request` 且行不变 |
| 单测 5 | 值与现值相同 ⇒ 不调用 `update` |
| 单测 6 | `running` / `is_processing` 且带新值 ⇒ `conflict` 且行不变 |
| 真机 | 同一会话连续两轮用不同模型：`conversation/get` 的 `model` 与**事件流读数**都指向第二轮的新模型；等级走 `conversation/get`（§5.5）读回确认（引擎是否**真的用上**仍是 OBSERVATION：catalog 不支持时引擎静默丢弃，§9.3） |
| 命令 | `cargo test -p nomifun-app-server`；`cd web && bun run typecheck && bun run test` |

### 5.5 读回：`ConversationView` 新增 `reasoning_effort`

**问题**：`ConversationView`（`lib.rs:3438-3457`）只投影了 `model`，没有等级；`conversation/model-options`
只返回**词表**。于是 `create` / `update` / `send` 三条路都能写等级，**没有任何方法能读回会话当前用什么等级**。
后果不是"UI 不方便"，而是三件硬事：

1. **「粘性」不可观测**——调用方设完拿不回自己设的值，本方案的验收就只能绕开协议去读库；
2. 第三方 SDK 调用方无法展示/确认等级；
3. 将来那个 UI 轮次（§9.4）做「与现值不同才挂上 send」时没有数据来源。

**做法**：`ConversationView` 加 `reasoning_effort: Option<String>`，`skip_serializing_if` 缺省不上 wire；
值是 `ConversationResponse.extra.reasoning_effort` 的**纯投影**（`project_conversation`，`lib.rs:3620`），
**不新增任何状态**，也**不改变** `extra` 里没有该键时的行为（字段缺席 = 未指定）。

**为什么现在做**：它和 `fp-6` 同批只需一次文档 / 站点 / 指纹同步；留到 UI 那一轮再补，等于为**一个字段**
再走一遍指纹 + 跨仓 + 站点全套。契约上它也不是可选的：**能写不能读的"粘性设置"是残的**。

---

## 6. `agent/run`

### 6.1 协议形状（纯加法）

`AgentRunRequest`（`lib.rs:5357-5385`）新增：

```json
{
  "agent_id": "<agent/list 的 id>",
  "goal": "…",
  "model": { "provider_id": "<UUID 或 config key>", "model": "<模型名>" },
  "reasoning_effort": "high"
}
```

同一形状、同一词表、同一解析函数——**不新造词汇**。

### 6.2 模型优先级：显式 > preset 自带 > 宿主默认

现状是「preset 自带优先，**只在未绑定时**才回退宿主默认」（`lib.rs:3115-3116`）。显式参数必须
**无条件**凌驾于前两者之上，实现上不新增解析路径：

```text
apply_mentions(...)                       # 既有：agent/skill/connector mention → preset + overrides
snapshot = resolve(preset, ExecutionStep, overrides)
若给了显式 model:
    snapshot = resolve(preset, ExecutionStep, with_model(overrides, &显式))
否则若 snapshot.resolved_model.is_none():
    snapshot = resolve(..., with_model(overrides, &default_run_model()))   # 既有回退，仅换函数名
```

- `ProviderWithModel → ModelPreference` 的换算与团队侧同一规则（`use_model` 优先，
  `team_run.rs:139-148`）——显式 `use_model` 时落到 `ModelPreference.model` 的应该是**实际会被调用的**那个名字。
- **为什么必须「并进 overrides 再解析一次」，而不是直接改写 `snapshot.resolved_model`**：
  `resolve()` 拿到 `overrides.model` 时**真的会走一遍权威解析**——`preset/service.rs:344-346` 用
  `provider_id` + `model` 组一个 `required: true` 的 `ModelPreference` 交给 `resolve_model_preference`
  （provider 解析 + 语义校验 + 警告收集）。这个 helper 的职责就是**「把模型交给解析器走一遍权威校验，
  同时保住 mention overrides」**；直接打补丁会绕开这条校验。
- **因此把它改名 `with_default_model` → `with_model`**：它唯一不关心的事就是"模型从哪来"（宿主默认
  还是调用方显式给的），而本次新增的显式调用点会让旧名字**直接说谎**。范围：私有函数、同文件——
  函数定义 + 2 个调用点 + 1 个测试（`lib.rs:7944`），**无语义变化**。
  附带收益：`AgentStoreConfig::with_default_model`（`agent_store.rs:503`，编辑 `config.toml` 文本）
  已经占着这个名字、含义完全不同；同 crate 留两个同名函数是将来误改的种子。
- 函数上保留那条不变量注释：**不要**用 `PresetOverrides::default()` 重解析——那会丢掉 mention 的
  skills / connectors（`lib.rs:3112-3114` 就是为此写的）。

### 6.3 思考等级的载体：`ResolvedPresetSnapshot.reasoning_effort`（免迁移）

`agent/run` 的思考等级**当下没有载体**：执行聚合只有类型化列（`agent_execution.rs:5-27`），
没有开放袋。四个候选：

| 候选载体 | 成本 | 结论 |
|---|---|---|
| `CreateAgentExecutionRequest.reasoning_effort` + `agent_executions` 新列 | **DB 迁移**（append-only 迁移属高风险区，需先问）+ 引擎穿线 | **不选**：为一个运行偏好动迁移，性价比不成立 |
| 参与者行新列 | 同样是迁移 | 不选 |
| 挪用 `initial_plan_input` | 免迁移 | **不选**：它是 `InitialPlanningCommand` 的序列化（`engine.rs:483` / `:2611`），挪用会污染规划输入语义 |
| **`ResolvedPresetSnapshot.reasoning_effort`** | **免迁移**（参与者行的 `preset_snapshot` 本来就是 JSON 列）、与 `resolved_model` 同位同性质 | **选** |

落地要点与风险：

- 字段必须是 `#[serde(default, skip_serializing_if = "Option::is_none")]`，并且 **preset 解析永不设置它**
  ——只有 App Server 在 `agent/run` 入口设置。这样 `ResolvedPresetSnapshot` 的序列化对既有行、既有生产者
  **逐字不变**（该类型被会话/定时/伙伴/模板/网关共用，见 §3 末两行）。
- **为什么「同位同性质」成立**：`resolved_model` 已经是「解析出来的运行偏好」，团队/会话都把快照当运行配置传；
  `reasoning_effort` 放在它旁边不需要新概念。
- **`content_digest` 会变**（`runtime_adapter.rs:275` = `digest(snapshot_json)`）：这是**正确的**——
  运行配置确实变了。
- **只有 App Server `agent/run` 入口设置它**；其它路径（会话创建、团队物化、preset 管理面）保持为空
  = 引擎默认，因此本期不会顺带改变任何既有运行的行为。

### 6.4 从快照到运行时（一段线）

```text
agent/run: snapshot.reasoning_effort = Some(effort)
  └─ runtime.start_agent_run(owner, snapshot, goal, work_dir, steps)
       └─ engine.create_for_app_server(... snapshot ...)      # 快照随参与者持久化（JSON 列）
            └─ attempt_runner::build_agent_extra(...)
                 + extra["preset_id"] / ["preset_revision"] / ["preset_snapshot"]   # 既有
                 + extra["selected_mcp_server_ids"]                                  # 既有
                 + extra["reasoning_effort"] = snapshot.reasoning_effort             # 新增（一行）
                      └─ 尝试会话的 extra 就是 conversation.extra
                           └─ 运行时构建期读取（agent_build_extra.rs:451-456）       # 与普通会话同一条路
```

一个附带结论：快照属于**参与者**，所以一次运行里**每个 attempt** 都拿到同一个等级——这是我们想要的
（「这次运行用高等级」而不是「第一次 attempt 用高等级」）。

### 6.5 验收

| 层 | 检查点 |
|---|---|
| 单测 1 | wire：两个字段可省、`deny_unknown_fields` 仍生效 |
| 单测 2 | 显式 `model` ⇒ 覆盖 preset 自带模型；**mention 的 skills / connectors 不丢** |
| 单测 3 | 不给 `model` 且 preset 未绑定 ⇒ 仍走宿主默认回退（既有行为不回归） |
| 单测 4 | `ResolvedPresetSnapshot` 加字段后：既有快照的序列化**逐字不变**（`None` 不上 wire） |
| 单测 5 | `attempt_runner`：快照带 `reasoning_effort` ⇒ 尝试会话 `extra["reasoning_effort"]` 存在；不带 ⇒ 该键不存在 |
| 真机 | `agent/run` 带 `model` + `reasoning_effort`：运行的 `model_pool` 与显式模型一致；尝试会话行上 `extra.reasoning_effort` 可读 |
| 命令 | `cargo test -p nomifun-api-types -p nomifun-agent-execution -p nomifun-app-server` |

---

## 7. 指纹与跨仓同步（`fp-6`）

两个 DTO 加字段 ⇒ 触发 `web/AGENTS.md` §5。落点清单（照做，别靠记忆）：

| # | 落点 | 改什么 |
|---|---|---|
| 1 | `crates/backend/nomifun-app-server/src/lib.rs` | `PROTOCOL_VERSION` |
| 2 | `web/packages/protocol/src/protocol.ts` | `APP_SERVER_PROTOCOL_VERSION` + 头部注释链补一条（说明本次为何 bump）+ `ConversationView` 加 `reasoning_effort` |
| 3 | `web/packages/client/src/http-transport.ts` | 常量 + `conversation/send` 与 `agent/run` 的 `body` 列表各加两个字段名 |
| 4 | `web/scripts/mock-server.ts` | 常量 + 两个方法按真实契约处理新字段（不能默默接受） |
| 5 | `web/scripts/smoke.ts` | 常量 + 一条「send 带 model/effort 被接受」与「`agent/run` 带 model 被接受」的断言 |
| 6 | `web/packages/sdk/src/readiness.test.ts` | 常量 |
| 7 | `scripts/probe-agent-store-runtime.mjs` | 常量 |
| 8 | 站点仓 `content/docs/{zh-CN,en-US}/typescript-sdk.md` | §2 常量示例（中英各一处）；changelog 未发布台账 |
| 9 | `docs/agent-store/05-…protocol.md` | 头部指纹链 + §12.1 方法表/请求体 + §12.4（模型解析与优先级）+ `agent/run` 章节 |
| 10 | `docs/agent-store/README.md` | 本轮记录 + 本文件的状态行 |

- **`bun run check:fingerprint`** 会逐标识符核对 1–8（本仓 7 文件 10 处 + 站点 2 处）。
- **方法计数与版本锁步**不动：没有新增方法，`check:release-sync` 的计数真源
  （`web/packages/client/src/http-transport.test.ts` 的 `DOCUMENTED_ROUTE_SPLIT`）**不该改**；
  站点 `content/release.json` 版本也不变。
- 完成标准：旧值 `fp-5` 在本仓代码里归零（只剩历史散文）；`bun run check` 绿；
  站点仓 `bun run check:docs-sync` 报 0 drift、`bun run test:docs-sync` 通过。

---

## 8. 风险与失败模式

| 风险 | 处置 |
|---|---|
| 调用方以为「只影响这一轮」 | §4.1 三条写进正文；`05` 与 SDK 的类型注释都用「**从本轮起生效**（会话级设置）」而不是「本轮覆盖」 |
| 先切换、后 send 被拒（用户同时丢回合与消息） | 忙判定 + 其余可预期拒绝全部前置（§5.2）；残余窗口只有内部错误，如实登记 |
| 每次 send 都写行 + 广播（噪音 / 无谓 IO） | 差异判定：全相同则完全跳过 `update`；单测把「不调用 update」钉住 |
| 重放把配置又写一遍 | 幂等（同值）；**写进 `05`**：`replayed: true` ≠ 新 turn，但配置已切换 |
| 改模型拆掉正在跑的运行时 | 忙判定前置；也解释了为什么 `send` 不能像 `update` 那样裸调 |
| 思考等级被引擎静默丢弃（catalog 不支持） | 与 `create`/`update` 同口径；在 `05` 写明「不保证生效，宿主按模型能力决定」；不新增错误码 |
| 显式模型与执行模型池冲突 | 交给 `update` 既有的 `validate_conversation_model_authority` → `BadRequest`，**不吞不降级** |
| 给 `ResolvedPresetSnapshot` 加字段影响既有快照 | `skip_serializing_if` + preset 解析永不设置；单测钉住「序列化逐字不变」；`content_digest` 变化是预期并写进正文 |
| 指纹漏落点 | `bun run check:fingerprint`（机械门禁） |
| WebUI 出现两套口径（选择器立即改 vs send 携带） | §9.4：本轮**不动选择器行为**（动了会静默改写用户没瞄准的会话），只补读回字段（§5.5）；UI 独立一轮，三条前置见 §9.4 |

---

## 9. 不做的事（含现状边界）

### 9.1 团队一侧的模型/等级

`prepare_team_leader_conversation` 现在硬编码 `resolve_app_server_model(state, None)`
（`team_run.rs:528`），`team/run` 与 `conversation/create(team_id)` 都如此 ⇒ **开会话那一刻不能指定**。
Leader 会话本身是普通会话，之后每轮可以用 `send`（粘性）改。要不要给团队入口加参数是**另一个决定**：
它牵动 `ensure_team_template` 的成员回退模型（`team_run.rs:523-530`），不是纯加法。

### 9.2 成员不跟着走（必须写清）

- **成员的模型**在模板**首次物化**时冻结（Leader 模型当回退，`participant_input(..., fallback)`），
  模板复用时不重写（`team_run.rs:269-294`）⇒ **改 Leader 的模型不会改成员的模型**。
- **成员的思考等级**本期仍是引擎默认：`team/run` / `create(team_id)` 不带参数，物化出的快照里该字段为空（§6.3 最后一条）。
- 委派出去的子运行有自己的尝试会话，`send` 的粘性切换**不会传染**给它们。

### 9.3 不做「按模型校验思考等级」

引擎在 catalog 没有白名单时静默丢弃（`factory/nomi.rs:157`），`create`/`update` 今天也是这样。
本文不发明第三种口径（要么错、要么静默），但**在 `05` 写明这一事实**。

### 9.4 WebUI 选择器：本轮不改行为，独立一轮

**现状**：`chooseModel` / `chooseEffort` 是「立即 `update`」（`appStore.ts:1516-1549`）。

**为什么不能"顺手改成随下一条消息"**——那不是翻一个调用点，会**静默改写用户没瞄准的会话**：

- `selectedModelKey` / `selectedEffort` 是**全局客户端设置**（localStorage，`appStore.ts:718-719` 初始化、
  `:757-761` 落盘），**不是每会话的**。今天「选择器 == 当前会话的模型」这个等式，正是靠立即 `update` 维持的。
- 若改成随 send 携带：切到会话 B（B 的模型与选择器不同）→ 直接打字发送 → **会把 B 的模型改掉**。
  今天这个改写发生在用户**操作选择器**的那一刻（可见、有意）；改了之后会发生在 send 那一刻
  （不可见、且 B 根本不是用户改模型时的目标）。这是行为破坏，不是重构。
- 这层松动本来就存在，有证据：`turnModelKey`（`lib/model-facts.ts:124-134`）的注释写着
  「用户显式选择优先、会话自身记录的模型兜底」——选择器**本来**就不跟着当前会话走。
  今天它只让**用量归属**归错费率；改成随 send 携带会升级成归错**模型**。

**将来那一轮的三个前置条件**（缺一条就做不对）：

1. 读回：`ConversationView.reasoning_effort`（**本轮已加**，§5.5）——否则选择器无法渲染/比较会话现值；
2. 状态：选择器改成**每会话**（从会话读值，而不是全局 localStorage 键）；
3. 交互：只有与**该会话现值不同**时才挂到 send 上，并决定"已选未发"期间芯片显示什么。

在满足这三条之前，保留现状（立即 `update`）是**风险最低**的选择：它是可见的、有意的，
而"随 send 携带"在没有 1–3 的情况下会把一个显示问题变成数据问题。

---

## 10. 实施顺序（每步一个验证）

```text
 1. conversation/send：DTO（HTTP + WS 同形）+ wire 单测
    → cargo test -p nomifun-app-server
 2. send 接线：忙判定 → 差异判定 → 复用 update
    → 同上；新增「不带新字段不调用 update」「running 时 conflict 且不写库」断言
 3. agent/run：DTO + 优先级（显式 > preset > 宿主默认）
    → 同上；新增「显式覆盖 preset 且 mention 不丢」断言
 4. ResolvedPresetSnapshot.reasoning_effort + agent/run 设置 + attempt_runner 落 extra
    + ConversationView.reasoning_effort（§5.5，纯投影）
    → cargo test -p nomifun-api-types -p nomifun-agent-execution -p nomifun-app-server
    → 新增「既有快照序列化逐字不变」「extra 无该键时字段缺席」断言
 5. SDK 与夹具：ConversationSendOptions / run 参数、mock-server、smoke 断言
    → cd web && bun run typecheck && bun run test
 6. fp-6 全套落点（§7 表 1–8）
    → bun run check:fingerprint；bun run check
 7. 文档：05（头部链 + §12.1/§12.4 + `agent/run` 章节）+ README 本轮 + 站点两侧
    → 站点仓 bun run check:docs-sync / bun run test:docs-sync
 8. 真机：web/scripts/sdk-live-send-model.ts（同一会话两轮不同模型 + 等级读数）
    → AGENT_STORE_BIN=<debug 二进制> bun scripts/sdk-live-send-model.ts
```

**实施期偏差一律回写本文**（`27`/`28` 的 §9.1 就是这么用的）：本节在实施后追加
「§10.1 落地记录」表，逐层列实际改动与验证读数，并列出与方案的偏差。

### 10.1 落地记录（2026-09-23 实施）

| 层 | 实际改动 | 验证读数 |
|---|---|---|
| `conversation/send` DTO | `ConversationSendRequest` + `model` / `reasoning_effort`；`WsConversationSend` 同形；解析复用 `resolve_app_server_model` / `normalize_reasoning_effort` | `cargo test -p nomifun-app-server` → **158 passed / 0 failed**（+5 条） |
| send 接线 | 新 `apply_send_preferences`（忙判定 → 差异判定 → 复用 `conversation/update`），调用点排在附件/mention 准入**之后**、真正发送**之前**；差异判定抽成纯函数 `send_preference_changes` 以便单测钉住"零副作用" | 同上 |
| 读回 | `ConversationView.reasoning_effort` + 唯一读取口径 `conversation_extra_reasoning_effort`（空白/空串 = 未指定） | 同上 + 真机 `SM-002` |
| `agent/run` | `AgentRunRequest` + 两个字段（`skip_serializing_if`，缺席不进幂等指纹）；优先级 显式 > preset 自带 > 宿主；`with_default_model` → `with_model`；复用 `team_run::provider_model_preference`（`pub(crate)`） | 同上 |
| 等级载体 | `ResolvedPresetSnapshot.reasoning_effort`（7 处字面量 + `preset/service.rs` 显式 `None`）+ attempt runner 的 `apply_preset_run_preferences` 投影进尝试会话 `extra`（抽成纯函数以便单测） | `cargo test -p nomifun-api-types -p nomifun-agent-execution` → **692 / 77 passed** |
| TS / SDK | `protocol.ts`（`fp-6` + 注释链 + `ConversationView.reasoning_effort` + `AgentRunInput`）、`conversations.ts`（`ConversationSendOptions` 两个粘性字段）、`runs.ts`、`http-transport.ts`（两个路由 body）、`mock-server.ts`（镜像忙判定与词表校验）、`smoke.ts`（2 处常量 + 读回断言 + send 切换断言）、`readiness.test.ts`、`probe-agent-store-runtime.mjs` | `cd web && bun run typecheck` → **exit 0**；`bun run test` → **67 files / 513 tests passed**；`bun scripts/smoke.ts`（mock）→ 全绿，含新断言 |
| 站点 | `typescript-sdk` 中英 §2 常量、`send()` 两个字段与粘性说明、`runs.agent()` 显式模型/等级与优先级、`changelog` §4 台账（`fp-5` → `fp-6`） | `check:docs-sync` → **0 drift**；`test:docs-sync` → **16 pass / 0 fail** |
| 指纹 / 门禁 | `fp-6` 全部落点 + `05`（头部链 / §12.1 / §12.4 / §5.2）+ `README` 本轮 | `check:fingerprint` → 本仓 7 文件 10 处 + 站点 2 文件一致；`bun run check` → **exit 0**（`48 / 71` 计数未动）；`cargo fmt --check` → exit 0 |

**真机（`web/scripts/sdk-live-send-model.ts`，含本次改动的 debug 二进制、全新宿主）——10/10 PASS**：

| 判据 | 读数 |
|---|---|
| `SM-001.send-switches-the-model` | PASS：`mimo-v2.5` → `opencode/mimo-v2.5-free`（会话行读回） |
| `SM-002.send-reads-back-the-effort` | PASS：`null` → `xhigh` |
| `SM-003.invalid-effort-refused` | PASS：`ultra` ⇒ `invalid_request` |
| `SM-004.invalid-effort-writes-nothing` | PASS：拒绝时会话值未变 |
| `SM-005.busy-refuses-the-switch` | PASS：轮中切换 ⇒ `conflict`（首次检查 `is_processing=true`） |
| `SM-006.a-plain-send-keeps-the-conversation-settings` | PASS |
| `SM-007.agent-run-resolves-the-explicit-model` | PASS：不存在的 provider ⇒ `provider_not_found`——**证明字段不是被静默忽略** |
| `SM-008.agent-run-refuses-an-unknown-effort` | PASS：`invalid_request` |
| `SM-009.agent-run-accepts-a-model-and-an-effort` | PASS（`run_id` 返回） |
| `SM-010.attempt-conversation-carries-the-effort` | PASS：尝试会话 `extra` 里 `"reasoning_effort":"high"`——**唯一能证明等级真的走到尝试会话的读法** |

**与方案的偏差（4 处，均为实现期发现）**：

1. **`AgentRunRequest` 的新字段必须 `skip_serializing_if`**（方案 §6.1 未写）：该结构体会被
   `request_fingerprint` 序列化，缺席的字段若进 JSON 会改变**所有**既有 `agent/run` 的幂等指纹——
   一次纯升级就让既有收据失配并重跑。已加 `skip_serializing_if = "Option::is_none"`，并补一条
   单测钉住"缺席不进指纹、带上才进"。
2. **`ConversationModelRef` 补 `Serialize`**：`AgentRunRequest.model` 用它，而 `AgentRunRequest`
   要序列化，编译期强制（E0277）。纯加法。
3. **`team_run::provider_model_preference` 由私有改 `pub(crate)` 复用**，而不是在 `lib.rs` 抄一份
   `use_model` 优先的换算——这条规则只能有一处。
4. **`conversation/team_run` 的 `ConversationSendRequest` 字面量补两个 `None`**（编译期强制）；
   `team/run` 的 Leader 首轮行为逐字不变（§9.1 的边界因此有了代码注释）。

**真机脚本自身的三处修正（是脚本的错，不是代码的错；登记以免下一个人重踩）**：

1. 宿主库文件是 `flowy-backend.db`（`nomifun_common::storage_paths::DATABASE_FILE`），**不是**备份包
   内部的 `database.sqlite3`；且 WAL 模式下不能只用只读打开。
2. 尝试会话**没有** `extra.preset_snapshot`（快照是一等列），判据要用 `extra.agent_source`。
3. 本机实测 `opencode/big-pickle` 会让 attempt `Agent attempt timed out`。**用对照实验排除**了
   "这是本次改动引入的"：同一 goal 与显式 step 的四个变体——不带任何新字段 `completed`、
   **只带 `reasoning_effort` `completed`**、只带该模型 `failed`、两者都带 `failed`。因此那是模型/环境
   事实。脚本据此改为优先选 mimo 家族的另一个模型，正向运行则显式写宿主默认模型（这样 `SM-010`
   读到的是一条成功的 attempt）。

**仍未做（登记为独立一轮）**：WebUI 选择器仍是"立即 `conversation/update`"。三条前置与"为什么不能
顺手改"见 §9.4；`ConversationView.reasoning_effort`（第 1 条前置）已在本轮落地。

---

## 11. 与 `27` / `28` 的关系

- `27` 管**会话绑定**：技能**每轮**（send 的 `mentions`）、专家与专家团**粘性**（create 的
  `agent_id` / `team_id`）。本文把**模型与思考等级**明确归到**粘性**那一侧，并给出判据
  （§4.2 末：能不能按轮重建）。
- `28` 管**连接器**：宿主级开关 + 退出 `@` 提及。本文不动连接器，也不参与它的授权面。
- 三者的共同口径：**「随消息解析」只适用于运行时能按轮重建的东西**（技能正文/快照）；其余
  （模型、等级、专家身份、连接器）都是**更大作用域的设置**，只能在明确的时机切换。
