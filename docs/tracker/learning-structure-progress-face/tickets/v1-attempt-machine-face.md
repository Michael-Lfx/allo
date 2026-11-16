---
id: v1
title: "注册表 attempt 机器面——TaskRecord/View 加字段，重生成循环回写（照 s4 三件套形状）"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# v1: 注册表 attempt 机器面

**What to build:** `generation_tasks.rs` 照 s4 `resolved_course_id` 三件套形状
加 attempt：`TaskRecord.attempt: Option<u32>`（`:102` 邻位）、
`GenerationTaskGuard::set_attempt(&self, n: u32)`（`:203-204` 形状）、registry
setter（`:507` 形状）、record 初始化 `None`（`:386` 邻位）；`GenerationTaskView`
暴露 `attempt: Option<u32>`（serde serialize，快照转换处 `:654` 邻位带出）。
`generate_structure_artifacts`（`structure_gen.rs`）加 attempt 回调参数
（`&dyn Fn(u32)` 或仓内等价形状），`for attempt in 1..=MAX_ATTEMPTS` 循环体
内每轮回调（含首轮 1）；调用侧 `learning_graph.rs::run_structure_generation_task`
闭包接 `task.set_attempt(n)`；`service/tests.rs` 调用侧传 noop 闭包。

## 法证证据（2026-11-11 真机 + 实码 @06091fd45）

- 真机：课程 01a1c8c3 的 attempt 1 于 12:08:01 被契约门拒收
  （`structure generation rejected — regenerating both artifacts with clause
  feedback` WARN），attempt 2 于 12:13:12 落库——全程 UI 无从得知轮次，注册表
  无 attempt 痕迹（G4）。
- `MAX_ATTEMPTS = 3`（`structure_gen.rs:19`，g2 裁决 learnhub 同款）；循环
  `:89 for attempt in 1..=MAX_ATTEMPTS`。
- s4 三件套先例：record `:102` / guard `:203-204` / registry `:507` /
  初始化 `:386`——形状逐点可抄；view 未暴露（s4 是后端内部终态解析用），
  attempt 是 UI 机器面必须暴露（serde 可选字段，老前端兼容）。
- 注册表进程内存态，无迁移面。

## 实施要点

- [ ] 四件套 + view 字段 + 快照转换带出（位置见 What to build，行号为当前
      尖端邻位）。
- [ ] 回调参数命名与仓内惯例对齐（评审时看 `&dyn Fn` 是否有先例，无则用
      最小闭包泛形）。
- [ ] 首轮也回调（attempt=1）——任务启动 `set_step` 处同步初始化，保证
      running 全程 attempt 非空（图课程流）；growth/lesson/outlin 等未回调
      任务 view 行 attempt = None。
- [ ] 单测：set_attempt → 快照行 attempt 正确；回调每轮触发（计数断言）；
      新任务 attempt 重置为首轮；未回调 kind 的 view 行 attempt=None；
      attempt 拒收重试路径（attempt 1 失败 → 回调 2）。
- [ ] `cargo fmt -p nomifun-learning`（本机按包跑）。

## 验收

- `cargo test -p nomifun-learning` 绿（含新增测试）。
- 真机（下次结构生成，可与 v2/v3 验收合并）：快照行 attempt 字段可见且随
  轮次更新。
- 快照 JSON 向后兼容：不带 attempt 的旧消费者零感知（serde 缺省）。

## 非目标

- WS 结构帧加字段（attempt 走快照通道，裁决见 map 非目标）。
- terrain 删键 / 契约门行为任何变更。
- v2 outline / growth / lesson 任务的 attempt 语义（它们的循环不在本票面）。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；真机目验与全模块欠账合并执行，FAIL 按票重开。**

- 施工 = `4b8ef6466`：四件套照 s4 形状 + `generate_structure_artifacts` 回调
  参数 `&(dyn Fn(u32) + Send + Sync)`（spawn 场景必要界，无界实测 E0277）+
  主路闭包接 `task.set_attempt` + tests.rs noop；TDD 红绿实录
  （E0599/E0609/E0061）。
- 评审修复 = `e36565525`（F5）：票面「任务启动 set_step 处同步初始化」首版
  落在生成循环入口，begin→循环窗口 running 行 attempt=None——修为
  `run_structure_generation_task` 在 set_step 处同步 `set_attempt(1)`，循环
  回调照旧；时序钉子测试（取消先于循环构造：旧实现 None / 新口径
  Some(1)，循环零回调读数只能来自起步段）。
- 验证（集成树 bb20d5261 独立复验）：cargo test -p nomifun-learning --lib
  **963 绿**（含三场景新测试：快照带出与轮次更新、同键重跑重置、拒收路径
  vec![1,2]）；fmt 过；警告对照基线零新增。
- 真机（下次结构生成）：快照行 attempt 可见且随轮次更新——与 v2/v3 验收
  合并执行。
