# Turn-tail `[Context]` 注入：现状记录与待验证项

> 状态：**P2 已消除、P4 已消除**（2026-09-23 更新；plan/goal 注入与 `Current date` 均已迁出
> `[Context]`，见 §7、§9；剩余项复核见 §8）。
> 这不是一份方案，是一份**现状与证据的登记**——
> 机制已读准，P2/P4 已处置，其余处置仍未定，且**本机仍未采集到生产基线**。
> 前置：`docs/architecture/agent-engine.zh.md`、`docs/agent-store/20-tool-injection-policy.zh.md`。
> 用途：回答「turn-tail `[Context]` 目前是什么问题」。**改动前请先读 §4**：现在的判断仍是
> 假设，先量再改。**§1 的机制表已按实测修正（见 §9），行号适用于 `34cb35987` 之前，
> 引用前请重新定位。**

---

## 1. 机制（读代码确认，非推测）

| 事实 | 位置 |
|---|---|
| `turn_tail_extras` **每回合**组装一次（外层 `loop {` 在 `:1910`，`turn_tail` 在 `:2066-2067`） | `crates/agent/nomi-agent/src/engine/mod.rs` |
| ~~第一条**无条件**是 `Current date: %Y-%m-%d`~~ **已于 2026-09-23 移出，改走 `<system-reminder>`**（见 §9） | 原 `:1969-1972`，现无 |
| 其余依次是：system resource notices、office plan nudge（`:1981`）、coding harness 的 plan nudge / `forced_finalize_instruction` / `turn_tail`、注册的 `ContextContributor`、round ledger section | 同上 |
| 用空行拼成**一个**字符串，前缀 `[Context]\n` | `context_contributor.rs:94`（标签）、`turn_tail_text_block` |
| **在 `'provider_attempt: loop`（`:2087`）里注入**，即**每个 provider pass 一次**（`turn_tail` 是每回合建、每 pass 复用） | `engine/mod.rs:2088-2092` |
| 注入方式：最后一条消息是 `Role::User` → 在**位置 0** 插一个 `Text` 块（**包括纯 tool-result 的 user 消息**）；否则追加一条新 `Role::User` | `context_contributor.rs:174-196` |
| **注入结果写回 `self.messages`**（`persist_turn_tail_context`），因此**历史里会累积副本**：已发送的消息不可改写，于是每 pass 追加一条 `[Context]`-only user 消息 | `engine/mod.rs:2088-2092`；分支 A2 `context_contributor.rs:181-184` |
| 设计目的：动态内容不进 system prompt，让 system prompt 逐字节稳定，保 DeepSeek 前缀缓存 | `context_contributor.rs:1-14` |

> **2026-09-23 实测修正。** 上表第 7 行曾写「注入结果**不写回** `self.messages`（局部变量），
> 所以历史里不累积副本」——**该结论是错的**，与当时引用的行号一起漂移。实测（§9）显示
> 注入会落盘，且在一个 2-pass 的回合里产生 **2 条** `[Context]` 消息。这条错误结论曾是
> §8「`Current date` 只是每 pass 重贴、历史不累积」判断的基础，故一并更正。

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
  仍恒返回 `Some`。（**2026-09-23 更正：P4 随后被消除，见 §9。**）
- **P1 的机制面减少但未消失。** 最高显著位置上不再出现「长指令块」，但
  `Current date` 依旧落在那里——§7 记录了基线缺失，§8 对 turn-tail 剩余注入项做了
  逐项复核：九项里只有 `Current date` 需要改（降频），其余八项建议原样保留，
  **均已识别、未排期**。

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

---

## 8. 已识别、未排期：turn-tail 剩余注入项的复核

plan/goal 迁出后，turn-tail 每 pass 仍注入的内容按「是否值得每 pass 发」过了一遍。
结论：**九项里只有 `Current date` 一项需要改，其余八项建议原样保留。**

| 注入项 | 触发 | 判定 |
|---|---|---|
| system resource notices | 有宿主通知时 | 真数据（宿主事件），删则丢信息 |
| **`Current date`** | **无条件** | **内容一天内逐字不变，却每 pass 重占最高显著位置 → 应降频** |
| office plan nudge / hard stop | 办公模式 + 计划滞留阈值到 | 已一次性（阈值触发，非每 pass） |
| coding plan nudge | coding harness 判定 | 已一次性 |
| `forced_finalize_instruction` | 强制收尾时 | 已一次性 |
| harness `turn_tail` | coding harness 判定 | 已一次性 |
| `ContextContributor` 块 | 注册了贡献者时 | 真数据（RAG/memory），通道属其设计接缝 |
| office working set 索引 | 办公模式且非空 | 真数据（文件工作集），压缩后回注用 |
| round ledger section | 重启后 | 已一次性（`take_section` 取走） |

