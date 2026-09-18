# Flowy Agent Harness 模式审计：Plan / Goal / 上下文压缩 / 子代理

> **最后维护：** 2026-09-17 · 核对基准：commit `a3482541f`（工作区另有未提交的 Agent Store 改动，与本审计无关）
> · 文档性质：代码审计报告（时点快照，结论只对该 commit 负责）
> · 主审查对象：`crates/agent/`（`nomi-agent` 为核心，辅以 `nomi-coding` / `nomi-tools` / `nomi-types`）
>
> 审计范围：四种"控制模式"——plan mode、goal mode、上下文压缩（compact）、子代理/委派。
> 每一条结论都标注 `文件:行号`；未取证的推断显式标注"未验证"。与
> [agent-harness-capability-analysis.zh.md](agent-harness-capability-analysis.zh.md)、
> [agent-loop-comparison.zh.md](agent-loop-comparison.zh.md) 的关系见 §8。

## 0. 结论摘要

Flowy 的这四块能力**都在**，而且其中若干实现（goal judge 的三态判决与 wait barrier、
compact 的七段式简报、coding harness 的硬停线）比同类开源 harness 更细。真正的问题不是
"缺能力"，而是**四类系统性问题**（完整分类见 §0.3；与 plan mode 最直接相关的是前三条）：

1. **有状态机、没有对外契约**：plan mode 只能"切换工具可见性"，没有任何计划对象、审批或
   版本（`PlanModeTransition::Exit { plan_content: None }` 是唯一取值）。模式本身不可审计、
   不可恢复、不可校验执行是否遵循计划。
2. **模式开关与持久化脱节**：plan 状态是 `AgentEngine` 内存字段，`save_session()` 保存的字段
   清单里没有它；`plan/file.rs` 的读写函数在生产线无调用者。计划与模式都无法跨会话恢复。
3. **模式的选择权与证据权分离**：goal judge（唯一会检查"完成是否成立"的组件）默认 opt-in
   且只看最后一段文本；普通会话仍以模型自然 `EndTurn` 作为成功。

### 0.1 最该先修的五件事

按"后果 / 修复成本"排序，与 §7 的编号对应：

| 序 | 事项 | 编号 | 为什么排在这里 |
| --- | --- | --- | --- |
| 1 | 摘要失败时**不要**退化成机械折叠、不要落盘 | #30 | 唯一"一次瞬时故障 = 不可逆上下文丢失"的路径，且熔断器是死代码；修复面很小（失败即中止 + 接回 `record_failure`） |
| 2 | 子代理的审批通道 | #1 | 子代理目前可能拿到**未经用户审批**的写权限（stdin EOF 视为批准），属于安全面 |
| 3 | 计划模式的只读承诺收口（MCP / Skill / Skill-fork 三条旁路） | #3 #4 #5 | 只读保证不应依赖第三方 `readOnlyHint`，也不应被"Info 类目"覆盖不到 |
| 4 | `update_goal(complete)` 必须先过 judge | #8 | 唯一完成审计被被审计者自己关掉；改成"申请 → 审核"即可 |
| 5 | 子代理 provider 调用的计费归属 | #2 | 子代理是最烧钱的部分，却漏出成本聚合 |

另外两条"沉默的谎言"值得单独点出，它们不需要改架构、只需要保持一致：
**#10**（coding 会话里 goal 永不续跑，turn tail 却仍承诺每轮会有人审计）与 **#35**（上下文爆掉时
给用户看的文案从不提示 `/compact`）。planned 模式还有第三条同类：**#38**（工具承诺"汇总结果会写回
本会话"，而回写的重试路径是死代码，一次瞬时投影错误就会静默丢失）。

### 0.2 与"前沿 harness"的差距（详见 §6）

三条收敛结论（详见 §6）：

1. **机制覆盖率上，Flowy 夹在 Codex 与 Claude Code/Kimi 之间。** Codex 的 plan mode 是纯提示词
   模板（真只读 PR 未合并），Claude Code 与 Kimi 都是"权限模式 + 计划落盘 + 显式审批"。
   Flowy 有工具面机制（比 Codex 强），但**没有审批、没有产物**（比 Claude Code/Kimi 弱一整层）
   ——而"审批 + 产物"恰好是唯一能让计划在压缩/恢复后仍然存在的东西。
2. **完成判定上，Flowy 是三者中唯一允许被验收者自我宣告完成的。** Codex 把完成当待证命题
   （completion audit + 三分类 no-progress），Claude Code 把裁决权交给另一个模型；
   Flowy 的 judge 与干活的是同一个模型，而且可以被 `update_goal(complete)` 直接短路。
   值得注意的是"同一阻塞条件持续 N 轮"这条规则 Codex 独立实现到了几乎字面一致——说明它的**方向**
   是被验证过的，Flowy 缺的是 Codex 那两个非 blocked 的收尾态（`paused` / `budget_limited`）。
3. **压缩上 Flowy 的保留策略最保守、失败路径最危险。** 用户消息原样保留 + 要求合并旧简报，
   比 Claude Code（会丢弃最旧 skill body、`paths:` 规则随历史被摘要）和 Kimi（只留 user prompt +
   摘要）都保守；但对手要么有失败上限（Kimi `compaction_max_attempts = 5`、Claude Code 连续 3 次
   失败后停止尝试），要么至少不销毁历史（Codex 的 notes/新窗口模型），而 Flowy 摘要失败即**机械
   折叠销毁历史并落盘**，且熔断器是死代码。Kimi 的一手 bug 链（子目录 AGENTS.md、权限模式提醒被
   压缩丢弃）提示：真正要盯的是**系统注入的约束**会不会被摘要吃掉，而不是用户原话。

### 0.3 第四类：失败路径比成功路径更危险

前三条已在上文列出，这里补上最容易被忽视的第四条，并给出全报告的判断：

**失败路径比成功路径更危险**：压缩失败会销毁历史并落盘（#30）；子代理审批在无
`approval_manager` 时退化成 stdin 交互，EOF 即视作批准（#1）；judge 解析失败 fail-open——
这个取舍本身是对的，但它顺带掩盖了"judge 在编码会话里根本没被调用"的配置问题（#10）。

一句话总结：这四块能力的**成功路径**写得比多数同类实现细（judge 三态与 wait barrier、七段式简报、
coding hard-stop、单调收窄的子代理权限、请求级工具权威），**失败路径与类目语义**才是短板集中区。

## 1. 范围与方法

| 项 | 内容 |
| --- | --- |
| 审计对象 | `crates/agent/nomi-agent`（72 个 `.rs`）、`nomi-coding`、`nomi-tools`、`nomi-types` |
| 范围边界 | 主体是 `crates/agent/`；`crates/backend/nomifun-agent-execution`（跨会话 execution/attempt 调度）作为 §5.4 planned 委派的宿主侧被追到"链路 + 缺陷"一级，不做全量审计 |
| 方法 | 只读代码审计：先读实现与其自带测试确认设计意图，再用调用方/被调用方交叉验证；不采信注释里的承诺 |
| 证据强度 | `path:line` + 关键行原文；推断标"未验证" |
| 未做 | 未做性能基准、未做同模型同工具的跑分（参见 `agent-loop-comparison.zh.md` §5 的同类告诫） |

## 2. Plan Mode

### 2.1 实现事实（已验证）

| 环节 | 位置 | 事实 |
| --- | --- | --- |
| 工具注册 | `nomi-agent/src/bootstrap.rs:956-964` | 由 `self.config.plan.enabled` 门控，注册 `EnterPlanModeTool` / `ExitPlanModeTool`，二者共享一个 `Arc<AtomicBool>` |
| 工具本体 | `plan/tools.rs:21-94` / `104-176` | 两个工具都无输入参数；`category() == Info`；`execute()` 只读/写提示文案，**自己不改变任何状态** |
| 状态切换 | `engine/mod.rs:3410-3427` | 引擎消费 `ContextModifier::plan_mode_transition`：Enter 时把 `allow_list` 存进 `pre_plan_allow_list` 并置 `is_active = true`；Exit 时恢复 `allow_list` 并置 `false` |
| 工具可见性 | `engine/mod.rs:1550-1562` | plan 激活时只向 provider 暴露 `ToolCategory::Info` 且非 `EnterPlanMode`；否则暴露除 `ExitPlanMode` 外的全部工具 |
| 同一逻辑第二份 | `engine/mod.rs:3219-3239` | `advertised_tools()` 复制了同一套过滤，含 forced-finalize 清空分支 |
| 提示词注入 | `engine/mod.rs:1609-1613` | `plan_mode_instructions()` 作为 turn tail 注入最后一条 user message，**不进 system prompt**（保 cache 前缀稳定） |
| 提示词内容 | `plan/prompt.rs:14-47` | 四阶段工作流（Understand / Design / Write the plan / Submit for review），并声明"Calling ExitPlanMode is the way to request approval" |
| 编码硬停线 | `nomi-coding/src/harness.rs:71-74, 355-378`、`progress.rs:247-259` | `plan_mode_budget` / `plan_mode_hard_stop`：超预算先给 nudge，超硬线则 `abort_before_provider` + `begin_forced_finalize`，下一轮 `tools.clear()`（`engine/mod.rs:1565-1571, 1626-1630`） |
| Authority 来源 | `engine/mod.rs:1572-1576` | 每请求由**实际advertise的工具集**生成 `ProviderToolAuthority`，dispatch 不信任 registry 全量 |

设计上值得肯定：**只读过滤 + turn-tail 注入 + 请求级 authority** 三者组合是干净的。plan mode
既没有污染 cache 前缀，也没有依赖"模型自觉"来约束工具面——过滤发生在组装请求的位置，
而不是事后校验。

### 2.2 缺陷

**[P1] `ExitPlanMode` 没有 plan 参数，"提交计划供审批"是空承诺**

- 证据：`plan/tools.rs:127-133` 的 input schema 是 `{"properties": {}, "required": []}`；`plan/tools.rs:164` 永远是 `Exit { plan_content: None }`。
- 但 `plan/prompt.rs:46-47` 告诉模型 "call ExitPlanMode to submit it for user review"，`plan/prompt.rs:98-99` 的测试只校验这句话存在。
- 后果：①模型被误导去"提交"一个根本不携带内容的调用；②宿主/前端拿不到结构化计划，没有批准/拒绝语义（对比 Claude Code 的 `ExitPlanMode` 会带 plan 文本并触发用户确认）；③执行阶段无法校验"实际改动是否符合计划"。
- 建议：给 `ExitPlanMode` 加 `plan` 必填参数，写入 `Exit { plan_content: Some(..) }`，由宿主做批准并在批准后把 artifact 注入后续 turn tail。

**[P1] 计划文件能力是死代码，计划不落盘**

- 证据：`plan/file.rs` 提供 `plan_file_path` / `write_plan` / `read_plan`；全仓 `grep` 显示非测试调用者**只有** `plan/file.rs` 自身与 `tests/plan_prompt_file_test.rs`。`nomi-config` 的 `plan.plan_directory`（`nomi-config/src/plan.rs:15-16`，默认 `.nomi/plans`）除配置解析与相等判断外无消费者。
- 后果：计划只活在该轮响应文本里；上下文压缩、会话切换、进程重启都会丢；`.nomi/plans` 与 `[plan].plan_directory` 给用户的印象与实际行为不符。
- 建议：要么在 Enter/Exit 边界真正落盘，要么删掉 `plan/file.rs` 与配置项，避免误导。

**[P2] plan 状态不进持久化，恢复后模式与记录不一致**

- 证据：`engine/mod.rs:3431-3447` 的 `save_session()` 只保存 `messages`/`total_usage`/`activated_deferred_tools`/`editable_turn`/`updated_at`，未包含 `plan_state`（字段定义见 `engine/mod.rs:553` 附近的引擎状态）。
- 后果：resume 之后 `is_active` 回到默认 false，但 transcript 里仍有 plan 阶段指令与 `ExitPlanMode` 调用记录；工具面又从只读恢复为全量，模型可基于旧上下文直接开始写。
- 建议：把 plan 状态并入 session（或在 projection/事件层重建），并明确"resume 后是否继续 plan"的产品语义。

**[P2] 只读过滤逻辑在两处重复实现**

- 证据：`engine/mod.rs:1550-1562` 与 `engine/mod.rs:3219-3239` 是同一套 `Info && name != EnterPlanMode && harness_advertise_tool(..)` 过滤加 forced-finalize 清空。
- 后果：两处已各自演进（`advertised_tools()` 是后加的），任何一处改动都可能让"真实请求工具集"与"对外公布的可用工具集"分叉；这两处谓词目前**一致**（审计逐一核对过，没有发现漏过滤的并行/重试路径），所以这是维护性风险而非当前漏洞：一旦只改一处，"真实请求工具集"与"对外公布的可用工具集"就会分叉。
- 建议：抽成单一函数（例如 `visible_tools_for_request()`），两处共用。

**[P2] 非编码会话的 plan mode 没有轮次上限**（未完全验证）

- 证据：计划超时的 nudge/hard-stop 只由 `nomi-coding` 的 `CodingHarness` 提供（`harness.rs:355-378`），引擎侧只判断 `is_active`。若宿主未装配 coding harness，plan mode 仅靠提示词劝导收敛。
- 已取证默认值：`plan_mode_budget = 10`、`plan_mode_hard_stop = 14`（单位是轮次，
  `nomi-coding/src/progress.rs:91-92`），对照 explore 预算 6 / 硬线 10（`:88,90`）。
  **待验证**：宿主在非编码会话是否也装配 coding harness。

### 2.3 只读承诺的三条旁路（新增，均已取证）

