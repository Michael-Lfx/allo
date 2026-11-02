---
id: x08
title: "structure_json 词表统一——中文字面量改稳定码、EDGE_TIERS 与 ConceptTier 合一"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x08: structure_json 词表统一

**What to build:** 趁 structure 可再生（`POST …/structure/regenerate`）、相关表全部未发布，
把落进 `structure_json` 的中文词表换成稳定码，并把两套 tier 词表合为一套。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 中文词表落库（`nomifun-learning/src/learning_graph/structure_contract.rs`）：
  `STAGES` 入门/进阶/高级（:107）、`DOMAIN_TYPES` 理论/工程/历史/技能（:121）、
  `SOURCE_TIERS` 名录/章节（:123）。模型直接输出这些中文 token，契约门按字面量校验，
  前端 `structure/Mindmap.tsx`、`StructureGraphTab.tsx` 读 `StructureView` 展示。
  改词/国际化 = 旧数据失配。
- 双 tier：`EDGE_TIERS` know/**use**/teach（`structure_contract.rs:119`）落
  `structure_json.edges`；`ConceptTier` know/**apply**/teach（`learning_graph/mod.rs:116`）落
  `learning_lesson_concepts.tier`（070 CHECK）。**两处都未发布**，词可自由选。
- `SectionPack.visual` 的中文词表（`models.rs:1208-1214`）落的是已发布表 051，归 x17。

## 实施要点

- [ ] 稳定码：`stage: basic|advanced|expert`、`domain_type: theory|engineering|history|skill`、
      `source_tier: catalog|chapter`（snake_case 与其它枚举一致，开工定名）；契约门校验码；
      展示由 lexicon/i18n 映射（`locales/*/learning.json` 双语对称）。
- [ ] 模型面：推荐**模型仍输出中文、落库前 normalize 为码**（单源函数），对模型更自然；
      契约门在 normalize 后校验。
- [ ] tier 合一：选一个词（推荐 `apply`）；`EDGE_TIERS` 与提示词同步；070 原地改。
- [ ] 读侧兼容一版：dev 库里的中文值/旧 tier 读到即规范化（不落盘，regenerate 自然升级）；
      smoke `structure` 档种子盘面（smoke-hardening t1 的 6 主干 × 42 块）同步改。
- [ ] 文档：`docs/contracts/learning-structure-artifacts.md` 词表节。

## 验收

- `cargo test -p nomifun-learning`；`learning-smoke structure/mount/repair` 三档 EXIT=0；
  `bun run check:i18n`；结构图四视图展示文案不变。

## 非目标

- `visual` 词表（x17）。
