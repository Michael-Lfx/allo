---
id: t7
title: "中文档位/est 字符串解析归一 + 教练面英文词表 + 批容量与就绪线双 5——grow 收官"
type: task
status: closed
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

**速览**：四项裁决全落地（commit `db4832b49`），learning 342 全绿。

- **归一面**：`growth_contract::normalize_proposal`（`validate_proposal` 前的
  原始 JSON walk）——中文档位（知道/会用/能教 + 英文大小写折叠）→
  know/apply/teach；est 字符串数字 → 整数；重写计数 tracing warn。词表/
  范围门照旧（est "0" 仍打回，未知档位词仍红）——归一不造值。红绿样
  `parse_coach_reply_normalizes_chinese_tiers_and_string_est`（g4 时代的
  「legacy 拒绝」测试按 t7 裁决反转为「归一接受」）。
- **教练面英文**：花名册（`render_roster`）、已教账（`render_taught_summary`）、
  概念足迹（`render_coverage_gauge`）、`ng.assumes.uncovered` 门反馈四处改
  `tier.as_str()`；正文管线（lesson.rs 前置段）保持中文。根因面消除：材
  料不再教模型中文档位（后续真机 `mechanically normalized` 触发 0 次）。
- **双 5**：`MAX_BATCH_NODES` 7→5、`READY_TARGET` 7→5；数量契约段改
  `{READY_TARGET}`/`{MAX_BATCH_NODES}` 占位符由新增 `coach_system()` 装配
  （常量单源，提示词不再手写数字）；【当前就绪】行同步插值。
- **CONTEXT.md**：新增词条 Mechanical Digestion（机械消化）与 Tier
  Vocabulary Boundary（档位词表边界）。
- **真机**：修复后首跑（污染库）EXIT=0——4 批 16 节点全绿；后续四跑
  round 1-2 全部一次过门（t7 前做不到）。残余死因从档位类完全转移到语
  义类 → t9 开票消化后，**干净库连续两跑 grow --batches 4 EXIT=0**
  （19 节点 / 16 节点）+ loop 档回执（见 t9）——验收达成。
