---
id: m2
title: "070 增课程失联孤儿子树清理段——到期计数归真 + 题库幽灵行清零"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# m2: 070 课程失联孤儿子树清理段

**What to build:** 迁移 070 增加一段「课程行已失联 → 自底向上清空孤儿子树」
的一次性清理（11 张旧形状表），并配基线迁移测试 fixture；随票订正 070 头注
措辞与 `delete_course` 的 question_tags 顺序 bug。

## 法证证据（2026-11-09 审计，两路子代理独立锚定 + 实码复核）

- **口径变更**：due 队列从 main 的 `LEFT JOIN learning_courses`
  （merge-base review.rs:46）改为
  `JOIN ... AND c.deleted_at IS NULL AND c.enabled = 1`（review.rs:36-48，
  三轮 g3「现存且启用」单源口径）。口径本身是对的（新删除语义），错的是
  没有同步处理历史孤儿。
- **两个人群**：
  1. main 时代 `delete_course(delete_reviews=false)`（merge-base
     course.rs:525-627）只硬删课程行+先修边+课程标签，enrollments/modules/
     lessons/activities/review_items 全保留且可复习（UI 默认勾保留，
     merge-base CourseCard.tsx:103）——升级后这些卡永不出队；
  2. 升级前已删的旧图课程：course 行已不在，070 的 DELETE 全部以
     `course_kind = 'learning_graph'` 键定（070:40-108）**键不上它们**。
- **计数失真**：`checkin_today.due_count`（checkin.rs:100-114）、日历到期
  桶（checkin.rs:308-322）、记忆预测（memory.rs:93-96，前端
  MemoryHealthPanel.tsx:81）都不 join 课程，照数幻影卡；问题管理页还列出
  它们（review.rs:256-271 LEFT JOIN）。到期数只涨不减 = 传统课程兼容回归。
- **启动修复救不了**：`repair_logical_reference_orphans` 只作用于
  `RequireParent` 引用（id_schema_contract.rs:1452-1454）；
  `learning_enrollments.course_id`（:907）与 `learning_modules.course_id`
  （:898）是 `historical_text_ref`，宏文档明写 "repair never deletes these
  rows"（:595-601）——「删课保留内容」是被契约明文容忍的历史态。唯一收口
  = 迁移内一次性清理。
- **孤儿课时 404**（course.rs:497-513 inner-join）同一人群，随本票清理自然
  消失。

## 用户裁决（2026-11-09）

- 统一清孤儿子树（kind 无关，按课程行失联键定）：图残余与传统删课卡一起
  清；行为变化（main 删过的传统课程卡不再可复习）已接受，与新删除语义
  「删课 = 卡清」一致。
- 070 未发布，原地修合法（先例 217030a7c 净形折叠）；本分支已跑过 070 的
  dev 库谱系本就要 reset:dev（070 头注既有约定），原地修无额外代价。

## 实施要点

- [ ] **清理段**（置于 §1 图课程删除之后、§10 rebuild 之前；自底向上、每条
      自含子查询；`NOT IN (SELECT course_id FROM learning_courses)`）：

      ```sql
      -- 1) 学史：失联卡的推进史联带清除（与 §1 对图课程同语义）
      DELETE FROM learning_review_log WHERE source = 'course' AND item_id IN (
          SELECT r.review_item_id FROM learning_review_items r
          JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id
          WHERE e.course_id NOT IN (SELECT course_id FROM learning_courses));
      -- 2) 课程题标签
      DELETE FROM learning_question_tags WHERE source = 'course' AND question_id IN (
          SELECT a.activity_id FROM learning_activities a
          JOIN learning_lessons l ON l.lesson_id = a.lesson_id
          JOIN learning_modules m ON m.module_id = l.module_id
          WHERE m.course_id NOT IN (SELECT course_id FROM learning_courses));
      -- 3) 复习卡 / 4) 作答（按失联注册）
      DELETE FROM learning_review_items WHERE enrollment_id IN (
          SELECT enrollment_id FROM learning_enrollments
          WHERE course_id NOT IN (SELECT course_id FROM learning_courses));
      DELETE FROM learning_attempts WHERE enrollment_id IN (
          SELECT enrollment_id FROM learning_enrollments
          WHERE course_id NOT IN (SELECT course_id FROM learning_courses));
      -- 5) 活动 / 6) 节 / 7) 进度（失联课时）
      DELETE FROM learning_activities WHERE lesson_id IN (
          SELECT l.lesson_id FROM learning_lessons l
          JOIN learning_modules m ON m.module_id = l.module_id
          WHERE m.course_id NOT IN (SELECT course_id FROM learning_courses));
      DELETE FROM learning_lesson_sections WHERE lesson_id IN (
          SELECT l.lesson_id FROM learning_lessons l
          JOIN learning_modules m ON m.module_id = l.module_id
          WHERE m.course_id NOT IN (SELECT course_id FROM learning_courses));
      DELETE FROM learning_lesson_progress WHERE
          enrollment_id IN (SELECT enrollment_id FROM learning_enrollments
              WHERE course_id NOT IN (SELECT course_id FROM learning_courses))
          OR lesson_id IN (
          SELECT l.lesson_id FROM learning_lessons l
          JOIN learning_modules m ON m.module_id = l.module_id
          WHERE m.course_id NOT IN (SELECT course_id FROM learning_courses));
      -- 8) 课时 / 9) 注册 / 10) 模块 / 11) 课程标签
      DELETE FROM learning_lessons WHERE module_id IN (
          SELECT module_id FROM learning_modules
          WHERE course_id NOT IN (SELECT course_id FROM learning_courses));
      DELETE FROM learning_enrollments WHERE course_id NOT IN (
          SELECT course_id FROM learning_courses);
      DELETE FROM learning_modules WHERE course_id NOT IN (
          SELECT course_id FROM learning_courses);
      DELETE FROM learning_course_tags WHERE course_id NOT IN (
          SELECT course_id FROM learning_courses);
      ```

      执行顺序即编号顺序（子查询依赖父链存活）。§1 已直接删掉的图子树对
      本段是 no-op——本段兜的是 §1 键不上的预删人群与任何残余。
