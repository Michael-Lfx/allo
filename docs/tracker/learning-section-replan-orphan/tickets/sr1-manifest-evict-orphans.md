---
id: sr1
title: manifest 重排真整组替换：清单外孤儿正文机械驱逐 + sections() 清单内过滤兜底
type: task
status: in-progress
assignee:
blocked_by: []
part_of: map.md
---

## Question

重生既有课时（G1-R 真实课程臂）暴露：模型中途重排节清单（s1–s5 →
sA–sD）后，草稿层把新旧两套节全部发布（9 节、标题重复、顺序乱、其一
练习节空壳）。真机工作流真实可达（生长线 redraft/生成重试），真实库
5 课至今未中（概率触发）。

## 已核验事实（2026-11-19 对码）

- `SetSectionManifest`（lesson_draft.rs:743-811）只搬幸存键、不驱逐被
  放弃键——「整组替换」名实不符（工具描述 lesson_content_loop.rs:1444
  「规划课时的分节清单（整组替换）」）。
- `sections()`（lesson_draft.rs:1019-1043）孤儿坠尾不丢（planned=旧清
  单 Some，`unwrap_or(usize::MAX)` 排尾）。
- 发布落库（service/lesson.rs:2206-2237）DELETE+按 output.sections 插
  入——正确，坏在输入。
- G1-R 实锤：9 节（sA–sD manifest 序 + s1–s5 哈希序坠尾）、双练习节
  （sD 空壳）、76 LLM 调用返工链。真实库 5 课全干净。

## R（grill 四问裁决，全按推荐）

- **R-a 机械驱逐**：`SetSectionManifest` 清单落定后 `section_bodies`
  retain 只留清单内键。幸存键的「已写正文按 key 保留 + planned 换新」
  语义原样保留。
- **R-b 过滤兜底**：`sections()` 只产出清单内键（防御纵深；孤儿即便因
  未来回归存在也进不了发布）。
- **R-c 不做审计门**：机械已保证，不加概率性冗余面（e 波 R-b 同律）。
- **R-d 范围外**：重生既有课时的旧 attempts/review_items 悬挂面另立候
  选（与 sq5 删题级联裁决同构）；重排诱因（返工链策略）不在本票。

## Acceptance

- 单测（lesson_draft tests）：①首排 s1/s2 写正文 → 重排 [sA,s2] →
  s1 孤儿被驱逐、s2 正文保留且 planned 换新、`sections()` 返回仅清单
  内键且顺序=清单序；②全换新键重排 → `sections()` 空（旧正文全清）；
  ③「已写正文按 key 保留」既有测试零改动绿。
- `cargo test -p nomifun-learning` 全量绿 + fmt。
- **真机复跑重生臂**（新快照重置「值存储位置」节点 → 冻结二进制 →
  API 同步重生）：节集合无重复、节数=清单数、键名标准、五指标维持
  （裸指代 0/回收进题面 0/evidence 100%/承诺 0/section_key NULL）。