plan mode 的过滤条件是**单一谓词** `t.category() == ToolCategory::Info`，而 `Info` 这个标记在
本仓被用作"默认不需要审批、默认安全"的宽松类目——两者的语义并不等价。由此产生三条旁路：

**[P1] MCP 工具的 Info 分类由第三方服务端自报，plan mode 的只读承诺被外包**

- 证据：`nomi-mcp/src/tool_proxy.rs:126-141` —— `category_from_annotations()` 在
  `is_read_only()` 为真时返回 `Info`，而 `is_read_only()` 就是 `annotations.read_only_hint.unwrap_or(false)`。
- 后果：任何 MCP server（第三方、或已被投毒的 server）只要把写工具标成 `readOnlyHint: true`，
  该工具就进入 plan mode 的工具面并被执行（分发只看请求快照，见 `tool_execution.rs:386-400`，不问 category）。
- 建议：plan mode 改用独立白名单并明确排除 `mcp__*`，或要求 MCP 工具一律非 Info。

**[P1] `Skill` 是 Info，但 inline 执行会跑 SKILL.md 里内嵌的 shell**

- 证据：`skill_tool.rs:282-285` 自报 `ToolCategory::Info` 并注明"does not directly modify files
  or run commands"；而 `nomi-skills/src/executor.rs:39-47` 在 `substitute_arguments` 之后调用
  `execute_shell_commands_with_shell(...)`，测试 `executor.rs:343-386` 断言 shell 输出被替换进技能正文。
- 后果：plan mode 下一次 `Skill` 调用即可执行技能正文中的 shell 命令；`SkillPermission::Ask`
  也只是返回错误文案而不弹审批（`skill_tool.rs:183-194`）。
- 建议：plan mode 下禁用 shell 替换阶段（传"禁 shell"标志），或直接隐藏 `Skill`。

**[P1] `Skill` 的 Fork 分支会起一个全权子 agent，且不继承 plan mode**

- 证据：`nomi-skills/src/executor.rs:83-94` 用 `tool_policy: AgentToolPolicy::Full`；
  `local_agent_invocation.rs:127-152` 经 `build_tool_registry` → `AgentEngine::new_with_provider`
  起新引擎（`engine/mod.rs:668` 的 `PlanState::default()`）；`CHILD_TOOL_CATALOG`
  含 `Write/Edit/Bash`（`local_agent_invocation.rs:807-819`），空 allow-list 等于 Unrestricted
  （`:840-846`）；`config_for_invocation`（`:174-191`）完全不碰 plan。
- 后果：plan mode 里 `Skill(fork)` 就是一条"洗白只读约束"的通道，子 agent 可写文件、跑命令。
- 建议：`AgentInvocationInput` 增加只读/plan 约束，并在 `ToolScope` 求交时纳入。
  （可利用性取决于本机存在 fork 技能且启用了嵌入版执行，**置信度 中**。）

**[P2] 有持久化副作用的工具被标成 Info**

- 证据：`companion_tools.rs:236-240`（`save_memory` 写 memory.db，注释自认"treat as Info so
  default session mode doesn't gate it behind approval"）、`companion_tools.rs:447-452, 524-526`
  （`create_companion_skill` 落库/落盘技能草案）；生产注册见宿主 `manager/nomi/agent.rs:1249,1288`。
- 后果：plan mode 下模型仍可写长期记忆与技能草案，`plan/prompt.rs:16` 的
  "MUST NOT make any changes to the system" 名不副实。
- 建议：把 Info 拆成 ReadOnly / BenignWrite 两档，plan mode 只放 ReadOnly。

**[P2] 过滤只用 `category()`，不用逐动作的 `category_for(input)`**

- 证据：过滤点 `engine/mod.rs:1550-1556`、`3220-3230` 只比较 `t.category()`；而 Browser
  （`nomi-browser/src/tool.rs:3082`）与 Computer（`nomi-computer/src/tool.rs:970-977`）是按动作分级
  （observe/screenshot 等为 Info），其 `category()` 恒为 Exec（`nomi-browser/src/tool.rs:4446`、
  `nomi-computer/src/tool.rs:965-968`）。
- 后果：当前方向是**过严**（plan mode 连只读的 observe/screenshot 也一并隐藏），不是泄漏；但两条
  分级轴不一致，将来任何"`category()==Info` 而 `category_for` 为 Edit/Exec"的工具都会变成真漏洞
  （现有 Info 工具中没有实现 `category_for` 的，已逐一核对）。
- 建议：plan mode 改为按 `category_for(input) == Info` 逐调用判定，或引入独立 ReadOnly 标记。

**[P2] plan 生效前的那一轮仍可写**

- 证据：工具权威来自请求快照（`engine/mod.rs:1576` `ProviderToolAuthority::from_request_tools(&tools)`，
  `tool_execution.rs:386-400` 拒绝未 advertise 的调用），而 `EnterPlanMode` 自报并发安全
  （`plan/tools.rs:53-55`）。
- 后果：模型可以在同一条 assistant 消息里同时发 `EnterPlanMode` 与 `Edit`，两者都在该请求里已
  advertise，`Edit` 照常执行——只读约束从**下一轮**才成立。
- 建议：这符合"请求即权威"的设计，但作为只读保证的边界必须写进文档与测试。

**[nit] `context_usage` 把 plan 指令记进 `rules` 桶，而它实际走 turn tail**

- 证据：`context_usage.rs:76-86` —— 计入 `breakdown.rules` 之后，在 turn-tail 循环里显式
  `continue`（因此没有重复计数），但 UI 分类会被误导：plan 指令被算作"系统提示"的一部分，
  而系统提示里其实没有它。

### 2.4 设计权衡、优点与疑问（plan mode）

**设计权衡**

- turn tail 注入换取 cache 前缀稳定：`engine/mod.rs:1578-1612` 明确 `system = self.system_prompt.clone()`
  不含 plan 段，`context_contributor.rs:75-116` 把 tail 作为 `[Context]` 块插到最后一条 user 消息的
  position 0。代价是 plan 指令每轮重复进入对话 token。`context.rs:1587-1608` 有
  "系统提示不得含 ExitPlanMode / plan mode 关键字"的回归测试（仅 grep 见存在，**未逐行验证**）。
- allow_list 快照与**整体覆盖**式还原：`engine/mod.rs:3413,3421`，单测 `engine/plan_mode_tests.rs:83-144`
  明确断言"plan 期间由 skill 追加的 allowed_tools 会被丢弃"。
- 探索预算硬停：`nomi-coding/src/harness.rs:355-374`（软提醒 → `plan_mode_hard_stop` 触发 forced
  finalize）+ `progress.rs:110-128`，引擎据此 `tools.clear()` 且不再 advertise 任何工具
  （`engine/mod.rs:1565-1571, 1618-1630`）。这是防"无限探索"的成熟做法。

**做得好的地方**

- **分发以请求快照为唯一权威且 fail-closed**（`tool_execution.rs:378-400`，注释见
  `engine/mod.rs:1572-1575`）——避开了"过滤了定义但照旧执行"这一经典漏洞。
- 工具面构造只有两处且谓词一致，attempt 循环复用同一个 `tools` 变量（`engine/mod.rs:1696` 起）。
- `EnterPlanMode` / `ExitPlanMode` 的重复进入/非法退出都会被拒（`plan/tools.rs:61-68, 143-151`），
  共享 flag 与引擎状态双向同步（`engine/mod.rs:3415-3424`，测试 `plan_mode_tests.rs:100-169`）。
- Info/Edit/Exec + 逐动作 `category_for` 的审批分级设计本身是对的（`nomi-tools/src/lib.rs:267-279`）；
  问题只在于 plan mode 没有复用更细的那一档。
- 对"为什么算 Info"普遍写了显式理由（`companion_tools.rs:237-239`、`summon_tools.rs:142-144`、
  `skill_tool.rs:283-285`）——结论有争议，但属于明示权衡而非疏忽。

**plan mode 下确认可用的工具**（`category() == Info` 且已核对注册路径的部分）：Read、Grep、Glob、
DirTree、Lsp、Skill、ToolSearch、update_plan、ExitPlanMode、update_goal、save_memory、
create_companion_skill、cron_list、meeting 的只读几个、knowledge_search，以及所有
`readOnlyHint=true` 的 `mcp__*` 代理。

**疑问**：`Session` 结构体是否有 plan 字段（两次 grep 超时，**未验证**，只能由 `save_session`
推断不持久化）；前端是否向用户呈现 plan（`ui/` 未查）；`Skill` 合并的 hooks 能否在 plan mode 执行
shell（`tool_execution.rs:1067-1089` → `merge_hooks`，执行体未读）；`cron_delete`/`meeting.ask`
是否真只读。

## 3. Goal Mode

### 3.1 实现事实（已验证）

| 环节 | 位置 | 事实 |
| --- | --- | --- |
| 目标工具 | `goal/tool.rs:26-104` | `update_goal` 只有 `status: complete\|blocked` + 可选 `evidence: string`；`is_deferred=false`（提示词要求直接调用，schema 必须一开始可见）；`category() == Info` |
| 终态写入 | `goal/tool.rs:78-84` | 仅在 `Active` 时写入，重复调用幂等；`complete` 后 `blocked` 不会覆盖 |
| Judge 契约 | `goal/judge.rs:43-82` | 三态 `done/continue/wait`，严格 JSON（`{"verdict": ...}`），允许从散文里抠 JSON、大小写不敏感、未知 verdict 保守回落 `continue`（`:519-525`）；`wait` 必须带 `wait_on_session` / `wait_on_pid` / `wait_for_seconds` 之一，否则降级 `continue`（`:548-573`） |
| 判决调用点 | `goal/runtime.rs:174-269` | 自然终止点调用；`Waiting` 且在等有效 barrier 时不调用 judge、不烧预算；barrier 释放后惰性清除（fail-open）；断路器：连续 parse 失败 / transport 失败各自计数到上限 → `Paused`，断路器维度相互独立 |
| 预算 | `goal/state.rs`（`max_auto_continuations`/`auto_continuations`）、`runtime.rs:121-132` | 续作次数有上限；`wait` 不消耗预算（`runtime.rs:263-266` 明确"等待不是进展"） |
| 续作提示 | `goal/templates/continuation.md`、`continuation_subgoals.md`、`continuation_contract.md` | 三分支模板（普通 / 有 subgoal / 有 contract），单独成文件而非条件块，**同一 goal 会话内逐轮字节稳定**（`runtime.rs:11-20`） |
| 每轮目标感知 | `goal/templates/goal_context.md`、`runtime.rs:141-151` | 只要 goal 仍可继续，就把目标块注入 turn tail；paused/terminal 返回 `None`，请求与无 goal 会话字节一致 |
| 阻塞阈值 | `goal/state.rs:178`（默认 3）、`runtime.rs:124, 290, 440, 459` | `blocked_threshold` 只被用来渲染提示词 |
| 恢复 | `goal/runtime.rs:85-112` | `from_state` / `restore` / `snapshot` 支持宿主完整快照恢复（probe 由宿主重新注入） |

`goal_context.md` 与 `continuation.md` 的提示词质量是本仓 harness 里最高的一处：它明确要求
"把'完成'默认视为未经证明"、要求从目标反推**可验证需求清单**、要求逐条给出权威证据、
"太弱/太间接的证据一律视为未完成"，并显式禁止"围绕已经做出来的东西重新定义成功"。
这比多数 harness 的一句"please verify your work"要硬得多。

### 3.2 缺陷

**[P1] `update_goal` 能让模型直接短路 judge——唯一的完成审计组件可被绕过**

- 证据：`goal/tool.rs:78-84` 只要 `status == Active` 就无条件写入终态，不经过任何审计；
  `goal/runtime.rs:155-158` 的契约注释写明 "a terminal state the model already declared via
  `update_goal` — first terminal state wins, **no judge call**"。
- 而 `goal_context.md:11` 对模型的要求是"不要靠主动调用 update_goal 宣告完成来结束这个循环"。
- 后果：整个 goal 模式的完成闸门是**提示词自律**，不是机制。模型（尤其在上下文被压缩、
  或在续作压力下）调用一次 `update_goal(status="complete")` 即可终止循环，judge 完全不参与。
  这与 `continuation.md:18` 那套"完成审计"承诺形成反差。
- 建议：把 `complete` 视为"申请完成"，由 judge 做一次强制审计再落终态；或对 `complete`
  要求 `evidence` 非空并把它作为 judge 输入的一部分。

**[P2] `evidence` 可选且不参与任何判定**

- 证据：`goal/tool.rs:52` 的 `required` 只有 `["status"]`；`:76, 86-97` 只把 evidence 回显进
  `{"kind":"goal_update", ...}` payload，没有校验、没有进入 judge 输入。
- 后果：结构化证据字段退化为"装饰性字段"，与提示词里"逐条列出权威证据"的强度不匹配。
- 建议：`complete` 时必填，并传给 judge / 宿主验收。

**[P2] 阻塞判定阈值只在提示词里，代码不统计**

- 证据：`blocked_threshold`（默认 3）的全部用处是 `render_continuation()` 渲染
  `{{blocked_threshold}}`（`runtime.rs:124, 290, 440, 459`）；`GoalStatus::Blocked` 只在
  `update_goal` 被调用时写入（`tool.rs:73`）。全仓不存在"同一阻塞条件连续 N 轮"的计数器。
- 后果：与 DSH 早期目标实现同类问题——阈值是散文常量；模型可以一轮就宣告 blocked。
- 建议：至少在状态里记录"上次 blocked 理由"，让工具层拒绝未达阈值的 blocked 调用。

**[P2] judge 只看模型最后一段文本，不看工作区**

- 证据：`runtime.rs:204-212` 传给 `judge_goal` 的只有 `objective` / `subgoals` / `contract` /
  `background` / `last_response`；judge 客户端是无工具的一次 completion。
