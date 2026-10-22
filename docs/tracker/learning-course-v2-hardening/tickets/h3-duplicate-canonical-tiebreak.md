---
id: h3
title: "重复对 canonical tie-break：规整文件名优先于 mtime,正身当 canonical"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# h3: 重复对 canonical tie-break 修正

**What to build:** 同内容(同 sha256)重复对的 canonical 选择,在「最新 mtime」之前增加「规整文件名」tie-break:数字章前缀、无备份/剪藏/错位标记的文件优先。真机复验中 `05-LinkedList.md` 被标 `duplicate_of=z9-LinkedList-备份.md`、`11-哈希.md` 被标 `duplicate_of=999-错位-哈希.md`(mtime 选中了备份/错位副本当正身),课时的 source 显示备份文件名,观感差且学习者按名回书会困惑。

**Blocked by:** 无(独立票)。

**Status:** ready-for-agent

## 证据

- `verify-dirty.json` triage:`05-LinkedList.md role=duplicate dup_of=z9-LinkedList-备份.md`(canonical_reason=「同内容文件中最新修改时间」);`11-哈希.md dup_of=999-错位-哈希.md` 同因。复制文件时副本 mtime 更新,规则系统性偏向副本。
- Q12 原文(map v2):「同语言选最新/最高质量(mtime/大小/标题完整度可作 tie-break)」——本票是对「最高质量」维度的补全,不是推翻。

## 实施要点

- [ ] **tie-break 顺序调整**(同语言同内容对):规整度评分 → mtime。规整度启发式(实施时可调,保持确定性、零 LLM):
  - `^\d{1,3}-` 数字章前缀 > 无前缀;
  - 文件名含「备份/backup/copy/副本/剪藏/old/-v\d+/错位」类标记 → 降权;
  - `999-`/`z9-` 这类明显错位前缀按「数字前缀但超序」处理——可与书内前缀分布对齐(如同库主流为 01–18,则 999 视为非正身),或简单归入降权名单。
- [ ] **确定性**:同分才落 mtime;再同分保持现有稳定序(路径字典序);整个选择纯规则可复现,单测钉死。
- [ ] **报告一致**:canonical_reason 文案随之更新(如「按规整文件名选定正身」),`duplicate_of` 指向正身;覆盖/课时 source 引用正身路径。
- [ ] **存量测试对齐**:t1 的 canonical 单测若钉了 mtime 优先,按新顺序更新断言(含翻译副本/README/语言各前置规则不变——本票只动「同语言同内容」这一层)。

## 非目标

- 不动翻译副本分流与非翻译后缀原文优先(那两层在 mtime 之前,保持)。
- 不做内容质量评分(标题完整度等启发式仅在规整度同分时考虑,可选)。
- 不改 duplicate 的披露语义(仍自动排除+理由)。

## 验收

- **单测**:`05-LinkedList.md` + `z9-LinkedList-备份.md`(同 hash,备份 mtime 更新)→ canonical=05;`11-哈希.md` + `999-错位-哈希.md` → canonical=11;两个都规整 → mtime;全同 → 稳定序。
- **真机复验**:脏库重跑后 `dup_of` 指向 05/11,课时 source 不再出现 z9/999 路径;其余读数不劣化。
- `cargo test -p nomifun-learning` 全绿;`cargo check --workspace` 干净。

## 备注

- 三票里最小的一张;若与 h1/h2 同波实施,建议最后合入以免 triage 报告断言互相踩。
