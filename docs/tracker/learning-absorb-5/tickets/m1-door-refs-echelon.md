---
id: m1
title: "门口梯队 door_refs 块注解 + 过门自检读侧"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# m1: 门口梯队 door_refs 块注解 + 过门自检读侧

**What to build:** allo 的地形门口已是 4-8 条 G-id 梯队，但块侧没有「哪条门口
判据整体覆盖了哪块」的注解（door_refs），也没有过门自检的消费面。吸收
learnhub #165（契约 v2.14）：goal-decomposition 产物 blocks 增可选 `door_refs`
标注（入门档各块被哪条梯队判据整体覆盖才标、宁缺勿滥），结构侧对账核验 +
读侧视图 + 块详情展示 + 过门自检读数（勾选已满足的门口 → 回显被覆盖的块）。
本票是 m2（基础声明的块命中语义）的前置。

## allo 现状

- 地形：`prompts/structure/prerequisite-terrain.md` 产 `doors` 4-8 条
  （`id` G1…、`criterion` 能…/可… 能力动词形、`kind` hard|self_check），
  存 `structure_json.terrain`（`service/learning_graph.rs:889` 写、:307/:404/:670
  读）；读侧 `TerrainView`（`models.rs:1769-1782`）。
- 结构生成门口对账：`structure_contract::reconcile_doors`（`structure_gen.rs:179`，
  G-id 不匹配 → 反馈重试，测试 :359-380）——对账只查 G-id 存在性，无块侧注解。
- goal-decomposition 资产：`prompts/structure/goal-decomposition.md`（42 行），
  金样三闸（`structure_prompts.rs:466/:295/:388`）；k06 已织入门语义措辞
  （硬前置=不可自检豁免非进入闸门）。
- 块结构：`BlockView`（结构视图投影）无 door_refs 字段；结构契约 gd.* 无此键。
- 前端：结构 Tab（`StructureGraphTab.tsx`）+ BlockDetail 组件现成。

## learnhub 参考（#165，323abed；契约 v2.14）

- `docs/contracts/decomposition-artifacts.md` §3 门位口径 L165（梯队 G1..Gn
  全文）、`gd.blocks.door_refs` 行 L120（shape `G<数字>`，facade
  `check_door_references` 叉积解析）、schema 字段注释 L75-76。
- 语义铁律：「学习者能满足这些门口判据，就必然已会本块」；标注面从「入门档
  最前的块」扩为「入门档各块被哪条梯队判据**整体**覆盖才标、**宁缺勿滥**」。
- 提示词：`prerequisite-terrain.md:18-19`（梯队句：门口是入门段梯队、带分不清
  或判据写不实就不硬凑门、判据须能…/可…、hard 门口 hard:true）+
  `goal-decomposition.md:33`（door_refs：入门档全块过一遍、仅整体覆盖才标、
  宁缺勿滥）+ :35（入门档先具体后抽象）。
- 消费面：过门自检 UI（gate-self-check.tsx）+ #164 声明 target_rows
  （basis_declaration/facade.rs:342-367，AND 语义）。

## 实施要点

- [ ] 结构契约：`gd.*` 増 block 级可选键 `door_refs`（G-id 数组，引用地形
      门口）；`reconcile_doors` 扩叉积核验（door_refs 引用的 G-id 必须在
      地形门口集内，悬空 → 反馈重试；对allo 现有反馈重试回路零新机制）。
- [ ] goal-decomposition 资产织入 door_refs 标注条款（learnhub :33 措辞域
      中立化：入门档全块过一遍、判据**整体**覆盖才标、宁缺勿滥、正式内容段
      不标）；prerequisite-terrain 资产补梯队句（learnhub :18-19 域中立化）。
      **金样三闸随批**（金样重生成 + 围栏 + 条款覆盖短语断言）。
- [ ] 读侧：`BlockView` 增 `door_refs: Vec<String>`（serde default 容旧快照）；
      TerrainView 补块覆盖统计（每条门口覆盖块数）。
- [ ] 过门自检读数（本票只做读侧，声明写侧归 m2）：结构 Tab 或块详情给
      「门口 → 覆盖块」映射展示；i18n 双语红线。
- [ ] 契约文档 `learning-structure-artifacts.md` bump 小版本（door_refs 条款 +
      梯队口径注记）。
- [ ] 纯资产面无运行时开关——双臂走**版本臂**（k06 先例）。

## 验收

- **单测（红绿）**：door_refs 叉积核验（合法 G-id 过 / 悬空 G-id 打回带指名 /
  旧快照无键兼容）；TerrainView 覆盖统计；金样三闸全绿。
- **smoke structure 档（版本臂双臂）**：旧 tip vs 新 tip 各跑一次（同 fixture
  目标同凭据，独立 CARGO_BUILD_DIR）。硬判据：两臂 EXIT 均按既有判据不劣化；
  on 臂（新 tip）产物 blocks 的 door_refs 引用全部可解析（机械核验）。
- **对照卡**：两臂结构产物并排——door_refs 标注块数 / 覆盖门口数（机械读数）
  + 人读抽判：标注块是否真的被判据**整体**覆盖（宁缺勿滥——错标比漏标严重）
  + 入门档外有无错标。两臂都跑，落 Resolution。
- `cargo test -p nomifun-learning` 全绿；typecheck/i18n/check 门过。

## 非目标

- 声明写侧（勾选 → 铸概念）——m2。
- 门口梯队本身的重设计（allo 已是 4-8 条梯队形态；#165 的梯队化对 allo 主要
  是措辞与消费面）。
- 机械门（door_refs 覆盖正确性无法机械判定——learnhub 同为提示词条款 + 形状
  核验）。

## 备注

learnhub 证据：ldata1 数学课 21 块入门档仅 3 块带 door_refs、自检面无处扩
（ADR-0013 增补缘由）——allo 对照卡里如实记录标注密度，供 m2 判块命中语义
是否够用。
