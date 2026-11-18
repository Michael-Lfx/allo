---
id: pf1
title: 判分逐项结果透出 + 全题型提交后逐项反馈（错项标红）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

提交后只有全局「需要再练习」+ 解释，哪一项错完全不可见（匹配题三行全
对的教学答案被判错时，用户无从自查）。要求：错项标红；反馈设计扩展到
全部题型——每种题型都要有提交后的逐项状态。

## 已核验事实（2026-11-18 对码）

- `evaluate`（progress.rs Matching 臂 :553-568）：逐位置精确比对，返回
  二元 (score 0/1, feedback)——**逐项正确性判分时已知但不透出**。
- attempt 存储（learning_attempts）：response_json/score/passed/feedback
  ——无逐项结果字段。
- 前端消费：ActivityInput 九种题型 + 提交面（课时练习/复习卡共用）——
  提交后只有全局文案。
- 题型清单（契约 :226）：single_choice / multi_choice / true_false /
  fill_in_blank / numeric / ordering / matching / reflection /
  open_question。

## Terms（方案草案，施工前过裁决）

- **R-a（后端）逐项结果透出**：evaluate 返回扩展（每题型产出
  per-item 结果数组——matching: 行级 bool[]；choice/multi/true_false:
  选项级 bool[]（与 options 对齐）；ordering: 位次 bool[]；fill/numeric:
  单元素）；attempt 存储与 DTO 跟随（字段形状按既有 DTO 风格择小，
  旧 attempt 无逐项字段 = 前端降级为全局文案，向后兼容）。
- **R-b（前端）全题型提交后逐项状态**：
  - matching：行级红/绿描边（错行红）；
  - single_choice / true_false：错选标红、正确项标绿（提交后定格）；
  - multi_choice：漏选/多选均红、全对绿；
  - ordering：错位次标红；
  - fill_in_blank / numeric：错值红、对值绿；
  - reflection / open_question：AI 批改语义，保持全局反馈不逐项（无
    客观逐项）。
  - 颜色走语义 token（danger/success 族），双主题过闸。
- **R-c 反馈文案**：全局「需要再练习」保留 + 解释保留；逐项标红是增量
  不替换。

## Acceptance

- 后端：evaluate 逐项结果单测（每题型一例）；attempt DTO 兼容旧记录。
- 前端渲染测试：每题型提交后逐项状态断言（红/绿/不适用三分支）；
  reflection/open 不回归。
- typecheck / i18n（若需新词条）/ theme / dead-css 绿；真机目验欠账。

## Resolution

（未结案）
