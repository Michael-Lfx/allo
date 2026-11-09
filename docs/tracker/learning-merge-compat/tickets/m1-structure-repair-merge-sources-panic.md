---
id: m1
title: "结构修补 merge_sources 重复源 panic——Err 化 + parse 去重 + 维护标志位守卫"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# m1: 结构修补 merge_sources 重复源 panic

**What to build:** 结构修补回合的 `merge_blocks` op 在 LLM 输出重复合并源
（`sources:["A","A"]`）时不再 panic（去重或 Err 化走既有重试反馈通道）；自动
维护 spawned 循环加死亡守卫（panic 不再让 `LEARNING_MAINTENANCE_STARTED` 永
久卡死）。

## 法证证据（2026-11-09 审计，实码复核）

- **panic 点**：`learning_graph/structure_ops.rs:633-638`——
  `blocks.iter().position(...).expect("source existence checked above")`。
  第一轮 `blocks.remove(position)` 移除 "A" 后，第二轮同源 `position` 返回
  None → expect 必炸。
- **三道前置检查都拦不住重复源**：
  - parse（:214-226）只拒绝 `sources` 为空，重复值照收；
  - validate（:344-345）逐端点查存在性，`["A","A"]` 两轮都过；
  - apply 预检（:571-575）同样逐端点查存在性，两轮都过。
- **触发通道**：修补回合 LLM op 流的重试循环（structure_ops.rs:816-828，本
  意是把 apply 失败转成重试反馈）与手动
  `POST .../graph/structure/repair`（service/learning_graph.rs:1026）。
- **爆炸半径**：生产定时器在 `LEARNING_MAINTENANCE_STARTED.swap(true)` 之后
  spawn 单任务循环（nomifun-app-server/src/lib.rs:2778-2830）；panic 杀死
  任务，标志位停留 true，此后每次调用 early-return「already scheduling」——
  **自动维护停摆到进程重启**；手动 repair 无 catch 层，直接断连。
- 命中「图课程可以破坏式更新，但至少不能导致程序报错」红线：panic 即报错。

## 影响面核查结论（已实码核实，无需再查）

- 图模块生产代码 panic 候选全量仅 2 处：本票 :637 必修；
  `name_adjudication.rs:157`（"mechanical check guarantees exactly one row
  per pending name"）有前置校验把守（:211-219 每名恰一条裁决 + :203 拒绝
  多余名字），属同类别顺手加固，不单独立票。
- `fragment_verdict.rs:400/408` 的 expect 全在 `#[cfg(test)]`，非生产炸点。
- 维护循环对 `Err` 是健壮的（run_maintenance_tick 失败只 warn 继续，lib.rs
  tick match 臂）——唯独 panic 没有兜底。

## 实施要点

- [ ] **parse 去重**：`merge_blocks` 的 sources 解析处保序去重
      （`["A","A"]` → `["A"]`，语义无害：合并 A,A→B 等价 A→B）。去重后仍走
      既有校验链，无新增违约形态。
- [ ] **apply 侧 Err 化兜底**：:633-638 的 `position(...).expect(...)` 改为
      `ok_or_else(|| format!("{at} 源块「{source}」不存在"))?`——即便未来
      出现绕过 parse 的新调用面，也走 Err → 既有重试/失败留痕通道，不 panic。
- [ ] **维护循环死亡守卫**：spawn 任务体加 panic 兜底（`AssertUnwindSafe`
      catch_unwind 或等价机制），panic 时 `tracing::error` 记死因并复位
      `LEARNING_MAINTENANCE_STARTED`，下个 tick 重新拉起；或用 drop guard
      在任务退出（含 panic 展开）时复位标志。取实现最简者。
- [ ] **顺手加固**：name_adjudication.rs:157 同类 expect 改 Err（前置校验
      在，防御性转换一行）。
- [ ] 红绿样单测：①`sources:["A","A"]` 完整回合（parse→validate→apply）
      不 panic 且等价单源合并；②重复源在 parse 层被去重的断言；③维护任务
      panic 后标志位复位、下轮可再拉起（可用注入 panic 的假 service 测）。

## 验收

- `cargo test -p nomifun-learning` + `-p nomifun-app-server` 全绿；fmt。
- 零迁移、零 DB 变化、零 API 形状变化。

## 非目标

- name_adjudication 校验逻辑重构（只做防御性 Err 化）。
- 维护回合的业务语义/调度策略改动（开关、间隔、水位全不动）。

## Resolution（2026-11-09 施工收口）

**代码面验收全过，已闭票。** 施工 = `b930762de`（parse 保序去重 / apply Err 化 /
liveness 守卫 Drop + catch_unwind 镜像 nomi-agent 工具执行面既有模式）+ 评审
补测 `c0aa5af73`（维护循环吞 panic 假 service 并推进下轮）。

- 红绿样：红样直接钉在票面 :637 panic 点（重复源完整回合 panic 实录）+ parse
  去重断言红样；绿 = `cargo test -p nomifun-learning` **941 passed** /
  `-p nomifun-app-server` **195 passed** 全绿，fmt 过。
- name_adjudication :157 落为 `let-else` → 既有 `Adjudication::Degraded` 通道
  （非字面 Err）——合「既有通道」精神，措辞偏差记档（评审轴确认）。
- 守卫测试路径取舍（评审轴补测时确认）：`start_learning_auto_maintenance` 收
  具体类型 `Arc<LearningService>`（无 trait/泛型）+ 私有 30s tick 常量 + 进程级
  one-shot 标志位，真实 spawn 链无法注入 panic 假 service 且不为测试开生产口子
  → 测 `learning_maintenance_tick_guarded` 包裹层（panic → Err 死因 → 循环推进
  → 下轮重拉）+ 守卫结构体复位单测，两者合计覆盖票面语义。
- 零迁移、零 DB/API 形状变化（验收条款满足）。