### `Current date`：建议降频，不建议删除

**它不是冗余信息。** 查证结论：

- `context.rs` 明确禁止日期进 system prompt（`:211` 注释、`:346` 注释，以及
  `prefix_stability_no_date_in_system_prompt` 与 `:1553` 两个断言）；
- 全工程只有 `engine/mod.rs:1970` 一处把当前日期送给 provider。

即：删掉它，模型就真的不知道今天几号。问题在**频率**（一天不变、每 pass 重发），
不在**必要性**。

两条候选路（均需改动 turn-tail，故未实施）：

1. **迁入 `<system-reminder>` 通道**（倾向）：现有 reminder 机制天然覆盖这个需求——
   文本由状态派生、同回合内相同文本不重发、**跨天文本变了自然重发**，不需要额外的
   「日期变了才发」逻辑。它同时从「数据」变成「状态告知」，更贴合信封定位。
2. **仅回合边界注入 turn-tail**：保留在 `[Context]`，每回合组装一次、本回合后续 pass
   复用。改动更小，但下一回合仍会重贴，P3 只缓解不消除。

**决定：走候选 1（已实施，见 §9）。** 用户于 2026-09-23 拍板，理由是日期是这一项里
唯一的「一天内逐字不变」者，迁走它既降频又不丢信息。原先三条「为什么现在不做」的理由，
逐条现状：

- 方案 §1.3 曾把「turn-tail 其余内容（date/ledger/`ContextContributor`）的去留」列入不做
  范围——那是 plan/goal 重构 PR 的范围，本次改动**另立提交**，不回溯改写该方案；
- §3「先量再改」的约束针对**基线缺失**。本次改动不依赖生产基线：它的安全性由可复现测试
  覆盖（信封归属、每回合恰好一次、前缀回放），而启动它的动机是**实测到的重复**
  （§9 的 2-pass → 2 条），不是对复述行为的猜测；
- 影响面确实覆盖**所有会话**，因此配套改了 4 处断言并复核了 prefix 与消息计数契约
  （见 §9 的验证表）。

**排期：** 已随本分支提交，未按「单独 PR」执行（用户指定并入当前分支）。

---

## 9. 已完成的处置：`Current date` 迁出 `[Context]`（2026-09-23）

### 9.1 实测到的机制（这是启动本次改动的依据）

用临时诊断测试驱动一个 2-pass 的 plan 模式回合，dump 引擎的持久消息历史：

```
[user] [Context]\nCurrent date: 2026-09-23     ← pass 1：prepend 到未发送的用户消息
[user] enter plan mode
[tool EnterPlanMode] {}
[user] [Context]\nCurrent date: 2026-09-23     ← pass 2：最后一条是 tool-result → 追加新消息
[tool result] Entered plan mode. …
[user] <system-reminder> …Plan Mode…
[assistant] planning
```

结论（修正 §1 的旧表述）：

- `persist_turn_tail_context` **会写回** `self.messages`，因而**历史里确实累积副本**；
- 累积规律：**每个「以工具调用结束的 provider pass」各产出一条 `[Context]` 消息**，
  即一个 N 轮工具循环的回合约产生 **N 条**，而**不是**「每回合一条」；
- 原因：分支 A1（相同文本 no-op）只比较**紧邻的最后一条**，中间隔着 assistant/tool
  消息即失效；已发送的消息又不可改写（前缀缓存不变量），只能追加（分支 A2/B）。
  工具结果被建模为 `Role::User`，所以工具轮走分支 A4（插进 tool-result 消息首位）；
- 这些 `[Context]` user 消息**不进 microcompact 的靶子**（`compact/micro.rs:85-97` 只按
  `compactable_tools` 收集 **tool result**），因此只能被全量 autocompact 摘要掉。

### 9.2 改动

- `engine/mod.rs`：`turn_tail_extras` 不再 push `Current date`；
- 新增宿主拥有的 reminder 变体 `DATE_INJECTION_VARIANT = "current_date"`
  （`engine/mod.rs`），在 `build_reminders()` 里注册，**不设刷新周期**
  （`refresh_after_passes = None`）；
