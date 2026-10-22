---
id: h2
title: "覆盖对账父子滚入：章级 topic 由被覆盖子 topic 滚入，消除「章已覆盖却报未覆盖」噪音"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# h2: 覆盖对账父子滚入

**What to build:** 覆盖对账(heading/anchor 级 topic)增加父子滚入规则:某文件的同章 H1 topic,若其名下任一 H2/H3/段落子 topic 已被课时锚点覆盖,则该 H1 topic 记为 covered(滚入),不再发「未被任何课时覆盖」警告。真机复验中成套库 12 个 uncertain topic 里 8 个、杂乱库 53 个里 7 个章级 topic 全是这种「实际已覆盖、对账不认亲」的假警报,污染披露质量。

**Blocked by:** 无(独立票)。

**Status:** ready-for-agent

## 证据

- `verify-clean-summary.txt`:「第三章 `ArrayList`、第四章 `LinkedList`、第六章 树的遍历…」8 个章级 uncertain,对应模块/课时全部存在且锚在 H2/H3。
- `verify-dirty-summary.txt`:「第七章 到达哲学、第十一章 `HashMap`…」7 个同因;警告区前 5 条即此类。

## 实施要点

- [ ] **滚入规则**:`generation/expand.rs` 的 SourceCoverage 重对账(或 `structure/topics.rs` 拆分时建父子链)——同一 `rel_path` 内,父 topic(H1)状态 = 任一后代 covered ⇒ covered(可带 `rolled_up` 标记);后代各自状态保持不变(子缺口仍逐条可见)。
- [ ] **不掩盖真缺口**:父 topic 名下**零** covered 后代时,维持现状三态(covered/skipped+reason/uncertain),警告照发——滚入只消灭假警报,不产生新的静默。
- [ ] **mixed 文件语义**:滚入按 heading 父子链,不跨文件;mixed 文件被 skip 的噪声段仍逐段披露。
- [ ] **披露面**:SourceCoverage/WS/UI 的计数口径同步(topics_covered 上升、uncertain 下降);信封向后兼容(新增 `rolled_up` 可选字段,旧读侧忽略)。
- [ ] 单测:构造「H1 + 2×H2,一 H2 成课」→ H1 covered(rolled_up)、另一 H2 保持自身状态;「H1 无任何子成课且无 skip」→ 警告保留。

## 非目标

- 不改 topic 拆分粒度与三级锚(heading→段落→path)。
- 不改 skipped/uncertain 的既有语义与 UI 呈现结构(只随数据口径自然变化)。
- 不做跨文件/跨章的滚入。

## 验收

- 单测如上;存量 442 测试零回归。
- **真机复验**:两库重跑后,章级「未被任何课时覆盖」类 uncertain 归零(或仅剩真缺口且各自有明确状态);警告条数显著下降(成套库 12→≈0 章级;杂乱库 53 中章级 7→0)。
- `cargo test -p nomifun-learning` 全绿;`cargo check --workspace` 干净。

## 备注

- 警告噪音是信任的反面:用户看到「第三章未覆盖」去翻课程,发现第三章明明有课,下次就不信这份报告了。
