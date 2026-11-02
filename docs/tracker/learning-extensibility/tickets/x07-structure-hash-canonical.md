---
id: x07
title: "structure_hash 规范化投影 + hash 版本——重序列化不再让全部课程挂载 stale"
type: task
status: closed
assignee:
blocked_by: [x21]
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
- [ ] 版本：mounts/repairs 加 `hash_version INTEGER NOT NULL DEFAULT 1`（**已裁决**，2026-11-02
      Q10）；v1=原样字节（dev 库历史），v2=规范化。读侧 `hash_version<当前` →
      按旧算法复算比对，相等则**静默升级**写回，不等才 stale。两表未发布 → **x21 基线**内落列。
      与 ADR-0012 §4 兼容（锚仍 FNV-1a 64、级联语义不变，只收窄「结构变」的判定面）——
      契约文档记档，不出后继 ADR。
- [ ] 契约文档 `docs/contracts/learning-structure-artifacts.md` 补「hash 作用域与版本」节。
- [ ] 测试：同结构不同键序/空白 → 相等；改一个 block 名 → 不等；信封加旁键 → 不 stale；
      v1→v2 静默升级。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db`；`learning-smoke mount` 档「重跑幂等
  （第二次 skipped）」与 `repair` 档「级联 stale」判据仍过。

## 非目标

- 扩展键命名空间本身（x14）。

## Resolution

**已关（2026-11-02，本提交）**。

- **单源 helper**（`learning_graph/mounting.rs`）：`STRUCTURE_HASH_VERSION_V1=1` /
  `STRUCTURE_HASH_VERSION=2`、`canonical_structure_hash(envelope)`（只取内层
  `structure`，紧凑序列化后 FNV-1a）、`hash_structure_column(raw)`（列 → (hash,
  version)）、`mounts_stale(stored_hash, stored_version, raw)`（按存量版本选算法；
  未知更高版本 fresh + warn）。
- **写侧恒 v2**：mount upsert 与 repairs INSERT 都写 `hash_version`；repairs 行内
  old/new 两 hash 自洽。
- **读侧按存量版本、不回写**（图 A2 覆盖票面「v1 相等静默升级写回」：GET 不写库；
  v1 行保持 fresh 直到下一次 mount/repair 回合自然升级）。view 的 `stale` 单源在
  service，`build_structure_view` 收 `stale: bool`。
- **基线 §6**：mounts / repairs 各加 `hash_version INTEGER NOT NULL DEFAULT 1`。
- **测试**：键序/空白相等、改块名不等、`terrain/lexicon/generated_at/version/ext`
  不进 hash、v1/v2/未来版本三态比较；mount 回合测试改为「尾随空白不 stale、真改块名
  才 stale」；smoke mount 断言改 `hash_structure_column` + hash_version 检查。
- **契约文档**：`learning-structure-artifacts.md` §9.1 改写 hash 作用域/版本/水位口径，
  §10.3 repairs 行补 hash_version 与跨表比较约束。
- **验收**：`cargo test -p nomifun-db -p nomifun-learning` 全绿；`learning-smoke
  mount/repair` 需 provider 的真机档本会话未跑（临时库无 provider），留波次真机批。
- 偏差：票面「静默升级写回」作废（图 A2），已记图纸 §8。
