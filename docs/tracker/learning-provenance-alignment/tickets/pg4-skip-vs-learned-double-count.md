---
id: pg4
title: 已学完仍可置跳过——账本/读数双计与卡片分子分母分裂
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub #286（2026-11-26，票面自述「来源: allo c1 的衍生问题」）发现：
已结算节点仍可被「跳过(我已会)」置位，于是同一节点同时进「已学完」与
「已跳过」两个读数。本仓有同构病灶，且更具体：完成过的课时可被
`skipped` 覆盖（`completed_at` 被清掉），XP 账本却保留；课程卡进度条
用 `completed_lessons`、文字分子用 `settled_or_completed`（含被跳过的
已结算节点），分母却剔 skipped——出现「1/1 课时 + 0% 条」的分裂脑。
本仓的 `skipped` 是课时状态（不是 learnhub 的节点级「自报已会」），
但「已学完仍可跳过」的语义矛盾与双计完全同构。

## 已核验事实（2026-11-26 对码）

### 置位不拦已结算

- `crates/backend/nomifun-learning/src/service/progress.rs:54-145` 只做
  形状校验（`:5-23`），不做当前态检查；upsert 直接覆盖
  `ON CONFLICT … DO UPDATE SET status = excluded.status …`
  （`:112-118`）。
- 置 skipped 时写 `(existing_started, None)`（`:104-107`）——把已完成的
  `completed_at` 清掉。
- XP 账本不删：`xp.rs:36-44` 幂等锚 `node:{lesson_id}`；置跳过后账本行
  仍在。
- UI 跳过钮无条件：`ui/src/renderer/pages/learning/LearningGraphWorkspace.tsx:1216-1228`
  （`status === 'skipped' ? 'not_started' : 'skipped'`）；测试注释明写
  「完成过的课时再被跳过（图工作区对已完成节点也开跳过钮）」
  （`service/tests.rs:11064-11067`）。

### 读数分叉

- 状态口径互斥：`service/course.rs:232-237` 按 `p.status = 'completed'`
  vs `'skipped'` 分别计数；图侧 gauge 也按状态
  （`service/learning_graph.rs:3567-3575`）。
- 账本口径不互斥：课程卡分子 `settled_or_completed` 来自
  `settled_nodes`（`course.rs:238-240`、`:1092-1097`），**无 skip
  过滤**；同一节点可以既在「已学完」文字里、又在 `skipped_lessons`
  里。
- XP gauge 是例外：分子分母都剔 skipped（`xp.rs:284-301`，
  `AND node_id NOT IN ({skipped_scope})`）。
- quiz 触发计数不剔 skipped：`xp.rs:428-446`。
- 卡片分裂：`CourseCard.tsx:113-114` 进度条百分比用
  `completed_lessons`；`:204-207` 文字分子用 `settled_or_completed`；
  分母是 `total − skipped`（`ui/.../model.ts:425-426`，
  `types.ts:260-262`）。
- 无「已跳过 K」拆读：`XpReading` 无 skipped 字段
  （`models.rs:3151-3171`）；XpPanel 只有「已结算 settled/total 节点」
  + 剩余时长/ETA（`XpPanel.tsx:44-62`）；课程卡只显示「N/M 课时」，
  skipped 被静默减掉（`CourseCard.tsx:198-212`）。

## 影响面（2026-11-26 对码）

### 生产面

| 文件 | 触点 | 改动性质 |
| --- | --- | --- |
| `service/progress.rs` | `update_lesson_progress`（`:54-145`）、置 skipped 分支（`:104-107`）、upsert（`:112-118`） | 置 skipped 前查当前状态/账本；已完成 → 人面错误 |
| `service/course.rs` | `skipped_lessons`（`:232-237`、`:296`）、`settled_or_completed`（`:238-240`、`:1092-1097`） | 读数同一口径：skipped 只计未结算；分子/分母一致 |
| `models.rs` | `CourseSummary.skipped_lessons`/`settled_or_completed` 文档（`:2117-2125`） | 口径注释同步；不改 DTO 形状 |
| `service/xp.rs` | XP gauge（`:284-301`，已剔）、quiz 触发清单（`:429-446`） | quiz 触发与 XP gauge 同口径剔 skipped |
| `ui/.../CourseCard.tsx` | 进度条（`:113-114`）、计数行（`:196-207`） | 分子改与进度条同口径（或统一用 completed/settled 中的一个） |
| `ui/.../CourseWorkspace.tsx` | 分母/百分比/文本（`:94-95`、`:304`） | 与新口径对齐（现在是一致的 completed 口径） |
| `ui/.../model.ts` | `progressDenominator`（`:425-426`）、`progressReadoutText`（`:437-447`） | 下拉口径说明；拆读时加文本 |
| `ui/.../types.ts` | `skipped_lessons`/`settled_or_completed`（`:260`、`:448-450`） | 注释同步 |
| `ui/.../XpPanel.tsx` | 只显示 settled/total（`:44-62`） | 若补拆读，加「已跳过 K」 |
| `ui/.../locales/*` | `learning.json` | 若补拆读：zh-CN/en-US 双语 + `gen:i18n` |

### 测试面

- `service/tests.rs`：`course_summary_counts_skipped_lessons_and_unskip_restores_denominator`
  （`:4522`）、`xp_reading_gauge_excludes_skipped_lessons`（`:11011`）、
  完成后再跳过的既有用例（`:11064-11067` 注释处）。
