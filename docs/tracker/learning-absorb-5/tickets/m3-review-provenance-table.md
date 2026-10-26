---
id: m3
title: "评审挂号表 term_provenance 程序化 + 定义原子性条款（#169/#176）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# m3: 评审挂号表 term_provenance 程序化 + 定义原子性

**What to build:** k07 把评审判据①挂号改成了结构化子查，但执行仍是纯提示词
——learnhub 病历（三角形批复发零发现放行）证明「判据写了 ≠ 会执行」。吸收
#169（ADR-0033）：评审输出增**必填 term_provenance 表**（每条提名的定义里
引用/预设的每个领域名词一行：来源四值 + 引用），评审接受前**机械校验**整表；
配套 #176 定义原子性条款与原子名出路指引。这是 k07 的程序化收口。

## allo 现状

- 评审门：`CONCEPT_REVIEW_HEAD`（mod.rs:707）+ `CRITERION_ONE_V2`（k07，
  :720-727，挂号子查四入口：已教/本节点 teaches/assumes/课程外通用常识；
  批内提名不是出处；内嵌上位词在审）+ `NOMI_LEARNING_REVIEW_V2` 开关。
- 评审上下文已有全部机械面输入：已教账（:707-710 跨课程全局账）、登记表节选、
  批内 mints/new_aliases/teaches——**机械校验所需的已知名字池现成**。
- 评审输出契约：`ReviewOutcome`（reuse[] + findings 通道 :748-753）；结构门
  机械校验先例（名字可解析/近形归一 mod.rs:302-336）。
- 评审失败处理：重试 ≤3 带条款报告（j03/growth_retry.rs）。
- findings 留痕：j09 warn 通道 + k02 水位消费。

## learnhub 参考（#169 8c8ba73 + ADR-0033；修复 939f28b/604f12b；#176 ec711a4）

- **ADR-0033** `docs/adr/0033-review-provenance-table.md`：挂号判据从原则变
  程序——评审必填表 `{concept, term, source: 已教|自教|常识|缺失, ref?}`
  （`node_growth/reviewer.rs:121-131`，TermSource :100-113）。**缺失是第四个
  承重值**：没有它模型会把缺口洗成常识（三角回放 22/24 行常识）。
- 机械校验（reviewer.rs `validate` :380-505）：ref 逐字可解析于已知名字池
  （taught ∪ 登记表名+别名，经 `ReviewInput.known_names` 注入 :176）；自教
  ref ∈ 批内 teaches 键且**非提名自身**（循环自证禁）；已教 ref **不得指向
  本批提名**（同循环形态，939f28b）；常识行无 ref 且 term **不得**命中名字
  池（事实矛盾）；提名的定义全覆盖；concept ∈ 提名集。校验失败 = 结论不可用
  → **恰一次**带死因重试 → 仍败 = Degraded 保守放行（评审器坏 ≠ 真缺陷）。
- 逐字落地校验（604f12b）：常识/缺失行的 term 必须逐字出现在该概念定义里
  （`term_grounded` :505-521，尾括号容忍；严格版误报 2/37 全是已教行——
  逐字校验只对常识/缺失行做）。
- serde 容旧录制缺键（schema 仍 required）。
- **#176**（ec711a4）：`node-growth.md:46` 定义原子性条款——「一条提名只定义
  一个对象——定义里不得顺带给第二个概念下定义；定义里预设的构件概念，要么
  单列提名、要么改写绕开。自检验收：把提名拆成两半，各半仍说得出独立定义的
  = 捆扎条目，必须拆条。概念严格原子，学习节点允许复合」；atomic_name 违规
  消息带出路「多个概念拆成多条提名」（validator.rs:539-544/:596-598）；评审
  头注记同形词盲区（「次数」被迫 ref 案）；机械门红 2/2、仅提示词条款 flash
  不翻案——**收敛靠机械门 + #175 反馈链**（m5 领地）。

## 实施要点

- [ ] **ReviewOutcome 增必填 `term_provenance` 表**（serde default 容旧包，
  新包 schema required）；评审提示词织入表格条款（来源四值 + 逐行定义 + 挂号
  判据①改为「按表核对」——CRITERION_ONE_V2 的框定/四入口措辞保留为表的
  语义说明）。
- [ ] **机械校验器**（新模块或 growth_contract 扩展，纯函数）：上述 learnhub
  全套检查（ref 解析/自教循环/已教不指本批/常识不命中池/全覆盖/concept ∈
  提名/term_grounded 逐字）+ `known_names` 注入缝（已教账名字 ∪ 登记表活跃
  名+别名——评审上下文已装配，复用）。失败 = 带死因重试恰一次 → Degraded
  保守放行 + findings 留痕（j09 通道，进 k02 水位消费）。
- [ ] **#176 织入**：COACH_SYSTEM 铸名纪律段加定义原子性条款（域中立措辞）+
  原子名违规消息加出路句 + 评审头注记同形词盲区。
- [ ] 开关 `NOMI_LEARNING_REVIEW_PROVENANCE=off`：不要求表、不做机械校验
  （= k07 后行为，off 臂）。
- [ ] 条款覆盖短语断言（评审提示词 + 教练铸名段各加关键短语）。

## 验收

- **单测（红绿样，机械校验器全表）**：合法表过；ref 指向不存在名字红；自教
  ref 指自身红；已教 ref 指本批提名红；常识行 term 命中已知池红；漏行（某
  提名无任何行）红；常识/缺失 term 不在定义里逐字红；表缺失 → 重试一次 →
  Degraded 放行留痕；off 臂零校验。
- **smoke grow 档双臂（开关臂）**：硬判据——on 臂观测存档评审调用含
  term_provenance 表、机械校验读数行（rows/invalid/retried/degraded）在
  smoke 输出在场；两臂 EXIT 按既有判据不劣化。
- **对照卡**：两臂同 fixture 真机各一轮生长——评审 findings 数量/类型 +
  provenance 行数/来源分布（机械读数）并排；人读抽判：常识行是否可信
  （抽 3 行人工核对 term 真是课程外通用常识）。两臂都跑，落 Resolution。
- `cargo test -p nomifun-learning` 全绿。

## 非目标

- 定义审计/词表资产（m4 盘问）。
- 收敛梯子改造（m5；本票的「重试恰一次→Degraded」是 learnhub 同款局部语义，
  不动外层 3 次预算与指纹早停）。
- #175 三断层（m5 领地——但本票的死因重试消息格式预留 advice 范围措辞）。

## 备注

learnhub 立票缘由（ADR-0033）：评审判据文本逐字在场、flash 仍零发现放行
（三角形批第二次复发）——「判据在场」到「判据被执行」的鸿沟只能靠程序化
闭环填。allo k07 对照卡若出现同形态（fixture 有挂号缺口而评审零发现），
即本票的前置证据。