- 后果：`goal_context.md:10` 宣称"judge 只认证据"，但 judge 实际只能读模型**转述**的证据；
  一个善于自我表述的模型仍可通过 judge。这与既有文档 P1-E 的结论一致（见 §8），本报告补充
  了调用点证据。
- 建议：把工具 receipt / artifact locator 摘要作为 judge 输入（既有文档 P0-A 的 evidence
  summary 方案），judge 只做判定，取证仍由受控工具完成。

**[P2] 未装配 judge client 时存在"无审计续作"路径**

- 证据：`runtime.rs:114-132` 的 `maybe_continuation()` 只看 `should_continue()` 就直接渲染续作
  提示，不评估完成（注释亦写明"When no judge client is wired"）。
- 待验证：宿主在哪些场景走这条路（`GoalJudgeClient` 未注入时）；若默认宿主都会注入，则风险有限。

**[P1] 预算耗尽后 goal 仍是 `Active`，且预算窗口随进程重建而复活**

- 证据：`goal/state.rs:198` 的 `should_continue()` 是 `status == Active && auto_continuations < max_auto_continuations`；`goal/runtime.rs:197-199` 预算用尽时直接 `return None`，**不改 status**；宿主侧 `goal_bridge.rs:96-98` 注释 `// Not persisted: the budget window restarts with the new process.` 并把 `auto_continuations` 置 0；`goal_bridge.rs:179-181` 又把 `Active` 视为可恢复，`factory/nomi.rs:859-866` 每次构建会话都把它 restore 进新 engine。
- 后果：`docs/guides/goals.md:85-88` 把预算耗尽描述为"loop simply stops continuing"，但实现既不产生终态、也没有事件或 UI 提示；下一次进程/会话构建会注入 `Active` + 全新 8 轮预算，目标在无人同意的情况下反复自动复活并继续消耗额度。
- 建议：耗尽时置 `Paused`/`budget_exhausted` 并发一次用户可见事件；`auto_continuations` 落库（或引入 `budget_epoch`）让跨进程预算真正有限。

**[P1] coding profile 下 goal 永不续跑，turn tail 却仍向模型承诺"每轮结束会有人审计续作"**

- 证据：`engine/mod.rs:2585-2589` 的 `skip_goal = …disables_goal_auto_continue()`；`nomi-coding/src/harness.rs:312-313` 返回 `config.disable_goal_auto_continue`，默认 `harness.rs:147` 为 `true`，且 `from_host_extra`（`harness.rs:158`）不暴露该开关；而 turn tail 只看 `status ∈ {Active, Waiting}`（`engine/mod.rs:1649-1651`、`goal/runtime.rs:143`），不看 `skip_goal`。模板 `goal/templates/goal_context.md:8` 声称每轮自然结束都会由 judge 评估并自动续作。
- 后果：编码会话里 judge 从不运行、续作从不注入，同时模型被告知"等完成审计指令"并被劝阻主动 `update_goal`（`goal_context.md:11`）⇒ 目标永久停在 `Active`、进度 0/N，用户看到的是 `/goal` 静默失效 + 提示词在说谎。
- 建议：harness 禁用续跑时让 `turn_context()` 返回 `None`（或立即把 goal 置 `Paused` 并告知用户）。

**[P2] `wait_for_seconds` 无上限，且等待不消耗预算**

- 证据：`goal/judge.rs:566-568` 只要求该值 > 0；`goal/runtime.rs:274-276` 用它算 `waiting_until`；`runtime.rs:263-266` 明写 park 不消耗 `turns_used/auto_continuations`；`Waiting` 状态仍渲染 turn tail（`runtime.rs:143`）且可被 restore（`goal_bridge.rs:117,179-181`）。
- 后果：一次（甚至解析漂移出的）wait 裁决即可把目标停放到近乎永久，且没有墙钟预算兜底。
- 建议：对 `wait_for_seconds` 设上限（如 ≤3600）与单目标累计等待上限，超限降级 `continue` 或 `Paused`。

**[P2] 预算上限在 build-extra 路径没有 clamp，`extra.goal` 也未被列为禁用键**

- 证据：`factory/nomi.rs:947-952` 用 `max_auto_continuations.unwrap_or(8)` 构造 `GoalSpec`，全仓无第二处 clamp；对照 `/goal set` 路径 `manager/nomi/agent.rs:3074` 是 `unwrap_or(8).clamp(1, 100)`；`extra` 来自客户端 JSON（`factory/nomi.rs:218`），而 `reject_execution_policy_extra_keys`（`nomifun-conversation/src/service.rs:15808-15831`）的禁用键里没有 `goal`；`NomiGoalSpec.resume_state` 同样是客户端字段（`nomifun-api-types/src/agent_build_extra.rs:285-291`）。
- 后果：文档承诺的 "bounded turn budget (1..100)" 存在一条无界入口；`resume_state` 还能注入任意 `turns_used/status`。
- 建议：在 `GoalSpec`/`set_goal` 收口处统一 `clamp(1, 100)`；把 `goal` 加入 extra 禁用键或强制走 DB。（**置信度 中**：`extra` 生产入口链未逐段确认。）

**[P2] 用户中断不落库 goal 快照；且每轮把内部 turn 计数清零，单次 `execute_turn` 可跑 8×200 次迭代**

- 证据：落库只在成功路径（`manager/nomi/agent.rs:2011` 取 `goal_state()` → `:2084-2086` `spawn_goal_persist`，其余调用点都在 `goal_action` 分支），取消分支 `manager/nomi/agent.rs:1819-1830` 直接 `break`；轮预算 `engine/mod.rs:2617-2618` 每轮 `turn = 0;` 重开，上限 `engine/mod.rs:1529-1530` 的 `DEFAULT_SAFETY_MAX_TURNS = 200`（`mod.rs:377`）。
- 后果：①用户点停止后，本轮已消耗的 `turns_used`/断路器/等待障碍全部丢失，DB 仍是 set 时的 0，恢复即重放预算；②不存在目标层墙钟或总迭代预算，最坏路径（8 轮，未 clamp 时更多）× 200 次 provider 迭代都在同一次 `execute_turn` 内，中途唯一出口是用户 stop。
- 建议：goal 快照落库挂到 turn 的所有终止路径（含 cancel/error）；为目标增加总墙钟预算。

**[P2] `loop_guard` 在每个自然终止点 reset，跨 goal 轮次的停滞不被覆盖**

- 证据：`engine/mod.rs:2517` `self.stagnation_guard.reset();` 就在自然终止处理里、紧接 goal 续作判断（`:2582-2619`）之前；`loop_guard.rs:1-6` 自述检测窗口是轮内（identical completed tool outcome / consecutive turns）。
- 后果：goal 模式最典型的失败形态正是"每轮重复同一条探测命令而不推进"；`max_auto_continuations` 只限轮数，guard 又是唯一停滞检测器，于是有 8×200 的空间反复空转。
- 建议：goal 续作时保留 guard 的 outcome 签名，或在 runtime 层加"连续 N 轮 judge=continue 且无状态变更工具成功"的收敛判定。

**[P2] judge 把 "blocked" 折叠成 DONE，而 `blocked_threshold` 只是提示词**

- 证据：`goal/judge.rs:54-55`「The response explains the goal is unachievable / blocked / needs user input (**treat this as DONE** …)」（contract 变体同义 `judge.rs:219-221`）；`goal/runtime.rs:239-242` 收到 Done 即把状态置 `Complete`；`goal/state.rs:120-121` 的注释自认 "P0 only constrains the model via the prompt"。
- 后果：judge 无法产出 `Blocked`；"卡住需要用户输入"在状态机里被记成 `Complete`，而模型一次 `update_goal(blocked)`（无阈值校验）又能立刻终止。两类终态都不可信，`/goal status` 的 `last_verdict` 与用户看到的状态不一致。
- 建议：judge 增加 `blocked` 裁决并映射到 `Blocked` + 用户可见事件；`update_goal(blocked)` 校验 `turns_used >= blocked_threshold`。

### 3.3 设计权衡、优点与疑问

**设计权衡**

- **judge 用主模型、无工具、单次 completion**：`goal/judge.rs:351-366`（`ThinkingConfig::Disabled`、`temperature: Some(0.0)`、`JUDGE_MAX_TOKENS = 4096`、30s 超时 `judge.rs:29,32,368`），并与主循环串行 await（`engine/mod.rs:2601`）⇒ 每轮固定一次额外 provider 调用 + 最多 30s 墙钟；输入有截断上限（goal ≤2000、contract ≤2500、response ≤4000 字符，`judge.rs:34-37`），8 轮约 8 次额外调用。省掉配置面，代价是不独立、不可验证。
- **fail-open 到 `Continue`**：空响应/非 JSON/未知 verdict/传输失败都不产生 `Done`（`judge.rs:424-433,492-527`），由独立断路器兜底（parse 连续 3 / transport 连续 5 → `Paused`，`state.rs:13-19`、`runtime.rs:246-261`）。方向正确（宁可不完成也不假完成），但"judge 永久坏掉"表现为目标静默不推进，且此前会烧掉最多 4 轮预算。
- **pause/resume 重置预算**（`runtime.rs:316-328`）与"每进程重置预算窗口"（`goal_bridge.rs:96-109`）是有意的逃生门，代价是预算并非安全边界。
- **契约自动起草是后台 fail-soft**（`manager/nomi/agent.rs:2994-3053`），失败仅告警 ⇒ 无契约时退回判据最弱的宽松形态。

**做得好的地方**

- 三分支续作模板**逐轮字节稳定**（`runtime.rs:11-20`，并有 `runtime.rs:977-1013` 的字节稳定性测试锁定）；空 contract 退化为无 contract 形态且同字节。
- wait barrier 不烧预算、惰性释放、fail-open，并有 session > pid > time 的优先级测试（`runtime.rs:263-284,423-431,706-724`）。
- judge 判据克制：明确"fluent answer ≠ done"、"evidence missing/weak/indirect → prefer CONTINUE"（`judge.rs:56-59`），wait 无 target 降级 continue。
- **goal 状态与对话历史解耦**：turn tail 每轮重渲染 objective/contract（`engine/mod.rs:1649-1651`、`runtime.rs:467-489`），所以压缩无法把目标"压没"——这是 plan mode 完全没有的性质。
- wire 契约有测试锁定（`state.rs:280-306`），`GoalState` 快照经 `goal_bridge.rs` 单点转换。

**未确认的疑问**

1. plan mode 与 goal 的交互：`update_goal` 自身是 `Info` 类（`goal/tool.rs:66-68`），因此 plan mode 下模型仍可标记终态，且续作不看 plan 门控（`engine/mod.rs:2582-2619` 无 plan 判断）；未找到宿主/UI 层禁止此组合的证据。
2. `extra.goal` 的生产入口链未逐段确认（只确认 `factory/nomi.rs:218` 反序列化 `options.extra`、禁用键列表未含 `goal`）。
3. `loop_guard` 触发后的确切行为（nudge 之后是 abort 还是继续）未读完 `loop_guard.rs`。
4. 子代理是否绝对不继承 goal：仅做了反证 grep（`local_delegate_tool.rs`、`local_agent_invocation.rs` 内无 `goal` 引用）。
5. judge 的真实 token/延迟未实测（上表为静态推导）。
6. 取消分支是否在 teardown 其他位置补落库（只核对了 `spawn_goal_persist` 的全部调用点）。

### 3.4 与 plan mode 的对照（同一仓库内的不一致）

| 维度 | plan mode | goal mode |
| --- | --- | --- |
| 持久化 | 无（`save_session()` 不含 `plan_state`） | 有（`snapshot`/`from_state`/`restore`） |
| 提示词注入 | turn tail（`engine/mod.rs:1611`） | turn tail（`runtime.rs:141-151`） |
| 终态/退出是否有机制校验 | 无（`ExitPlanMode` 无参数、无审批） | 有 judge，但可被 `update_goal` 短路 |
| 预算/硬停线 | 依赖 coding harness | `max_auto_continuations` + 双断路器 |

这张表本身就是结论：两个模式的成熟度不对称，plan mode 明显落后于 goal mode。

## 4. 上下文压缩（Compact）

### 4.1 实现事实（已验证）

- 三层结构：microcompact（工具输出）+ LLM autocompact + emergency gate。编排在
  `engine/mod.rs:3242-3300`（`estimate_tokens_from_request` → `micro::should_microcompact`
  → `auto::should_autocompact` → `auto::autocompact_with`），请求前的闸门在
  `engine/mod.rs:1719-1750`（先取 `max(上一轮 input_tokens, 本次本地估算)`，再判
  `auto::should_compact_before_turn` 与 `emergency::is_at_emergency_limit`）。
  （既有文档把它定位在 `2273-2360`，HEAD 已位移，见 §8。）
- 摘要提示词：`compact/prompt.rs:11-15`（system）与 `:24-66`（正文），要求输出 `<summary>`
  包裹的**七段式"次轮简报"**：Standing Facts & Constraints、Goal、Decisions & Rationale、
  Files & Code、Commands & Outcomes、Errors & Fixes、Pending & Next Step。
- **关键设计**：`compact/prompt.rs:11-13` 明说 "The agent keeps your summary alongside the
  user's own turns (kept verbatim) and the recent tail" —— 用户消息保留原文，只折叠
  assistant/tool 工作；并要求把**上一份简报合并进新简报**（`:37-39`），这是递归压缩的保真保护。
- 解析容错：`format_compact_summary` 先剥 `<analysis>`，取 `<summary>`；取不到标签就退化为
  原文；摘要为空也退化为原文（`:73-88`）。
