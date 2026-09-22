# Turn-tail `[Context]` 注入：现状记录与待验证项

> 状态：**部分处置已完成**（2026-09-22 记录；plan/goal 注入已迁出，见 §7）。
> 这不是一份方案，是一份**现状与证据的登记**——
> 机制已读准，P2 已消除，其余处置仍未定，且**本机仍未采集到生产基线**。
> 前置：`docs/architecture/agent-engine.zh.md`、`docs/agent-store/20-tool-injection-policy.zh.md`。
> 用途：回答「turn-tail `[Context]` 目前是什么问题」。**改动前请先读 §4**：现在的判断仍是
> 假设，先量再改。

---

## 1. 机制（读代码确认，非推测）

| 事实 | 位置 |
|---|---|
| `turn_tail_extras` **每回合**组装一次（外层 `loop {` 在 `:1525`，`let mut turn = 0` 在 `:1522`） | `crates/agent/nomi-agent/src/engine/mod.rs:1604-1678` |
| 第一条**无条件**是 `Current date: %Y-%m-%d` | 同上 `:1605-1608` |
| 其余依次是：plan 模式指令（`:1611-1613`）、coding harness 的 plan nudge / `forced_finalize_instruction` / `turn_tail`（`:1614-1640`）、注册的 `ContextContributor`（RAG/memory，`:1644-1648`）、goal 的 `turn_context()`（`:1649-1651`）、round ledger section（`:1676-1678`） | 同上 |
| 用空行拼成**一个**字符串，前缀 `[Context]\n` | `context_contributor.rs:56-73`（拼）、`:103-105`（标签） |
| **在 `'provider_attempt: loop`（`:1696`）里注入**，即**每个 provider pass 一次**（`turn_tail` 是每回合建、每 pass `clone`） | `engine/mod.rs:1697-1700` |
| 注入方式：最后一条消息是 `Role::User` → 在**位置 0** 插一个 `Text` 块（**包括纯 tool-result 的 user 消息**）；否则追加一条新 `Role::User` | `context_contributor.rs:107-115` |
| 注入结果**不写回** `self.messages`（局部变量），所以历史里不累积副本 | `engine/mod.rs:1697-1700` |
| 设计目的：动态内容不进 system prompt，让 system prompt 逐字节稳定，保 DeepSeek 前缀缓存 | `context_contributor.rs:1-14` |

---

## 2. 问题（按与「agent 重复思考」的相关度排）

**P1 最新的 user 位置经常只有非指令文本 —— 主因。**
带工具的一轮里，最新 user 消息就是**工具结果**（本管线把 tool result 建模成
`Role::User` + `ToolResult` 块，见 `context_contributor.rs:178-197`）。注入把它变成
`[Text("[Context]…"), ToolResult…]`，于是 prompt 最高显著位置上的最新人类可读文本是
「Current date: …」。每到一次工具结果就再来一遍。模型被反复递上「末尾有新内容、且不是
指令」的信号，容易被当成新线索去重读、复述。

**这条失败模式仓库自己撞过并写下来了**：`context_contributor.rs:79-80` —— 多出一条连续的
`User [Context]` 消息「**confuses coding agents into another exploration loop**」。现在的
形态只是它的弱化版（同一条消息内前插，而不是新增一条）。

**P2 一个块混了三类东西，标签只说「这是上下文」。**
`Current date`（**数据**）、`plan_mode_instructions()` / plan nudge /
`forced_finalize_instruction` / goal `turn_context()`（**指令**）、RAG/memory 与 round ledger
（**证据**）全被贴成 `[Context]`。标签告诉模型「这不是用户说的」，内容里却有「去做 X /
别做 Y」。plan 模式下这一段实质上是一份**第二 system prompt**，还很长，贴在用户位上。

**P3 同一回合每个 pass 重复注入。**
`turn_tail` 每回合建一次、每 pass 注入一次（`:1696-1700`）。历史不累积（P1 表格末行），
但**每个 pass 的末尾都会重新出现同一段**：缓存稳定只要求「tail 挂在最后一条消息上」，
并不要求「每个 pass 都重贴一遍」。

**P4 `date` 无条件注入 ⇒ 零贡献快速通道在引擎里永不发生。**
`:1605-1608` 无条件 push，于是 `build_turn_tail_context` 永远返回 `Some`，
`inject_turn_tail_context` 的 `None`/空串早退路径（`:95-101`）在引擎里**从不触发**。
文件头那句「with no contributors registered, the messages are returned unchanged」
（`:13-14`）只对**纯函数**成立，对引擎不成立：每个会话、每一轮、每个 pass 都带这段 tail。

**P5 一个名字与行为不符的测试（潜在陷阱，不是活 bug）。**
`inject_appends_new_message_when_last_is_tool_result`（`context_contributor.rs:178`）——名字
说「追加一条新消息」，断言却是 `out.len() == 1`（**不**追加）并注明「tool-result user
message should stay a single message」。谁按名字去「修」实现，就会把 P1 里那个已被记录的
反模式复活。

---

## 3. 为什么现在不改

