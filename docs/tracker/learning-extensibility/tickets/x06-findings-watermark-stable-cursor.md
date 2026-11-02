---
id: x06
title: "findings_watermark 换锚——自增 id 不是可移植身份，备份还原后水位漂移"
type: task
status: open
assignee:
blocked_by: [x21]
part_of: ../map.md
---

# x06: findings_watermark 换锚

**What to build:** `learning_maintenance_state.findings_watermark` 从
`learning_structure_findings.id`（自增整数）改锚到稳定游标，备份还原后水位不漂移。

## 现状（盘点 2026-11-01，行号实现会话复核）

- `087_learning_maintenance_state`（未发布）`:24` 存 findings 的整数 id；读取/推进
  `nomifun-learning/src/service/learning_graph.rs:1071-1073`；开关
  `NOMI_LEARNING_FINDINGS_WATERMARK`（`service/learning_graph.rs:17`，off=全量重读不推水位）。
- v3 契约：整数 `id` 不是可移植身份（`docs/architecture/id-system.md`）。
  `nomifun-db/src/backup_bundle.rs:959-982` 还原时清 `sqlite_sequence` 并剔除 `id` 列重插
  → **重编号** → 水位语义漂移；代码钳到 `MAX(id)` 兜底，代价 = 重跑全部修补。
- findings 表（`082_structure_mounts_findings`，未发布）有 `ts NOT NULL`；同毫秒多条需处理。

## 实施要点

- [ ] **方案 B（已裁决，2026-11-02 Q15）**：findings 加每课程单调
      `seq INTEGER NOT NULL`（写入 `MAX(seq)+1`，插入点单处 `service/learning_graph.rs:1046`，
      SQLite 单写者安全），水位锚 seq——业务列随 bundle 导入导出不变；方案 A `(ts, 同 ts 计数)`
      作废。
- [ ] 两张表均未发布 → 在 **x21 基线**内落列（082 findings 加 seq、087 水位列改锚）。列名遵守
      v3 契约命名禁忌（参照 070 `findings_watermark` 命名讨论：不用 `_id` 后缀 INTEGER）。
- [ ] 还原路径：`backup_bundle.rs` 钳位逻辑移除；补「导出→还原→水位不变、不重跑已消费
      findings」集成测试。

## 验收

- `cargo test -p nomifun-db -p nomifun-learning`；`learning-smoke repair` 档 EXIT=0。

## 非目标

- findings 台账加 kind/code 机器列（gd.*/ng.* 目前只在 message 文本里）——记入 x20。
