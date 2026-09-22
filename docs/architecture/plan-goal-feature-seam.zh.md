# nomi-agent Plan/Goal Feature 化技术方案

> 状态：**已实施**（Phase 0–5 落地，见 §10「实施记录」；Phase 6 为文档同步，即本节所在改动）。
> 范围：`crates/agent/nomi-agent` 的架构重构 + 注入通道迁移；**不含**审批面板、计划文件、CreateGoal 工具。
> 前置阅读：`docs/architecture/agent-engine.zh.md`、`docs/architecture/turn-tail-context-investigation.zh.md`（改动前必读其 §4「先量再改」）、`docs/guides/goals.md`。
> 参考系：kimi-code `packages/agent-core-v2/src/features/`（Feature seam）与 `features/{plan,goal}/injection/`（system-reminder 注入）。

---

## 1. 背景与目标

### 1.1 问题

plan/goal 的逻辑目前散落在 `engine/mod.rs`（4356 行）中：

- 状态是引擎结构体裸字段（`plan_state`/`plan_active_flag`/`plan_exit_latch`/`horizon`/`goal`/`goal_wait_probe`，`engine/mod.rs:609-630`）；
- 行为是 turn loop 里的六处内联分支（用户入口审批、请求门控、turn-tail 组装、authority 快照、dispatch 只读门禁、modifier 迁移、自然结束续作）；
- 装配散在 `bootstrap.rs` 的多个段落，靠 `Arc<AtomicBool>` 与 `set_*` 方法回填引擎。

结果：引擎对 plan/goal 有硬编码感知；新增同类模式（第三个"模式"）必须继续改引擎主循环；plan/goal 的行为测试只能白盒构造整个引擎。

### 1.2 目标

把 kimi-code 的 **Feature seam** 架构移植进来：

1. plan/goal 各自收敛为一个**自注册 Feature 目录**，通过统一的贡献通道（工具 / 钩子 / reminder）接入引擎；
2. 引擎只提供**钩点**，不再包含 plan/goal 专属字段与分支；
3. plan/goal 的上下文注入从 turn-tail `[Context]` 迁到 **`<system-reminder>` 持久消息**通道，顺带解决 turn-tail 调查文档里的 P2（指令混入 `[Context]`）与部分 P3（每 pass 重复注入）；
4. **wire 契约与 HTTP API 完全不变**，全量测试绿。

### 1.3 范围决策（已确认）

| 决策项 | 结论 |
|---|---|
| 范围 | 架构重构 + 部分行为（仅注入通道） |
| 注入通道 | 改用 `<system-reminder>`，放弃 turn-tail 承载 plan/goal 指令 |
| 测试 | `engine/plan_mode_tests.rs` 结构体字面量构造随重构同步改写 |
| 保留的既有硬化闸门 | plan 滞留预算（8/12 轮）、goal blocked 证据 ≥3 次、judge fail-closed、horizon 对 plan 的续作否决 —— 全部原样保留 |
| 明确不做 | ExitPlanMode 显式审批面板、计划文件与 revision、CreateGoal/GetGoal/SetGoalBudget 模型工具（goal 维持宿主创建）、turn-tail 其余内容（date/ledger/`ContextContributor`）的去留、跨 crate 类型形状变更 |

---

## 2. 现状与依据

### 2.1 参考系：kimi-code 的 Feature seam

- Feature 在包入口自注册（`src/index.ts:349/386` → `featureRegistry.ts`），三条贡献通道：`contributeTool`、Agent 级 DI 服务、reminder variant。
- **状态强制与状态告知分层**：BeforeExecute guard veto（模型绕不过）负责强制；`<system-reminder>` 包装的 user 消息（`systemReminder.ts:3-8`）负责告知与引导。
- 注入有频率策略：plan full/sparse/refresh/exit（`planModeInjection.ts:60-80`），goal 仅新 turn（`goalInjection.ts:24`）。

### 2.2 改造对象：nomi-agent 现状

**引擎内联点清单**（`engine/mod.rs` 行号，重构时逐一迁移）：

> **行号校准（实施期实测，基线 `1bb17c918`）**：下表左列是方案写作时的行号，右列为实测值；
> 两列不一致处已在下文「实施记录」中说明。迁移完成后这些内联点已全部消失（turn loop
> 内只剩通用折叠），故行号仅对应当初的迁移坐标。

