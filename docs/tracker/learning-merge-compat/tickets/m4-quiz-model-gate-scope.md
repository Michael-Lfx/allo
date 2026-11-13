---
id: m4
title: "课内答题模型门收窄到 AI 判分题型——客观题不再被误拦"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# m4: 答题提交模型门收窄

**What to build:** `useCourseLearning.ts` 的 `submitAttempt` 不再对所有题型
要求已选模型——只有 AI 判分题型（`reflection` / `open_question`）保留模型
门，规则判分的客观题（single_choice / true_false / fill_in_blank /
multi_choice / numeric / ordering / matching）未选模型也能正常作答。

## 法证证据（2026-11-09 审计，实码复核）

- **误拦点**：`ui/src/renderer/pages/learning/hooks/useCourseLearning.ts:139-142`
  ——`if (!modelChoice) { Message.warning(t('learning.form.modelPending')); return; }`
  对**全部题型**生效。本分支新增（merge-base diff 确认，main 无此门）。
- **误伤面**：客观题后端纯规则判分零 AI 调用；`modelChoice` 为 null 的场景
  不罕见——模型目录加载中、目录加载失败且无保存选择、用户根本没配任何
  chat 模型（LearningModelSelector.tsx:51-54）。
- **后端本就接受缺省模型**：models.rs:2747-2756 有回退，门是前端单方面加的。
- **不一致**：同一道题在复习会话里作答无此门（useReviewSession.ts:31-48）。
- **判分谓词单源**：`ActivityKind::is_ai_graded`（models.rs:1521-1524）=
  `Reflection | OpenQuestion`——前端门按此谓词镜像即可，两分支已有
  `progress.rs:173/453` 用同一谓词分流的先例。

## 影响面核查结论（已实码核实，无需再查）

- 全仓 `modelChoice` 守卫共 3 处，仅本处错：
  - `LessonQuestionDialog.tsx:137`（生成题目 generateLessonActivity）——真
    需要 AI，门正确，不动；
  - `LessonStudy.tsx:752`（节重写 rewriteLessonSection）——真需要 AI，门
    正确，不动；
  - 本票唯一修改点 useCourseLearning.ts:139。
- 前端 `Activity.kind` 是 `ActivityKind` 字符串联合（types.ts:301），与后端
  `as_str` 值一致，谓词可直接写成
  `kind === 'reflection' || kind === 'open_question'`。
- 对 AI 题保留的门也是 UX 选择而非正确性需求（后端有回退）——保留的理由
  是让用户知道用哪个模型批改，与复习会话的无门不一致暂不强求对齐（见非
  目标）。

## 实施要点

- [ ] `submitAttempt` 的门收窄为 AI 判分题型才检查 `modelChoice`；未选模型
      且 AI 题时保留现有 toast（`learning.form.modelPending` 双语已在）。
- [ ] 谓词写法与后端单源对齐（两个字符串字面量 + 注释指回
      `ActivityKind::is_ai_graded`，防两端漂移）。
- [ ] 单测/组件测试：无模型配置时客观题（至少 single_choice 与 numeric）
      可完整作答出分；reflection 未选模型被拦且有提示；reflection 选了模型
      正常提交。

## 验收

- `bun run typecheck` + 相关测试全绿。
- 手验：清空模型选择 → 打开含客观题的课时答题全流程可走通；主观题提示
  选模型。

## 非目标

- 复习会话补模型门（现状无门、后端有回退，是否要对齐属产品语义，另议）。
- `LessonQuestionDialog` / `LessonStudy` 两处正确的门不动。
- 模型选择器/目录加载行为不动。

## Resolution（2026-11-09 施工收口）

**代码面验收全过，已闭票；清空模型手验流程留待下次真机批次。** 施工 =
`d1d9b6b42`。

- 门收窄为 `!modelChoice && (kind === 'reflection' || kind === 'open_question')`，
  注释指回后端 `ActivityKind::is_ai_graded` 防两端漂移（漂移风险的契约级收口
  在 map 后置清单）。
- 红绿样：新增 `useCourseLearning.test.ts` 5 用例（single_choice/numeric 无
  模型可提交出分；reflection/open_question 被拦且提示；带模型正常提交带参）。
  红样实录 + 反向验证（临时删门 → 两个拦截用例变红 → 恢复）。mock 只在本文件
  用例生效防 bun test 单进程串扰，同跑 LessonStudy/ReviewSession/
  CapacityFeedbackCard 13 用例无串扰。
- `bun test src/renderer/pages/learning` 16 文件 110 pass；typecheck 存量债
  持平零新增。
