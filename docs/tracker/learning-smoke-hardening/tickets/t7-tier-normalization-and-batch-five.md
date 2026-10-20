---
id: t7
title: "中文档位/est 字符串解析归一 + 教练面英文词表 + 批容量与就绪线双 5——grow 收官"
type: task
status: open
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

grow 4 批 EXIT=0 的最后一段死链（结案波两跑 round 4 确定性复现）：教练稿
assumes/teaches 写中文档位（「会用/知道」）→ serde 枚举解析（或
`ng.refs.tier` 契约红条）整批打回 → 重裁时 flash 重写整批而非点修 →
`ng.redraft.scope` 守卫拦截，三轮耗尽。

**根因（2026-10-20 实锤）**：我们自己的教练材料在教模型中文档位——已教账
渲染 `canonical@会用`（`render_taught_summary`）、花名册同款
（`render_roster` / 概念足迹 / `ng.assumes.uncovered` 门反馈），模型照抄进
tier 字段，而输出契约只认 know/apply/teach。模型行为合理，材料自相矛盾。

**裁决（2026-10-20 grill 轮，用户定案）**：
1. **双管齐下**：解析面机械归一（中文档位→know/apply/teach、est 字符串
   数字→整数，归一发生留 warn）**且**教练面材料改英文词表——花名册、已教
   账、概念足迹、档位门反馈四处用 `apply` 等英文；正文生成管线
   （lesson.rs 前置段，不回吐 tier JSON）保持中文行文；`tier_zh` 保留给
   纯展示面。
2. **est 字符串一并归一**（"15"→15；范围门 1–60 照旧——"0" 这类语义违规
   仍打回，不造值）。
3. **批容量与就绪线双 5**：`MAX_BATCH_NODES` 7→5（learnhub 1–5 档口径）
   且 `READY_TARGET` 7→5（用户明裁——推荐队列深度是产品行为，用户认
   可整体测试兜底）。
4. 归一位置在**原始 JSON 面**（`validate_proposal` 之前 walk 重写 tier/
   est）——契约词表保持纯英文单源，typed serde 不放松。

**验收**：连续两跑 `learning-smoke grow --batches 4` EXIT=0（flash 概率
性，单跑不足以收官——历史教训）+ 补跑一次 `loop` 档（九档矩阵补齐）。

## Resolution

（结案时追加）