| # | 钩点 | 方案行号 | 实测行号 |
|---|---|---|---|
| 1 | 用户入口 `approve_pending_plan()` + `horizon.on_user_request()` | `:1755-1756`（实现 `:1386-1400`） | 调用 `:1755-1756`、实现 `:1386-1400`（一致） |
| 2 | thinking/effort 请求门控（`plan_state.is_active`） | `:1290`、`:1302` | `:1290`、`:1302`（一致） |
| 3 | turn-tail 组装：plan 指令、office plan nudge、harness nudge | `:1852-1866` | `:1852-1866`（一致） |
| 3b | turn-tail 组装：goal `turn_context()` | `:1911-1913` | `:1911-1913`（一致） |
| 4 | authority 快照 `plan_mode_read_only` | `:3671`；dispatch 门禁 `tool_execution.rs:492-516` | `:3671`（一致）；门禁 `:492-516`（一致） |
| 5 | modifier 迁移（Enter/Exit → plan 状态） | `:3902-3926`（调用点 `:3354`） | `:3902-3926`、调用点 `:3354`（一致） |
| 6 | 自然结束：horizon 观测/decide/judge/续作 push | `:2915-2977`（辅助 `:1402-1449`） | `:2915-2977`、辅助 `:1402-1449`（一致） |

辅助内联（`horizon_observe_tools` / `sync_goal_progress` / `emit_horizon_decision`，`:1402-1449`、`:3351`）随 GoalFeature 的工具后钩与自然结束钩一并收编。

**已存在、可复用的 seam**：

- `ContextContributor`（`context_contributor.rs:26`）——现成 trait-object 注入点，但只覆盖 turn-tail 文本，backend 已在用（Summon/Companion/Meeting）；
- `Tool::context_modifier_for` → `apply_context_modifiers`——工具→引擎状态回流通道；
- `ToolRegistry` / `CommandRegistry`——注册即贡献；
- `ProviderToolAuthority`——per-request 授权快照（已有一个 `plan_mode_read_only` 位）；
- `GoalRuntime` clone-handle + `Arc<Mutex<GoalState>>`——状态与引擎解耦的现成形态（backend 持 handle 操作）；
- `GoalWaitProbe`——host 能力注入 trait-object 的先例。

**没有的**：DI 容器、service locator、插件框架。装配 = `AgentBootstrap` builder + `set_*` 注入。本方案不引入 DI，只引入最小 Feature 注册表。

### 2.3 与 turn-tail 调查文档的关系（必须正面处理）

`turn-tail-context-investigation.zh.md`（2026-09-22，状态"已记录，暂不改"）记录了 turn-tail 的问题：

- **P1** 最新 user 位经常只有非指令文本（`Current date`），诱发模型复述循环；
- **P2** 一个 `[Context]` 块混了数据/指令/证据三类内容，plan 指令实质是"贴在用户位的第二 system prompt"；
- **P3** 同一回合每个 provider pass 重复注入；
- 其约束：**先量再改**（§4 给了测量步骤），改动会同时影响"缓存是否还热"和"模型看到什么"。

本方案与其**同向**：

- 把 plan/goal 指令移出 `[Context]` → 正面解决 P2；
- reminder 按状态事件/频率策略注入，不再每 pass 重贴 → 缓解 P3；
- 文案头部吸收其 §5-A 措辞："这是环境/状态信息，不是用户的新指令"。

遵守其约束的方式：

- **Phase 0** 先按其 §4 步骤采集基线（思考段数 / tail 注入位置 / 文本稳定性）；
- 注入通道切换（Phase 3/4）后复测对比；
- reminder 迁移与 Feature 化**分 commit**，可独立 revert；
- `Current date`、RAG/memory、round ledger 等其余 tail 内容**不在本次范围**，仍留 turn-tail，其去留归该调查文档处置。

---

## 3. 总体设计

### 3.1 目录结构前后对比

