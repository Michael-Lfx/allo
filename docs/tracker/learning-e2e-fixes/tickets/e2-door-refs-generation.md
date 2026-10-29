---
id: e2
title: "door_refs 生成侧标注——解锁已会声明旅程（BLOCKER）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# e2: door_refs 生成侧标注（BLOCKER）

**What to build:** 结构生成提示词补 door_refs 标注条款（#165 扩面版域中立移
植）+ 生成时零标注 warn 读数 + 声明端点 400 文案带替代出路。真课验收 = 声明
端点 200 且零节点写入。

## allo 现状（E2E 实锤 2026-10-29）

- 真课 42 块 door_refs **全部为空** → `POST /basis/declare` 400「勾选的门口
  没有命中任何带门位标签的知识块」——用户旅程「跳过已会基础」死路。
- 五轮 m1 雾区（生成侧 0/23×2）在真课复现并坐实为 BLOCKER。
- allo 结构提示词（gd.* 家族，learning_graph 结构生成）无 door_refs 条款；
  m2 锚表/declare/revoke 端点机制面五轮真机已验证可用（basis smoke 全绿），
  缺的只是生成侧标注。

## learnhub 参考（Reference 表——实码先读铁律）

| 件 | learnhub 路径 | 取什么 |
| --- | --- | --- |
| door_refs 标注条款全文 | `prompts/decomposition/goal-decomposition.md:33` | #165 扩面版：入门档**全部块**逐块判「主题已被某门口判据整体覆盖」→ 写 `door_refs:[G编号…]`；编号逐字取自 prerequisite_doors；一块可引多条；**宁缺勿滥**；非从零课程全块不写 |
| 块键白名单（door_refs = 唯一可选键） | `goal-decomposition.md:32,45` | 五键必填 + door_refs 唯一可选；白名单违约 = 机械红 |
| 校验器形态 | `engine/decomposition/validator.rs:25,30,545-561` | `gd.blocks.door_refs` 形态 `G<数字>` + 块内去重 |
| 低标注史 | `docs/adr/0013:21,38` | learnhub 自己 3/21 → #165 扩面到「入门档全部块」的裁决原文 |
| 零标注读数（warn-only，刻意不加机械门） | `engine/decomposition/facade.rs:190-205` | `check_door_references`：「从零声明课程门口对账通过但零块门位标签——静默失活」tracing::warn，不拒收不回炉 |
| 端点拒绝+出路文案 | `engine/basis_declaration/facade.rs:125-129,135-149,173-180` | 零命中拒绝原文带出路「**请改用逐个跳过**」；空勾选拒绝；越门勾选拒绝 |
| 物化方案已废 | `docs/adr/0032:1-40`、`docs/contracts/gate-materialization-artifacts.md:5` | 不走「勾选→生成节点→置跳过」老路 |

## 实施要点

- [ ] allo 结构生成提示词补 door_refs 条款（域中立移植 :33 全文；allo 的门口
      编号/梯队形态 = TerrainView doors G-id，已就位）；块键白名单同步。
- [ ] 校验器补 `door_refs` 形态条款（G<数字>、块内去重、编号 ∈ 门口清单——
      learnhub `gd.blocks.door_refs` 对应 allo gd 契约新条款，金样重生成）。
- [ ] 生成门面加零标注 warn 读数（learnhub check_door_references 同款，读数
      不拒收）；smoke structure 档补读数行。
- [ ] declare 端点 400 文案带出路（「请改用逐个跳过」——allo 对应逐节点跳过
      的既有 UI 面）；顺带核空勾选/越门勾选文案对齐。
- [ ] 真课回归：重建一门课 → 结构 → 抽查入门档块 door_refs 标注率（对照卡：
      标注块数/入门档块数并排 learnhub 3/21 基线）→ 声明 G1+G2 → 200 +
      零节点写入 + 下游就绪投影变化。

## 验收

- 结构契约红绿样：door_refs 形态违约打回、合法标注过门、编号越清单打回。
- 真课声明端点 200（计数守恒硬判据：登记表增行、零 learning_nodes 写入）。
- smoke structure/basis 档全绿；`cargo test -p nomifun-learning` /
  `-p nomifun-ai-agent` 全绿；金样重生成 + 条款断言。

## 非目标

- 存量无标注课程补标（learnhub 出路 = 删课重建；allo 同——不加修补通道）。
- 过门自检读侧消费（m1 已落）。
- 机械门（learnhub 刻意 warn-only，照搬）。