- 恢复指令：`build_summary_content` 在 auto 场景追加"不要复述摘要、直接继续"（`:96-113`）。
- 水位线与估算：`emit` 侧把 `last_input_tokens` 取 `max(provider 上报值, 本地估算)`
  （`engine/mod.rs:1719-1720`）；本地估算 = 文本 `len/4`、JSON `len/3`、图片固定 1600
  token，并计入 system prompt、工具定义（deferred 工具的 schema 不计）与 turn tail
  （`compact/estimate.rs:4-66`）。

**触发阶梯（默认配置，均已取证）**

- **microcompact**（与 token 无关）：`compact/micro.rs:37-71` 两条或条件 —— 时间上最近一条带
  时间戳的 assistant 距今 ≥ `micro_gap_seconds`（默认 3600s），或"可压缩且未清空"的工具结果
  条数 > `micro_keep_recent × 2`（默认 5 ⇒ ≥11 条）；仅受 `enabled` 门控
  （`nomi-config/src/compact.rs:61-62`）。
- **autocompact**：`compact/auto.rs:76-93` —— 若配置了 `pct` 则阈值 = `window × pct/100`，
  否则 `window − output_reserve − autocompact_buffer`。实测换算：128k 窗口 → 95k（74.2%）；
  200k（后端把 context_limit 灌进 window，`manager/nomi/agent.rs:67-74`）→ 167k（83.5%）；
  1M → 96.7%。`fit_context_budget`（`nomi-config/src/compact.rs:183-214`）只缩 reserve/buffer/
  emergency_buffer，**不动 pct**。
- **回合起点追加触发**：`auto.rs:101-107`，`last_input_tokens >= context_window` 也算。
- **emergency**：`emergency.rs:26-31` 阈值 = `window − emergency_buffer`（默认 3000）⇒ 128k 窗口
  为 125k（97.7%）；`enabled = false` 也照样生效（`emergency.rs:24-25`、`engine/mod.rs:3202`）。
- 折叠比与下限：`COMPACT_FORCE_RATIO = 0.9`、`MIN_FOLD_TOKENS = 400`（`auto.rs:137,148,395`）。
- 估算器覆盖：`estimate_tokens_from_request` 计入 system prompt + 工具 schema + messages +
  turn tail（`estimate.rs:51-66`），图像按 1600 token/张；`context_usage` 的三个分桶覆盖全部 9 个
  system section（`context.rs:249-350`、`context_usage.rs:19-28`），无漏计。

这套提示词工程明显强于常见实现：它显式区分"用户约束/事实"与"模型工作"，显式要求合并历史简报，
且失败时降级而不是丢弃。

### 4.2 缺陷

**[P2] 本地 token 估算对中文系统性低估，注释里的"保守高估"在中文会话下不成立**

- 证据：`compact/estimate.rs:4-6` 用 `CHARS_PER_TOKEN_TEXT = 4`、`CHARS_PER_TOKEN_JSON = 3`；
  `:40-41` 注释声称 "Intentionally conservative (slightly over-estimates)"。
- 中文（以及日文/韩文）在主流 tokenizer 下约 1 字 ≈ 1 token，因此 `len/4` 对中文是
  **低估约 25%**（`text.len()` 是 **UTF-8 字节**：中文 3 字节/字 ÷ 4 ≈ 0.75 token/字，而实际约
  1 token/字），方向与注释相反。本仓的提示词、计划、续作模板全是中文，这不是边缘情形。
- 缓解：`engine/mod.rs:1719-1720` 用 `max(provider 上报 input_tokens, 本地估算)`，所以拿到
  第一轮真实 usage 之后水位线以 provider 为准；风险集中在"尚无 provider 数字"的窗口
  （恢复会话首轮、刚压缩完的下一轮）以及 `should_compact_before_turn` 的判定。
- 建议：按字符集加权（CJK 记 1 token/字），或直接用 provider 的分词器/上报值作为唯一水位线，
  并把注释改成与实现一致的表述。

#### 审计补充的缺陷（按严重度）

**[P0] 摘要失败即销毁历史，而熔断器永远不会生效**

- 证据：`compact/auto.rs:412-432` —— 摘要调用返回 `Err` 时走
  `(mechanical_fold_digest(...), true, …)`，即**机械折叠**；占位符文本是
  `auto.rs:617-622` 的「N earlier message(s) were folded here … Ask the user if you need details
  from before this point.」；`engine/mod.rs:3340-3351` 无条件 `self.messages = result.messages`；
  `engine/mod.rs:2621-2623` 压缩后立即 `save_session()` **落盘**。
- 同时：`record_failure()` 全仓只被 `state.rs:65-67` 的自身测试调用，**没有任何生产调用点**，
  因此 `consecutive_failures` 恒为 0 ⇒ `auto.rs:370` 的 `CircuitBroken`、
  `engine/mod.rs:3374-3378` 的 Err 分支、配置里的 `max_failures` 全是死路径。
- 后果：一次 provider 500 或网络抖动（`auto.rs:602-605` 只重试一次）就会把整段历史替换成占位符
  并写进会话文件。这是本报告里唯一"一次瞬时故障即造成不可逆上下文损失"的路径。
- 建议：摘要失败时**中止本次压缩**（不改 `messages`、不落盘）；只在窗口确实仍然爆满时才退到机械
  折叠；把失败计数真正接回 `record_failure()`。

**[P1] 摘要在回合结束的关键路径上同步阻塞，最坏约 4×90s**

- 证据：`engine/mod.rs:2551` 与 `:2621` 的 `self.run_compaction(CompactReason::TurnEnd).await?;`
  位于 `save_session()` 与返回 `AgentResult` **之前**；`auto.rs:539-554` 单次 90s 超时；
  `auto.rs:519,602-605` 瞬时错误重试一次（`continue` 重新计 90s）；`auto.rs:570-596` PTL 再重试 2 次。
- 后果：文本已经流式输出完毕，但回合迟迟不终结、不落盘，用户看到"回答完了却还在转"。

**[P1] 摘要调用本身是全价 prefill，且重写消息中段 → provider 前缀缓存失效**

- 证据：`auto.rs:499-516` 把整个 fold 区间作为对话再发一次；`auto.rs:466-472` 的输出结构是
  `head + kept + boundary + summary + tail`（fold 区间被删除）；`engine/mod.rs:3353`
  `cache_detector.notify_compaction()`。
- 后果：默认 128k 阈值下，单次摘要请求的输入可达约 95k token（全价 prefill），且插入点之后
  （含 tail）的 provider 前缀缓存全部失效，下一回合要重新 prefill。压缩的收益与成本在长会话里
  不是单向的，本仓目前也没有针对这条路径的成本量化。

**[P2] PTL 重试的 20% 截断只保证首条是 User，可能切断 tool_use/tool_result 配对**

- 证据：`auto.rs:737-762` —— `let drop_count = (messages.len() / 5).max(1);` 然后
  `let remaining = &messages[drop_count..];`，之后只检查 `remaining.first()` 的角色是否为
  `User` 并插入占位（`auto.rs:750-758`）。
- 后果：落点若是孤立的 user tool_result，摘要请求自身会变成 provider-invalid ⇒ 失败 ⇒ 退回到上面
  那条破坏性的机械折叠。建议截断后按 block 重新对齐到 assistant tool_use 边界
  （复用 `align_tail_start` 的思路）。**置信度 中**（provider 侧最终校验未验证）。

**[P2] 熔断器唯一的集成测试根本触发不到压缩**

- 证据：`tests/engine_compact_test.rs:966-972` 用 `context_window: 500_000` 配默认 reserve
  （20k/13k）⇒ 阈值 467k，而该 mock provider 只回报 170k（`:930,942`）；断言只有
  `assert_eq!(result.text, "Final")`（`:987`），没有任何失败计数或熔断断言。
- 后果：测试名「Circuit breaker after repeated failures」与它实际跑的路径不符，给出**虚假安全感**
  ——这也解释了为什么 P0 那条死路径一直没被发现。

**[P2] 50% 软提醒与紧急用户文案都是死代码**

- 证据：`compact/state.rs:71-80` 的 `check_soft_compact` 无生产调用（仅自身测试）；
  `emergency.rs:13-14` 的 `EMERGENCY_USER_MESSAGE` 只被 `emergency.rs:117-127` 的测试使用；
  真实路径抛 `AgentError::ContextTooLong`（`engine/mod.rs:1744-1750, 3206-3212`），而文案
  （`engine/mod.rs:3651-3652`）只说 token 数，从不提示用户可以用 `/compact`。
- 后果：用户在最需要引导的时刻（上下文爆掉）拿不到可操作的建议。

**[P2] 摘要器看不到它被要求归纳的那些用户原话**

- 证据：`auto.rs:291-306` 的 `partition_fold` 把"小且纯文本的 user 消息"放进 `kept`（不进 fold），
  而 `auto.rs:421` 只把 `fold` 交给摘要器；同时 `compact/prompt.rs:31-35` 要求它写出
  "hard never do X rules"。
- 后果：用户指令的去向出现分叉——一部分原样保留、一部分既不在摘要输入里也不在保留集里；摘要与
  保留的用户话是"盲接"。建议把 kept 用户消息也作为只读上下文发给摘要器，并在提示词里区分
  "可见的用户原话"。

**[nit] 尾部对齐不躲 boundary marker**

- 证据：`auto.rs:250-258` 只越过 `is_tool_result_message` 与 `is_compact_summary`，不检查
  `is_compact_boundary`；而 `context_usage.rs:117-125` 用 `rev().find_map(extract_compact_metadata)`
  取向量中最后一个 boundary。
- 后果：可能出现第二条 boundary；若旧 boundary 落在 tail 内，用量面板会报**旧的** trigger/统计。

### 4.3 切点完整性（正面结论）

- 尾部选点：`auto.rs:232-286` —— `tail_start` 从最新往回累计到 `TAIL_TOKEN_BUDGET = 16384` 且
  不超过 `window/2`，`MIN_RECENT_KEEP = 4` 保证尾部至少 4 条且 `start <= len-4`；随后
  `align_tail_start`（`:250-258`）向前越过 user tool_result，从而把其前面的 assistant tool_use
  一并纳入尾部。
- **正常路径不会切断 tool_use/tool_result 配对，也不会丢整条消息**（工具结果始终单独成一条 user
  消息，`engine/mod.rs:3044`）。唯一已知例外是上面 PTL 截断那条，且它作用在**摘要请求**上，
  不是主请求。
- 中途触发只可能发生在 `turn == 0`（`engine/mod.rs:1722-1732`）或紧急/溢出恢复
  （`:1734-1753, 1779-1784, 2188-2193`），都在两次 provider pass 之间、工具结果已入库之后，
  尾部保留最近 ≥4 条 ⇒ 当前工具对一定在尾部。
- 多模态：估算计入 `ContentBlock::Image` 与 tool-result images（`estimate.rs:86-93`）；
  `microcompact` 会清空工具结果里的 images（`micro.rs:125`），但不清用户消息里的 Image。

### 4.4 设计权衡、优点与疑问

**做得好的地方**

- **状态后写、无 tombstone**：`autocompact_with` 只返回新的 `Vec`，`engine/mod.rs:3351` 才赋值
  （`/compact` 同样在 `Ok` 之后赋值，`commands/compact.rs:55`）——不存在"已标记压缩但摘要缺失"
  的中间态，失败不会把会话锁死。
- **停滞闩锁** `compact_stuck`（`state.rs:11, 93-106`；`engine/mod.rs:3190, 3198`）防止连环压缩。
- **水位线取 `max(provider 上报, 本地估计)`**（`engine/mod.rs:2308-2323`），不单信启发式；压缩是
  唯一允许下调水位线的路径（`:2303-2306`），且下调有据可查。
- boundary 携带 JSON metadata（`auto.rs:443-446`），`context_usage.rs:67-74` 能把"摘要区"与
  "活跃对话"分桶，可观测性好。
- 与 `nomi-compact` 的关系是清晰的：后者是**输出文本**压缩（`nomi-compact/src/lib.rs:10-24`，
  ANSI/重复行/JSON/TOON），用在 `tool_execution.rs:605-607`，另外提供 TOON 提示词
  （`context.rs:315`）；**上下文压缩不使用它**。命名相近但职责不重叠，没有混淆实现。

**未能确认的疑问**

1. 原始 transcript 是否在别处仍可回读：引擎侧 `save_session` 用压缩后的列表**整体覆盖**
   （`engine/mod.rs:3431-3434`），模型侧也没有回读工具。宿主侧已补查到：`nomifun-db` 的
   `001_v3_baseline.sql:94` 有独立的 `messages` 表，`nomifun-conversation` 有大量
   `insert_message` 调用点（`message_persistence.rs:50,78`、`stream_relay.rs:3803` 等），
   说明**用户可见的消息层有独立副本**；但工具结果/中间步骤是否入表、是否存在
   `clear_terminal_conversation_messages`（`service.rs:13897`）之外的截断语义，**仍未验证**。
   这条决定了 #30 是"数据丢失"还是仅仅"模型失忆且无法回读"。
2. 两次压缩能否并发（业务层 turn lease / engine 之外的 `/compact` 入口）**未验证**；代码上压缩
   只经 `&mut self`（`engine/mod.rs:3186`）。
3. provider adapter 对孤立 tool_result / 首条 assistant 的最终校验与 400 行为 **未验证**。
4. 1M 窗口下 96.7% 才触发自动压缩是否过晚（`fit_context_budget` 只留约 33k 余量）**未实测**。

## 5. 子代理与委派

### 5.1 调用链路（真实函数名）