```
改造前                                    改造后
crates/agent/nomi-agent/src/              crates/agent/nomi-agent/src/
├── engine/mod.rs   (4356 行,             ├── engine/mod.rs   (只留钩点调用
│   含 plan/goal 内联 ×6)                 │   + façade 委托, 目标 <3500 行)
├── plan/                                 ├── features/
│   ├── state.rs                          │   ├── mod.rs        (Feature/Registry/Hooks)
│   ├── tools.rs                          │   ├── reminder/     (ReminderService + 包装)
│   ├── prompt.rs                         │   ├── plan/
│   └── file.rs                           │   │   ├── state.rs / service.rs
├── goal/                                 │   │   ├── tools/    (enter/exit)
│   ├── state.rs                          │   │   ├── injection.rs + *.md 模板
│   ├── runtime.rs                        │   │   └── hooks.rs
│   ├── tool.rs                           │   └── goal/
│   ├── judge.rs                          │       ├── state.rs / runtime.rs / tool.rs
│   └── templates/                        │       ├── judge.rs / templates/
├── horizon/                              │       ├── injection.rs
│   ├── mod.rs / budget.rs / delta.rs     │       └── hooks.rs
└── context_contributor.rs  (不动)        ├── horizon/           (保留, 由 GoalFeature 持有)
                                          ├── context_contributor.rs  (不动)
                                          └── plan/ goal/ horizon/ 删除
```

> `horizon/` 物理位置可保留在 `src/horizon/`（它同时服务 office/coding nudge 等非 goal 逻辑），但其 `decide`/记账/预算的**调用权**收归 `GoalFeature` 的钩子；若实施中发现 office nudge 与 goal 耦合过深，允许 horizon 留在顶层、仅经 `HookCtx` 暴露的窄接口被 GoalFeature 调用——两种形态二选一在 Phase 4 落地时定，不影响本方案契约。

### 3.2 Feature trait 与注册表（Rust 版 seam，无 DI）

```rust
// src/features/mod.rs
pub trait Feature: Send + Sync {
    fn name(&self) -> &'static str;
    fn register_tools(&self, _registry: &mut ToolRegistry) {}
    fn reminders(&self) -> Vec<ReminderSpec> { vec![] }
    fn hooks(&self) -> FeatureHooks { FeatureHooks::default() }
}

#[derive(Default)]
pub struct FeatureHooks {
    pub on_user_request: Option<HookFn>,   // 用户消息入口（plan: 批准 pending 计划）
    pub dispatch_gate:   Option<GateFn>,   // -> GateDecision::{Allow, Deny(msg)}
    pub apply_modifier:  Option<ModifierFn>, // 消费 ContextModifier，feature 自行迁移状态
    pub request_gates:   Option<RequestGateFn>, // thinking/effort 等请求参数调整
    pub on_natural_end:  Option<NaturalEndFn>,  // -> Option<Continuation>（goal 续作）
}

pub struct FeatureRegistry { features: Vec<FeatureEntry> /* name + Arc<dyn Feature> */ }
```

约定：

- `AgentEngine` 新增字段 `features: FeatureRegistry`；
- `AgentBootstrap::build()` 按配置注册：`PlanFeature`（`config.plan.enabled` 门控）、`GoalFeature`（恒注册，goal 本身仍由宿主 `GoalSpec` 决定是否激活）；
- 钩子按**注册顺序**链式调用；`on_natural_end` 的顺序即现契约（steering → coding harness → office nudge → goal），在代码与文档中钉死；
- 实现用 `Arc<dyn Fn…>` + 枚举分发，不引入重量级 async-trait 框架；钩子在既有 async 上下文中 await；
- 每个钩点的引擎侧调用处**只有一行**循环/折叠，不含任何 plan/goal 专名判断。

### 3.3 状态归属

| 现状（engine 字段 / 方法） | 目标 |
|---|---|
| `plan_state` + `plan_active_flag` + `plan_exit_latch` | 收进 `PlanService`（feature 私有：phase / allow_list / pending_plan / latch），`Enter/ExitPlanMode` 工具经句柄共享 |
| `horizon` + `goal` + `goal_wait_probe` | `GoalFeature` 持有 `HorizonController` + `GoalRuntime` + probe |
| façade：`approve_pending_plan` / `set_plan_active_flag` / `set_plan_exit_latch` / `set_goal` / `set_goal_state` / `set_goal_wait_probe` / `goal_state` / `goal_runtime_handle` | **签名一律不变**，内部委托对应 feature 服务；backend（`goal_bridge.rs`、`manager/nomi/agent.rs`）零改动 |

### 3.4 六个钩点 → FeatureHooks 映射