- 新增：已完成 → skip 被拒；撤销 skip 照常；存量「completed+skipped」
  在读数里只计一次；卡片分子 ≤ 分母；quiz 触发清单剔 skipped。
- 前端：`model.test.ts` 的 `progressDenominator` 用例（`:429-440`）、
  `CourseCard`/`CourseWorkspace` render test（若有）。

### 不受影响

- **XP 账本/事件**：`learning_xp_events` 不删不改（ADR-0017）。
- **DB/迁移**：零迁移；存量行读侧自愈。
- **学习会话/结算**：只改置位门与读数，不动结算流程。
- **lesson `skipped` 状态本身**：保留状态与 unskip 可逆语义。

### 风险

- **撤销完成通道**：现状 UI 只有「跳过/撤销跳过」；若用户把它当撤销
  完成用，直接拦会砍通道（见上文裁决点；备选是保留转换但按撤销完成
  语义读 + 另开撤销入口）。
- **卡片口径切换**：`CourseCard` 文本用 settled、条用 completed；
  修完必须让两者在「有 skip 的完成课」上给出同一读数，且分母不为 0。
- **存量重叠行**：只靠读侧自愈，不改旧行；若用户再次点「撤销跳过」，
  状态回 `not_started`，完成态与账本仍在——这是既有语义，不用改。


## 裁决（建议）

- **已结算课时不可再置跳过**：置 skipped 前查当前状态/账本；已完成 =
  人面错误句（如「这个课时已经学完，不需要跳过」），不覆盖
  `completed_at`。撤销跳过（`skipped → not_started`）不受限——存量
  重叠数据靠撤销清理。
- **裁决点（若「跳过」是撤销完成的唯一通道）**：现状 UI 只有
  「跳过/撤销跳过」，没有独立「撤销完成」；若用户实际把它当撤销完成用，
  直接拦会砍掉通道。备选 = 保留转换但按「撤销完成」语义读（完成数掉、
  账本留、skipped 不计），并另开「撤销完成」入口。建议先按 learnhub
  #286 拦置位（语义正确），真机若发现缺撤销入口，再补独立动作。
- **读数互斥**：`skipped_lessons` / 分母剔 skipped / 卡片分子全部用
  同一口径「未结算的 skipped」；已完成的不计入 skipped（存量自愈，
  不改旧行）。`settled_or_completed` 要么改名为实际口径，要么剔
  skipped 后再加 completed——二选一，以卡片分子=进度条分子为准。
- **quiz 触发计数**与 XP gauge 同口径剔 skipped（现在不剔）。
- **建议补拆读**：「已学完 N/M · 已跳过 K 个」——零跳过零版面
  （learnhub #265 的形制）；用户可见文案须 zh-CN/en-US 双语 +
  `bun run gen:i18n`/`check:i18n`。若本票只修正确性，拆读可另票。
- UI：已完成课时的跳过钮隐藏或禁用并给理由；撤销跳过仍可见。

## Acceptance

- 单测：
  - 已完成课时置 skipped → 拒（人面错误），`completed_at` 不变；
  - `skipped → not_started` 照常；
  - `skipped_lessons` 只计「未结算的 skipped」；存量「已完成 + skipped」
    行在读数里不计 skipped；
  - 课程卡分子/分母同口径：同一数据下文字 N/M 与进度条一致；
  - XP gauge 与 quiz 触发计数对 skipped 同口径。
- 前端：`bun run typecheck`；若动文案，`bun run check:i18n`；相关
  render test 更新。
- `cargo test -p nomifun-learning` 全绿；无 DB 迁移（读侧自愈，旧行
  不改）。

## Out of scope

- 学习账本/XP 事件的删除或回滚（ADR-0017：账本独立、事件不删）。
- 节点级「自报已会」独立状态（learnhub 的 node learned；本仓无此物）。
- 硬删除/恢复语义。

## Resolution（2026-11-26 施工）

**结论**：已学完/已结算不可再置跳过；读侧 settled 优先（skipped 排除有
node-completed 的节点），卡片分子分母同口径；「已跳过 K」拆读欠账。

- `progress.rs`：置 `Skipped` 前查当前进度状态与 `learning_xp_events`
  的 `node-completed` 事件；已完成/已结算 → `Conflict("这个课时已经学完，
  不需要跳过")`，不覆盖 `completed_at`；撤销跳过不受限。
- 读侧口径：`skipped_lessons`（list/detail 两查询）与 XP gauge 的
  `skipped_scope` 都加 `NOT EXISTS node-completed`——已结算课时不算
  skipped；分母不剔它，分子保留结算，尺不倒挂。quiz 触发只认
  node-completed，本身不受 skipped 影响。
- UI：`CourseCard` / `CourseWorkspace` 的百分比与文本分子统一用
  `settled_or_completed`（后端在账本为空时回退 completed_lessons）；
  `LearningGraphWorkspace` 对 `status === 'completed'` 隐藏跳过钮，
  存量 skipped 仍出「撤销跳过」。
- 测试：新 `completed_lesson_cannot_be_skipped_and_legacy_overlap_reads_as_learned`；
  重写 `xp_reading_settled_nodes_follow_skip_denominator` 为新口径；全量
  1137/0；UI 学习页 529/0 + typecheck。
- 欠账：「已跳过 K 个」拆读未做（需新增双语 i18n；当前 skipped 被静默
  从分母剔除）。传统课**无 XP 账本**时，存量「完成后再跳过」没有结算
  证据，读侧仍按 skipped 计；图/完成链路有 node-completed，按已学完算。
