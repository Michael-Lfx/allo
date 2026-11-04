---
id: y05
title: "建课计划态轻确认——填写→计划（四槽回显+确定性读数）→确认才生成"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# y05: 建课计划态轻确认

**What to build:** 建课表单提交后不立刻触发分钟级生成，先进入一个**计划态**：
回显四槽（来源/方式、目标或简报、教学风格、模型）并给出**确定性读数**
（KB 流用现有零 LLM 估算；图/描述流如实显示「按声明/简报酌定」，不编区间）。
用户点「开始生成」才发请求；返回修改保留原值；计划态期间零生成调用、
零课程行、零任务登记。**纯前端 v1，不改后端队列语义。**

## 现状（2026-11-04 实码核实）

- `ui/src/renderer/pages/learning/hooks/useCourseCreation.ts:105-130`
  `generateCourse` 立即 `learningApi.generateCourse(request)`；成功后置
  completed。
- KB 流 `submitGeneration`（:158-221）：校验后直接 `generateCourse`；唯一
  确认是 t5 成本门（估算 `needs_confirm` 时弹
  `CourseCostGateModal`，`estimateCourseGeneration` 规则分诊、零 LLM）。
- 描述流（:221）：直接 `generateCourse`。
- 学习图 `confirmGraphCreation`（:131-145）：`description + proficiency +
  basis + style + model` 直接 `generateCourse`。
- 后端 `generate_course` 收到即占槽、登记 generation task、跑管线
  （`service/generate.rs:101-135`）；学习图路径直接插课程行并后台启动
  （`service/learning_graph.rs:107-145`）。不存在「确认前零分解调用」。
- 估算端点 `POST /api/learning/courses/generate/estimate` 已存在：
  `service/generate.rs:551-591`；无 KB 时返回全零
  `CourseGenerationEstimateView::default()`（:557-560）。

## learnhub 参考（main@22d016d，5746078 #193）

- `src/pages/courses/ui/new-course-dialog.tsx:22,86-89,158-201`：状态机
  `filling → planning → generating`；planning 只读回显四槽 + 推定读数卡
  （入门档由后端从零声明判定；规模用前端常数表并明标估计）+「返回修改」
  「开始生成」。
- `src-tauri/src/app/commands.rs:173` + `engine/decomposition/facade.rs:48-58`：
  `course_plan_estimate` 是只读纯函数命令；`runCourse.mutate` 只在确认时调用。
- 关键不变量：计划态零分解调用、零课程行；确认后才入现有队列。

## 实施要点

### A. 状态机（`useCourseCreation.ts` / `CreateCourseDialog.tsx`）

- [ ] 新增 `creationPhase: 'form' | 'plan' | 'generating'` 与
  `planRequest: GenerateCourseRequest | null`；现有 `generation` 状态保留给
  generating/completed/failed/cancelled。
- [ ] 三个入口统一：KB / 描述 / 学习图在校验通过后**只构造 request 并进入
  plan 态**，不调用 `generateCourse`。`generateCourse` 只由计划态的
  「开始生成」调用（一次）。
- [ ] plan 视图（对话框内就地切换，不新开 Modal）：
  - 四槽回显：方式/来源（KB 列表名称或「描述」/「学习图」）、目标或简报、
    教学风格、模型；空值显示「未指定」。
  - 读数卡：
    - KB：进入 plan 时调 `estimateGeneration(request)`（零 LLM 规则分诊），
      显示 `total_files / estimated_batches / estimated_llm_calls /
      estimated_seconds` 与跳过摘要；`needs_confirm` 时在卡内醒目提示
      （超阈值），确认按钮即成本门——**不再二次弹 `CourseCostGateModal`**。
    - 描述/学习图：显示「按简报酌定」/「按声明酌定」+ 已填的程度/基础声明
      是否在场；**不显示虚构的节数/批数/时长**。
  - 按钮：「返回修改」（回 form，原值保留，零请求）、「开始生成」（唯一
    触发点）、关闭（零请求）。
- [ ] 估算失败/超时：显示「估算不可得，仍可继续」，确认按钮可用（fail-open，
  与现有成本门口径一致）；真正生成错误仍走现有 `generation.error` 面。
- [ ] 成本门合并：`submitGeneration` 的估算+弹窗路径改为进入 plan 时预取；
  确认时直接 `generateCourse(request)`。若保留旧路径（例如快捷键/旧 UI），
  必须保证同一请求只确认一次。

### B. 文案与可达性

- [ ] i18n 新增计划态标题、四槽标签、读数标签、`按声明酌定`/`按简报酌定`、
  `估算不可得`、`返回修改`、`开始生成`；zh-CN/en-US 对称，
  `bun run gen:i18n` + `bun run check:i18n`。
- [ ] 计划态是对话框内状态，不改变关闭/悬浮指示条语义；生成开始后仍走
  现有进度视图与「查看」入口。

### C. 测试