| # | 现内联位置 | 迁移为 | 说明 |
|---|---|---|---|
| 1 | `approve_pending_plan` @ `:1756` | `on_user_request`（PlanFeature） | "下一条用户消息即审批"语义不变 |
| 2 | thinking/effort 门控 @ `:1290/:1302` | `request_gates`（PlanFeature） | plan 激活时阻断降档 |
| 3 | `plan_mode_read_only` @ `:3671` + `tool_execution.rs:492-516` | `dispatch_gate` | engine 遍历 features 汇总 `GateDecision`；**"schema 仍广播、只拒执行"的前缀缓存语义不变**；`ProviderToolAuthority.plan_mode_read_only` 保留但值改由 gate 推导（Phase 5） |
| 4 | `apply_context_modifiers` plan 分支 @ `:3902-3926` | `apply_modifier` | engine 只路由；Enter/Exit 状态迁移、allow_list 快照/恢复全在 PlanFeature 内；`PlanModeTransition` 类型不动 |
| 5 | turn-tail plan/goal 块 @ `:1852-1866/:1911-1913` | **删除**，改走 §3.5 Reminder | **本方案唯一行为变更点** |
| 6 | 自然结束续作 @ `:2915-2977` | `on_natural_end`（GoalFeature） | horizon 观测/judge/续作整体收编；plan×goal 耦合改经 `HookCtx` 携带只读 `plan_status` 快照，替代 `horizon.decide(plan_active, awaiting)` 的参数耦合 |

辅助内联（`horizon_observe_tools` / `sync_goal_progress` / `emit_horizon_decision`，`:1402-1449`、`:3351`）随 GoalFeature 的工具后钩与自然结束钩一并收编。

### 3.5 ReminderService（system-reminder 注入通道）

```rust
// src/features/reminder/mod.rs
pub fn wrap_system_reminder(text: &str) -> String {
    format!("<system-reminder>\n{}\n</system-reminder>", text.trim())
}
// 变体注册（对齐 kimi-code 的 injector.register）：
reminders.register("plan_mode", |trigger: TriggerCtx| -> Option<String> { … });
reminders.register("goal",       |trigger: TriggerCtx| -> Option<String> { … });
```

**注入形态**：包装后的文本作为**持久化 `Role::User` 消息** append 到 `self.messages`。
区别于 turn-tail 的每 pass 临时注入——追加式历史，已发送前缀不受影响，前缀缓存只增不改。

> **实施修正（原方案此处写「带 `origin: { kind: "injection", variant }` 元数据」，未采用）：**
> `nomi_types::message::Message` 被 4 个 crate 共享，其 serde 形状在 §4 冻结清单内，
> 不为一个内部标记改 wire 契约。实际采用的等效机制是**「文本由 feature 状态派生 ⇒ 可再生」**：
> reminder 不携带任何消息级标记，其文本完全由 feature 状态决定，因此 compaction 丢弃后
> 在下个触发点自动重建。截断重启风险由另一条保证覆盖——信封以 `<system-reminder>` 开头、
> **不以 `[Context]` 开头**，所以 `is_turn_tail_context_text` /
> `is_context_only_user_content` 永不匹配它（有测试钉死）。

**触发点与频率策略**（对齐 kimi-code，按 nomi 现状校准；括号内为实际实现的常量）：

| variant | 触发（实际实现） | 内容 |
|---|---|---|
| `plan_mode` | plan mode **激活期间每回合一次**；每 `PLAN_REFRESH_AFTER_PASSES`（= `horizon::OFFICE_PLAN_SOFT`，8）个 pass 重发 | 现 `features/plan/prompt.rs` 的 `plan_mode_instructions()`（未改文案） |
| | plan mode **未激活**时返回 `None` | ——（「退出」不做一次性告知：退出后模型已无只读约束需要解除，注入只会多花 token） |
| `goal` | 回合开始、或该回合带新 user 消息（对齐 kimi 的 `isNewTurn`）；每 `GOAL_REFRESH_AFTER_PASSES`（= `horizon::OFFICE_PLAN_HARD`，12）个 pass 重发 | 三态派发，见下 |
| | `Active`/`Waiting` | 现 `goal/templates/goal_context.md`（迁移，未改） |
| | `Paused` | 新写的短状态块（为何暂停、不要自行开工、如何恢复） |
| | `Blocked` | 新写的短状态块（judge 给的阻塞原因 + 需要用户提供什么） |
| | `Complete`/`Cleared` | 静默（`None`），使已完成会话与无 goal 会话逐字节相同 |

