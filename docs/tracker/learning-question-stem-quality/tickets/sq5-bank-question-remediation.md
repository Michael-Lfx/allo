---
id: sq5
title: 存量题救济：题删除路由 + 前端入口 + 级联面裁决（attempts 账本保留）
type: task
status: closed
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

## Resolution（2026-11-19 施工落地，wave/sq5）

**落地哈希**：`8f00ee2eb`（后端路由+服务+单测）、`12db59d4c`（前端入口
+i18n+交互测试）。TDD 红→绿：后端先写失败测试（E0599 红）再实现；前
端静态接缝 + jsdom 交互测试先行。

- **R-a 后端删除路由（按票面倾向落地）**：
  - 路由 = `DELETE /api/learning/lessons/{id}/activities/{activity_id}`
    （路径参数名照既有 lesson 路由族 `{id}` 惯例，线上 URL 与票面
    `{lesson_id}` 形状完全一致）；handler 在 routes.rs，与 create/generate
    同族。
  - 权属校验照惯例：enrollment join（create_lesson_activity /
    set_question_tags 同款）——不存在/不属于/未报名 404 三态合一，不泄
    露存在性；activity 与 lesson 错位前置校验（同一事务内先定位后动手，
    不依赖回滚语义）。
  - 级联（单事务，先子后父照 purge_course_content 次序）：
    - `learning_review_items` **硬删**（复用单卡删除语义，不走 archive
      墓碑——墓碑是课程级删除的账本保全形态，单题救济无此需要；已删课
      程的卡由课程级清理覆盖，本路由对已 purge 的题本就 404，不双删）。
      依据对码：due 队列出队 SELECT 是 INNER JOIN（review.rs:65），死卡
      不会真出队，但 due_today 徽标直数 review_items 行——卡留 = 徽标虚
      高 + 永不出队的死行，故删。
    - `learning_question_tags` 删（course 命名空间，随题；自拟题走
      custom 命名空间不受影响）。
    - `learning_attempts` **保留**（作答事实 = 学习账本，ADR-0017 族
      「账本独立于内容」先例）。
  - **attempt 挂空题 id 的读数面处置（施工对码项，票面点名）**：全量对
    码结论 = 读数面天然「剔除」，零改动——`fold_behavior_digest`
    （learning_graph.rs:3091/3123）、xp ratio（xp.rs:77/88）、quiz LRU/
    touch（quiz.rs:289/847）全部 activities 驱动或 inner-join/IN 过滤，
    悬挂 id 不进任何前瞻读数；账本行原样留档，如实语义成立。
  - 幂等语义：重复删除 404（delete_review_item / delete_course 同款
    NotFound 收口）。
  - 响应 = 删除后的最新 `LessonView`（create/generate 同族惯例），前端
    一次调用即收敛。
- **R-b 前端入口（按票面倾向落地）**：
  - 入口位 = `ActivityBlock` 管理位（行尾 mini danger text 按钮，
    QuestionManager 删除按钮同款形态），透传覆盖练习轮 / 全部答完回看 /
    旧清单内容节 / 综合步 / 旧课时无节面——题卡全在；`onDelete` 缺席 =
    旧调用面零变化（静态接缝测试钉住）。
  - 已答态入口照常在场（disabled 不吃删钮）——救济的典型场景就是答完
    才发现题坏；确认对话框照既有删除惯例（Modal.confirm + danger 主键，
    QuestionManager confirmDelete 同款），单题级不进危险区。
  - 收敛 = 返回的 LessonView 整体替换本地详情（rewrite 同款）+
    `onRefresh()` 同步大纲/徽标；练习轮游标 `min(cursor, len-1)` 天然
    收敛删题后的位次。
  - i18n：`activityDelete` 语义域 4 键（activityDelete /
    activityDeleteConfirmTitle / activityDeleteConfirmHint /
    activityDeleteDone），zh-CN/en-US 对称（词典集合断言 + 提示文案
    「作答记录保留」双边如实）。gen:i18n 已重跑（12561 键）。
- **R-c 补题闭环**：确认零新生成面——既有手动加题 / AI 加题即补题路径，
  删除+补 = 救济闭环成立。
- **R-d 「一键重建本课题库」**：不做（裁决点留痕），整课重生成走既有课
  时级操作。
- **测试面**：后端 `delete_lesson_activity_cascades_cards_and_tags_keeps_
  attempts`（级联三面 + 账本保留 + 权属拒绝 + 幂等 + 错位 404 不越界 +
  due 队列收敛只剩兄弟题）；前端 ActivityBlock 静态接缝三用例（缺席不
  渲染 / 双语在场 / 零中文回落）+ LessonBlock jsdom 交互两用例（确认框
  唤起且账本语义在场 → API 恰一次 → 列表收敛兄弟题保留 / en 零回落）+
  activityDeleteDictionary 词典四用例（quizDictionary 纪律）。
  施工注记：bun test 无 vite 别名，arco 落 CJS lib 构建，jsdom 交互
  测试须引 `react-19-adapter` 的 **lib 侧**补丁（es 侧补丁管不到 lib 的
  copyRender）+ focus-lock 所需 HTMLIframeElement 等 jsdom 全局。
- **验证阶梯（全绿）**：`cargo test -p nomifun-learning` 992 passed /
  0 failed；`bun run typecheck` exit 0；聚焦 UI 测试 63 + 5 passed；
  `bun run gen:i18n && bun run check:i18n` up to date；`bun run
  check:theme` 绿；`bun run check:dead-css` clean。真机目验欠账（照票面
  Acceptance 原样）。
