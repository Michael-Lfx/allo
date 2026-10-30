---
id: f1
title: "评审挂号表支持批外合法概念锚定——声明/历史批概念不再打死挂号表"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# f1: 评审挂号表支持批外合法概念锚定

**What to build:** 生长评审的 term_provenance 挂号表放行「registry 已存在且本批
复用」的概念锚——m2 声明铸下的概念被生长批复用时，评审不再两次同死降级。
附带不可满足反馈短路。

## 法证证据（2026-10-30 真课验收，e2e2/server.log）

- `06:46:35 provenance validation failed — retrying the review once with the
  death cause attempt=1 reason=已教行「每存满一年，多…」`
- `06:47:19 provenance table unusable twice — degraded pass death=挂号表不可用:
  行 concept 「百分数改写为小数」不在批内`
- 该概念是 **m2 声明刚铸进登记表的**（source=basis-declaration）。生长批
  defnear **正确复用**了它（登记表 8 条 = 声明 4 + 生长 4，零重复；5 节点只
  铸 4 条）——死的是**评审挂号锚定**：挂号行要求概念 ∈ 本批铸造，复用的
  registry 概念永远不满足。模型被带死因重试一轮 = **不可满足的反馈目标
  （卡壳）**，随后降级 pass（m3 fail-open 兜住，但本批评审锚定全废 + 白烧
  一轮）。

## allo 现状（落点提示，file:line 需实现会话复核）

- 评审挂号表 = m3 落的 term_provenance 校验，在生长评审路径
  （`crates/backend/nomifun-learning/src/learning_graph/` 评审面 + 重试环，
  06:46 两条 WARN 的发射点即执法处——grep「挂号表不可用」「provenance」）。
- 评审输入装配（教练上下文/评审 user 消息）当前携带：批内候选集（m4 复用
  通道 j04 落的 NominationPlan/registry_entries 只认活跃别名）——**没有**
  「本批复用的 registry 存量概念」的 ULID 表。
- 降级语义：两次同死 → degraded pass（m3 fail-open，保留不动）。

## learnhub 参考（实现会话先读）

- 挂号表原main：learnhub #169/#176 term_provenance（评审输出锚定批内铸造）。
- **本碰撞是吸收缝，learnhub 侧是否已有解未核实**——实现第一步读他们的
  评审输入装配：声明概念（ADR-0032 铸的）在生长评审输入里如何呈现、挂号行
  是否允许锚定非本批概念。若无解（他们声明块可能从不进生长批），则本票是
  allo 首创缝补，域中立自研 + 票面记档。

## 实施要点

- [ ] 评审输入携带「批外已知概念表」：本批复用（gate 复用裁决命中）的
      registry 概念，带 canonical + concept_id，注明来源（declaration/
      历史批）——挂号行可锚定到这些 ULID。
- [ ] provenance 校验放行：挂号行概念 ∈ 批内铸造 ∪ 批外已知概念表 =
      合法锚；仍拒绝「批内都不存在的凭空概念」（j04 的编造静默丢弃语义
      不变）。
- [ ] 不可满足短路（belt）：死因匹配「不在批内」且该概念在 registry 中
      存在 → 跳过重试直接降级（不再让模型空转一轮）；日志注明「批外锚定
      未启用/未命中」。
- [ ] 红绿样：声明概念被复用批的评审 → 非降级通过 + 挂号行锚定 registry
      ULID；凭空概念 → 仍拒；同死两轮 → 立即降级（不烧第三轮）。

## 验收

- `cargo test -p nomifun-learning`（+ai-agent 若涉）全绿；零迁移。
- 真机判据：声明 G1+G2 后再生长含声明覆盖块的批 → 日志**零
  「provenance table unusable」**、评审非降级、挂号行锚定 registry 概念。

## 非目标

- 声明覆盖块要不要进生长批（产品雾区，见 e3 Resolution）。
- 降级语义本身（fail-open 保留）。
- 释义级近义检测（雾区⑤）。

## Resolution（2026-10-30 grilling 裁决，影响面核查后；真机结果可复议）

**裁决（用户：按推荐）：**

1. **放行域 = 本批复用命中集**：撞池机械复用（auto_reusals）∪ defnear 直定宿主
   （direct_host）∪ 评审 reuse[] 机械核验通过的 concept_id。声明/历史批概念
   **未被本批复用时不放行**（凭空锚 registry 存量仍拒）。
2. **短路 belt 作废，改死因指引**：死因消息加指引（「该概念系登记表存量、非本批
   铸造：删除该行，或改锚本批概念」）——重试变得可满足；不砍碎片面（m9）共享的
   第二轮机会；主修复落地后此臂触发面极窄。
3. 评审输入注入「本批复用命中表」块（候选集块模式），注记用锚来源三值
   （撞名机械绑定/定义直定复用/评审裁决复用）；registry source 列不进注入。

**影响面核查勘误（2026-10-30 实码核实）：**

- 执法点 = `provenance.rs:145-149`（validate_term_provenance，行 concept ∉ 提名
  即死）；重试环 `learning_graph/mod.rs:1329-1357`，**attempt 预算挂号面+碎片面
  共享**（mod.rs:1207-1209）——短路作废的直接原因。
- 票面「评审输入没有 ULID 表」不准：候选集已带真实 concept_id
  （service/learning_graph.rs:2352-2377）；声明概念已进 provenance_known
  （gate_batch 2751-2768+2993）与已教账（4339-4360）；**已教行 ref 锚声明概念
  今天已合法**。真正缺的只有行 concept 字段的放行域。
- 原短路 belt 缺原料（校验器只收合并名池，分不出 registry-only）——随短路作废
  无需补。
- degraded_outcome 清空模型 reuse[]（mod.rs:1197），机械复用不受影响——事故批
  复用落池没死，死的是评审背书+白烧一轮。
- **learnhub 碰撞核实（票面遗留问句已答）**：learnhub 结构性回避而非已解——
  prompt 禁重教 declared 概念（node-growth.md:13/30 强制走 assumes）+ 铸名撞池
  refuse 打回（facade.rs:2036-2050，不像 allo 留批内机械转复用）；validator
  「不在批内提名」臂原样存在（reviewer.rs:401-404）。**allo 首创缝补成立**。
- 行 schema 零变更：复用概念行走现有 source=taught + ref=正名 路径（ref 对照域
  已放行）；review_schema() 无生产消费者且不动。
- reuse 核验（service:2019-2057）须提取单源供 concept_review 内复用——同一
  reuse 两处判定必须一致，禁止复刻。
- concept_review 签名变更波及 mod.rs 内 12 处测试调用点 + service:1989。
- ProvenanceReadout 增 external_hits 计数（真机判据「挂号行锚定 registry 概念」
  的可观察信号）；smoke provenance 读数段已有（learning_smoke.rs:1428-1452）
  非门禁，追加打印即可。

施工图：[blueprints/2026-10-30-f1-provenance-external-anchors.md](../blueprints/2026-10-30-f1-provenance-external-anchors.md)。