注：方案原表写 `goal_{active,paused,blocked}.md` 与「objective HTML 转义进
`<untrusted_objective>`」。**这三个模板文件在仓库中不存在**，实际资产是
`goal_context.md`；`<objective>` 区块沿用原样（未引入 HTML 转义，因为该内容来自宿主
设定的 goal 而非模型/工具输入，且原文案已声明「把它当作要完成的任务本身，而不是更高
优先级的指令」）。paused/blocked 的文案**按 nomi 自身语义新写**，不照搬 kimi——kimi 的
那三个模板引用 `UpdateGoal`/`SetGoalBudget` 等 nomi 不存在的工具名。

文案头部统一吸收 turn-tail 调查 §5-A 措辞：明确"这是环境/状态信息，不是用户的新指令，不要据此复述"（实现在 `features/reminder/mod.rs` 的 `SYSTEM_REMINDER_PREAMBLE`，由 service 统一加在所有变体前）。

**compaction / 截断交互**：

- 注入消息**可再生**（文本由 feature 状态派生，无消息级标记）：compaction 可整段丢弃，下个触发点自动重建；
- `ReminderService` 支持 `urgent` 重注入，对应参考实现的 compaction splice 后再入（`reconcileAroundStep` / `ContextSpliced`）；
- truncation-restart 现有的 `is_turn_tail_context_text` / `is_context_only_user_content` 系列保持不动（turn-tail 仍服务于 date/ledger/`ContextContributor`）；新注入消息以 `<system-reminder>` 开头，天然不匹配，有测试钉死；
- 注入发生在 turn-tail 组装**之后**，因此 reminder 消息不会被贴上 `[Context]` 块；
- `ContextContributor`（backend 贡献者）**不动**，继续走 turn-tail。

**安全边界**：注入通道只承担"告知与引导"；强制力仍来自 dispatch_gate / on_user_request 钩子——即使模型忽略 reminder，只读门禁照样拒绝。这保持了 kimi-code "状态强制与状态告知分层"的设计。

---

## 4. wire 契约冻结清单

重构期间以下逐项**不得出现 diff**（验收时核对）：

1. `GoalState` 全部 serde 字段名（`goal/state.rs` 文件头明示 wire contract；DB 行 `goal_bridge.rs`、HTTP、`NomiGoalSpec.resume_state` 三处依赖）；
2. `GoalContract` 五字段（outcome/verification/constraints/boundaries/stop_when）与 `GoalContractDto` 镜像；
3. `GoalStatus` / `GoalVerdict` 的 snake_case 枚举串；
4. `GoalActionRequest` 动作词表与 `GoalStatusResponse`（`nomifun-api-types/src/goal.rs`）；
5. `GoalSpec` / `GoalRuntime` / `GoalWaitProbe` 公开方法签名（backend manager 大量调用）；
6. `nomi-types::skill_types::{PlanModeTransition, ContextModifier}` 形状（4 个 crate 共享 + `nomi-types/tests/plan_mode_transition_test.rs` 锁定）；
7. 工具名与 input schema：`EnterPlanMode` / `ExitPlanMode`（含 `plan`/`plan_content` 别名）/ `update_goal`；
8. 配置键：`plan.enabled` / `plan_directory`；
9. 引擎公开 façade 方法签名（§3.3 表格末行）。

**非契约、可自由重构**：`PlanState`/`PlanPhase`（纯 crate 内部、不持久化）、horizon 内部实现（遥测 `observation_payload` JSON 除外）、`Session` 结构（本就不含 plan/goal）、`engine/mod.rs` 私有字段布局。

---

## 5. 分阶段实施步骤

每阶段独立 commit、**测试全绿**才进下一阶段；Phase 3 与 Feature 化分 commit，保证注入迁移可独立 revert。

| Phase | 内容 | 行为变化 | 验证 |
|---|---|---|---|
| **0** | 按 turn-tail 调查 §4 采集**基线**：真实会话中每回合思考段数、tail 注入位置与段边界关系、tail 文本逐字稳定性 | 无（只读） | 基线数据存档进调查文档附录或 PR 描述 |
| **1** | Feature 骨架：`Feature`/`FeatureRegistry`/`FeatureHooks` + `AgentBootstrap` 注册路径；钩子空接线，plan/goal 暂仍内联 | 无 | `cargo test -p nomi-agent` |
| **2** | `ReminderService`：包装、变体注册、触发去重 + 单元测试；尚无注册方 | 无 | 同上 |
| **3** | **PlanFeature 抽取**：状态收归 `PlanService`；钩点 #1–#4 接线；plan 注入迁 system-reminder；删 turn-tail plan 块（`:1852-1866`）；同步改写 `engine/plan_mode_tests.rs`；复测 Phase 0 指标 | **有**（注入通道切换） | `cargo test -p nomi-agent` + `cargo test -p nomi-types` + 基线复测对比 |
| **4** | **GoalFeature 抽取**：horizon/goal 收编；钩点 #6 接线（`HookCtx` 带 `plan_status` 快照）；goal 注入迁 system-reminder；删 turn-tail goal 块（`:1911-1913`）；goal/horizon 模块内测试跟迁 | **有**（注入通道切换） | `cargo test -p nomi-agent` + 基线复测对比 |
| **5** | 收尾：`ProviderToolAuthority.plan_mode_read_only` 改由 `dispatch_gate` 推导；清理 engine 残留字段与死代码 | 无 | `cargo test -p nomi-agent` |
| **6** | 全量验证 + 文档同步 | 无 | 见 §7 |