```text
宿主用户轮 (backend nomifun-ai-agent/src/manager/nomi/agent.rs:1819 select! → engine.execute_turn_with_content_for_source)
 → engine/mod.rs:1432 execute_turn_inner → :2646/:2678 execute_tool_calls_scoped
 → local_delegate_tool.rs:80 LocalDelegateTool::execute   [或 host_delegate_tool.rs:152 HostDelegateTool::execute → HostDelegateSink::plan]
 → parse_request(136) → task_invocation(147) → LocalAgentInvocationRunner::execute_fanout(local_agent_invocation.rs:201)
 → FanoutWorkspacePlan::for_scopes(750) → execute_bounded(531)（JoinSet + Semaphore）
 → invoke_with_effective_scope(102) → effective_tool_scope(168) → config_for_invocation(174)
 → build_tool_registry(898) → AgentEngine::new_with_provider(620) → execute_delegated_agent(655, timeout 300s)
 → map_agent_invocation_outcome(584) →（可选 invoke_one → synthesizer(98)）→ completed(191) → AgentExecutionReceipt JSON
 → ToolResult → tool_execution.rs:604 truncate_result(默认 50_000 字符) → 父 transcript
用户停止：宿主丢弃 execute_turn future → execute_bounded 的 JoinSet Drop 中止全部子任务
```

### 5.2 缺陷

**[P0] 子代理绕过宿主审批通道，退回 stdin 交互确认（EOF 等于"批准"）**

- 证据：`engine/mod.rs:660` 子引擎构造时 `approval_manager: None`；`:2646` 只有在
  `self.approval_manager` 存在时才走协议审批，否则落到 `:2678 execute_tool_calls_scoped(… &self.confirmer …)`；
  `confirm.rs:46` `if self.auto_approve || self.allow_list.contains(tool_name) { Approved }`，
  `confirm.rs:62` `"y" | "yes" | "" => ConfirmResult::Approved`（stdin EOF → 空串 → **批准**）；
  宿主侧 `manager/nomi/agent.rs:836` `auto_approve = session_mode == "yolo"`（非 yolo 即 false）；
  默认 allow_list 只有 Read/Grep/Glob（`nomi-config/src/config.rs:621-623`）。
- 后果：子代理的 Write/Edit/Bash 不走父级的审批协议；桌面端会阻塞在无人输入的 `read_line`
  （父轮次被拖到 300s 超时），Web 端 stdin 为 EOF 则**直接批准**——子代理在用户未审批的情况下拿到写权限。
- 建议：子引擎显式降级为拒绝（除非父级 `auto_approve` 或 allow_list 已覆盖），或透传父级的审批结论。
  置信度：机制 高 / 各宿主 stdin 运行态 中。

**[P0] 子代理的模型调用丢失本轮计费归属（`x-flowy-turn-id`）**

- 证据：`nomi-providers/src/billing_turn.rs:13-15` 用 `tokio::task_local!` 承载 turn id，
  `openai.rs:74` 据此加 `FLOWY_TURN_ID_HEADER`；子任务在 `local_agent_invocation.rs:543`
  的 `set.spawn(async move { … })` 内运行，而父级 scope 在宿主 `agent.rs:1831` 的
  `with_flowy_billing_turn_id(data.msg_id, engine.execute_turn…)`。
- 后果：`task_local` 不随 `tokio::spawn` 传播，子代理（含 fork skill，共用同一 runner）的 provider
  请求不带 turn id ⇒ 同一次 Agent Run 的额度/成本聚合漏掉子代理 token，也就是最烧钱的那部分。
- 建议：在 spawn 内重新 `with_flowy_billing_turn_id`（把 turn id 作为参数传进 runner），或改用显式
  请求上下文而不是 task-local。置信度：机制 高 / 是否有其他归因兜底 中。

**[P1] 子代理产出只有一条 JSON，且被 50k 硬截断、无持久化可恢复**

- 证据：`nomi-tools/src/lib.rs:262-264` `max_result_size()` 默认 50_000，两个 delegate 工具都未覆写；
  `tool_execution.rs:604` 调 `truncate_result`，`:1098-1122` 保留 head 25k + tail 25k、挖掉中段；
  子引擎输出走 `NullSink`（`local_agent_invocation.rs:145`）且 `config.session.enabled = false`（`:183`）。
- 后果：16 个子代理（`nomi-types/src/agent.rs:266` `MAX_TASKS = 16`）的输出拼进同一 receipt 时必然
  超限，中间子代理的结论被裁掉；`compact_output/toon` 还会改写内容（`tool_execution.rs:605-610`），
  receipt 不再是合法 JSON；子会话不落盘 ⇒ 被裁掉的部分**无法找回**，也没有审计痕迹。
- 建议：覆写 delegate 的 `max_result_size`，或把每个子代理的输出落成 artifact/独立会话文件，
  receipt 只放摘要 + 句柄。

**[P1] 委派期间父级与用户零进度（进度只在兄弟子代理之间可见）**

- 证据：子引擎用 `NullSink`（`local_agent_invocation.rs:145`）；`:238-257` 只在
  `invocations.len() >= 2` 时把 `SiblingProgressContributor` 注册到**兄弟**子引擎；
  `local_delegation_progress.rs:84-115` 的 `render_for(viewer_index)` 只渲染给兄弟；runner 没有向父
  `OutputSink` 汇报的接缝。
- 后果：`execute_fanout` 同步阻塞父轮次（单子最坏 300s，16 个分两波最坏约 600s），期间 UI 只有一条
  "Delegate: …"，用户无法区分卡死与推进。
- 建议：给 runner 注入父级进度事件（阶段 = 调度/运行/合成）。

**[P1] 超时不可配置、并发与总量上限靠常量与可选预算，默认没有累计上限**

- 证据：`local_agent_invocation.rs:457` `DELEGATED_AGENT_TIMEOUT = 300s`（无配置读取点）；
  `:463 MAX_CONCURRENT_DELEGATIONS = 8`；token 预算来自 `config.tools.delegation_token_budget`
  （`bootstrap.rs:927-934`），而 `nomi-config/src/config.rs:471` 的默认值是 `None`（不封顶），
  `:466-470` 的注释也自认是 soft ceiling；单子代理轮数上限 `local_delegate_tool.rs:18` = 200。
- 后果：超过 5 分钟的合法任务（构建、评测）必被砍；默认组合下 16×200 轮没有总量约束，只有并发 8
  的节流；父引擎只累加 provider 轮 token（`engine/mod.rs:2297-2301`），子代理用量看不到。
- 建议：超时/并发/预算进配置；把 receipt 里的 usage 计入父级会话用量。

**[P2] 空结果与任务错位不会被发现**

- 证据：`map_agent_invocation_outcome:593-635` 只对 MaxTokens/MaxTurns/Refusal/四事实判 `is_error`，
  没有"text 为空"判据；`completed:196-205` 只统计 `is_error`；`execute_bounded:300-313` 里 join 失败
  的项以 `name: "unknown"` 追加到尾部。
- 后果：子代理返回空文本时父级收到 `status=completed` 与空 text；panic 时 receipt 的 `results` 与
  `tasks` 不再一一对应，父级无法知道丢了哪个子任务。
- 建议：空文本显式标 empty/failed；spawn 时把 task 名一起带出。

**[P2] 工作区隔离只覆盖"并行且 ≥2 个可写"的情形；取消会销毁隔离区里未提交的成果**

- 证据：`local_agent_invocation.rs:750-760` `has_parallel_mutation = mutation_capable >= 2`，
  `isolate_mutating = has_parallel_mutation && is_git_repo`；`:771-775` 非 git 仓库只给子代理一段文本
  警告，写入仍直落父工作区；`nomi-tools/src/worktree.rs:807-823` 在 Drop 时 `git worktree remove --force`，
  注释自认 "discards the worktree's uncommitted edits"；隔离产生的 diff 只作为**文本补丁**回传
  （`:359-365`），宿主不会自动应用。
- 后果：单写子代理与非 git 工作区没有隔离，冲突仲裁留给父级；父轮次被取消时隔离区里未 capture 的
  改动被静默删除。
- 建议：写路径强制要求 git 隔离或给出更硬的告警；取消时先 capture diff 再清理。

**[P2] 委派没有结果校验，子代理的工作不留痕**

- 证据：`local_delegate_tool.rs:191-246` 的 `completed()` 只聚合文本 + usage；子代理从不经过
  bootstrap（`bootstrap.rs:915-954` 只给父级装 runner/tool），子引擎的 `goal: None`
  （`engine/mod.rs:675`）、`coding_harness: None`（`:681`）、`observation: None`（`:687`）。
- 后果：子代理"声称完成"的唯一机器判据是四事实启发式（`local_agent_invocation.rs:609-619`）与
  worktree diff；没有 Session Logs、没有 observation，审计链断裂（谁写的、写了什么）。
- 建议：把子引擎纳入 trace/observation，并在 receipt 里带 changed-files 摘要。

### 5.3 设计权衡、优点与疑问

**设计权衡**

- **两版 delegate 工具互斥**：嵌入版 `LocalDelegateTool`（parallel-only、1-16 任务、200 轮、同步内联、
  无独立生命周期，`local_delegate_tool.rs:19-26, 80-115`，由 `bootstrap.rs:915-954` 在
  `install_embedded_agent_execution` 时注册）与宿主版 `HostDelegateTool`（planned-only、
  `deny_unknown_fields` 拒绝 `max_parallel/work_dir/members` 等宿主旋钮、立即返回 planning receipt、
  后台续跑，`host_delegate_tool.rs:37-45, 67-94, 143-161`）。`tests/bootstrap_test.rs:213-220` 断言宿主
  组合会移除嵌入版。
- **子代理不可能再委派**：子注册表只注册 6 个内建工具（`local_agent_invocation.rs:800-831` 的
  `CHILD_TOOL_CATALOG`、`:938-964`），没有 nomi_delegate/Skill/MCP/记忆工具。全仓找不到 depth 常量
  ——不是"有深度上限"，而是**结构上不可嵌套**；这比一个可配置的深度数字更可靠。
- **hooks 全部清空**：`config_for_invocation:184-189` 把 `config.hooks` 置为默认空值，理由是 hooks
  执行裸 shell、会绕过子注册表与进程监督器；测试 `:1259-1338` 论证了即使是 read_only 子代理也无法
  触发父级可变 hook。父级权限以 `parent_tool_scope ∩ policy ∩ exact_tools` 单调收窄（`:168-172`，
  测试 `:1476-1547`）。
- **能力与隔离**：shell 继承父级 `CapabilityPolicy` 但把 `cwd_roots` 收窄到子 cwd（`:908-937`），
  Write/Edit 带 `write_root`（`:945-954`），Seatbelt write_roots 同步翻译；继承的拒绝不降级
  （测试 `:1550-1569`）。子引擎 `messages: Vec::new()`（`engine/mod.rs:646`）看不到父 transcript，
  但**继承父级完整 system prompt**（`bootstrap.rs:892-903 + 915-921`，`local_delegate_tool.rs:158`
  的 `system_prompt: None` 不覆盖）⇒ AGENTS.md/记忆/环境段每个子代理重新付一次；隔离 worktree 的
  子代理 cwd 与 prompt 里的 "Working directory" 段还会不一致。
- **取消传播是干净的**：宿主 biased select（`agent.rs:1819-1830`）丢弃整条 engine future →
  `execute_bounded` 的 JoinSet Drop 中止全部子任务（`:528-530`，测试 `:1806-1839`）→ 每个子引擎的
  `SupervisorShutdownOnDrop`（`:661, 689-714`）经专用 relay 线程 shutdown（`:716-741`）回收受监督进程。
  父轮次正常结束时子任务全被 await，没有 fire-and-forget ⇒ **不存在未被 join 的泄漏子代理**。
- **结果契约是结构化的**：回传 `AgentExecutionReceipt`（`nomi-types/src/agent.rs:142-240`），失败/超时用
  `status=failed|completed_with_failures` 而非抛工具错误（`local_delegate_tool.rs:199-217, 239-245`），
  超时文本可识别（`:646`），不完整的 partial 文本保留作证据（`:622-635`）。
- **`is_deferred() == true`**：两个 delegate 工具都要先经 ToolSearch 激活、下一轮才可调用
  （`tool_execution.rs:351-372`），激活后不再 deferred。
- plan mode 与委派的关系在**入口**被挡住：delegate 工具是 Exec 类目
  （`local_delegate_tool.rs:117`、`host_delegate_tool.rs:163`），plan mode 只广告 Info
  （`engine/mod.rs:1550-1556`），所以"plan 状态没传给子代理"这件事在正常入口不可达——但 §2.3 的
  `Skill(fork)` 通道绕过了这个入口。

**做得好的地方**

- 逐子引擎独立 `ProcessSupervisor`，并把 capability/Seatbelt 收窄到子 cwd；继承的拒绝不可降级。
- hooks 全清有专门回归测试并写明理由。
- 兄弟进度注入用"不可信 JSON 边界 + 转义"（`local_delegation_progress.rs:100-114`，防越界测试
  `:262-286`），且未完成 guard 投射 failed 而不是陈旧的 running（`:148-154`）。
- 共享生命周期 vocabulary 与合法迁移表（`nomi-types/src/agent.rs:13-98`），宿主旋钮逐字段拒绝
  （`host_delegate_tool.rs:275-281`）。
- "四事实"不完整判据（`local_agent_invocation.rs:601-621`）避免把 plan/Info-only 子代理误判为失败
  （`:1443-1458`）。

**未能确认的疑问**