- [ ] **清理集红线**：只碰上列 11 张旧形状表。070 新生表（xp_events /
      mounts / findings / repairs / maintenance / quiz_rounds / basis /
      system_actions / capacity_feedback）对被清课程零 legacy 行，不写任何
      语句；`learning_review_events` 与 `learning_checkins` 按 070 既有契约
      **保留**（打卡统计不随卡删）。
- [ ] **基线迁移测试补 fixture**
      （learning_domain_baseline_migration.rs）：seed_v170 增加「课程行失联
      子树」两形态——①传统课：插 modules/lessons/activities/review_items/
      review_log/progress/enrollments 但**不插** courses 行（模拟 main 删课
      保留复习）；②图课残余：同形态 + 断言与 course_kind 无关地被清。断言：
      失联子树归零、活课程传统行守恒断言不动、learning_review_events/
      checkins 行数不变、孤儿课时不存在后 lesson 链干净。
- [ ] **070 头注订正**：「已发布迁移 001–068 一字不动」→「已发布迁移
      001–065」（066–068 随本次合并首发，非已发布）；同时在头注节号清单里
      登记新清理段。
- [ ] **delete_course question_tags 顺序 bug**（course.rs:692-700）：标签
      删除在 activities 删除之后跑，子查询恒空 = 永不生效（main 同序但当时
      只在删复习分支跑）。把 question_tags 删除挪到 activities 删除之前
      （或按 course_id 直删 `source='course'` + 活动前缀匹配），补一条红绿
      单测（删课后 `learning_question_tags` 无该课活动残留）。

## 验收

- `cargo test -p nomifun-db` 全绿（含新 fixture）；`cargo test -p
  nomifun-learning` 全绿（question_tags 红绿样）。
- 升级演练判据（真机）：001–065 老库（含一门前删课保留复习的课程）→ 应用
  070 → 打卡页 due_count 不含幻影卡、日历/记忆预测归真、问题管理无幽灵行、
  活课程复习队列正常出卡。
- 已发布用户库前缀兼容不受影响（001–065 → 001–068+070 全新链）。

## 非目标

- 契约声明不动：`historical_text_ref`（enrollments/modules）保留——它变成
  不可达的读侧容忍声明，无成本，删声明反而扩 diff。
- due 队列 join 口径不动（「现存且启用」单源口径是对的）。
- 新删除语义不动（墓碑 + 物理删 activities/lessons，未来不再产生失联子树）。
- XP/checkin 历史语义（后置清单，产品裁决另议）。

## Resolution（2026-11-09 施工收口）

**代码面验收全过，已闭票；真机升级演练判据留待下次真机冒烟批次（代码面已
备齐，FAIL 按票重开）。** 施工 = `16815a9b8`（070 §8 清理段 + 头注订正）+
`fda5259e1`（基线 fixture）+ `e391fa019`（delete_course question_tags 顺序）+
评审补测 `e6519df63`（by_title 红绿样）。

- §8 十一条与票面 SQL 逐字一致（评审轴核对），执行顺序即编号顺序；§1–§14
  既有语句一字未动（全文件仅头注措辞行一删，001–068 → 001–065 订正 + 节号
  清单登记 §8）。
- fixture 断言读数：两形态失联子树（ORPHAN_COURSE_TRAD 传统删课保留残影 /
  ORPHAN_COURSE_GRAPH 图课残余）11 表计数全零；活课程守恒面（4 课时/1 活动/
  1 注册/4 进度/1+1 标签）不动；`learning_review_events`×3（含两张被清卡的
  作答流水）与 `learning_checkins`×1 保留面行数不变；悬空课时/模块/注册 = 0。
  `cargo test -p nomifun-db` 52 测试二进制全绿。
- 票面外收口（如实记档，评审轴判「合理同缺陷收口」）：`delete_courses_by_title`
  与 delete_course 逐字复制的同款死语句一并换序修；红绿样
  `course_delete_by_title_sweeps_question_tags_of_purged_activities`（临时
  revert 验证真实红：幽灵标签残留 1 行）。
- 真机判据（下次冒烟批次）：001–065 老库（含一门删课保留复习的课程）→ 070 →
  due_count 不含幻影卡、日历/记忆预测归真、问题管理无幽灵行、活课程出卡正常。
