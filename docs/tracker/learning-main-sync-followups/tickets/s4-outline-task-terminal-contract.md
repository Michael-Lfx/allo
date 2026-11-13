---
id: s4
title: "v2 outline 任务终态契约——成功路径 finish_done + 补挂真实 course_id"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# s4: outline 注册表终态契约（create-progress-ux 后置项 #1 正式立票）

**What to build:** v2 建课（outline）任务当前**成功也在注册表落
failed**，且注册表行的 course_id 是会话 id——恢复态翻 done 后前端拿它拉
课程详情必 404，课程建成却呈现失败。两处一起修：

1. 成功路径显式收尾：建课完成点 `_task.finish_done(...)`（终态首写即定，
   Drop 兜底只留给失败/中断/panic）。
2. 课程落库点把注册表行的 course_id 补挂成真实课程 id（新增
   `set_course_id` 面板 API），快照 DTO 优先取显式字段——前端恢复态
   `fetchCourse(row.course_id)` 从此拉得到。

## 法证证据（2026-11-11 审计，实码复核）

- `service/generate.rs:132-135`：`begin_generation_task(KIND_OUTLINE,
  &session, None, &topic)`，`_task` 守卫全文件零 finish 调用。
- `generation_tasks.rs:217-229`：Drop 对未收尾守卫写
  `STATE_FAILED "任务中断（未收尾即退出）"`；`finish` 首写即定
  （:508-510 非 running 直接 return）→ 成功 outline 任务终态恒
  failed，观测行（learning-tasks.jsonl）同样记 failed。
- 注册表键 course 段 = 会话 id（generate.rs:129-134 注释「键的课程段 =
  会话 id（建课时课程尚不存在）」）；注册表函数面（begin/pre_register/
  begin_registered/set_step/finish/cancel/cancel_running/snapshot/
  running_newest）无补挂 course_id 的 API。
- 前端恢复态（useCourseCreation.ts:126-141）：`row.state === 'done'` →
  `fetchCourse(row.course_id)`，失败即收敛 failed 并把原始错误给用户。
- 此票 = `docs/tracker/learning-create-progress-ux/map.md` 后置清单 #1
  （「待在飞恢复波尘埃落定后另立」，c 波已收图，正式立入本波）。

## 实施要点

- [ ] 注册表记录 + 守卫新增 `set_course_id`（显式字段，初值 = 键上的
      课程段；snapshot DTO 优先读显式字段）。
- [ ] generate.rs 建课成功点：`set_course_id(真实 id)` +
      `finish_done(...)`（detail 措辞沿 c3 契约的诊断面惯例）。
- [ ] 失败/取消路径行为不变（Drop 兜底 + cancel 首写语义保持）。
- [ ] 注册表级红绿测试：outline 任务 done + 显式 course_id 进快照；
      未收尾仍 failed「任务中断」；cancelled 不被 Drop 覆写。
- [ ] 前端无改动（`row.course_id` 现在拿到的就是真实 id）。
- [ ] `cargo test -p nomifun-learning service` 过；全套并入收图验证。

## 验收

- 新测试红绿两面齐；nomifun-learning 全套绿。
- 机制自查：模拟 outline 成功路径后快照 state=done、course_id=真实 id。

## 非目标

- 注册表键 re-key（键格式 `{kind}:{course}:{node}` 不动，仅 DTO 显式字段）。
- 恢复态 UI 其它行为（c2 已收敛的重试守卫等不动）。
- lesson/growth/structure 各 kind 的收尾（它们已有显式 finish）。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；恢复态收敛的用户可见目验留待真机批次。**
施工 = `eb0e84bed`。

- 设计要点：不改 `record.course_id`（视图 key 由它现算，改动会打断恢复态
  按 taskKey 匹配），新增独立字段 `resolved_course_id: Option<String>`，
  快照视图 `course_id` 优先取它、键保持会话 id 段。
- `generate.rs` 成功臂：`set_resolved_course_id(真实 id)` + `finish_done("课程已建成")`；
  Drop 兜底（failed「任务中断」）与 cancel 首写语义保持不变。
- 注册表 21 测试全绿（含 2 新：done+真实 id 不 re-key；未收尾仍 failed+会话段）。
- 观测面副作用一并修正：成功 outline 的 finish 行不再记 failed。
