---
id: pg3
title: 删课收尾——在途生成任务/课时草稿/迟到写（t3 只收了 quiz 回合，没收这些）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub #275（2026-11-26，票面自述「来源: allo t3 的同构面」）在那边
发现：删课不取消在途任务/在簿会话，写回复活目录，课程列表整体报错。
本仓是 DB 软删（墓碑 + 读侧 `deleted_at` 过滤），不会出现「目录复活
拖垮列表」；但同一族缺口在本仓**没有被 t3 收掉**——t3 只废弃了 quiz
回合与死卡，没收在途生成任务、课时草稿、以及迟到写。用户删课后仍可能
看到该课的 growth 任务 pill；迟到写会把内容/挂载/findings 写回墓碑课，
而其中几张表不在 purge 清单里。

## 已核验事实（2026-11-26 对码）

### 删除路径只做四步

`crates/backend/nomifun-learning/src/routes.rs:1083-1090` →
`service/course.rs:751-811`：死卡打标 → `abandon_active_rounds` →
`purge_course_content`（8 张内容表）→ 墓碑（`deleted_at` +
`enabled=0`）。**没有** `generation_tasks`、没有课程锁、没有清内存草稿。

### 在途任务不收

- 生成任务只有按键取消（`routes.rs:40`、`:284-300`；
  `generation_tasks.rs:582-611`），删课不调用。
- 课时内容任务明确不可远程取消（`generation_tasks.rs:601-605`），全局
  清扫也跳过它（`:615-630`：`STATE_RUNNING && kind != KIND_LESSON`）。
- 课程级 slot 只覆盖 structure/mount/repair（`learning_graph.rs:330`、
  `:645`），删除路径不取/不查；学习 crate 无 `Resource::Course`。
- 任务记录快照（`generation_tasks.rs:639-706`）与 UI growth pill
  （`ui/src/renderer/pages/learning/growthTaskPill.ts:90-113`）都不按
  课程存活过滤——删课后 pill 仍列该课任务。

### 内存草稿/唤醒不收

- 课时草稿：`service/mod.rs:114-119`、`lesson_draft.rs:16,57-77`，
  TTL 1h；发布路径只移除内存条目（`lesson_draft.rs:164-220`），落库
  时不复核课程存活。
- 生长/修补唤醒：`service/mod.rs:163-175`；后续轮次会重新判
  `deleted_at`/`enabled` 并 no-op（`learning_graph.rs:1603-1612`、
  `repair_followup.rs:102-109`）——这一半已有防御。

### 迟到写不设防

- `apply_batch` 在事务外读 `module_id`（`learning_graph.rs:3938-3946`），
  然后 `INSERT INTO learning_lessons`（`:4072-4087`）——全段无
  `deleted_at/enabled` 复核。
- 结构快照直写：`learning_graph.rs:456-460`、`:1321-1324`
  `UPDATE learning_courses SET structure_json = ?` 无存活谓词。
- 挂载 upsert：`:885-901`（`ON CONFLICT(course_id) DO UPDATE`）。
- 课时发布：`lesson.rs:2239-2250` 不检查 UPDATE 的 `rows_affected`，
  照插 sections/activities（`:2218-2237`）。
- purge 清单只有 8 张内容表（`course.rs:912-942`）；
  `learning_growth_batches`、`learning_structure_mounts/findings/repairs`、
  `learning_maintenance_state`、`learning_system_actions` **不在**清单里，
  且契约无物理外键（`course.rs:899-901`）。

### 严重度（本仓形态）

- 不会拖垮课程列表（墓碑 + 读侧过滤）；不会复活目录（无目录）。
- 可见面：growth pill 列墓碑课任务；迟到写可能污染全局已教摘要
  （`render_taught_summary` 无课程 join 过滤，`service/learning_graph.rs:5662-5670`）
  与未 purge 的观测/结构表。
- 概率：删课与自动补批/课时生成并发。用户的常用流程「删旧课重建」
  正是这个窗口。

## 影响面（2026-11-26 对码）

### 生产面

