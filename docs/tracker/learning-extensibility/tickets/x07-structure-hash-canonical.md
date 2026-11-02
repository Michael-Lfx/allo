---
id: x07
title: "structure_hash 规范化投影 + hash 版本——重序列化不再让全部课程挂载 stale"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x07: structure_hash 规范化投影 + hash 版本

**What to build:** 结构哈希作用于「规范化的 structure 内层」而非列的原样字节，并带版本，
使「结构变了」与「序列化变了/算法换了」可区分。

## 现状（2026-11-01 实码核实）

- `nomifun-learning/src/learning_graph/mounting.rs:45-51` `fnv1a_64` 注释明示
  「对 `structure_json` 列的原样字节做锚」；u64 存有符号 INTEGER。
- 消费：`learning_structure_mounts.structure_hash`（082）、
  `learning_structure_repairs.old_hash/new_hash`（083）——两表均未发布。
  stale 判定 = 当前列字节 hash ≠ 存值。
- 任何重序列化——键序、空白、serde 升级、外层信封加字段（x09/x14）、`lexicon` 旁键
  （`lexicon.rs:80 carry_lexicon_side_key`）、`generated_at`——都让**全部课程** mounts_stale。
  x14 的 ext.* 必须排除在 hash 外，本票是其前置。

## 实施要点

- [ ] 规范化输入：解析信封 → 取内层 `structure`（排除 `lexicon`/`generated_at`/`version`/
      未来 `ext`）→ 键排序 + 紧凑序列化 → FNV-1a。单源
      `structure_hash(envelope) -> (hash_version, u64)`。
- [ ] 版本：mounts/repairs 加 `hash_version INTEGER NOT NULL DEFAULT 1`（推荐；
      map 裁决点 3）；v1=原样字节（dev 库历史），v2=规范化。读侧 `hash_version<当前` →
      按旧算法复算比对，相等则**静默升级**写回，不等才 stale。两表未发布 → 原地改。
- [ ] 契约文档 `docs/contracts/learning-structure-artifacts.md` 补「hash 作用域与版本」节。
- [ ] 测试：同结构不同键序/空白 → 相等；改一个 block 名 → 不等；信封加旁键 → 不 stale；
      v1→v2 静默升级。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db`；`learning-smoke mount` 档「重跑幂等
  （第二次 skipped）」与 `repair` 档「级联 stale」判据仍过。

## 非目标

- 扩展键命名空间本身（x14）。
