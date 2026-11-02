---
id: x08
title: "structure_json 稳定码统一——中文字面量改落库码、EDGE_TIERS 与 ConceptTier 合一取 apply"
type: task
status: open
assignee:
blocked_by: [x21]
part_of: ../map.md
---

# x08: structure_json 稳定码统一

**What to build:** 趁 structure 可再生（`POST …/structure/regenerate`）、相关表全部未发布，把落进
`structure_json` 的中文取值换成落库码（CONTEXT.md「Stored Code」），并把两套 tier 合为词汇表
钉死的 know/**apply**/teach。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 中文取值落库（`nomifun-learning/src/learning_graph/structure_contract.rs`）：
  `STAGES` 入门/进阶/高级（:107）、`DOMAIN_TYPES` 理论/工程/历史/技能（:121）、
  `SOURCE_TIERS` 名录/章节（:123）。模型直接输出中文 token，契约门按字面量校验，前端
  `structure/Mindmap.tsx`、`StructureGraphTab.tsx` 读 `StructureView` 展示。改词/国际化 = 旧数据失配。
  这与 `CONTEXT.md:104/196` 为 Tier 立的「DB 存 locale 无关码、模型契约面只认英文码、中文只住
  展示面、解析面机械归一」直接冲突。
- 双 tier：`EDGE_TIERS` know/**use**/teach（`structure_contract.rs:119`，`learning_graph/*.rs` 共 8 处）
  落 `structure_json.edges`；`ConceptTier` know/**apply**/teach（`learning_graph/mod.rs:116`）落
  `learning_lesson_concepts.tier`；前端 `types.ts:801` tier union 含 `use`。**`use` 是对词汇表的
  偏离，不是待选项**（Q5 裁决）。
- `SectionPack.visual` 的中文取值（`models.rs:1208-1214`）落的是已发布表 051，归 x17。

## 实施要点

- [ ] 落库码：`stage: basic|advanced|expert`、`domain_type: theory|engineering|history|skill`、
      `source_tier: catalog|chapter`（snake_case 与其它枚举一致；实现可微调命名但须英文稳定码）；
      契约门校验码；展示由 lexicon/i18n 映射（`locales/*/learning.json` 双语对称）。
- [ ] 模型面：**模型仍输出中文、落库前 normalize 为码**（单源函数，机械消化律先例
      `CONTEXT.md:192`），契约门在 normalize 后校验。
- [ ] tier 合一取 `apply`：`EDGE_TIERS` 8 处 + 提示词 + `types.ts:801` 同步；`lesson_concepts.tier`
      本就是 apply，不动。070 的 CREATE 在 x21 基线内已为终态。
- [ ] 读侧兼容一版：dev 库里的中文值/`use` 读到即规范化（不落盘，regenerate 自然升级）；
      smoke `structure` 档种子盘面（smoke-hardening t1 的 6 主干 × 42 块）同步改码。
- [ ] 落地时 bump structure 信封 `version`（x19 格式演进口径的第一次真实使用）。
- [ ] 文档：`docs/contracts/learning-structure-artifacts.md` 取值节。

## 验收

- `cargo test -p nomifun-learning`；`learning-smoke structure/mount/repair` 三档 EXIT=0；
  `bun run check:i18n`；结构图四视图展示文案不变；`rg '"use"' learning_graph` 归零。

## 非目标

- `visual` 取值（x17）。