---

## 6. 测试策略

**改写（白盒 → 公开接口）**：

- `engine/plan_mode_tests.rs`（255 行，结构体字面量构造 `AgentEngine` + 断言私有字段）→ 改为经 `AgentBootstrap` 测试构造 + 断言 feature 服务的公开读接口；**断言语义保持**（进入/退出/allow_list 恢复/latch 行为）。

**跟迁（尽量零改）**：

- plan 模块内 39 个测试、`tests/plan_tools_test.rs` / `plan_e2e_test.rs` / `plan_engine_test.rs` / `plan_prompt_file_test.rs`、`tests/acceptance/plan_mode_test.rs`——随目录移动；断言对象从 engine 私有字段改为服务公开接口，**工具名与文案断言不变**；
- `tool_execution.rs::plan_mode_read_only_refuses_write_without_execute`——改为对 `dispatch_gate` 的等价断言；
- `nomi-types/tests/plan_mode_transition_test.rs`——不动（契约）；
- goal 模块内 87 个 + horizon 17 个测试——随目录移动，预期零改；
- `plan_state_not_persisted_across_sessions`（plan_e2e）——语义保持。

**新增**：

- Reminder 包装与频率策略（full/sparse/refresh/exit 去重、goal 仅新 turn）；
- `dispatch_gate` 链式判定（多 feature 时 Deny 优先）；
- 注入消息 compaction 可再生（丢弃后下个触发点重建）；
- 注入消息不被 truncation-restart 误判为 turn-tail。

---

## 7. 验证与文档同步

按仓库 Verification Ladder：

| 步骤 | 命令 |
|---|---|
| 单 crate 行为 | `cargo test -p nomi-agent` |
| 跨 crate 契约 | `cargo test -p nomi-types` |
| bridge 层 | `cargo test -p nomifun-ai-agent`（goal_bridge / manager 相关） |
| 编译面 | `cargo check --workspace` |
| 聚合门 | `bun run check`（pre-PR 全量：`cargo check --workspace && bun run check`） |

**文档同步**（Phase 6）：

- `docs/architecture/agent-engine.zh.md`：引擎职责描述去掉 plan/goal 内联，补 Feature/钩点一节；
- `docs/guides/goals.md`：注入形态描述更新；
- `turn-tail-context-investigation.zh.md`：补一行"plan/goal 指令已迁出 `[Context]`，P2 就此消除；date/ledger/contributors 待各自处置"，并把 Phase 0/复测数据附上；
- 本文档：状态改为"已实施"，回填实际钩点行号。

---

## 8. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 注入通道切换同时改变"缓存是否还热"和"模型看到什么"，与调查文档"先量再改"冲突 | 归因困难 | Phase 0 基线 + Phase 3/4 后复测；reminder 迁移与 Feature 化分 commit，可独立 revert |
| 长工具循环中 plan 指令"过期"（原先每 pass 重贴 tail） | 模型引导变弱 | 频率策略含 pass 计数 refresh；**安全不依赖指令新鲜度**——dispatch_gate 始终硬拦 |
| `plan_mode_tests.rs` 等白盒测试大面积编译破坏 | 阶段性红 | 随 Phase 3/4 同步改写，不留编译红 |
| backend 契约被无意破坏（DB 行 / HTTP / resume） | 跨层回归 | §4 冻结清单逐项核对 + bridge 测试全跑 |
| `engine/mod.rs` 巨文件重构引发合并冲突 | 协作成本 | 按 Phase 分 commit；抽取一律"移动 + 委托"，不重写逻辑 |
| 持久注入消息在 compaction/truncation 路径产生边角 bug | 会话恢复异常 | 可再生标记 + 专项测试；truncation-restart 既有谓词不动 |

