---
id: x14
title: "structure_json 扩展键命名空间 ext.*——门忽略、lint 警告、hash 不计"
type: task
status: open
assignee:
blocked_by: [x07]
part_of: ../map.md
---

# x14: structure_json 扩展键命名空间

**What to build:** 给结构产物一个正式的扩展口：信封层 `ext.*` 下的键契约门默认忽略、lint 只
警告、hash 不计入；`lexicon` 旁键迁入成为第一个 ext 键。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 内层 `structure` 是裸 `serde_json::Value`，`structure_contract.rs:57` `TOP_KEYS`（14 个）
  白名单，生成门 `gd.keys` 拒未知顶层键 = **事实 deny_unknown_fields**；
  `REQUIRED_KEYS/SCALAR_KEYS/BLOCK_KEYS/SPINE_KEYS/TERRAIN_*/DOOR_KEYS`（:57-104, 833-849）
  各层同样封闭。
- 唯一非正式扩展口：`lexicon` 旁键（`lexicon.rs:80 carry_lexicon_side_key` 重生成时保留）。
- 外层信封 `{version, generated_at, structure, terrain}`（`service/learning_graph.rs:946`）。
- x07 把 hash 作用域收到规范化 `structure` 内层并排除旁键——本票依赖它。

## 实施要点

- [ ] 契约：信封顶层允许 `ext: {}`；门与 lint 对 `ext` 内容一律不校验；`structure_view`
      投影默认不读 ext（读 ext 的功能各自显式取）。内层 `structure` 保持封闭（模型产物
      必须干净），**扩展只在信封层**。
- [ ] `lexicon` 迁入 `ext.lexicon`：写侧改路径，读侧双读一版（旧旁键→ext），regenerate 的
      carry 逻辑改 carry 整个 `ext`。
- [ ] hash：确认 x07 的规范化输入排除 `ext`（测试：改 ext 不 stale）。
- [ ] 契约文档 `learning-structure-artifacts.md` 加「扩展键纪律：先落 ext，稳定后升正式键并
      bump version」。

## 验收

- `cargo test -p nomifun-learning`；`learning-smoke structure/mount/repair` 档 EXIT=0；
  用例：带任意 `ext.foo` 的结构过门、不 stale、regenerate 后 ext 保留。

## 非目标

- 内层 structure 词表（x08）。
