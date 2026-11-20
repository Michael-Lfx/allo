---
id: rs2
title: 吸收 learnhub #252——常识行撞名池机械归一+留痕，同形词注记条款退役
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

常识行 term 逐字命中名池（同形异义，如「次数」计数义撞多项式 degree
义）现行=机械否决→**整份评审结论作废重掷**（每次重掷=重付 ~28k 思考
token ≈ 2-6 分钟）。allo 已付费：+303s 死因重掷（手冲案）；上游同族病
2026-11-17 真机病历=同一行重掷 3 次、每次 57–137s、共 4-5 分钟，正是
learnhub #252（2026-11-17）落地机械归一的驱动。allo 落后该裁决，仍在
用「提示词伤疤」（mod.rs:979 同形词盲区注记）让模型自己绕 + 机器层兜
底否决。

## 已核验事实（2026-11-20 对码）

- allo 否决点：`provenance.rs:15`（头注「常识行 term 不得逐字命中名池
  （事实矛盾）」）+ 校验器内对应 Err 臂。
- 提示词伤疤：`mod.rs:979`「同形词盲区注记」条款；条款在场断言
  `mod.rs:3405-3406`（「同形词盲区」短语断言测试）。
- 上游实码：`learnhub/src-tauri/src/engine/node_growth/reviewer.rs:535`
  （归一函数 `ProvenanceNormalization`）+ `:2103`（两条常识行都归一测
  试）+ `:2178`（带 ref 常识行仍重试不归一的 Scripted 两轮测试）。
- findings 通道现成：`record_review_findings`（service/learning_graph.rs:1283）
  → `learning_structure_findings` 表 → k02 水位消费（:939 读、:1419 计
  数）——归一留痕零新面。
- 读数面：`ProvenanceReadout`（provenance.rs:81-95，rows/invalid/
  retried/degraded/external_hits）→ `GrowthRunStats`（service:2923）→
  smoke 读数。

## R（上游 #252 裁决 B 平移；用户条件批准 2026-11-20）

- **R-a 归一三前提（缺一不归一，维持 Err）**：term 逐字命中名池 ∧ 已
  过定义原文逐字核 ∧ ref 为空（常识行本不带 ref）。
- **R-b 机械改判**：命中批内 teaches 键且 term ≠ 本条提名 concept →
  改 `自教 ref=term`；命中名池且非本批提名 → 改 `已教 ref=term`。改判
  逐条进 findings（node="批"，issue 点名 term→改判后 source/ref），
  **不挡门**。声明是被改写不是被接受——E1 常识通道不开放。
- **R-c 其余行伪零放宽**：只提名未教 / ref 指向本条提名（循环自证）/
  未逐字落地 / 缺失行放行 → 维持 Err 走死因重试恰一次。
- **R-d 条款退役**：`mod.rs:979` 同形词盲区注记整段删除（机器接手），
  `mod.rs:3405` 断言改钉「条款已退役」（或直接删断言——按该测试既有
  结构定）。
- **R-e 读数**：`ProvenanceReadout` 增 `normalized: usize`（归一行数），
  GrowthRunStats 增 `review_provenance_normalized`——归一频率真机可
  观察不造数。

## Acceptance

- 单测（provenance.rs tests + mod.rs 评审族）：
  ①常识行撞名池+过逐字核+无 ref → 归一（自教/已教两臂）+ findings 留
  痕 + 结论可用不重掷；
  ②带 ref 的常识行撞名池 → 仍 Err 重试（上游 :2178 形态平移）；
  ③循环自证/未逐字/缺失放行 → 仍 Err（零放宽回归）；
  ④「次数」型 fixtures（同形异义词）归一后 findings 可读。
- `cargo test -p nomifun-learning` 全量绿 + fmt。
- 双臂 smoke 对照（归一 on = 新代码 vs 旧二进制基线）：reject 方向不翻
  转、retried 计数不升、无该打不打的放行。
- **回退条件（负优化判定）**：真机 findings 出现同形异义**错改判**（
  「次数」型：计数义被错挂到 degree 概念）或双臂对照出现打回率异常下
  降（该打的没打）→ 回退「只审计不归一」（上游同款回退句），换法重立。

## Resolution

已施工收口（2026-11-20，implement-spec 并行波）。

- **落地**（wave/rs2 8ddcaa2ad，并入 feat/learn1116 merge 5710c1a94）：
  5 文件 +466/−85。provenance.rs 否决臂→归一臂（三前提：常识行∧已过
  定义原文逐字核∧ref 为空；锚概念无定义对照域核不了=不归一维持 Err，
  对齐上游 `definition.is_some()` 前提）；残余行伪四形态零放宽；新
  `ProvenanceNormalization`+`normalization_findings()`（node="批"）经
  既有 j09 通道落库；`ProvenanceReadout.normalized` 计数 +
  `GrowthRunStats.review_provenance_normalized` + smoke 读数行跟进；
  mod.rs:979 同形词条款整段退役（on 臂负断言钉「不得回潜」）。
- **TDD 红绿样**：红1=16 编译错→绿1=provenance 32 绿；红2=退役钉
  FAILED→绿2=删条款过。夹具①滤杯循环自证（仍重试恰一次）+②次数
  同形（归一零重掷+findings 可读）+③带 ref 常识行仍重试（上游
  :2178 形态平移）随票落地（rs5 R-a 前半）。
- **检查**：本分支全量 `cargo test -p nomifun-learning`
  **1021 passed / 0 failed**（397s）；合并点三票合树全量
  **1022 passed / 0 failed** + `cargo check --workspace` 过
  （2026-11-20）。fmt 干净，零 AI 署名。
- **并后 A/B（回退判定的读数，挂真机）**：双臂对照 reject 方向与
  retried/normalized 计数——smoke 档案（ArchivingCompleter usage
  读数）为源；「次数」型错改判出现即回退只审计不归一。