1. 各宿主 stdin 的真实状态（决定上面 P0 是"卡死"还是"静默批准"）——只验证了代码路径，**未验证运行时**。
2. Flowy 代理侧是否有 turn id 之外的归因兜底（决定计费漏记的可观测后果）——**未验证**。
3. 子代理 receipt 的 usage 是否在宿主某处被计入会话成本——只确认父引擎不累加，未穷尽计费路径（中置信）。
4. 任务 abort 瞬间写工具是否可能留下半截文件（未读 `nomi-tools/src/write.rs`）——**未验证**。
5. 进程被强杀（非协作退出）时 supervisor/relay 能否回收整棵进程树——**未验证**。
6. `install_embedded_agent_execution` 在各部署下的取值（`backend/.../factory/nomi.rs:1723`）未逐分支读完，
   "哪个宿主暴露嵌入版"只按 `tests/bootstrap_test.rs:213-242` 的契约与函数名推断（中置信）。

### 5.4 planned 模式：宿主持久化的那一半

`nomi_delegate` 有两种**互斥部署**（一个会话只注册一个）：嵌入版 `LocalDelegateTool`（parallel-only、
同步 inline，见 §5.1–5.3）与宿主版 `HostDelegateTool`（**planned-only**、durable）。后者才是"团队 /
Agent Store"主线上唯一有持久化与回写语义的路径，前面几节完全没覆盖。

**模型面契约**（`host_delegate_tool.rs:100-201`）：schema 只有 `{strategy: enum["planned"], goal}`，
`additionalProperties: false`；deferred；`category = Exec`。`execute` 只有三段：parse → `sink.plan(goal)`
→ `accepted(receipt)` 或 `rejected(error)`。关键语义在 `:183-192`：**"开始了一个 execution 就是一次
成功的工具调用，尽管活还在跑"，只有被拒绝的请求才是 error**。回执在**规划提交那一刻定型、之后永不
更新**（`status: "planning"`，`step_ids/results/synthesis/summary` 全不出现），所以模型的动作就是
"发一次调用、结束本回合"。

**组合门槛**（`factory/nomi.rs:1723-1729`、`factory/delegate.rs:10-14`）：嵌入版由
`host_allows_embedded && !has_platform_gateway && is_instance_owner` 决定——是"已解析的运行时权威 +
宿主组合决定，不是用户配置"；planned 版由 `DelegateSinkProviderSlot` 是否被 install 决定，该槽
**刻意不是 fail-closed**（没装 = 这个宿主没有 durable facade，CLI 宿主只走嵌入版是合法组合），
装第二次是 `Conflict`。

**端到端链路**

```text
HostDelegateTool::execute (host_delegate_tool.rs:152) → EngineDelegateSink::plan / plan_via_engine (app_server_delegate.rs:84/102)
 → ConversationService::get + delegation_policy 闸 (116) → agent_caller_for_delegation (122)
 ├─ attempt 会话: execution_for_attempt_conversation (129) → delegate_from_attempt (135) → append_steps_from_attempt
 └─ 新执行: create_from_conversation (215) | create_from_template_for_conversation (193)
      → persist_execution (engine.rs:445) → INSERT agent_executions + participants + conversation_execution_links(relation=lead)
      → spawn_initial_plan (517, tokio::spawn) → probe_plan → LlmPlanProducer::produce (planner.rs:256)
      → parse_plan_opt (428) → plan_materializer::materialize → reconcile_plan (status=Running)
 → ExecutionScheduler::start (scheduler.rs:262, tokio::spawn) → execute_loop (775) → ready_steps (2344)
 → select_agent_steps (2449) → execute_agent_step (1473) → ConversationAttemptRunner::execute (attempt_runner.rs:378)
 → settle_agent_outcome (1677) → finalize_if_settled (2058) → terminal_summary (2635) → after_terminal_commit (577)
 → reconcile_lead_report (537) → ConversationEffects::report_lead (production.rs:93)
 → project_assistant_message_idempotent (nomifun-conversation/service.rs:9422) → WS message.stream
```

**关键机制与数字**

| 项 | 事实 |
| --- | --- |
| 前置条件 | owner 合法 + conversation 属于该 owner + `delegation_policy != Disabled` + **该会话没有未终态的 lead 执行**（`sqlite_agent_execution.rs:1763-1783`）。绑定 Team 模板**不是**前置条件；没有 feature gate，"不该委托的 host 干脆不安装 provider" |
| 写表 | `agent_executions` + `agent_execution_participants` + `conversation_execution_links(relation='lead')` |
| Planner | 用**同一个 lead participant 的模型**（`pick_lead`，planner.rs:308），一次非流式问答，`PLAN_MAX_TOKENS=8192`、`PLAN_TIMEOUT=5min`；解析走"剥栅栏 + 大括号配平"，`steps` 为空即非法；**非法或散文输出不失败，而是退化成单步计划**（`fail_execution`，participant_index=0），照样以 Running 起跑 |
| 审批门 | delegate 路径硬编码 `plan_gate: PlanGate::Automatic` ⇒ `AwaitingApproval` / `Engine::approve` 在 planned 模式**不可达**（见 #39） |
| 物化 | `agent_execution_steps` + `agent_execution_step_dependencies`；上限 `MAX_AGENT_EXECUTION_STEPS=128`；物化期做拓扑/环校验并**完成 participant 路由** |
| ready 定义 | `Pending` + `dispatch_after<=now` + 所有未 superseded 依赖 `Completed`；按 `created_at` + `step_id` 排序（重启后确定性） |
| 并发 | 默认 `DEFAULT_MAX_PARALLEL=4`（模板可 1..64）+ per-participant `max_concurrency`；控制节点（verify/judge/loop）一次一个、同步求值 |
| 调度模型 | 不是 tick/worker：`tokio::spawn(execute_loop)`，`select!` 等 in-flight job 或 `next_retry_at`；重启由 `engine.recover()` 重建，单飞靠 DB lease（`LEASE_DURATION_MS=30_000`，10s 续约） |
| 超时/重试 | `DEFAULT_ATTEMPT_TIMEOUT=30min`、`MAX_PROVIDER_RETRIES=2`、`MAX_TIMEOUT_RETRIES=1`、退避 `1s<<attempt_no`；**只有 `AdaptationPolicy::Adaptive` 才重试模型失败，而 delegate 路径是 `Fixed`**（见 #40） |
| attempt 会话 | `create_idempotent(..., attempt_creation_key=attempt_id)`，名字 `协作 · {step_title}`；`extra` 只放运行态（`session_mode=yolo`、`system_prompt=brief`、收窄的 `allowed_tools`、`workspace`）；**身份唯一事实源是 `conversation_execution_links`**，单测断言 extra 里没有 execution_id/step_id/attempt_id |
| 再派生 | attempt 会话里再调 `nomi_delegate` → `delegate_from_attempt` 往**同一个 DAG** 追加；深度上限 `MAX_AGENT_DELEGATION_DEPTH=4`，到顶后该 attempt 的 `delegation_policy=Disabled` 且网关屏蔽工具 |
| 写回 | 终态提交 → 事件带 `lead_report_operation_id = "exec-lead-report:{id}:event:{seq}"` → `report_lead` 取 `execution.summary`（空则"执行已结束，但没有生成汇总。"）→ 投影为一条 assistant 消息 + `conversation_delivery_receipts` 去重 → WS `message.stream`。**不经过 lead 模型、不再跑一次 LLM 汇总、不是新 turn**（production.rs:108-110 原文 "never feed it back through the lead model"） |
| 用户控制面 | 在 `routes.rs`（`GET /api/agent-executions/{id}`、`/events`、`/workspace`、`POST /cancel`、`/pause`、`/resume`、`/approve`、`/replan`、`/adjust`、`/steps/{id}/retry|steer|adopt|configure|reassign`、`/attempts/{attempt}/answer`），按 `CurrentUser` + owner 作用域。`control_steps.rs` **不是**控制面，它是 verify/judge/loop 控制节点的纯求值器 |

**缺陷**

**[P1] 终态回写的进程内重试是死代码，"汇总回写本会话"会在一次瞬时错误后静默丢失**

- 证据：`scheduler.rs:542-548` 只在 `reconcile_lead_report_once` 返回 `Ok(false)` 时才调
  `schedule_lead_report_reconciliation`；而该函数四条出口全是 `Ok(true)`（`:599,643,646,673`）
  ⇒ `Ok(false)` 不可达，`schedule_lead_report_reconciliation`（`:676-722`）与 `pending_lead_reports`
  整个重试循环是死路径；真出错时 `report_lead(...).await?` 直接上抛，`after_terminal_commit`
  只 `tracing::warn`（`:586-588`）。
- 后果：终态那一刻的瞬时 DB/投影错误就会丢掉工具承诺的"汇总回写"，只能等进程重启
  （`engine.recover()`）或用户恰好触发 `add_steps`/`retry_step`/`adopt_step_output` 才补投。
- 建议：Err 路径也排重试，或让 `..._once` 在投影失败时返回 `Ok(false)`。

**[P1] "审批策略来自 Team 模板"在代码里不成立**

- 证据：`app_server_delegate.rs:166` 硬编码 `plan_gate: PlanGate::Automatic`；模板表根本没有
  `plan_gate` 列（`001_v3_baseline.sql:215-241` 只有 `max_parallel`/`work_dir`/`context`/
  `primary_participant_id`）；而 `host_delegate_tool.rs:43-44` 与 `app_server_delegate.rs:13-14`
  都宣称 approval policy 来自模板/服务器策略。
- 后果：planned 模式永远自动开工，`AwaitingApproval` 与 `/approve` 不可达，但 UI 已暴露该按钮
  ⇒ 文档与实现互相矛盾。

**[P2] 无自适应重试：一次模型失败即终止整个执行**

- 证据：`adaptation_policy: AdaptationPolicy::Fixed`（`app_server_delegate.rs:167`）使
  `scheduler.rs:1782-1784` 的 `can_retry = adaptation == Adaptive && …` 永不成立；只有"Queued
  未启动"的派发失败会重试（`attempt_no <= MAX_PROVIDER_RETRIES`）。
- 后果：长 DAG 里一次 5xx/超时即按 `failure_policy` 判 Failed 或把下游置 Skipped，与"host 会继续
  把活干完"的模型侧承诺张力很大。

**[P2] 追加路径丢弃 `receipt.step_ids`**

- 证据：`app_server_delegate.rs:145-153` 只把数量写进 message；`AgentExecutionReceipt.step_ids`
  （注释明确"Steps appended to an already-running execution"）始终为空，而网关路径是填的
  （`caps_agent_execution.rs:846`）。
- 后果：参与者追加委托后无法引用新 step id 做 steer/reassign。

**[P2] 词汇不一致：`DecisionRequested` 被映射成 `approval.requested`**

- 证据：`runtime_adapter.rs:707`；领域词汇是 decision（`decision_policy` + `/answer`）。
- 后果：容易被客户端/读者误当成计划审批，与上一条叠加。

**值得肯定**

- 契约极窄且拒绝未知字段：`max_parallel`/`plan_gate`/`adaptation_policy`/`members`/`work_dir` 被
  **显式拒绝**而不是静默忽略（`host_delegate_tool.rs:62-74`）。
- 非法计划在**写库前**就被挡掉（环检测、空 title/spec、控制步不得声明路由字段），`Explicit` 计划
  还会预检；计划输出与持久化共用同一套 typed step 词汇。
- 调度确定性（`created_at` + `step_id` 排序）与 lease + generation fence 保证单飞、且"stop 后立刻
  start 不丢唤醒"。
- 回写幂等：稳定 `operation_id` + delivery receipt 支撑崩溃重放；终态后的 retry/adopt 会**先强制
  补投旧 epoch 再 reopen**，把"未送达的终态结果"当成可恢复状态——思路正确，只是被上面那条死代码
  拖累。
- 显式 `cancel` **故意不回写**（`engine.rs:1441-1445`）。

**未验证**：lead 会话正在流式输出时新插入的 report 消息是否会被**当前这一轮**的模型上下文看到；
会话"归档"是否保留 `conversations` 行、投影是否成功；`plan_gate`/`adaptation_policy` 是否存在
planned 之外的上游写入点；`recover()` 补投与首个用户请求的时序。lead 会话被删时投影报 NotFound、
只 `tracing::warn`，summary 留在 `agent_executions.summary` 但无人接收（已确认链路，未实测）。

## 6. 与 Codex / Kimi / Claude Code 的对照

本节只做**机制对照**，不做性能或优劣跑分（同模型、同工具、同工作负载的对比本报告没有做，
`agent-loop-comparison.zh.md` §5 对此已有明确告诫，本报告沿用）。

**来源与可信度**：Codex 侧结论来自 `openai/codex` 仓库的 raw 源码 / 模板 / config schema；
Claude Code 侧来自官方文档的 markdown 端点（`code.claude.com/docs/en/<page>.md`），其中
MicroCompact/Session Memory 的具体数值与 `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES` 来自第三方对
minified bundle 的**逆向分析（二手）**，凡引用处已标注；Kimi 侧来自 Moonshot 自有仓库与文档。
凡未能从一手源核实的，一律标 `未验证`，请不要把本节的表格当作官方规格书。

对照的四个轴，是前面四节反复出现的分歧点：

| 轴 | 问题 | Flowy 现状（本报告已取证） |
| --- | --- | --- |
| A. 约束靠机制还是靠提示词 | 只读/审批/完成判定由谁强制 | 工具面过滤 + 请求级 authority 是**机制**（好）；但 `Info` 类目被当作只读用，且完成判定可被 `update_goal` 短路（坏） |
| B. 是否有可审计产物 | 计划/目标能否被用户看到、批准、恢复 | goal 有 contract/subgoals/快照；plan 完全没有产物（`plan_content: None`、`plan/file.rs` 死代码） |
| C. 压缩保留什么、失败怎么办 | 用户原话、约束、失败降级 | 用户消息原样保留 + 七段式简报 + 要求合并旧简报（好）；失败退化为机械折叠并落盘（致命） |
| D. 子代理的隔离、权限与取消 | 上下文/权限/进度/取消传播 | 上下文与权限收窄、取消传播（JoinSet + supervisor）做得干净；审批、计费、进度、结果截断是短板 |

