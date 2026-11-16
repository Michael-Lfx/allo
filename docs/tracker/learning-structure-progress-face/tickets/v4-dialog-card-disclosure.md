---
id: v4
title: "（可裁）对话框完成卡片结构态披露——「知识结构生成中…」一行接棒"
type: task
status: closed
assignee:
blocked_by: [v1]
part_of: ../map.md
---

# v4: 对话框完成卡片结构态披露（可裁项，裁决见 map 裁决 2）

**What to build:** 图课程流 `completed` 卡片（`CourseGenerationProgress`
completed 分支）的「N 个模块 · 0 个课时」下方加一行结构态披露，数据源与
v3 同一快照轮询：

- structure 在跑：`知识结构生成中 · 已 mm:ss`（secondary 文案行，无按钮无
  交互——交互面在 Pill/工作区）。
- 落库后：**本票不刷新卡片**（卡片是 HTTP 响应快照，避免为它再加轮询面；
  落库真话由 Pill 退场与工作区徽章接棒）。若施工时轮询源天然在手、一行
  判零成本换成「结构已就绪」，允许顺手做，不强制。

裁撤说明：本票是锦上添花——v2/v3 落地后，用户从对话框卡片拿到的误导
（「0 个课时 + 开始学习」却不知结构未就绪）已由 Pill 真话兜住；裁掉不影响
任何其它票。施工顺序最后，可随时 drop。

## 法证证据（2026-11-11 渲染探针）

- 探针 S2（真组件 + 真状态）：completed 卡片全文 =
  「通盘认识股票市场基础 1 个模块 · 0 个课时 开始学习」——结构在跑 12 分钟
  期间卡片零披露，用户点「开始学习」进入的课当时还没有结构。
- 卡片数据是 HTTP 响应快照（`generation.result`，建课瞬间的 CourseDetail）：
  `total_lessons: 0`、无结构读数——卡片自身没有结构态信息源，披露必须
  外接轮询（即 v3 的源）。

## 实施要点

- [ ] completed 分支卡片加结构态行（courseKind === 'learning_graph' 才
      渲染；v2 流卡片零变化）。
- [ ] 轮询源与 v3 共用同一状态（不新增第二份轮询）；无 structure 行时
      该行不渲染（如快照已终态）。
- [ ] 新 key 双语 + 断言（真实词典 JSON）。
- [ ] 渲染测试：在跑行出现 / 终态行不渲染 / v2 流卡片零变化。

## 验收

- 渲染测试绿；typecheck 零新增；i18n 双闸绿。
- 真机（与 map 欠账合并）：建图课 → 对话框卡片出现「知识结构生成中 ·
  已 …」行；v2 描述流卡片无该行。

## 非目标

- 卡片落库后的自刷新（本票明确不做，接棒面在 Pill/工作区）。
- 卡片布局/其它内容改动。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；真机目验与欠账合并执行，FAIL 按票重开。**

- 施工 = `3807ce671`：`structureCardLine(watch, t)` 纯函数（在跑 → 文案；
  queued/终态/idle/missing → null）+ props 链（index → CreateCourseDialog →
  CourseGenerationProgress，仿 courseKind/restored 形态）；卡片「N 个模块 ·
  0 个课时」下渲染 secondary 结构态行，仅 graphFlow 渲染（防误传守卫）。
  13 例测试。
- **单一轮询源约束达成**：useStructurePillWatch 激活面由「仅 Pill 可见」
  放宽为「图课程 completed」——对话框打开与 Pill 可见互斥且必居其一，
  watch 两期连续服务，未新增第二份轮询（组件内 hook 方案被否：静态渲染
  不跑 effect 不可测，且字面违背约束；留痕）。
- 票面裁定执行：卡片落库不自刷新（终态行退场，接棒在 Pill/工作区）；
  queued 态与 attempt 轮次不入卡片文案（真话在 Pill），测试钉住。
- 评审修复 = `73983597c`（卡片两键入占位符全对称断言）。