---

## 9. 验收标准

1. `engine/mod.rs` 中不再出现 plan/goal 专属字段与分支（钩点调用处为通用循环）；
2. `src/features/{plan,goal}/` 自包含：状态、工具、注入模板、钩子实现全在目录内；
3. plan/goal 指令与状态只经 `<system-reminder>` 注入，turn-tail 不再含二者内容；
4. §4 冻结清单逐项核对无 diff；
5. `cargo test -p nomi-agent`、`cargo test -p nomi-types`、`cargo test -p nomifun-ai-agent`、`cargo check --workspace` 全绿；
6. Phase 0/复测数据附档，确认注入迁移未引入 P1 式复述恶化。

**实施后的闸门口径修订（已与仓库负责人确认）：** `cargo test -p nomifun-ai-agent`
在本机（Windows）有 32 个**环境性**失败（29 个 spawn `sh -c` / `cat <<EOF`，
3 个环境敏感断言），在 `origin/main` 基线上同样失败。因此第 5 条的判据改为
**「与 `origin/main` 基线失败集逐项 diff 为零」**，而不是字面上的零失败。
`nomi-agent` 另有 1 个基线失败
（`badcase_regression_test::a_round_that_keeps_truncating_stops_at_three_passes`，
截断清理误删带 `[Context]` 的 resumable hint），本分支不修，已单列于 PR。

### 实际达成情况（2026-09-22 复核）

| # | 结论 | 说明 |
|---|---|---|
| 1 | **部分达成** | 专属字段已全部清空（`plan_state`/`goal`/`horizon` 等不再存在于 engine）；但 turn loop 仍残留 3 处 `self.goal_service()` 专名调用——用户入口直连（约 `:1867`，压在通用折叠上一行）、office plan nudge 取文案（约 `:1981`）、续作记账 `record_continuation`（约 `:3074`）。façade 委托（`set_goal`/`goal_state` 等）属 §3.3 允许范围，不计入。 |
| 2 | **plan 达成 / goal 未达成** | `src/plan/` 已删、内容入 `features/plan/`；goal 的状态/运行时/判官/模板仍在顶层 `src/goal/`，`features/goal.rs` 只是接线文件，两侧不对称。 |
| 3 | 达成 | `git grep` 确认 turn-tail 不再含 plan/goal 内容。 |
| 4 | 达成 | 契约 crate 零 diff。 |
| 5 | 达成（按上述修订口径） | 与 origin/main 基线失败集逐项 diff 为零。 |
| 6 | **未达成** | Phase 0 基线因本地 session store 早于 turn-tail 落地而无法采集；如实记录于 turn-tail 调查文档 §7，补采义务移交后续。 |

第 1、2 条的收尾工作曾启动（"Phase 5b"：钩点全量化 + `git mv src/goal → features/goal/`），
经权衡**决定取消**：属整洁性而非正确性问题，硬指标（3、4、5）已全部达标；中途 WIP
已丢弃，分支保持 6 个 commit。对应偏离见 §10 第 7 条。

---

## 10. 实施记录

五个 commit（基于 `origin/main` @ `1bb17c918`）：

| Commit | Phase | 内容 |
|---|---|---|
| `974e0f091` | 1 | `Feature`/`FeatureRegistry`/`FeatureHooks` 骨架 + bootstrap 注册路径，行为零变化 |
| `835a892e2` | 2 | `ReminderService`（信封、变体注册、回合内去重、刷新、compaction 重注入） |
| `613cd3de5` | 3 | `PlanFeature` 抽取 + 钩点 #1–#4 接线 + plan 注入迁移 |
| `7e9388da2` | 4 | `GoalFeature` 抽取（horizon/goal 收编、`on_natural_end`）+ goal 注入迁移 |
| `3d20e7ce5` | 5 | 删除 `src/plan/` shim 与失效 seam API |

### 与方案的偏离（逐条）

1. **Phase 3 的两次拆分合为一次 commit。** 方案要求「注入迁移」与 Feature 化分
   commit 以保证可独立 revert；但两者单独拆开**不可编译**——删掉 turn-tail plan
   块就删掉了当时唯一存在的 plan 注入路径，`plan_mode_instructions_*` 会一直红到
   reminder 通道落地。已改为一个 commit 并在 message 中分两部分讲清，整体可 revert。
   （已确认接受。）
