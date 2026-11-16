---
id: d5
title: "建课流程简化——学习图只留目标输入，删除「生成计划」确认页（提交即生成）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# d5: 建课流程简化（原「下一批」项，用户 2026-11-10 点名开工）

**What to build:** 用户裁决（2026-11-09 首批截图 + 2026-11-10 点名）：

1. 学习图建课表单只留**学习目标**输入——程度声明/基础声明两个可选输入框和
   讲解风格单选全部移除（后端 DTO 三者皆 `#[serde(default)]`：程度/基础缺省
   由结构生成扩写、按纯零基础画地形；风格缺省 standard——前端不传即缺省）。
2. 删除「生成计划」确认页（四槽回显 + 确定性读数对用户无信息量）：提交按钮
   校验通过后**直接发起生成**，对话框就地进生成进度视图（原样保留）。

## 法证证据（2026-11-09/10 实码核实）

- 表单：`CreateCourseDialog.tsx`——graph tab 三输入（:261-289）+ 共享讲解风格
  Radio（:292-304）+ 模型选择器（保留）；plan 分支渲染 `CoursePlanCard`
  （:152-164）。
- 计划态机器：`coursePlan.ts` 375 行（buildPlanRequest 之外全是计划态面）+
  `useCourseCreation.ts` 的 planState/dispatchPlan/estimates/confirmPlan/
  backToPlanForm（:74-77, 137-237）+ `CoursePlanCard.tsx`（120 行，含
  render 测试）。
- 估算链：plan 态唯一网络面是 KB 流的 `estimateGeneration`（api.ts:63）——计
  划页删除后成死代码；连带 `model.ts` 的 `triageCountsLine`/`estimateLines`
  （唯一用户是 planReadout）与 i18n `costGate*`/`genTriageLine`/
  `planEstimateSeconds`。`triageViewCounts`/`uncertainTooMany` 有
  CourseSourceCoverageCard 用户，**保留**。
- 后端契约：`GenerateCourseRequest`（models.rs:27+）teaching_style /
  proficiency_statement / learner_basis 全部 serde default；estimate 端点
  （POST /courses/generate/estimate）保留不删（后端零改动）。
- i18n 删除清单（生产面仅剩将死引用，全部双侧对称删）：
  planTitle/planHint/planReadoutTitle/planSlotSource/planUnspecified/
  planNeedsConfirm/planEstimating/planEstimateUnavailable/planEstimateByBrief/
  planEstimateByDeclaration/planDeclarationPresent/planDeclarationMissing/
  planBack/planStart/planSlotTarget/planSlotStyle/planSlotModel/teachingStyle/
  styleStandard/styleSocratic/styleFeynman/learningGraphProficiencyLabel/
  learningGraphProficiencyPlaceholder/learningGraphBasisLabel/
  learningGraphBasisPlaceholder/costGateFiles/costGateBatches/costGateCalls/
  costGateDuration/genTriageLine/planEstimateSeconds。
  **保留**：listSeparator、genUncertainTooMany、genTriageExcludedTitle、
  learningGraphCreateSubmit（学习图提交按钮）、createSubmit、learningGraph*
  其余（含 learningGraphCreateHint 模式说明）。
- 纪律测试：`multiSourceDisclosure.test.ts` 的断言面（kbSelectionTooMany 在
  plan/dialog 源、onSelectedBaseChange(next)、listSeparator）在新形状下全部
  仍成立（buildPlanRequest 保留、Select 超限拒绝保留）。

## 实施要点

- [ ] CreateCourseDialog：删 plan 分支/CoursePlanCard/plan·planEstimate·
      onPlanBack·onPlanConfirm props；删讲解风格块与 graph 两声明输入及对应
      props（graphProficiency/graphBasis/onGraph*Change/teachingStyle/
      onTeachingStyleChange）；footer 仅 generating 隐藏主按钮。
- [ ] useCourseCreation：删计划态机器（planState/dispatchPlan/enterPlan/
      confirmPlan/backToPlanForm/plan/planEstimate）；graphProficiency/
      graphBasis/teachingStyle state 删除；submitPlan → build + 直接
      `generateCourse(request)`（校验失败仍 Message.warning 停留表单）。
- [ ] coursePlan.ts 瘦身：保留 CreationTab/CreationPhase（收窄为
      'form' | 'generating'）/CourseCreationForm（去声明与风格字段）/
      buildPlanRequest（graph 分支不再带 declarations/teaching_style）；
      其余（reducePlan/estimates/slotLines/readout/teachingStyleLabel）删除。
