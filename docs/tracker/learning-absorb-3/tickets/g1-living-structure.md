---
id: g1
title: "结构转活文档：挂载 + 修补 + 维护回合进多少？"
type: grilling
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

learnhub #52 战役把结构从「生成时点快照」升级为「可增量维护的活文档」（契约 v2.7，
ADR-0017 七裁决）：挂载事实（导航用、无判定身份）+ 七型修补 ops（ops = 意图引擎管自洽）+
显式维护回合（不内联生长批）+ findings 留痕（评审 warn 通道生产 → 修补消费）。三重护栏 =
全量 gd.* 门 + 观测审计 + 出处，**不建受理门机器**。

allo 现状：`structure_json` 单字段快照（正是 v2.7 之前的语义）+ 只读投影；二轮 g3 已裁
别名闭环未实施。r1 报告的分级：A 强建议（挂载 facts/级联口径/七型 ops/RoundRetry/ops 空
= 合法/别名记录/new_aliases/smoke 判据）；B 弱可移植（载体形态重裁、findings 生产方、
渲染品 no-op、五图徽标前端）；C 已有等价物。

需要裁决（候选组合）：

- **(a) 全量进**：挂载表 + 三态降级 + 七型修补引擎 + 维护回合（两命令）+ findings 留痕
  + 门位口径（v2.10 随地形提示词）——结构故事一次闭环。
- **(b) 只进挂载**（最小闭环）：挂载表 + 三态降级 + 挂载回合 + 门位口径——结构仍是快照
  但有了导航层与「结构变了会显影」的机制；修补（结构编辑）后置。
- **(c) 缓**：本轮只吸收文本面（门位口径、术语纠偏进提示词/契约），维护体系等 allo 真机
  用出结构维护需求再说。

连带裁决面（随主选）：

1. **载体形态重裁**（learnhub 是单机文件 + 空间 Git）：allo 等价物 = 挂载 DB 表
   `(course_id, node_id, blocks[], reason, structure_hash)` + 修补审计表（dossier 内容
   成行）；「自动落盘无受理门」在多用户语境照抄还是加约束（learnhub 单机单写者）。
2. **findings 生产方**：allo 概念评审门要不要加「不打回只留痕」通道（warn 级 findings
   落表）——修补的输入源；若 (b) 无修补，可只留痕不消费。
3. **触发位**：显式命令入口照抄（allo 无调度器，正好同构）；重跑幂等/待挂空 skip 语义
   照搬。
4. **门位口径（v2.10）**独立于主选——它只动分解提示词与地形语义（从零课程的地面段进
   结构），建议随二轮 g2 结构提示词波一并落（即使选 (c)）。

约束：课程边界铁律不变（learnhub 挂载同样严守「结构↔概念经节点两跳、挂载无判定身份」，
同源无冲突）；gd.* 门复用勿造第二套；RoundRetry 与二轮 t1 早停同源合并实施。

## Resolution

> **裁决方式注记（2026-10-14）**：三问发出未获回答，按三项推荐项默认裁决——与本 effort
> 一贯取向（全量吸收）一致，有 learnhub 真机实证背书；票面标注可复议，实施前提出即可。

**速览**：三项全采推荐——**全量进（两步实施：先挂载后修补）**、**载体 = 单行 JSON +
审计表**、**findings 采纳评审 warn 通道**。

1. **范围：全量进（a）**——结构转活文档完整闭环。r1 报告论断：挂载与修补合起来才是
   「结构从快照变活文档」的最小闭环，拆开则挂载无质量回流（findings 无消费方）、修补无
   独立价值。实施顺序内部分两步：**先挂载**（表 + 三态降级 + 挂载回合），**后修补**
   （七型 ops 引擎 + 修补回合）——findings 从第一步就开始留痕（不追补）。
2. **载体形态：单行 JSON + 审计表**——挂载新表一课一行（`mounts_json` +
   `structure_hash` + `updated_at`），回合内原子覆盖无中间态，与 `structure_json` 单字段
   快照先例同构；修补审计 = dossier 内容成行落审计表（learnhub 文件 + Git 出处的 DB
   等价物）；st.mount.*（六条款）与 st.ops.*（五条款）落
   `docs/contracts/learning-structure-artifacts.md` 新章，validator ↔ 契约互钉测试照
   learnhub 形态。「自动落盘无受理门」照抄——回合是显式命令触发、单课串行，无并发写者。
3. **findings：采纳评审 warn 通道**——概念评审门输出扩可选 `findings[]`（warn 级、
   不阻断的数据缺陷观察，与现有 problems[] 阻断通道并行）→ 落 findings 表（learnhub
   JSONL 的 DB 等价：追加行，坏行跳过不 Broken）；**从第一轮生长就开始留痕**。修补输入源
   = findings ∪ 挂载观察（结构外/空块/超载 ≥5，仅观察不构成编辑义务）。

**连带定案**：

- **门位口径（v2.10）**随二轮 g2 结构提示词波落地（独立于本票主选生效）；
- **RoundRetry**（同错指纹早停 + 退回全文回灌）与二轮 t1 早停合并实施，learnhub
  `maintenance/mod.rs:62-94` 为参考实现；
- **两个防幻觉/防浪费语义照搬**：ops 空 = 合法结论（noop 不写盘）、待挂集空 = skipped
  （零 LLM 零写）；
- **结构 hash = 手写 FNV-1a 64**（勿用语言默认 hash——跨版本稳定性）；
- **触发 = 显式命令**（`POST /courses/{id}/graph/structure/mount` 与 `.../repair`）+
  工作区「结构图」Tab 两入口按钮；
- **渲染品重渲 = allo no-op**（一轮 g2 已裁不产人读文档）；smoke 判据照 learnhub 裁剪：
  mount（落位/hash 锚定/重跑幂等 EXIT=4）、repair（盘面复验/级联标记——「渲染一致」
  判据删除）；
- **前端**：结构图 Tab 补挂载三态段与已挂数徽标（读侧 join 零第二真相）。

**派生影响**（供实施切票）：新迁移（挂载表 + findings 表 + 修补审计表）；两命令端点 +
DTO（`MountRoundSummary`/`StructureRepairSummary` 对应形态）；`structure_view` 扩挂载
三态与 mount_index；评审门 schema 扩展（findings 可选键 + 落表）；两份提示词资产 +
金样 + 围栏测试（structure-mount / structure-repair）；learning-smoke 增 mount/repair
子命令。