2. **§3.5 的 `origin: { kind: "injection", variant }` 消息元数据未实现。**
   `nomi_types::message::Message` 被 4 个 crate 共享，其 serde 形状属 §4 冻结清单，
   不为一个内部标记改 wire 契约。等效机制：文本由 feature 状态派生 ⇒ 可再生；
   并由测试钉死 reminder 永不匹配 `is_turn_tail_context_text` /
   `is_context_only_user_content`（即元数据想防的截断重启风险）。
3. **Phase 4 的 goal 文案形态改为三态派发 + nomi 自写 paused/blocked。**
   方案 §3.5 写的是迁移 `goal/templates/goal_{active,paused,blocked}.md`，但**这三个
   文件不存在**；实际资产是 `goal_context.md` + continuation 系列。落地为：
   active/waiting 用原 `goal_context.md`（迁移，未改），paused/blocked **按 nomi 自身
   语义新写**（不照搬 kimi 模板——那三个引用 `UpdateGoal`/`SetGoalBudget` 等 nomi
   不存在的工具，照搬会误导模型），complete/cleared 静默。continuation 系列及其
   `Role::User` push 机制**原样未动**（那是续作通道，不在迁移范围）。
4. **horizon 物理位置取「进 goal 目录（逻辑上）」的形态。** 见 §3.1 留的实施期决策：
   `src/horizon/` 目录**保留在顶层**（纯算法 + 自有测试，且 `PLAN_REFRESH_AFTER_PASSES`
   复用了它的 `OFFICE_PLAN_SOFT` 常量，`goal/runtime.rs` 也依赖 `render_continuation_delta`），
   但 `HorizonController` 的**实例与调用权归 `GoalService`**，经 `HookCtx` 暴露的窄接口
   被 goal 的钩子调用。选此形态的理由：物理搬迁只会制造一个跨目录的相对路径改动 +
   重排 diff，而方案允许二选一，契约面不受影响。
5. **`update_goal` 的注册点留在引擎 façade，未进 feature。** 工具必须与其 runtime
   共享同一个 `Arc<Mutex<GoalState>>`，而工具注册发生在引擎的 `ToolRegistry` 上——
   feature 钩子拿不到 registry 句柄。`set_goal` / `set_goal_state` 因此先从 service 取
   状态槽再注册工具，工具与 runtime 仍不可能不一致。
6. **`ProviderToolAuthority.plan_mode_read_only` 不是「保留字段但改由 gate 推导」，
   而是整个换成 `DispatchGate`。** 方案 §3.4 钩点 #3 要求「保留该字段但值改由 gate
   推导」；实施下来一个冻结的 gate 快照比一个布尔位更贴合「per-request 授权快照」的
   语义（gate 需要知道本次调用的 category，而 category 依赖调用入参）。该字段
   **零调用方**（`git grep plan_mode_read_only` 只剩测试名与注释），因此删除无 API 损失。
7. **§9 第 1、2 条未完全达成（"Phase 5b" 收尾取消）。** 引擎 turn loop 残留 3 处
   `self.goal_service()` 专名调用（用户入口 / office nudge / 续作记账），goal 模块
   仍在 `src/goal/` 未并入 `features/goal/`。曾启动针对性清理（钩点全量化 +
   `git mv` + 公开路径再导出），经确认属整洁性问题、决定不作为本 PR 范围，
   WIP 已丢弃。若后续要补，入口即这 3 个调用点与一次目录搬迁，契约面不受影响。
   §9 实际达成状态见该节末尾的复核表。

### 实施期发现的方案与代码现实冲突

- **§2.2 的行号清单已漂移。** 实测（`1bb17c918`）：`approve_pending_plan` 在
  `:1386`、turn-tail plan 块在 `:1852`、goal `turn_context()` 在 `:1911`、
  `plan_mode_read_only` 在 `:3671`、modifier 迁移在 `:3902`；其余（用户入口
  `:1755`、请求门控 `:1290/:1302`、authority 快照、自然结束 `:2915`）与方案一致。
- **§3.5 的 goal 模板文件名不存在**（见偏离 3）。
- **§6 提到 `engine/plan_mode_tests.rs` 是 255 行、结构体字面量构造** —— 与现状一致；
  但同类字面量构造还散布在 `set_config_tests.rs`、`phase6_tests.rs`、
  `compact_tests.rs`、`handle_command_tests.rs`，方案的测试清单未列出，
  这些文件本轮按字段增删做了最小同步。