- `build_reminders()` 同时服务三个入口（`new_with_provider`、`resume_with_provider`、
  `set_features`），因此「装上 feature 注册表」不会丢掉宿主 reminder；
- 日期**没有** `Feature` 属主（它是宿主环境数据），故由引擎直接注册，不塞进
  `FeatureRegistry`。

**频率语义**（`ReminderService::collect` 的既有契约，未改）：同一变体在本回合内文本不变
→ 不重发；`begin_turn()` 每回合清空去重状态 → 每回合发一次；跨天文本变化 → 自然重发。
这正是需求「只有日期变才发」所需的全部逻辑，**无需新增机制**。

### 9.3 实测结果

同一诊断（2-pass plan 回合，改动后）：

| 指标 | 改动前 | 改动后 |
|---|---|---|
| 历史中 `Current date:` 出现次数 | **2** | **1** |
| 历史中 `[Context]` 出现次数 | 2 | **0** |
| `<system-reminder>` 出现次数 | 1（plan） | 2（plan + date） |

### 9.4 验证

| 门 | 结果 |
|---|---|
| `cargo test -p nomi-agent` | **1138 passed / 0 failed** |
| `cargo test -p nomi-types` | 76 passed / 0 failed（与基线同） |
| `cargo test -p nomifun-ai-agent` | 1067 passed / **32 failed**，与基线失败集**逐项相同**（无新增） |
| `cargo check --workspace` | 通过（仅既有 warning） |
| `cargo fmt -p nomi-agent -- --check` | 通过 |
| `system prompt 不含日期` | `context.rs:1551-1554` 与 `engine_test.rs` 断言**未改**，仍通过——午夜前缀缓存不变量保持 |

**改动的断言（4 处，均为语义跟进而非放宽）：**

1. `engine_test.rs` `contributor_context_rides_turn_tail_not_system_prompt`：tail 消息改为
   按 `[Context]` 标记定位（它不再是最后一条），并新增「日期**恰好一次**且**不在** tail 里」
   两条断言；
2. `engine_test.rs` `persisted_turn_tail_is_replayed_as_the_next_request_prefix`：改为断言
   message 0 逐字节回放（此会话无 contributor，`[Context]` 块为空）、并显式断言**日期
   reminder 本身也按前缀回放**；回合增量 2 → 3（assistant + 新 user + 该回合的日期 reminder）；
3. `engine_test.rs` `test_engine_message_accumulation`：4 → 6（每回合多一条日期 reminder）；
4. `engine_test.rs` `a_goal_less_session_gets_no_goal_reminder`：原断言「无任何 reminder」
   已不成立（日期无条件发送），改为**逐块断言没有任何 goal 块**——仍是原契约的更强形式。

**`badcase_regression_test::a_round_that_keeps_truncating_stops_at_three_passes`（此前被接受为
已知基线失败）现在通过**，但这不是修复：日期 reminder 是持久的 user 消息，每个 pass 都在，
故该用例的 `pass + 1` 计数需改为 `pass + 2`。已核实**原断言在新引擎下仍然失败**（2 vs 1），
因此这是断言跟进，不是行为修复；同时把 per-message 断言写得比原来更严（恰好一条 reminder，
其余仍必须是 `[resumable round` 提示），以免计数放宽掩盖真实回归。

### 9.5 未做（已识别，待排期）：降低 `[Context]` 的 persist 频率

日期迁走后，`[Context]` 里剩下的（working set / ledger / resource notices / contributor）
**都可能在回合内变化**，因此「每 pass 重贴」不再是纯浪费。若仍要收紧，正确做法是让
`persist_turn_tail_context` 只在**内容确有变化**或**tail 块已被压缩丢弃**时追加，而**不能**
用「这是本回合第几个 pass」判断：内层有 4 条 `continue 'provider_attempt` 会先跑
`run_compaction` 再重试（`:2127` idle / `:2139` turn-start / `:2160` emergency / `:2196` overflow），
而压缩会改写消息列表——若在压缩后跳过 persist，模型将**看不到** working set / ledger。
已核实的内层事实：5 条 `continue 'provider_attempt`（`:2127/:2139/:2160/:2196/:2623`）；
`sent_prefix_len` 只在 `stream_llm` 返回 `Ok` 时推进（`:2189`），故前 4 条属于「未发送即重来」，
而 `:2623`（流中途 overflow）是在**已发送**之后，会落进分支 A2 追加重复块——且当
`run_compaction` 为 no-op（如 compaction 关闭）时该重复可达。
