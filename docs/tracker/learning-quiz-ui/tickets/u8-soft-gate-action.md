# u8: 软闸行动承诺兑现（跳转 + 历史可进 + 口径对齐）

## Status

已施工收口（2026-11-12，2ebeccc41 落 `feat/learn-quiz-ui-u8`）。

## 事实源

[分析文档](../assets/2026-11-12-course-kind-quiz-surface-analysis.md) 轴 3
（G5）+「软闸三处不符」附节；裁决记录 Q4。ADR-0009 :84-87 写「提示条
**直达复习**」、CONTEXT.md 软闸词条写「学习者仍可从历史记录进入任何
节点」——两处文档承诺 UI 均未兑现；「就绪 x/7」读数与生长水位口径不一。

## Terms

- **R1（软闸 Alert 加行动）**：`LearningGraphWorkspace.tsx` 软闸建议条
  （:738-749）加「去复习供给节点」行动——跳转到**供给节点课时内容**
  （LearningGraphWorkspace 选中该节点；轻，正文即补习材料；到期卡自然
  在复习队列里，不另起复习会话）。跳转 = 选中节点进学习视图（复用既有
  选中/推荐位机制）。
- **R2（学习记录行可点）**：学习记录页签的节点行可点进入该节点课时
  （兑现 CONTEXT.md 软闸词条「学习者仍可从历史记录进入任何节点」）——
  被软闸挡下的节点与已完成节点都可进。models.rs:2216-2217 注释随之相符。
- **R3（ready_count 口径对齐，票面裁决）**：「就绪 x/7」现为软闸后口径
  （learning_graph.rs:4006-4010），生长水位为软闸前口径（:3906-3912），
  读数偏低。**推荐对齐软闸前**（与水位同语义：结构就绪数；软闸挡下的
  单独提示已有建议条承载，读数不再重复扣减）；备选双读数。票面定。
- **R4（文档对齐）**：R1/R2 落地后 ADR-0009「直达复习」与 CONTEXT.md
  「历史记录可进」即为真，两文档零改动；若 R3 选双读数，ADR 补一句
  读数口径说明。

## Acceptance

- 渲染测试：软闸条带跳转行动 + 点击选中供给节点；学习记录行可点进入。
- typecheck 绿；learning 回归绿；i18n 双语（新增行动文案对称）。
- 真机：seed v3 软闸臂（u6 交付）验跳转链路 + 被挡节点经历史记录进入。

## Resolution

已施工收口（2026-11-12，2ebeccc41 经 merge 合入 learn0928；闸门：新渲染
测试 4 用例 + learning 全量 392 绿 · typecheck 绿 · i18n/theme/dead-css
绿）。

- **R1（软闸行动）**：建议条 Alert 加 `action` 钮（新键
  `learningGraphBlockedGoSupplier` 1 对，zh/en 对称）——点击走 `openNode`
  （选中任意节点 + 回学习视图，与主推荐卡「点击即进内容」同一条选中路径；
  `lessonsById` 覆盖全部节点，非就绪集节点照常渲染正文）。**偏离票面一处
  （守卫臂）**：`ready_partition` 的供给表跨课程取教师（learning_graph.rs
  :4058-4063），`supplier_lesson_id` 可能不在本课——此时不出行动钮，保持
  纯文字建议，不做落到推荐首位的假跳转（渲染测试有守卫用例）。
- **R2（历史行可点）**：学习记录批次节点行 div → button（`onOpenNode`
  透传 `openNode`），被挡与已完成节点都可进——CONTEXT.md 软闸词条与
  models.rs:2217 注释自此为真；ADR-0009「直达复习」随 R1 兑现。三处文档
  零改动（R4 达成）。
- **R3（ready_count 口径，核实结论）**：后端 `LearningGraphView` **只有
  软闸后一个计数**——`ready_count = ready_partition 的就绪桶`（
  learning_graph.rs:4006-4010）；软闸前水位口径（`ready_count_for_owner`
  :3906-3912）是生长触发判定的内部读数，**不进 DTO**；且 `blocked` 截断
  至 ≤3（:4215），前端 `ready_count + blocked.length` 也无法复原软闸前数。
  按票面约束**后端零改动、前端展示保持现状**。若要按推荐对齐软闸前口径
  （与水位同语义），需开小票：DTO 增一字段（如 `ready_total`，后端
  `ready_set_for(...).len()` 即得）+ 前端读数换字段——请裁决是否开票。
- 跳转/可点机制一句话：软闸行动钮与历史行都汇入同一个 `openNode(lessonId)`
  （`setSelectedLessonId` + `setActiveTab('study')`），正文渲染与主推荐卡
  共用，无新增分支组件。
