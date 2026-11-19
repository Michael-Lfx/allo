---
id: sq4
title: section_key 绑定退役：拆「按节覆盖」诱因，题组取向改学习效果中心
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

用户裁决：「每题必须带 section_key 绑定到教它的那一节」有什么用，没有
用就删掉；「按节覆盖」出题模式根本不健康——一切都应该以学习者的学习
效果为中心，不是给每节配一道证明覆盖的题。

## 已核验事实（2026-11-19 诚实对码）

- **后端零消费**：种卡（progress.rs:685）、复习出队（review.rs:57）、
  判分（evaluate）、变式装配（quiz.rs）全不读 activities.section_key。
- **前端唯一消费**（model.ts:58-84 stepActivities）：outline 硬契约
  「恰好一个收尾练习节」下 earlierPracticeKeys 恒空，内容节绑定题 + 通
  用题全归收尾轮——**绑定对学习者可见面零效果**。多练习节分支
  （findLastPracticeIndex）是契约不再产生的形态，死路径。
- **唯一活效果是反向的**：绑定条款（lesson_content_loop.rs:304）+
  旧线每节配额句（generation/lesson.rs:1074「questions per content
  section (concept/example/demo)」）诱导模型按节均摊出题——本课 8 题
  按节均摊（s1×2/s2×2/s3×1/s4×2/s6×1）即实锤；演示节剧情 ordering
  题正是「每节配一题」的产物（map 取证）。
- 审计本无绑定 finding（对码确认）——退役不动机械面。

## Terms（方案草案，施工前过裁决）

- **R-a 契约退役（两线）**：删绑定条款与每节配额句（agent 线 :304、
  传统线 TAIL :190 与 build_activities_prompt 配额句）；替换为题组取
  向条款——题组围绕本课时 teaches 的核心判别/推导轴组织、数量与意图
  画像相称（挂靠既有「规划即雄心」）、认一认/推一推两考向照旧（R20
  不动）。
- **R-b 落库面**：列保留（存量兼容零迁移）；新题 section_key 写法=
  裁决点：统一 NULL（「归练习轮」语义显式化）vs 写收尾练习节 key。
  倾向 NULL；旧清单综合步回退形态（hasGeneral 判 section_key===null）
  兼容验证。
- **R-c 前端不动 + 注释如实化**：stepActivities 行为不变（收尾轮全收
  编已是事实）；model.ts/LessonStudy 注释补绑定退役说明；死分支（多
  练习节）留不留=施工裁决（倾向留代码路径+注释，少动面）。
- **R-d 风险对冲留痕**：绑定曾是「题面材料来自哪节」的对账锚；退役后
  该对账由 sq1 自足条款 + sq2 scope 闸接管——题面自携材料后节归属失
  去对账价值。

## Acceptance

- 两线契约一致性测试同步（删句断言红）。
- 新生成课 section_key 语义统一（按裁决写 NULL 或收尾节 key）+ 单测。
- 前端全回归：练习轮收编/门禁行为不变（现有测试零改动即绿为证）。
- 真机 smoke 抽查：题组不再按节均摊、题量与课时画像相称。
