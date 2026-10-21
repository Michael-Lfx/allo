---
id: t8
title: "铸名 source 出处落地（t2 条款 3）——attestation 口径 + registry source 列"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

t2 条款 3（概念铸名 source 出处）在 t2 结案时缓行：registry 加列是
append-only 迁移（AGENTS.md 高风险区需 owner 签核），且 learnhub 口径
「出处是簿记不是判定」不进门禁。2026-10-20 grill 轮用户当场签核落地，
语义裁定 **attestation**（出处说明），不做书目引用——allo 无教材数据源，
要模型写「人教版七年级上册」必是编造（违反 never-invent）；learnhub 有真
实教材数据才写书引用。

**修法**：
1. 迁移 082：`learning_concept_registry` 加可空 `source TEXT` 列
   （append-only；迁移测试同步）；
2. 铸名链路透传：`ConceptMint` 加 `source: Option<String>`、
   growth_contract 铸名键白名单 += `source`、`apply_batch` 落 registry 行
   带 source；
3. COACH_SYSTEM 提示词：mints 可带可选 `source`（attestation 形态
   「教练提名：…」——谁在哪个语境铸的名），明令不得编造书目；
4. 不进门禁：source 缺省合法（serde default），门与评审都不因缺 source
   打回。

**验收**：迁移测试 + 契约白名单测试 + 服务层往返样（带 source 的 mint 落
库读回）；真机不单独设档（随 t7 的 grow 两跑顺带读数）。

## Resolution

**速览**：全项落地（commit `db4832b49`），owner 现场签核迁移。

- **迁移 082**：`ALTER TABLE learning_concept_registry ADD COLUMN source TEXT`
  （可空、append-only）；nomifun-db 测试全绿，旧行读侧默认 NULL。
- **链路**：`ConceptMint.source: Option<String>`（serde default）→
  `MINT_KEYS` 白名单 += `source` → `apply_batch` 落 registry 行（trim、空串
  归 NULL）。门禁不因缺 source 打回（纯簿记）。
- **提示词**：COACH_SYSTEM 的 mints 形状示例加 `source` 字段（「教练提
  名：……」attestation 形态，明令不是书目引用、禁止编造教材出处）。
- **往返样**：`mint_source_attestation_lands_on_the_registry_row`（带
  source 落库读回一致；种子旧行 NULL 容缺）。learning 342 全绿。
