---
id: g4
title: "结构化输出基建：按 ADR-0014 建多少？"
type: grilling
status: closed
assignee: zhangyj
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

## Resolution

**速览**：四问裁决（三采推荐，一反推荐）——

1. **教练批同期全迁**（⚠ 反推荐，用户明确选择）：教练批管线同迁新基建——schema 单源 +
   条款化 validator（ng.* 编号）、容错 deserializer 退役（minutes 字符串/单对象数组/tier
   中英文等全部移除）、重试反馈对齐新基建（≤3 轮带条款化违规报告，替换 redraft ≤4 稿）；
   AI 概念评审门保留（语义兜底，与契约门分离）。**刚落库的教练批需全量重验**
   （`service/tests.rs` 既有用例为底）。实施影响：本 effort 改动面扩大，教练批输出契约
   与结构管线统一。
2. **桥层新增 json 接缝**（推荐）：`LearningCompleter` trait 增 json 模式方法
   （response_format=json_object，thinking 开关同现状两通道对应）；schema 单源、validator、
   重试装配全留 nomifun-learning 新模块（learnhub `json_contract.rs` 对应物，schema↔
   validator 互钉测试）；桥只多通道不厚化。
3. **provider 不支持即报错**（推荐）：结构生成声明硬要求 json_object；provider 不支持时
   建课即失败并明示原因（fail loud），模型选择面可过滤支持 provider；不做静默降级。
4. **提示词资产化：仅新管线**（推荐）：structure/地形等新提示词走 md 文件资产
   （include_str! + 头注五字段 + 具名占位符互锁 + 围栏词表机械拦截 + 金样快照测试），
   温度/thinking 住注册表不进文本；**存量教练批提示词保持内联常量**（契约迁移 ≠ 提示词
   资产化，两者解耦）。

**新基建覆盖范围清单**：structure 生成、前置地形生成、教练批（全迁）、概念评审输出——
全部生成面 JSON 统一走 schema 单源 + 条款 validator + 带反馈重试。连带确认：
`analyze_scope`/`SCOPE_SYSTEM` 随 g1/g2/g3 退役（其两个消费方——范围参考与终点提议——
均已退场）。