### 6.1 三个目标各自的做法

**Codex CLI（`openai/codex` Rust 线）**

- **plan mode 是纯提示词**：`ModeKind{Plan, Default}` 只是一种 collaboration mode，内容是
  "一段 developer 模板文本 + 默认 reasoning effort"，没有任何工具层拦截。模板自称
  "Mode rules (strict)"，靠一句 `"You are in Plan Mode until a developer message explicitly ends
  it. Plan Mode is not changed by user intent, tone, or imperative language."` 约束模型。
  曾经有真·只读方案（PR #4770：`request_plan_turn` 临时切 read-only sandbox + planner model），
  **已 CLOSED 未合并**。
- **plan 与沙箱正交**：沙箱是 `SandboxMode{ReadOnly(默认), WorkspaceWrite, DangerFullAccess}`，
  审批是独立的 `AskForApproval`；plan mode 不动这两者。
- **`update_plan` 是 checklist，与 plan mode 互斥**：plan mode 下调用直接报错；没有计划落盘、
  没有用户审批、没有产物。
- **plan 与 goal 不并发**：`ext/goal/src/extension.rs` 里 plan mode 会清空本回合的 goal。
- **压缩有两套机制**：①`model_auto_compact_token_limit`（阈值口径 `effective_context_window_percent`
  默认 **95**，未配置时回落 `window × 9/10`，比较对象另加 `auto_compact_fallback_buffer_tokens`
  默认 **16384**）；②checkpoint 换窗口：用 `notes` 工具把 goal/decisions/progress 连同
  **window ID 与 item ID** 写进 notes，再用只读 `history`/`read_item`/`search_contents` 找回，
  模板原文 "Future context windows will not automatically include the current conversation"。
  当前 `models.json` 里多数模型已把 `auto_compact_token_limit` 设为 `null`。
- **goal 扩展**：状态含 `complete` / `blocked` / `paused` / **`budget_limited`**，
  `[goals] max_goal_token_budget` 控制预算；预算耗尽有专门的 `budget_limit.md`
  （"不要开始新的实质工作，给用户清晰的下一步"）。
- **blocked 规则与 Flowy 几乎字面一致**：`"Only use status 'blocked' when the same blocking
  condition has repeated for at least three consecutive goal turns, counting the original/
  user-triggered turn and any automatic goal continuations"`，并要求"措辞不同但等价的 blocker
  视为同一条件"。续跑模板还含 completion audit（"treat completion as unproven"）与三分类
  no-progress check（progress / verified wait / no progress）。
- **子代理**：`spawn_agent`（`fork_turns` 省略或 `"all"` = 继承父模型与全部历史）；
  `send_message`/`wait_agent`/`interrupt_agent`/`list_agents`；所有 agent **共享同一容器、
  文件系统与 cwd**（一方编辑立刻对他人可见）；`max_threads` 默认 **6**、`max_depth` 默认 **1**；
  子代理继承当前 sandbox policy，父回合的 live override（`/yolo`）会重新套用。

**Claude Code**

- **plan mode 是真权限模式**：六种 permission mode 之一，写操作在**工具层**被阻断直到计划被批准，
  批准会把会话切到另一个 mode。
- **但有一个与 Flowy 直接可比的例外**：在可用 bypass permissions 的交互终端里，Claude Code
  **也不执行阻断**——"Claude is still instructed to plan without editing, but a file edit or shell
  command it attempts during planning runs without prompting"。此时 plan mode 退化为纯提示词。
  上游还有复发类 issue #50969（v2.1.89）：用户**拒绝** `ExitPlanMode` 后，Write/Edit/Bash 突变
  **全部成功**，"技术上仍在 plan mode"但已是 advisory-only。
- **有产物、有审批**：计划写入磁盘文件（`plansDirectory`），`ExitPlanMode` 只是"提交并请求批准"
  （`tool_input` 通常为空，内容由 Claude Code 从文件注入）；**压缩后会从磁盘重新注入计划文件**。
- **压缩三层，且官方给出"什么在压缩后活下来"表**：
  保留 —— system prompt/output style、项目根 CLAUDE.md 与无 `paths:` 的规则（磁盘重注入）、
  auto memory、**plan mode 写的计划文件**、最多 **5 个**最近修改过的已读/已改文件
  （>5,000 token 的文件只留路径引用）、最近调用过的 skill body；
  丢失 —— 带 `paths:` frontmatter 的规则与子目录 CLAUDE.md（跟着消息历史被摘要）、hook 早先注入的
  context、skill body 有 **5,000 token/技能、25,000 token 总量**预算且**超预算时最旧的被整个丢弃**。
  后台命令与后台 subagent 在压缩后**继续跑**。阈值可配
  （`/autocompact`、`--autocompact`、`CLAUDE_CODE_AUTO_COMPACT_WINDOW`；1M 原生窗口模型默认约
  **967K**）；总开关 `DISABLE_AUTO_COMPACT=1` / `DISABLE_COMPACT=1`。
- **`/goal` 是 Stop hook 的包装**：每回合结束由一个**小快模型**（默认 Haiku）读条件 + 对话，
  给三值裁决 **Not yet met / Met / Impossible**，Met 或 Impossible 都自动清除；条件上限 4,000 字符。
  官方明确 evaluator **不执行命令、不读文件**，因此要求条件必须是"Claude 自己的输出能证明的东西"。
  **没有"N 轮同一条件才 blocked"的门槛**——"不可能"直接由 evaluator 判 `Impossible`。
  防跑飞：连续几回合无工具调用即停止循环并交还控制权；后台工作超过 30 分钟触发 check-in，
  之后 2× 退避、交互会话最多 3 次；错误重试 3 次后暂停。
- **子代理**：默认全新隔离上下文 + 独立 system prompt + 独立权限；**`fork` 继承父会话**；
  内置 `Explore`/`Plan` 是 **one-shot 且不返回 agent ID（无法 resume）**，这与 Flowy 的
  `send_message`/`list_agents` 续跑能力方向相反；主会话压缩**不影响** subagent transcript。

**Kimi（kimi-cli → kimi-code）**

- **kimi-cli 的 plan mode = 只读工具集 + 审批闸**：plan 下只放 `Glob`/`Grep`/`ReadFile`；
  `ExitPlanMode` 提交计划文件并给 **Approve / Reject / Reject-and-Exit / Revise** 四选一。
  YOLO 模式下进入自动批准但**退出仍会问**。
- **压缩阈值是全篇最明确的双条件**：`context_tokens + reserved_context_size(默认 50000) >= max`
  **或** `context_tokens >= max_context_size × compaction_trigger_ratio(默认 0.85)`，先到先触发。
- **kimi-code 把压缩做成控制器**：在 **step 边界暂停 → 摘要 → 切分支 → 恢复**，事件
  `compaction.started/blocked/cancelled/completed`；保留策略是"只留 user prompt + 摘要"；
  `compaction_max_attempts` 默认 **5**，token 计数用 `measured+estimated`。
- **压缩是这个项目当前最大的问题源**（一手 issue/PR 链）：启动即连续 auto-compaction 死循环
  （#2325，修复 PR #2498 还披露固定请求开销约 **29k tokens**，并加了"2 次无效压缩后熔断"）；
  回合中压缩后 agent 放弃进行中的请求并重新执行会话最早那条消息（#2680 → PR #3537）；
  **子目录 AGENTS.md 提醒被压缩丢弃**（PR #3409）；**权限模式提醒压缩后丢失**（PR #1602）；
  摘要被截断时返回 `COMPACTION_UNABLE`（PR #120）。
- **子代理**：`Agent` + `AgentSwarm`，完全隔离、可并行/后台、`resume` 可重调；三个内置不能再生成
  子代理，深层链需 frontmatter `subagents:` 白名单；`[subagent] timeout_ms` 默认 **2 小时**；
  状态落在 `sessions/<workDirKey>/<sessionId>/agents/<subagentId>/wire.jsonl`。
- 权限三档（Always Ask 默认 / Ask When Needed / Never Ask）+ `[[permission.rules]]`
  的 `allow|deny|ask` 规则 + `dangerous_command_guard` 默认 true。
- **未见 goal / 自动多轮续跑**（文档中未出现，**不等于已确证不存在**）。

### 6.2 横向对照表