| 文件 | 触点 | 改动性质 |
| --- | --- | --- |
| `routes.rs` | `delete_course` 处理（`:1083-1090`） | 若选「忙拒」：取课程闸/返回 409；若选「取消再删」：调用收尾 |
| `service/course.rs` | `delete_course`（`:751-811`）、`purge_course_content`（`:899-942`） | 收尾调用 + 非内容表清理/过滤 |
| `service/generation_tasks.rs` | 任务键 `{kind}:{course_id}:{node_id}`（`:331`）、`cancel`（`:582`）、`cancel_running`（`:615`）、`snapshot`（`:639`） | 新增 `cancel_for_course(course_id)`；lesson 任务取消策略 |
| `service/mod.rs` | `lesson_drafts`/`lesson_draft_ids`（`:114-124`）、`growth_wakes`/`repair_followup_wakes`（`:163-175`） | 新增按课程清（draft id 反查 lesson → course） |
| `service/learning_graph.rs` | `apply_batch`（`:3929-4340`）、结构快照 UPDATE（`:456-460`、`:1321-1324`）、挂载 upsert（`:885-901`）、`require_live_graph_course`（`:1892-1917`） | 写前存活复核（可见失败、零写） |
| `service/lesson.rs` | 课时发布（`:2239-2250`、`:2218-2237`） | UPDATE 的 `rows_affected` 必须命中；否则零写失败 |
| `service/lesson_draft.rs` | 草稿发布路径（`:164-220`） | 发布前复核课程存活 |
| `ui/.../growthTaskPill.ts` | 快照合并（`:90-113`） | 后端快照筛墓碑课即可，或删除事件触发重拉 |
| `ui/.../GrowthTaskPillHost.tsx` | 轮询/订阅 | 若快照已筛，零改 |

### 测试面

- `service/tests.rs` 删除族：`delete_course_soft_purge_leaves_zero_content_orphans`
  （`:3696`）、`delete_course_finalizes_active_round_and_dead_cards`
  （`:3756`）、`archived_course_cards_leave...`（`:4321`）等 5+ 条；
  新增红测（修前红）：删课中途 growth、删课时 lesson 草稿、墓碑课
  结构快照/挂载/发布零写。
- `generation_tasks.rs` 取消/快照测试（`:984-1162`）——新增按课程取消
  与快照过滤用例。
- `learning_graph.rs` growth apply 测试——新增「课程已删 → apply 失败
  零写」。
- `lesson.rs` 发布测试——新增「墓碑课发布零写」。
- UI：`growthTaskPill.render.test.tsx` / `growthTaskPill` 单测（若改）。

### 不受影响 / 已覆盖

- **quiz 回合与死卡**：t3 已收（`course.rs:772-789`），本票不重复。
- **账本/XP/复习卡**：ADR-0017 保留语义不动。
- **墓碑/恢复**：不引入硬删或恢复路径。
- **DB 迁移**：无；非内容表清理是 DELETE 语句，不是 schema 变更。若
  选择「保留审计表 + 读面过滤」，则只改读侧查询。

### 风险

- **lesson 任务不可取消**：`generation_tasks.rs:601-605` 明写暂不支持；
  若选「取消再删」需先把 lesson 任务纳入取消协议，或接受「先登记
  删除态、发布时零写失败」的软收尾（可见失败，不静默）。
- **审计表去留**：findings/repairs/maintenance/system_actions 是审计/
  观测面，purge 会丢历史；建议优先「读面按墓碑课过滤」，物理清理由
  独立清理票裁。
- **任务 pill**：后端快照按课程存活过滤最省 UI 改动；不要让 UI 猜
  课程列表。
- **task key 前缀**：`{kind}:{course_id}:` 可枚举，但 `kind` 有多种
  （structure/growth/lesson/outline），取消要按 course_id 扫全部 kind。


## 方案（建议）

1. **删除持课程闸**：删除前取本课 structure/mount/repair/growth 的
   slot（与在跑任务互斥）：有在跑任务 = 忙拒并告知，或先取消再删
   （裁决点；建议「取消再删」对用户更顺，但需保证取消可见）。
2. **收尾在途**：取消本课 growth 任务；把课时草稿按课程清掉
   （`lesson_drafts`/`lesson_draft_ids`）；唤醒表按课程清掉（即便后续
   轮次会 no-op，也省一次调度）。
3. **迟到写防御**：在 `apply_batch` 事务内、结构快照 UPDATE、挂载
   upsert、课时发布前判课程 `deleted_at IS NULL AND enabled = 1`；
   不存活 = 可见失败、零写（不是静默 drop）。
4. **非内容表收尾**：purge/关闭本课的 growth batches、mounts、
   findings、repairs、maintenance_state、system_actions；或至少让读面
   统一过滤墓碑课（审计面仍可查）。
