---
id: sq4
title: section_key 绑定退役：拆「按节覆盖」诱因，题组取向改学习效果中心
type: task
status: closed
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

## Resolution（2026-11-19 施工落地，裁决点按票面倾向）

- **R-a 契约退役（两线）**：绑定条款（agent 线【活动契约】「每题必须带
  section_key 绑定到教它的那一节；至多 1 道跨节综合题用 general」+
  `ls_patch_activities` 工具描述「每题带 section_key 绑定来源节（跨节综
  合题用 general）」）与旧线每节配额句（`generation/mod.rs` TAIL 的
  questions-per-content-section 句 + Every-question-binds 句、
  `build_activities_prompt` 头部配额句与「section_key values for
  binding」manifest 引语）全部删除；传统线 HEAD 形状示例不再携带
  section_key 字段（模型不再写该字段，`ActivityPack.section_key` 保留为
  存量兼容位）。替换条款：传统线 `QUESTION_GROUP_RULE`（EN，TAIL 原位逐
  字嵌入，never-fewer-than-3 下限并入该句）、agent 线
  `QUESTION_GROUP_ORIENTATION_CN`（CN，【活动契约】原位注入）——题组围
  绕本课时 teaches 的核心判别/推导轴组织、不按节均摊、不给每节配证明覆
  盖题；数量面挂靠既有「规划即雄心」（agent 线由节清单容量定、本条不另
  立配额；传统线照课时规模与档位）。R20 认一认/推一推两考向条款未动。
  与 sq1/sq2 新加条款并存不覆盖：改后通读【活动契约】块——活动数 → 答
  案形状 → evidence → 题面自足（sq1）→ scope 边界（sq2）→ 题组取向
  （sq4）→ estimated_minutes，语义连续。
- **R-b 落库统一 NULL（按票面倾向）**：`persist_lesson_output`（传统线与
  学习图线共用的落库尾）新生成题 section_key 恒写 NULL——引擎自报值（节
  key 或 general）一律不落列；列保留零迁移；旧「general→NULL 归一」失去
  输入面随之删除。手动加题 INSERT（create_lesson_activity）本就不带该列
  = NULL（对码确认）。旧清单综合步回退形态（model.ts `hasGeneral` 判
  `section_key===null`）兼容验证：新题全 NULL → hasGeneral 恒真，但新规
  范恒带练习节（outline 硬契约）→ 综合步臂不触发、零行为变化；仅旧清单
  存量课时走回退，语义正确。
- **R-c 前端零行为变化（死分支留代码路径+注释，按票面倾向）**：
  stepActivities 行为不动（收尾轮全收编已是事实，`section_key===null` 臂
  恒真=语义显式化）；多练习节分支（`index !== lastPracticeIndex`）留代
  码路径+注释（旧契约形态，存量数据兼容）；model.ts / LessonStudy.tsx
  注释如实化（绑定退役说明）。前端测试零改动应绿（见检查记录）。
- **R-d 对冲留痕**：绑定曾是「题面材料来自哪节」的对账锚；退役后该对账
  由 sq1 题面自足条款（作答材料题面原样重述）+ sq2 scope 闸（考点轴边界）
  接管——题面自携材料后节归属失去对账价值，无残余风险面。

### 检查记录

- `cargo fmt -p nomifun-learning -p nomifun-ai-agent -- --check`：clean。
- `cargo test -p nomifun-learning`：1012 passed / 0 failed。
- `cargo test -p nomifun-ai-agent`：1178 passed + 集成面全绿 / 0 failed。
- `bun install` + `bun run typecheck`：绿。
- 前端聚焦测试零改动：model.test.ts 33 pass；LessonStudy 相关 16 pass。
- 无新增 i18n 字符串（只删句+注释）→ 未跑 gen:i18n；`bun run check:i18n`
  通过（无意外）。
- 全量 ui 测试：3896 pass / 4 fail——4 个失败全部为与本票无关的存量环境
  面（ContentAside localStorage 未定义类，对码确认与 learning 面零交集；
  本票只改 learning 两文件的注释）。learning 相关文件全绿。
- 真机 smoke 抽查（题组不再按节均摊、题量与画像相称）：归波级收尾统一
  执行（map 施工顺序尾段），本票不单独跑真机。

### 落地哈希（补录）

- 施工提交：`c752afd853a1ec544901161cb4895fd4f5701092`
  （feat(learning): retire section_key binding and adopt question-group
  orientation (sq4)，作者/提交者 = zhangyj）。
- `git merge feat/learn1116`：Already up to date（wave/sq4 自
  feat/learn1116 尖端 488757c4b 分出、施工提交其上——零冲突平凡满足）。