| 维度 | Flowy | Codex CLI | Claude Code | Kimi |
| --- | --- | --- | --- | --- |
| plan 靠机制还是提示词 | 有工具面机制，但类目语义不可靠：MCP 自报、Skill 内嵌 shell、Skill-fork 可穿透 | 纯提示词模板，无工具拦截（只读 PR 未合并） | 真权限模式，工具层阻断（bypass 终端例外） | 只读工具白名单硬限 |
| plan 与沙箱关系 | 只改工具可见性，不动沙箱 | 完全正交 | 内嵌为一种 permission mode | 内嵌：plan 下只放只读工具 |
| 有审批 / 有产物 | **都没有**（`ExitPlanMode` 无参数、不落盘） | 都无；plan 下 `update_plan` 报错 | **都有**：计划落盘 + 审批菜单 + 压缩后重注入 | **都有**：计划文件 + 四选一审批 |
| 压缩层次 | 三层：microcompact / LLM autocompact / emergency gate | 两机制：token 阈值 + `notes`/`new_context` 换窗口 | 三层：MicroCompact / Session Memory / Full LLM 摘要 | 单层 LLM 摘要（kimi-code 为 step 边界控制器） |
| 压缩触发口径 | 本地估算 `len/4`；128k→74.2%、200k→83.5%、1M→96.7% | 可用窗口 95%；阈值默认回落 90% | 默认在模型上限处；1M 模型约 967K；可配 | `ctx+50k ≥ max` 或 `ctx ≥ max×0.85` |
| 压缩保留什么 | 用户消息原样 + 七段式简报 + 要求合并旧简报 | `scope=body_after_prefix` 保留被携带前缀，否则只剩摘要 | 重注入计划/CLAUDE.md/memory/5 文件/skill body | 只留 user prompt + 摘要 |
| 压缩失败怎么办 | **机械折叠销毁历史并落盘，熔断器是死代码** | 无干净硬关（#4106 仍 open；有"压缩请求自身 stream disconnected 后线程不可挽救"的证词） | `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3` 后本会话不再尝试（**二手逆向**） | `compaction_max_attempts` 默认 5；2 次无效后熔断 |
| 目标循环 | opt-in LLM judge（done/continue/wait）；模型可 `update_goal(complete)` 短路 | `ext/goal`：complete/blocked/paused/**budget_limited** + token budget | `/goal` = Stop hook；三值 Met/Impossible | 未见 goal |
| judge 看什么 | 只看模型最后一段回复文本 | completion audit + 三分类 no-progress check | 条件 + 对话（evaluator 不跑命令、不读文件） | — |
| blocked 门槛 | 同一条件持续 N 轮（**仅提示词**） | 同形规则 ≥3 consecutive goal turns（**也是提示词**），另有 `budget_limited` 出口 | 无 N 轮门槛，直接判 `Impossible` | — |
| 子代理隔离 | 进程内子引擎，看不到父 transcript，但继承父 system prompt | 全历史 fork vs 新子；共享同一 cwd/文件系统 | 默认全新隔离；`fork` 继承父会话 | 完全隔离 |
| 子代理可续跑 | 可以（`send_message`/`interrupt_agent`/`list_agents`） | 可以（`send_message`/`wait_agent`/`interrupt_agent`） | `Explore`/`Plan` 是 one-shot，**不可 resume** | `resume` 可重调实例 |
| 子代理上限 | 并发 8、单次 300s 硬编码、默认无累计预算 | `max_threads` 6、`max_depth` 1 | 未核实（agent teams 约 7× token） | 禁嵌套；timeout 默认 2h |

### 6.3 对 Flowy 最尖锐的五条差异

1. **"机制 vs 提示词自律"这条线上 Flowy 与 Codex 同侧，但 Flowy 是三家里唯一连"计划产物"都没有的。**
   Claude Code 与 Kimi 都把 plan 做成"落盘的计划文件 + 显式审批（四选一 / 批准切模式）"，
   Claude Code 还把"压缩后从磁盘重注入计划"写进了存活表。Flowy 的 `ExitPlanMode` 无参数、
   计划只存在于对话文本里 ⇒ **用户无法修改计划后再执行，压缩/恢复后计划即丢**。
2. **`update_goal(complete)` 短路 judge 是 Codex 与 Claude Code 都没有的口子。**
   Codex 的续跑模板把完成当**待证命题**（"treat completion as unproven and verify it against the
   actual current state"，逐条 requirement 找权威证据）；Claude Code 干脆把裁决权交给**另一个模型**
   （Haiku 三值裁决）。Flowy 让"干活的模型"兼任"验收的模型" ⇒ 这正是 Codex 模板
   "Treat uncertain or indirect evidence as not achieved" 想防的情形。
3. **judge 只看最后一段文本这件事，三家都知道是弱点，但只有 Flowy 没有配套的止损设计。**
   Claude Code 明确承认 evaluator 不跑命令、不读文件，然后用"无工具调用即停循环 + 30 分钟 check-in
   退避 + 3 次 idle 上限"兜底；Codex 用三分类 no-progress check 兜底。Flowy 的 judge 同样是文本
   判定，却**既不降级也不兜底**：judge fail-open 到 `continue`，靠 `max_auto_continuations` 和
   双断路器收场。
4. **"blocked 需同一条件持续 N 轮"不是 Flowy 的孤例设计——Codex 独立实现到了几乎字面一致。**
   这反过来说明该规则是经过验证的；但 Codex 另外给了 `paused` 与 `budget_limited` 两个**非 blocked
   的优雅收尾态**，而 Flowy 的 `blocked_threshold` 只是提示词常量、预算耗尽后状态仍停在 `Active`
   （#9/#20）⇒ Flowy 缺的不是"更聪明的 blocked 判定"，而是**除 blocked 之外的退出通道**。
5. **压缩上 Flowy 的"用户消息原样保留 + 要求合并旧简报"比三家都保守，但真正的风险点不在保留策略，
   而在系统注入的约束。** Kimi 的一手 bug 链恰好证明这一点：**子目录 AGENTS.md 提醒被压缩丢弃**
   （PR #3409）、**权限模式提醒压缩后丢失**（PR #1602）；Claude Code 也会把带 `paths:` 的规则与
   子目录 CLAUDE.md 一起摘要掉、并整体丢弃最旧的 skill body。对照 Flowy：用户原话与 goal 的
   turn tail（每轮重渲染）是安全的，但**系统注入类内容**（权限/沙箱提醒、贡献者注入的 `[Context]`
   块、hook 注入）会被 `fold` 进程吃掉且没有任何重注入机制——这与 #36（摘要器看不到被折叠的用户
   原话）是同一类问题的两面，值得作为下一条压缩改进项。

**调研中提到的学术证据（本报告未逐篇复核，仅记录线索）**：压缩本身不等于失控，失控取决于
"控制性约束是否随摘要丢失"这一维度；本次调研汇总中引用的论文名包括 *Lost in Compaction*、
*Governance Decay*、*The Missing Boundary*，以及 context rot / lost-in-the-middle 类工作。
数字（例如"compactor 平均只保留约 17% Session Constraints"）来自调研摘要，**未由本报告复核**。

### 6.4 外部素材未纳入本报告的部分

本次调研还整理了 Gemini CLI（policy-engine 的 `plan.toml` catch-all deny，强制程度最高）、
OpenCode（plan 下 `bash` 实际未被 deny）、Amp（无 plan mode，90% 压缩阈值）、Cursor（plan mode
仅提示词约定）以及 compression/long-context 的学术线索。它们与本报告的四轴结论一致或只是补充
样本，为控制篇幅未展开；需要时可以单独成篇。

## 7. 缺陷分级汇总

编号 1–29 覆盖 §2/§3/§5 的嵌入版委派，30–37 是 §4 压缩子系统，38–42 是 §5.4 的 planned（宿主持久化）委派；各区间内按严重度排序。P0 = 用户可感知的安全/成本后果，P1 = 机制失效或承诺与实现不符，P2 = 可维护性或边界问题。

| # | 级别 | 模式 | 缺陷 | 关键证据 |
| --- | --- | --- | --- | --- |
| 1 | P0 | 子代理 | 子代理绕过宿主审批通道：无 `approval_manager` → 落 `ToolConfirmer` stdin 分支，EOF 空输入即"批准" | `engine/mod.rs:660,2646,2678`；`confirm.rs:46,62` |
| 2 | P0 | 子代理 | 子代理 provider 请求丢失 `x-flowy-turn-id`（`task_local` 不跨 `tokio::spawn`）→ 成本/额度漏记 | `billing_turn.rs:13-15`；`local_agent_invocation.rs:543` |
| 3 | P1 | plan | MCP 工具 Info 分类由服务端自报 `readOnlyHint` 决定 → 只读承诺外包给第三方 | `nomi-mcp/src/tool_proxy.rs:126-141` |
| 4 | P1 | plan | `Skill` 是 Info，但 inline 执行会跑 SKILL.md 内嵌 shell | `skill_tool.rs:282-285`；`nomi-skills/src/executor.rs:39-47` |
| 5 | P1 | plan | `Skill(fork)` 起全权子 agent（含 Write/Edit/Bash），不继承 plan 约束 | `executor.rs:83-94`；`local_agent_invocation.rs:807-819` |
| 6 | P1 | plan | `ExitPlanMode` 无 plan 参数、"提交供审批"是空承诺、无审批与回退 | `plan/tools.rs:127-133,164`；`plan/prompt.rs:46-47` |
| 7 | P1 | plan | 计划文件能力是死代码，计划不落盘（`plan_directory` 是死配置） | `plan/file.rs:10-32`；`nomi-config/src/plan.rs:14-16` |
| 8 | P1 | goal | `update_goal(complete)` 可无证据短路 judge，唯一完成审计被模型自己关掉 | `goal/tool.rs:78-84`；`goal/runtime.rs:155-158` |
| 9 | P1 | goal | 预算耗尽后仍 `Active` 且预算窗口随进程复活 → 目标无人同意地反复自动重启 | `goal/state.rs:198`；`goal/runtime.rs:197-199`；`goal_bridge.rs:96-98` |
| 10 | P1 | goal | coding profile 下 goal 永不续跑（`disable_goal_auto_continue` 默认 true），turn tail 却仍承诺"每轮会有人审计" | `engine/mod.rs:2585-2589`；`harness.rs:147,312-313`；`goal_context.md:8` |
| 11 | P1 | goal | judge 只读模型自述（goal ≤2000 / contract ≤2500 / response ≤4000 字符），"只认证据"不成立 | `engine/mod.rs:2601`；`goal/runtime.rs:204-212`；`judge.rs:34-37` |
| 12 | P1 | 子代理 | 子代理产出被 50k 硬截断、无落盘、`toon` 改写后不再是合法 JSON → 结论不可找回 | `nomi-tools/src/lib.rs:262-264`；`tool_execution.rs:604,1098-1122` |
| 13 | P1 | 子代理 | 委派期间父级/用户零进度（进度只在兄弟间可见），同步阻塞最坏约 600s | `local_agent_invocation.rs:145,238-257` |
| 14 | P1 | 子代理 | 超时 300s 硬编码、默认无累计预算、子代理用量不计入父级 | `local_agent_invocation.rs:457,463`；`config.rs:471` |
| 15 | P2 | plan | 有持久副作用的工具被标 Info（`save_memory`、`create_companion_skill`） | `companion_tools.rs:236-240,447-452,524-526` |
| 16 | P2 | plan | 过滤只用 `category()` 而非逐动作 `category_for(input)`（当前过严，但是潜在泄漏轴） | `engine/mod.rs:1550-1556`；`nomi-browser/src/tool.rs:3082` |
| 17 | P2 | plan | plan 生效前那一轮仍可写（同一条 assistant 消息并发 `EnterPlanMode` + `Edit`） | `engine/mod.rs:1576`；`plan/tools.rs:53-55` |
| 18 | P2 | plan | plan 状态不持久化，resume/重启即丢（`save_session` 不含 plan 字段） | `engine/mod.rs:3431-3445` |
| 19 | P2 | plan | 只读过滤逻辑在两处重复实现（当前谓词一致，存在漂移风险） | `engine/mod.rs:1550-1562, 3219-3239` |
| 20 | P2 | goal | `blocked_threshold`（默认 3）只是提示词常量，代码不统计；judge 又把 blocked 折叠成 DONE | `goal/templates/continuation.md:31-32`；`judge.rs:54-55`；`runtime.rs:239-242` |
| 21 | P2 | goal | `wait_for_seconds` 无上限且等待不烧预算 → 可无限期停放 | `judge.rs:566-568`；`runtime.rs:274-276` |
| 22 | P2 | goal | build-extra 路径没有预算 clamp，`extra.goal` 不在禁用键内 | `factory/nomi.rs:947-952`；`manager/nomi/agent.rs:3074` |
| 23 | P2 | goal | 取消不落库 goal 快照；每轮清零 turn 计数 → 单次 `execute_turn` 可跑 8×200 迭代 | `manager/nomi/agent.rs:1819-1830,2011,2084-2086`；`engine/mod.rs:2617-2618` |
| 24 | P2 | goal | `loop_guard` 在每个自然终止点 reset，跨轮停滞不被覆盖 | `engine/mod.rs:2517,2582-2619`；`loop_guard.rs:1-6` |
| 25 | P2 | 子代理 | 空结果与任务错位不会被发现（`unknown` 占位、无空文本判据） | `local_agent_invocation.rs:593-635`；`execute_bounded:300-313` |
| 26 | P2 | 子代理 | 隔离只覆盖"并行且 ≥2 个可写 + git 仓库"；取消会丢弃隔离区未提交改动 | `local_agent_invocation.rs:750-760,771-775`；`worktree.rs:807-823` |
| 27 | P2 | 子代理 | 委派无结果校验，子代理工作不留痕（无 trace/observation） | `engine/mod.rs:675,681,687`；`local_delegate_tool.rs:191-246` |
| 28 | P2 | compact | 本地 token 估算对中文系统性低估（`len/4`），注释声称的"保守高估"不成立 | `compact/estimate.rs:4-6,40-41` |
| 29 | nit | plan | `context_usage` 把 plan 指令记入 `rules` 桶，实际走 turn tail | `context_usage.rs:76-86` |

| 30 | P0 | compact | 摘要失败即机械折叠销毁历史并落盘；熔断器的 `record_failure` 无生产调用点，是死路径 | `auto.rs:412-432,617-622`；`engine/mod.rs:2621-2623,3340-3351`；`state.rs:65-67` |
| 31 | P1 | compact | 摘要在回合结束关键路径上同步阻塞，最坏约 4×90s | `engine/mod.rs:2551,2621`；`auto.rs:539-554,602-605` |
| 32 | P1 | compact | 摘要调用是全价 prefill，且重写消息中段 → 前缀缓存失效 | `auto.rs:466-472,499-516`；`engine/mod.rs:3353` |
| 33 | P2 | compact | PTL 的 20% 截断只保证首条是 User，可能切断 tool 配对 | `auto.rs:737-762` |
| 34 | P2 | compact | 熔断器唯一的集成测试根本触发不到压缩（虚假安全感） | `tests/engine_compact_test.rs:966-987` |
| 35 | P2 | compact | 50% 软提醒与紧急用户文案是死代码 | `state.rs:71-80`；`emergency.rs:13-14`；`engine/mod.rs:1744-1750` |
| 36 | P2 | compact | 摘要器看不到被折叠的用户原话（摘要与保留的用户话盲接） | `auto.rs:291-306,421`；`compact/prompt.rs:31-35` |
| 37 | nit | compact | 尾部对齐不躲 boundary marker，用量面板可能报旧 metadata | `auto.rs:250-258`；`context_usage.rs:117-125` |

| 38 | P1 | planned | 终态回写的进程内重试是死代码（`Ok(false)` 不可达），瞬时投影错误即静默丢失"汇总回写本会话" | `scheduler.rs:542-548,599,643,646,673,676-722` |
| 39 | P1 | planned | `plan_gate` 硬编码 `Automatic`，模板表也没有该列，但文档/描述声称审批来自 Team 模板 ⇒ `AwaitingApproval`/`/approve` 不可达 | `app_server_delegate.rs:166`；`001_v3_baseline.sql:215-241`；`host_delegate_tool.rs:43-44` |
| 40 | P2 | planned | `AdaptationPolicy::Fixed` ⇒ 无自适应重试，一次模型失败即终态 | `app_server_delegate.rs:167`；`scheduler.rs:1782-1784` |
| 41 | P2 | planned | 追加路径丢弃 `receipt.step_ids`（网关路径是填的） | `app_server_delegate.rs:145-153`；`nomifun-gateway/caps_agent_execution.rs:846` |
| 42 | P2 | planned | `DecisionRequested` 被映射成 `approval.requested`，与 decision 领域词汇冲突 | `runtime_adapter.rs:707` |

**模式成熟度不对称**：goal 有状态机、预算、断路器与快照恢复；plan 只有工具面过滤 + 提示词。
四条 P0/P1 的核心（#3/#4/#5 只读旁路、#8 judge 短路）指向同一个根因：
**本仓把"安全类目"复用成了"只读类目"，又把"完成判定"交给了被判定者自己**。

## 8. 与既有两份文档的关系

本仓已有两份相关审计，本报告与它们的关系必须写清，避免重复劳动：

| 既有文档 | 基线 | 本报告的处理 |
| --- | --- | --- |
| [agent-harness-capability-analysis.zh.md](agent-harness-capability-analysis.zh.md)（2026-08-24 维护，内容基线 2026-08-06，基准 `d791691c6`） | 能力/性能审查 | §2 的 P1-B（"plan mode 是安全过滤，不是可审计计划协议"）与 P1-E（"Goal Judge 仍是 opt-in 且只看最后文本"）**在 HEAD 仍然成立**，本报告给出了新的代码证据并补充其未覆盖的三点：计划文件死代码、plan 状态不持久化、只读过滤重复实现 |
| [agent-loop-comparison.zh.md](agent-loop-comparison.zh.md)（同基线） | 单轮 loop 边界对照 | 其 §1 的阶段表和 §2 表格结论仍可用；但它把 plan mode 一行写为"过滤为 Info 工具，计划写在响应文本"，漏掉了 turn-tail 注入、请求级 `ProviderToolAuthority` 与 coding harness 硬停线 |

**行号已失效（引用旧文档时需重定位）**：

- 旧文档 §2 称 plan filter 在 `engine/mod.rs:1319-1329` → HEAD 实际在 `engine/mod.rs:1550-1562`（另有一份 `3219-3239`）。
- 旧文档 §3 引用 `engine/mod.rs:1872-1905` 的 goal judge 与 `2273-2360` 的 compaction → HEAD 已位移，引用前请重新 `grep`。
- 旧文档整体以 `d791691c6` 为基准，HEAD 为 `a3482541f`，中间隔了一个多月的 Agent Store 主线，行号漂移是必然的。

**结论**：两份旧文档方向正确、无需推翻；本报告是它们在四个模式上的加深与校正，而不是替代。
