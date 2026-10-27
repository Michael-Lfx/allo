---
id: m10
title: "自动补批落地——verdict 显式化 + 工作区加载触发 + 开关与重入集"
type: task
status: closed
assignee: zhangyj
blocked_by: [m7]
part_of: ../map.md
---

# m10: 自动补批落地

**What to build:** 把 allo 既有的「完成时点 + 就绪<3」生长触发包装成显式纯读
verdict 函数，新增第二触发点（课程工作区加载 → 只读裁决端点 → Due 自动补批），
配设置开关、环境闸与重入集。定案见 m6 Resolution；learnhub 参考 = #121/ADR-0024
（coach.rs `auto_refill_verdict` :2624-2686、start_session 补批循环、770cfd4
「自动准备中」派发口教训、c2967cf 防失控四件）。

## allo 现状

- 触发现状：手动 POST grow（routes.rs:284-297）+ `maybe_kick_growth`
  （service/learning_graph.rs:1394，progress.rs:100 完成或跳过后调）；
  `kick_growth`（:1350-1388）内含 `require_live_graph_course` :1359、
  `READY_TRIGGER=3` 门 :1362、停滞臂 :1365、课级 slot 锁 :1371-1378、
  spawn :1383。
- 常量：`READY_TARGET=5` / `READY_TRIGGER=3` / `MAX_BATCH_NODES=5`
  （learning_graph/mod.rs:49-55；CONTEXT.md Growth 词条已对齐）。
- k02 维护 tick（app-server lib.rs:2736-2820）：调度器形态先例（tick 重读设置）。
- m7 资源闸落地后：任务提交走闸（本票前置）。

## 实施要点

- [ ] **verdict 纯函数**：`auto_refill_verdict(course 读数) ->
      AutoRefillVerdict { Due, BelowThreshold, Disabled }`——输入就绪存量与
      开关；带数字裁决日志一行（`auto_refill: ready=X verdict=Due`）。
      `kick_growth` 现有 READY_TRIGGER 门改为消费 verdict（行为等价重构，
      既有测试不回归）。
- [ ] **触发②**：`GET /courses/{id}/growth/verdict`（纯读，含 Disabled 臂）；
      前端课程工作区加载时调用，Due → 自动调既有 POST grow（后台补批）+
      「自动准备中」诚实呈现（不放假通知）；非图课程/停摆臂按现有语义。
- [ ] **重入集**：每课每裁决轮至多触发一批（前端会话内标记 + 后端
      slot 锁/任务注册表双保险，learnhub BTreeSet 语义域中立化）。
- [ ] **设置 + 开关**：`learning.autoRefillEnabled`（client_preferences，
      默认 1；设置页控件 + i18n 双语）+ `NOMI_LEARNING_AUTO_REFILL=off`
      （switches.rs 同款，off = verdict 恒 Disabled、端点如实读数）。
- [ ] WS 零新事件（复用既有 growth 事件）；slot 锁串行既有语义即并发 1。

## 验收

- **单测（红绿）**：verdict 三态（就绪≥3 BelowThreshold / <3 Due / 开关 off
  Disabled）；重入（同轮二次裁决不重复触发）；重构后 kick_growth 既有门语义
  等价（既有测试全绿）。
- **smoke 双臂（开关臂）**：`grow`/`loop` 档既有判据不劣化；新增读数行
  （verdict 结果 + ready 数）；off 臂 verdict=Disabled 如实。
- **对照卡**：同 fixture 课程两臂真机——on 臂工作区加载触发补批（批后 ready
  读数上升）、off 臂不触发；ready 前后读数并排落 Resolution。两臂都跑。
- `cargo test -p nomifun-learning` 全绿；typecheck/i18n/check 门过。

## 非目标

- 暂停线（m6 定案：无源不造）；寄宿维护定时器；传统课程触发面（grow 是图
  课程专属）；#145 完整资源元组（m7 领地）。

## 备注

learnhub 两条教训直接继承：日预算闸撤销（过度拦截「长批→跳批→再长批」正常
循环）、「自动准备中」空承诺（770cfd4：派发口必须真实可达——本票前端 Due
即自动发批，不挂死通知）。

## Resolution（2026-10-27 关票，implement-spec 波收口）

- **交付**（learn-v5/m10-auto-refill，14d340ae8 等）：`auto_refill_verdict` 纯函数（Due/BelowThreshold/Disabled，阈值维持 READY_TRIGGER=3、无暂停线——m6 定案）+ kick_growth 等价重构消费 verdict（带数字裁决日志）+ `GET /courses/{id}/growth/verdict` 纯读 + 前端工作区加载 Due 自动发批（诚实「自动准备中」，失败/未派发释放配额、BTreeSet 重入语义中立化，7 bun 测试）+ 设置 `learning.autoRefillEnabled` 缺省开 + 环境闸 `NOMI_LEARNING_AUTO_REFILL`。
- **双臂对照（开关臂）**：assets/m10-auto-refill-comparison-card.md——on 臂 `verdict=due ready=0 → below_threshold ready=4 → ready=5`（补批真实落库、就绪读数上升）；off 臂 `verdict=disabled enabled=false`（EXIT=2 系开关门控自动臂的**既定行为**，手动/force 豁免）。工作区 UI 触发 = 桌面手测项（CLI 不可达，如实注记）。