1. **用户决定**（2026-09-22）：先记录，不动代码。
2. **因还没有被测量**。§2 五条都是**读代码能钉死的机制**；但「P1 就是你观察到的重复思考的
   因」仍是**假设**——症状与形状吻合、且仓库记录过同族失败，这足以立案，不足以动机制。
3. `turn_tail` 牵扯**前缀缓存**与每轮上下文，改动面在引擎主循环上；先动它会同时改掉
   「缓存是否还热」和「模型看到什么」两件事，出问题时无法归因。

---

## 4. 待做的测量（改任何东西之前）

**步骤**：在「问候」那个会话里发一句**「继续」**，然后按
`GET /api/app-server/conversations/{id}/messages` 取回该轮的消息，对齐两件事：

| 观测 | 判据 |
|---|---|
| 每回合的**思考段数** | 同一轮里出现 N 段 `thinking`、且内容高度重叠 → 复述成立 |
| **tail 注入位置**与段边界的关系 | 每个 `tool_call` 之后都跟着新的 `[Context]` 位置 → P1/P3 成立 |
| tail 文本在该轮内是否**逐字相同** | 相同 → 问题在「重复出现」，不在「内容每轮变」 |

**如果复述不成立**（例如只是模型自身策略、或工具结果本身触发的），那么本文的 P1 假设被证伪，
应当先去查别的因，而不是改注入。

---

## 5. 候选处置与建议顺序

| 选项 | 判定 | 理由 |
|---|---|---|
| **A** 改措辞：明确「这是环境信息，不是用户的新指令，不要据此复述」 | **确认后先做这个** | 最便宜、**正对 P1 的因**（无指令的 user 位置）；即使假设不完全对，这句话本身也是对的；可加测试钉住那段声明 |
| **B** `Current date` 只在回合开始时注入一次、动态 extras 仍每 pass | **A 之后再看** | 它是**另一个杠杆**：改善 cache 命中与 tail 抖动（P3/P4），**不消除**「无指令的 user 位置」 |
| **C** 引擎层每回合护栏（`round.ledger.replace_plan` 接缝；目前没有每轮 `update_plan` 上限） | **单独排期，不当本 bug 的解** | 它是**兜底不是病因修复**，动的面最大。任何循环都该有预算，这值得做——但拿它当复述问题的解会掩盖真正的因 |

一句话：**A 治因，B 降噪，C 是保险；先量，再按这个顺序。**

---

## 6. 我没验证、也不打算声称的点

把 `Text` 块插在 `ToolResult` 块**之前**，对某些 provider 的 tool_result 配对规则是否合规。
生产上没出 API 错，所以大概率没事，但这条只有推理没有证据；它影响的是「会不会报错」，
不影响 P1 的「模型读到什么」。**要动 §5 的 A/B 之前，这一条应当顺手确认一次。**

---

## 7. 已完成的处置：plan/goal 指令迁出 `[Context]`

`docs/architecture/plan-goal-feature-seam.zh.md` 已实施（Phase 1–5）。其中与本文件直接
相关的部分：

- **P2 就此消除。** plan 与 goal 的指令/状态不再混进 `[Context]` 块，改走
  `<system-reminder>` 持久 user 消息（`nomi-agent` 的 `features/reminder/`）。
  信封文案明确声明「这是环境/状态信息，不是用户的新指令」（吸收 §5-A 的措辞），
  且信封不以 `[Context]` 开头，因此不会被 truncation-restart 的两个谓词误判。
- **P3 部分缓解。** reminder 同一回合内去重（相同文本只发一次），plan 每 8 个
  provider pass 刷新一次、goal 每 12 个刷新一次；不再每个 pass 重贴。
- **P4 不受影响。** `Current date` 仍无条件注入 turn tail，`build_turn_tail_context`
  仍恒返回 `Some`。
- **P1 的机制面减少但未消失。** 最高显著位置上不再出现「长指令块」，但
  `Current date` 依旧落在那里——§5-A/B 对 date/ledger/contributor 的处置**仍待各自排期**。

### 基线采集状态：**未采集**

按 §4 的步骤在本机重放不可行，因此本文件的 P1 假设**至今未被生产数据验证**：

- 本地 session store（`%LOCALAPPDATA%\nomifun\nomi-sessions`）只有 5 个会话，
  最后写入时间均为 2026-08-10 / 08-12；
- turn-tail 持久化机制于 2026-08-28 才落地（`f17af9963`），晚于这些会话；
- 5 个文件的原始 JSON 中 `[Context]` 出现次数为 **0**，无法据以对齐「思考段数 /
  tail 注入位置 / 文本稳定性」。

**残余风险：** 无法用真实会话确认迁移是否改善或恶化了复述行为。部分对冲来自
可复现的测试而非生产数据——`features/reminder/` 的单测钉死「同回合去重 / 每回合
重发 / 刷新间隔 / compaction 后可再生」四条策略，`engine_test.rs` 钉死「plan/goal
块恰好一次、走信封、不含 `[Context]`、goal-less 会话零注入」。

**补采义务：** 若后续环境能起整栈，应按 §4 补采一次并回填本节。
