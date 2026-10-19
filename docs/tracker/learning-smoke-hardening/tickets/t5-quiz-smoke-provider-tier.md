---
id: t5
title: "quiz 冒烟的 provider 档——带凭据的全链真机变体验证"
type: task
status: open
assignee:
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

（结案时追加）
