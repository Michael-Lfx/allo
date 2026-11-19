---
id: sq5
title: 存量题救济：题删除路由 + 前端入口 + 级联面裁决（attempts 账本保留）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

本课实锤三道坏题（ordering 裸指代+答案键可争议、回收超纲 single_choice、
matching 已废存量）没有任何下架路径——routes.rs:128-137 只有
create/generate/attempts，无 delete/update。生成面修好之前（sq1/sq2 落
地之前的时间窗）与修好之后的漏网题，都需要人手救济。

## 已核验事实（2026-11-19 对码）

- activities 无删除/编辑路由（routes.rs 只有 POST create/generate +
  attempts）；前端无删题入口。
- 关联面：learning_attempts（作答事实）、learning_review_items（出队
  卡，UNIQUE(enrollment_id, activity_id)）、learning_question_tags。
  复习线已有单卡 delete/archive（review.rs:628/650）——那是卡面操作，
  题本体仍无删除。
- 手动补题路径已存在：create_lesson_activity（手动）/generate_lesson_
  activity（AI 加题 + pf5 422 闸）——删除+补 = 救济闭环，无需新生成面。
- matching 存量已退隐（countsTowardSection=false 不入练习轮、
  review_eligible=false 不出队，pf5 消费面派生）——本课 matching 题
  无需处置。
- **存量画像（dev 库 2026-11-19 全量清点）**：1 课 5 课时 33 题——题
  面裸指代 1（本课 ordering）；evidence 全空 3 课时 15 题（evidence 是
  判底揭底缺源，非不可答，不强制删）；待删坏题实名单 = ordering（裸指
  代+答案键可争议）+ 回收超纲 single_choice（position 7）两道，删除后
  手动补题。规模小，人工救济可行；变式站不落库坏源遗传（sq1 留痕）。
- 删除语义先例：课程删除只 purge 内容、账本保留（墓碑化，ADR-0017
  族 + 删除语义波）——学习账本独立性原则。

## Terms（方案草案，施工前过裁决）

- **R-a 后端删除路由**：DELETE
  `/api/learning/lessons/{lesson_id}/activities/{activity_id}`（权属校
  验照既有 lesson 路由惯例）。级联裁决点：
  - review_items：**删**（卡随题走——题没了卡还出队会炸 join；或走既
    有 archive 墓碑，倾向硬删+复用单卡删除语义）；用户已删课程时该卡
    已被课程级清理覆盖，不双删。
  - question_tags：删（随题）。
  - **learning_attempts：保留**（作答事实与账本不随内容删除——学习账
    本独立性先例）；attempt 挂空题 id 的读数面处置（剔除或如实标记）
    =施工对码项。
- **R-b 前端入口**：练习轮/题目卡管理位「删除本题」（确认对话框照既
  有删除惯例，单题级不进危险区）；删除后练习轮/收编面/种卡面即时收敛
  （重新拉取）。i18n 双语 + theme + dead-css 闸。
- **R-c 补题闭环**：既有手动加题/AI 加题即补题路径——本票零新生成面。
- **R-d 「一键重建本课题库」不做**（裁决点留痕）：手动加题已足，少加
  面；整课重生成走既有课时级操作。
- 本课存量处置（用户自持，施工后）：ordering/回收题删除，按需手动补。

## Acceptance

- 删除路由单测：权属拒绝/级联（review_items+tags 删、attempts 留）/
  幂等语义。
- 前端删除交互测试：确认框/列表收敛/已答态处理。
- 删除后复习队列与练习轮收敛测试（无悬挂卡、无悬挂题）。
- typecheck / i18n / theme / dead-css 绿；真机目验欠账。
