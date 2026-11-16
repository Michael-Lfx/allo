---
id: r1
title: "learnhub 结构维护体系深挖（#52 战役）"
type: research
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub #52 战役（#93–#97、concepts 重构、契约 v2.7–v2.10、ADR-0016/0017）各机制 Substance
是什么；对 allo（结构 JSON 快照 + 只读投影、二轮已裁别名闭环未实施）什么是增量、什么已有
等价物、可移植性如何分级？

## Resolution

**速览**：learnhub 把结构从「生成时点快照」升级为「**可增量维护的活文档**」（契约 v2.7，
ADR-0017 七裁决）——挂载事实 + 七型修补 + 显式维护回合，全靠三重护栏（全量 validator 门 +
观测审计 + 出处）而非受理门机器。

- **挂载（#95）**：挂载事实独立存储（单写者 = 维护 agent），`structure_hash`（手写
  FNV-1a，勿用语言默认 hash）锚定当前结构——hash 变 = 全量待重评；三态读侧降级
  （NotEvaluated/Outside/Mounted，悬挂 slug 逐条降级不 Broken）；`merge_mounts` 图序
  upsert 幂等；待挂集空 = 零 LLM 零写。st.mount.* 六条款（含 `no_concept_fields` 红线
  扫描——概念字段禁入挂载）。
- **修补（#97）**：七型 ops（add/remove/flip 边 + add/split/rename/merge 块）；
  **「ops = 意图，引擎管自洽」**——模型只表达意图，纯函数 `apply_ops` 依序应用（拆/并的
  引用面级联引擎代管：改名跟改、拆块退场、并块去重去自环），应用后 gd.* 全量重过零放宽；
  `st.ops.apply`/`endpoints`（应用序模拟）/`no_slug_churn`（rename 键闭禁 slug、新 slug
  不得复用退场 slug）；**级联不建逐 op 跟迁——structure_hash 重评估单点兜住**。
- **维护回合**：唯一触发 = 显式命令（不内联生长批、滞后无害）；RoundRetry ≤3（同错指纹
  早停 + 退回全文回灌）——二轮 t1 早停设计的现成参考实现；**ops 空 = 合法结论**（防凑动作）。
- **findings 留痕**：生长评审门的 warn 通道（数据缺陷不打回只留痕）落机读 JSONL，修补任务
  的输入源；留痕是线索不是第二真相。
- **概念视图/三视图跳转（#94/#96）**：concept_view（登记表条目 + 跨课程反查按投影名
  ref 分组）、节点结构位置三态段、区块已挂清单、五图已挂数徽标——全部读侧 join 零第二
  真相。allo 的 `GraphConceptRowView.refs` 已覆盖大半，缺「按投影名分组」与别名结构化。
- **concepts 重构（对二轮 g3 最关键）**：层字段收缩（去 operation/example/pitfall——
  教学材料归内容层，登记表只存判定所需定义与名字事实）；**别名从字符串升为记录**
  `{name, lang, kind, definition, def_src, attestation, display_priority, status}`；
  **批内 new_aliases 通道**：归一化定义撞判据 = 直定宿主 → 近邻裁决但必须裁复用（裁新铸
  = 整批打回指点改走 new_concepts）→ 登记单点（宿主非废弃/名字全库唯一活跃占池退役不占/
  幂等 AlreadyPresent/**复活 Reactivated**）；display 指针废除（展示名 = 首别名）。
- **门位口径（v2.10）**：从零声明课程，门划在学习者真实起步地板，地板以上全进结构
  （开局贴地、可学可挂）——挂载的前提。v2.8 术语纠偏：块 = 主题单元（六件套血统归概念
  条目）；收敛率/转正退役。
- **分级**：A 强建议（挂载 facts、级联口径、七型 ops、RoundRetry、ops 空=合法、概念层
  收缩与别名记录、new_aliases、smoke 判据模板）；B 弱可移植（文件/Git 载体重裁、findings
  生产方、渲染品重渲 = allo no-op、五图徽标前端、门位口径随地形提示词）；C 已有等价物
  （gd.* 门、概念反查大半、投影纪律、重试语义）。

完整报告（含 mounting schema、st.* 条款清单原文、ops 表、new_aliases 铸名门处理流程、
ADR-0017 七裁决与否决清单、smoke 判据）：
[assets/r1-structure-maintenance.md](../assets/r1-structure-maintenance.md)