- [ ] model.ts 删 triageCountsLine/estimateLines（triageViewCounts/
      uncertainTooMany 保留）；api.ts 删 estimateGeneration；types.ts 删
      CourseGenerationEstimateView（依赖类型 triage/excluded 有他用户，保留）。
- [ ] 测试：coursePlan.test.ts 重写（只留 buildPlanRequest：三入口 + 上限 +
      模型缺失 + graph 不再带 declarations/teaching_style 断言）；删
      CoursePlanCard.render.test.tsx；model.test.ts 删对应 helper 段；
      multiSourceDisclosure.test.ts 预期不变。
- [ ] i18n 双侧对称删（上方清单），`gen:i18n` 后 `check:i18n` 过。

## 验收

- `bun run typecheck` 绿（55 错基线一致）；`check:i18n` 绿；学习域前端单测全绿
  （重写后的 coursePlan.test.ts + 存量）；`check:dead-css` 学习域零命中。
- grep 验证：`CoursePlanCard|planReadout|reducePlan|estimateGeneration|
  teachingStyle` 在 learning 页面域归零（types.ts 契约镜像字段除外）。
- 后端零改动（`git diff --stat crates/` 为空）。
- 真机目验（用户侧）：学习图 tab = 目标输入 + 模型选择 + 提交按钮；点提交直
  接进生成进度；知识库/描述两 tab 表单照旧但同样直达生成。

## 非目标

- 后端 DTO/端点改动（estimate 端点保留）；两个非图 tab 的输入面重构（知识库
  选择/领域/描述即目标，本就 minimal）；`teaching_style` 的课程级存档读侧
  （存量课程不受影响）。

## Resolution（2026-11-10 施工收口）

**代码面全验收过；真机目验待用户。** 施工 = `46125d745`。

- **学习图表单 = 目标输入 + 模型选择 + 提交按钮**：程度/基础声明输入与讲解风
  格 Radio 全部移除；请求不再携带（后端 DTO 全 serde default：结构生成扩写 /
  纯零基础 / standard）。模式说明段（learningGraphCreateHint）保留。
- **计划页整页移除**：CoursePlanCard + render 测试删除；对话框提交按钮（学习
  图 = 创建课程并生成知识结构；其余 = 创建课程）校验通过**直接发起生成**，就
  地进生成进度视图（进度/取消/重试/完成进课程原样保留）；footer 仅生成视图
  隐藏主按钮。`CreationPhase` 收窄为 'form' | 'generating'。
- **死面清理**：coursePlan.ts 375→103 行（保留 buildPlanRequest/类型）；hook
  删计划机器与声明/风格 state；api.ts 删 estimateGeneration；types.ts 删
  CourseGenerationEstimateView（triage/excluded 类型有 SourceCoverageCard 用
  户，保留）；model.ts 删 estimateLines（triageViewCounts/uncertainTooMany/
  triageCountsLine 保留——后两者有 SourceCoverageCard/进度视图活用户）。
- **i18n 双侧对称删 30 keys**（plan*/teachingStyle/style*/声明 label/
  Placeholder/costGate*/planEstimateSeconds；11588 keys 收口）。
- **施工记档**：①首轮误删 `triageCountsLine` + `genTriageLine`——它有活用户
  （CourseGenerationProgress 的 triageCompleted 事件流），typecheck 当场暴露，
  即刻回滚并在票面更正删除清单（估算死面的 grep 口径要含 import 面不止 t()
  调用面）；②测试用例笔误（空简报用例没切 description tab），自测暴露即修。
- **验收**：typecheck 55 错基线一致（learning/i18n 零错）；`check:i18n` /
  `check:theme` / `check:css-scope` / `check:motion` / `check:icons` 绿；
  `check:dead-css` 学习域零命中；学习域前端单测 **115 pass / 0 fail**（重写
  coursePlan.test.ts 6 例：三入口形状 + 图流不带声明/风格断言 + 校验键 + 上
  限）；grep `CoursePlanCard|planReadout|reducePlan|estimateGeneration|
  teachingStyle` 在 learning 页面域归零；`git diff --stat crates/` 为空（后端
  零改动，estimate 端点保留）。
- **真机判据**（用户侧）：学习图 tab 只剩目标输入；点提交直接进生成进度；知
  识库/描述两 tab 照旧直达生成。