5. **UI**：growth pill 过滤已知存活课程（或删除发 WS 事件让 pill
   刷新）；不为墓碑课显示任务。

## Acceptance

- 红测先行（修前红）：
  - 启动一个可控延迟的 growth（替身 completer）→ 删课 → 任务收尾 →
    `apply_batch` 零写、无孤立 lesson/concept 行；
  - 开一个课时草稿 → 删课 → 发布失败可见、无 lesson 行；
  - 删课带在跑任务：行为符合裁决（忙拒/取消），日志有一行。
- 单测：墓碑课的结构快照/挂载/发布写路径全部零写；purge 后非内容表
  无本课残留（或读面过滤有测试）。
- UI：growth pill 不再显示墓碑课任务（若有改动，
  `bun run typecheck` + 相关测试）。
- `cargo test -p nomifun-learning` 全绿；无 DB 迁移。

## Out of scope

- 硬删除/恢复路径（ADR-0017 墓碑语义不动）。
- 课程列表读侧「坏行单目录降级」——本仓无目录，不适用。
- 为课时任务新增远程取消协议（若选「取消再删」则最小化：复用现有
  token/任务键，不扩 UI 协议）。

## Resolution（2026-11-26 施工）

**结论**：删课收尾（任务/唤醒/草稿）与六张观测表 purge、四条迟到写
守卫落地；在跑 lesson 任务仍不可远程取消，靠发布守卫可见失败。

- `generation_tasks.rs` 新增 `cancel_for_course(course_id)`：按
  `{kind}:{course_id}:{node}` 取消在跑/排队的非 lesson 任务；在跑 lesson
  仍不可远程取消（发布路径有墓碑守卫），排队 lesson 可取。
- `LearningService` 新增 `cancel_course_generation_tasks` /
  `clear_course_wakes`（growth + repair follow-up 两表）；
  `lesson_draft.rs` 新增 `clear_lesson_drafts_for_course`（按课时 id 清
  draft 与 lesson→draft 映射）。
- `delete_course`：存在性预检后、purge **之前**清课时草稿（purge 后
  lesson→draft 反查会落空）；事务内取消任务/清唤醒；purge 清单 8 → 14，
  新增 growth batches / mounts / findings / repairs / maintenance_state /
  system_actions；提交后打一行 info（含取消数/草稿数）。
- 迟到写守卫：`apply_batch` 入口查存活（删/归档 → 可见失败、零写）；
  结构快照两处 UPDATE 加 `deleted_at IS NULL AND enabled = 1` + rows_affected
  检查；挂载 upsert 前查存活；课时发布 UPDATE 加课程存活子查询 + 零行回滚。
- 测试：`delete_course_purges_observation_tables_and_cancels_tasks`、
  `delete_course_clears_lesson_drafts_before_purge`、
  `apply_batch_refuses_deleted_course_before_any_write`（新）、
  `cancel_for_course` 2 项（新）、既有软/硬删零孤儿测试扩查六表；全量
  1137/0。
- Code-review 修复（2026-11-27）：草稿清点从 purge **后**挪到 purge
  **前**（purge 删课时行后反查恒空，旧实现会漏清）；补
  `delete_course_clears_lesson_drafts_before_purge` 与
  `apply_batch_refuses_deleted_course_before_any_write`；`(bool,bool)`
  存活旗收为 `CourseLiveness` 枚举；`cancel_for_course`/`cancel_running`
  共用 `cancel_matching`；「跳过但未结算」SQL 片段收为
  `skipped_not_settled_clause` 单源。
- 欠账：在跑 lesson 任务无法取消是既有协议限制；它靠发布守卫失败可见，
  不再写回墓碑课。UI pill 未加删除事件，任务被取消后由快照显示终态。

### 真机补跑（2026-11-27，HTTP/DB）

新二进制 + 临时 fresh 数据目录起本地 `nomicore --local`，真 HTTP：

- 种入 `learning_growth_batches` / `learning_structure_findings` /
  `learning_structure_mounts` 各 1 行 → `DELETE /api/learning/courses/{id}`
  返回 **200** → 三表按 `course_id` 归零；课程行 `deleted_at` 置位、
  `enabled=0`；`GET /courses` 不再含该课。
- 在途任务/草稿取消仍由单测覆盖（`cancel_for_course` 2 项、
  `delete_course_clears_lesson_drafts_before_purge`、
  `apply_batch_refuses_deleted_course_before_any_write`）；真实“删课时有
  生长任务在跑”的 UI 时序未跑。