- [ ] Hook/组件测试（新增 `CreateCourseDialog.plan.test.tsx` 或
  `useCourseCreation.plan.test.ts`）：
  - 三入口提交后均进入 plan，`learningApi.generateCourse` **零调用**。
  - 「开始生成」恰调用一次，request 与表单逐字段一致。
  - 「返回修改」/关闭 → 零调用，表单值保留；再次进入 plan 不重复预取
    相同 request 的估算（可缓存 key）。
  - KB `needs_confirm=true` → 计划卡提示在场，确认直接生成（不再弹成本门）。
  - KB 估算抛错 → 仍可确认；描述/图流不发估算请求、不显示数字区间。
- [ ] `bun run typecheck` 全绿；现有 `autoRefill.test.ts` / `model.test.ts`
  不回归。

## 验收

- 单测覆盖上述零调用/恰一次/返回保留/估算失败/成本门合并。
- 手动走查三入口：计划态无网络生成请求（DevTools 确认），确认后课程正常
  创建；学习图确认前课程列表无新行、无 generation task。
- `bun run check:i18n`、`bun run check:theme`（新视图双主题）通过。

## 非目标

- 不提前跑 LLM 结构发现/大纲草稿给用户看（违背「确认前零分解」）。
- 不为图/描述流新增后端估算端点或虚构规模区间。
- 不改生成队列、202/异步、取消、任务注册表语义。
- 不做「入门档」后端判定（allo 无 learnhub 的 goal_type/零声明口径；
  图流只如实显示声明是否在场）。

## 风险

- 三个 tab 语义不同，plan 视图必须分支渲染；i18n 文案与状态机测试是主要
  工作量。
- 成本门从独立 Modal 并入计划卡，需确认没有其他入口依赖旧弹窗。
- 估算有文件 I/O（KB 规则分诊），计划态需 loading/失败降级，避免卡住确认。

## Resolution（2026-11-04）

**速览**：纯前端 y05 全项落地，无后端/API/队列改动。

- **状态机（`ui/src/renderer/pages/learning/coursePlan.ts`，新增）**：
  `buildPlanRequest` 三入口共用校验 + 请求构造（校验失败只回 i18n 键）；
  `reducePlan` 纯转移 `submit / estimate-loaded / estimate-failed / confirm /
  back`，effects 是唯一网络出口——submit 仅 KB 流预取一次估算（描述/图零
  估算、零生成），confirm 消费计划并恰发一次 generate（重复确认零 effect），
  back 零请求且保留估算缓存（`planEstimateKey` = 来源库顺序 + 目标）。
  `planEstimateView / planSlotLines / planReadout` 产出计划卡读数：KB 四个
  确定性数字 + 分诊/跳过摘要；图/描述恒为空数字行，只给「按声明/简报酌定」
  与声明在场情况。
- **Hook（`hooks/useCourseCreation.ts`）**：新增 `creationPhase: form | plan |
  generating`、`plan`、`planEstimate`、`confirmPlan`、`backToPlanForm`；
  `submitGeneration` / `confirmGraphCreation` 只走 `submitPlan → enterPlan`，
  `generateCourse` 仅由 plan 的 generate effect 与失败重试调用；估算失败
  静默缓存为 failed（fail-open，确认按钮始终可用）；`costGate` 状态删除。
- **UI（`components/CreateCourseDialog.tsx` + 新增 `components/CoursePlanCard.tsx`）**：
  计划态在对话框内就地切换（不新开 Modal），四槽回显（空值「未指定」）、
  读数卡、`needs_confirm` 卡内 `Alert` 醒目提示、`返回修改 / 开始生成` 与
  单「关闭」footer；关闭是隐藏语义（零请求），重开回到计划。旧
  `CourseCostGateModal.tsx` 删除（无其他入口引用），成本门并入计划卡。
  `index.tsx` 接线更新。
- **i18n**：zh-CN / en-US 对称新增 18 键（计划标题/四槽/读数/酌定/估算
  不可得/超阈值/返回/开始），`gen:i18n` 已重新生成 `i18n-keys.d.ts`。
- **测试**：
  - `cd ui && bun test src/renderer/pages/learning/` → **85 pass / 0 fail**
    （12 文件）；新增 `coursePlan.test.ts`（构建/零生成/恰一次/返回缓存/
    估算失败/needs_confirm/图描述零数字/hook 接线纪律）与
    `components/CoursePlanCard.render.test.tsx`（双语四槽与读数、成本门
    内联、fail-open、无虚构区间）。
  - `bun run gen:i18n && bun run check:i18n` → 11559 keys up to date。
  - `bun run check:theme` → 通过（新视图只用语义 token）。
  - `bun run typecheck` → 本仓分支尖端既有 55 条无关错误（videoCanvas 等），
    学习模块 0 条；与集成分支 296c39f 基线计数一致，无新增。
- **偏差**：
  - `multiSourceDisclosure.test.ts` 的 `kbSelectionTooMany` 结构断言改指向
    `coursePlan.ts`（上限校验随请求构造迁移，错误仍可见、不截断）。
  - `model.ts::estimateLines` 增加可选 `duration: 'minutes' | 'seconds'`
    （默认分钟，旧测试不回归），计划卡按 `estimated_seconds` 原值展示。
  - 票面手动 DevTools 走查（零生成请求 / 学习图确认前无课程行）未在本环境
    执行；自动测试以 state-machine effect 计数覆盖同一不变量，真机走查留待
    集成后验收。
