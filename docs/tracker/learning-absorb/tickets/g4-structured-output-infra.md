---
id: g4
title: "结构化输出基建：按 ADR-0014 建多少？"
type: grilling
status: open
assignee:
blocked_by: [g2]
part_of: map.md
---

## Question

learnhub 的结构化输出基建（ADR-0014）：provider `json_object` 保证合法 JSON + 本地 JSON
Schema 单源（schema 与 validator 由互钉测试锁定）+ 语义 validator 分离永不退役 + 整批重试
带反馈（MAX_ATTEMPTS=3，违规报告注入重试装配）+ wire-only 落盘。它取代的旧路是
「YAML 提示词纪律 → parse_repair 修复链 → validator」。

allo 现状：统一 `parse_json_object`（剥代码围栏后 serde）+ 大量容错 deserializer
（minutes 字符串、单对象数组、tier 中英文、终点裸标题等）+ 修复循环 ≤4 稿。

需要裁决：

1. **新结构管线**（structure + 地形生成）是否直接按 ADR-0014 模式建（schema 单源 + 语义
   validator + 整批重试带反馈）？——基本是必选项，争议小。
2. **既有教练批是否同期迁移**：容错 deserializer 与 ≤4 稿循环保留原样，还是统一到新基建？
   learnhub 的经验是「无反馈盲目重试零收敛」，allo 的 redraft 已带门报告（形态接近），
   差异在 schema 单源与违规报告的条款化编号。
3. **nomifun-ai-agent 桥**：`LearningCompleter` trait 是否需要新增返回结构化错误细节的
   通道（供违规报告注入重试），还是解析与重试都留在 nomifun-learning 侧（现状）。
4. **提示词资产化**：allo 提示词全内联在 `mod.rs` 常量里；learnhub 是 md 文件 + 头注 +
   占位符 + 围栏词表 + 金样。结构生成的新提示词是否走资产化（含金样快照测试），存量提示词
   是否顺手迁移——还是只对新管线资产化，避免扩大改动面。

约束：g2 的契约定案是本票前提；裁决输出一张「新基建覆盖范围」清单。
