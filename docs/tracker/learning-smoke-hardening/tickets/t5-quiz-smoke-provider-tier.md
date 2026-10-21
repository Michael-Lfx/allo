---
id: t5
title: "quiz 冒烟的 provider 档——带凭据的全链真机变体验证"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

`learning-smoke quiz` 的六判据按**零 provider 环境**写死：变式诚实跳过、
bank-only 计划（每节点恰一题）、`plan_json` 不得含 `"origin":"variant"`。
指向带凭据的库（`--db` 开发库）时变式真实生成 → 断言必失败。结果是：

- 零 provider 路径已绿（q2/q3 证据、本会话复测）；
- **带 provider 的 quiz 全链真机冒烟不存在**——q3 的「变体生成的 LLM 调用
  落观测面档案」只在开发中人工看过，没有可重复的机械判据。

**修法**：quiz 档按 provider 可用性分支——
- 无 provider：现有六判据原样（诚实跳过路径）；
- 有 provider（`--db` 解出或显式 `--provider/--model`）：变体在场断言——
  每节点题数 > bank 题数、`plan_json` 含 `"origin":"variant"` 条目、变式
  判底字段随 grade 下发、重裁时变式不重铸。

**DoD 含文档**：`learning.zh.md` 冒烟节补 quiz 档（八档 → 九档记载，q3 遗
漏）时把双环境判据一并写清。

**验收**：临时库（无凭据）与开发库（有凭据）两条路径各自 EXIT=0。

## Resolution

**速览**：双环境判据落地（commit `931932de4`），验收两条路径均 EXIT=0。

- **分支设计**：在场与否以 plan 实况为准（`wire` 恒挂 LiveLearningCompleter，
  凭据在调用时解析，冒烟不预判）——`--provider/--model` 显式或 `--db` 指库
  （`require_variants`）时变式在场是硬判据：每节点 bank + 恰一变式（题数 >
  bank 题数）、`plan_json` 含 `"origin":"variant"`、变式判底随 grade 下发
  （payload.config_json 的 answer 与揭示一致）、同计划折叠复用不重铸（全
  计划答满、折叠零双付）。在场要求而一条未生成 = 硬失败；零凭据临时库 =
  原六判据（bank-only + 诚实跳过）原样。
- **判分底扩展（t2 联动）**：变式 payload 的 evidence 同步落 config_json。
- **实施中发现并修正两处冒烟自身缺陷**：① 全卷结算的 XP 断言写死 12——
  结算档位随答题量（full = 2 + 该节点题数），变式在场是 4 分/节点 ×4 =
  16，改为按分支计算；② 盘面复验的账本查询没按课程过滤——dev 库累积历史
  冒烟课程的行，全局口径只在从零临时库成立，改为 `AND course_id = ?`。
- **文档**：`learning.zh.md` 冒烟节补 quiz 档（八档 → 九档），双环境判据
  写清。
- **验收**：临时库 `learning-smoke quiz` EXIT=0（六判据 + earned 12）；
  dev 库（flash）`--provider/--model` EXIT=0（6 变式在场、判底揭示、
  4×4+0 结算、折叠零双付、首写为终）。
