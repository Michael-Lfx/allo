---
id: d1
title: 账本保留不变式测试钉死
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

「已发生的学习行为记录独立于课程存在」（GLOSSARY 学习账本词条）目前由
「删除路径不存在」保证——全仓无 `DELETE FROM learning_xp_events` /
`DELETE FROM learning_checkins`。缺一道测试护栏：未来重构若误加删除语句
无人报警。如何钉死？

## 已核验事实（2026-11-17 对码）

- `delete_course`（course.rs:753-814）触面：复习卡打死卡标、quiz 回合废
  弃、内容八表 purge（:910-943，无账本表）、课程行墓碑。
- `delete_courses_by_title`（:844+，教程种子替换路径）wipe 面 =
  enrollments/progress/attempts/review cards + 内容表——**也不含账本
  表**（该路径是种子设施非用户路径，wipe 面更宽是既有裁决）。
- 孤儿修复会清 purge 后的 progress 孤儿行（内容态，非账本）。

## Terms（施工定案）

- **R-a（主修）**：nomifun-learning 测试新增不变式用例——造一门课 +
  用户 xp_events（两类 kind 各一笔）+ checkin 一笔 → `delete_course` →
  断言 `learning_xp_events` 行数不变、`learning_checkins` 行数不变、
  复习卡行数不变且 archived_at 已打标。挂进既有 delete_course 测试组
  （service/tests.rs :2242/:2367/:2795 邻域）。
- **R-b（同族钉）**：`delete_courses_by_title` 路径补一条账本不变式断言
  （该路径 wipe 面更宽但账本表同样零触碰——现有 :2454 测试邻域）。
- **R-c（不做）**：不为打卡/账本建「删除 API 禁令」类静态检查脚本
  （grep 护栏已足够 + 测试已钉）；不动 GLOSSARY（词条已落）。

## Acceptance

- 两条不变式测试绿（用户路径 + 种子路径）；`cargo test -p
  nomifun-learning` 全绿。

## Resolution

已施工收口（2026-11-17，feat/learn1116：`99ed8cbc2`）。

- R-a 用户路径不变式：`course_delete_preserves_the_learning_ledger` —
  造课 + xp_events 两类 kind 各一笔 + 真实服务面打卡 + 复习卡两张 →
  delete_course → 账本两表行数不变（kind × course_id 逐类核对）、复习
  卡行数不变且 archived_at 全部打标（死卡语义顺带钉住）；删除以墓碑 +
  内容清空自证，杜绝空转绿。
- R-b 种子路径不变式：`delete_courses_by_title_preserves_the_learning_
  ledger` — 宽面（enrollments/progress/attempts/复习卡/内容表/课程行）
  清零自证 + 账本两表凭 course_id 归账行原样保留，两面对照。
- 落库裁量留痕：复习卡 2 张各挂一活动（UNIQUE(enrollment_id, activity_id)
  约束）；进度行直插不走服务路径（避免结算搅浑账本种子）。
- 审查批顺带：F6 两用例种子 helper 去重（`290190742`）；F5 预存 flaky
  测试补 serial（同文件测试卫生家族）。
