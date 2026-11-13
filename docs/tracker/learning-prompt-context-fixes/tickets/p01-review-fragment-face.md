---
id: p01
title: "评审碎片面——第四出口条款、清单瘦身（合并渲染/单字过滤）、自证显性化"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# p01: 评审碎片面修复——让模型诚实、让清单瘦身、让禁令生效

**What to build:** 三件互叠的评审碎片面修复，全部有 absorb-8 真机病历支撑：
①碎片裁决条款补「声明面正向提及已覆盖」的处置出口（消除模型被迫用噪声
撒谎/字面执行误打回的两难）；②点名清单瘦身（同词跨源合并渲染 + 非
definition 源单字碎片过滤 + prompt 一行覆盖多源条款——机械核零改动）；
③挂号表自教行循环自证禁令显性化（条款在场但首轮仍犯，重排为自查项 +
负例）。**不加第四裁决值、不放宽任何机械核。**

## 现状（2026-11-05 实码核实，基线 a672c7eb4）

- `learning_graph/mod.rs` 碎片裁决条款（FRAGMENT_OUTPUT_CONTRACT 段，
  ~`:905-917` 区，z02 后扩三值）：
  - 噪声定义 = 「切词碎片或元语言组合（停用词/虚字残留、跨词边界噪声）」
    ——**不覆盖**「声明面正向提及且已被 teaches/已教账覆盖」的词面；
  - 名词 = 「像样的领域名词却无出处——必须 pass=false」；
  - 范围外 = 仅限否定性边界提及 + 三件机械核。
- `learning_graph/fragment_verdict.rs`：
  - `derive_naming_list`（~`:175`）：四源切片经 `grounding_runs`（宁滥勿缺）
    + known 掩码 + `apply_ignores`；**无单字过滤、无跨源合并**；
  - `naming_list_block`（~`:158`）：`{i}. {term} [{source}]` 逐行渲染；
  - `validate_fragments`（~`:218`）：`verdicts.iter().find(|fv| relates
    (&fv.term, &frag.term))` ——**同词一行裁决天然覆盖全部跨源行**（真机
    已隐式验证：响应 127 行对 92 unique terms 也过核）；`FragmentReadout`
    的 fragments 计数 = 清单行数、rows = 响应行数。
- 挂号表条款（mod.rs ~`:895-917` 附近【挂号表 term_provenance】段）：自教行
  禁令原文「ref 指向本条提名自身 = 循环自证，无效（定义预设的词不由定义
  自身供给）；提名的名字本身不算 teaches」——**已在场**。
- smoke grow 臂读数：`[fragments: total=N rows=M unhandled=0 degraded=false]`
  ——N=清单行数、M=响应行数。

## 真机病历（run-1793828585963）

1. **出口缺失**：响应 127 行裁决中 113 噪声/14 范围外。滤杯 [definition]
   note=「通用器物词，且本批首节点scope正面教学覆盖，非缺口」；教辨认滤杯
   [scope] 噪声；流程线/变量线 [intent]（路线词）噪声——**全部与噪声定义
   （切词碎片）矛盾**。模型自创第四范畴「scope 正面教学覆盖」却只能挂在
   噪声名下。字面执行的模型会把滤杯判名词 → pass=false 误杀好批。
2. **清单负担**：127 行 = 单字 38（途/萃/教/粗/中/细/档/足/度…，30%）+
   跨源重复 35（滤纸×3/温度计×3/途×3/咖啡粉×3/粗×3…）；unique terms=92；
   全部标题碎片含整行标题「辨认手冲基础器具 [node_title]」。评审调用
   303s/308s 各一次；范围外 6→14、噪声 121→113 轮间方差。
3. **自证重试**：call2 挂号表行 `{concept:手冲器具功能, term:滤杯,
   source:自教, ref:手冲器具功能}`（ref=concept 自身）被机械核拒 →
   死因重试恰一次 → +303s。禁令条款在场但淹没在 8 行密排文字里。

## 实施要点

### A. 第四出口条款（mod.rs 碎片裁决条款，扩噪声定义）

- [ ] 噪声定义扩写（保持词表三值不动）：在「切词碎片或元语言组合」后追加
  一类——「**声明面正向提及且出处已覆盖**：[node_title]/[intent]/[scope]
  来源的片段若属本节点 teaches（或其别名/定义词面）、已教账、课程外常识
  的词面，或 intent/scope 的角色/路线描述词——判**噪声**放行，note 写明
  出处（如『本节点 teaches 手冲器具功能覆盖』）；这些词不是登记缺口」。
- [ ] 同条款补一行与挂号表的关系：「[definition] 来源的片段若已在
  term_provenance 落行，fragments 行裁噪声并在 note 引该挂号表行（同一
  出处两表共用，不重复找出处）」。
- [ ] substring 断言更新（既有条款覆盖断言测试）。

### B. 清单瘦身（fragment_verdict.rs + 渲染 + prompt 配套）

- [ ] `derive_naming_list`：**非 definition 源（node_title/intent/scope）
  的单字碎片（chars().count()==1）不进清单**（definition 源保持宁滥勿缺
  不动——真机病历显示单字垃圾全部出自声明面切片）；课程豁免
  `apply_ignores` 位置不动。
- [ ] `naming_list_block`：同 term 跨源合并为一行多标签
  `1. 滤纸 [definition][intent][scope]`（保序按首现行）；prompt 配套条款
  「清单中同词多源合并为一行——输出**每个词一行**即可，term 相同的一行
  裁决覆盖它的全部来源行（子串互含亦同）」。
- [ ] `validate_fragments` / `check_out_of_scope_row` **零改动**（relates
  多对一已覆盖；真机 127 行对 92 unique 的过核记录即证明）。
- [ ] `FragmentReadout.fragments` 口径改为合并后行数（readout 语义=
  「机械派生待裁决词数」；smoke 读数行注释同步；rows 口径不变）。
- [ ] 单测：单字过滤三源×三态（definition 单字保留/声明面单字剔除）、
  合并渲染一行多标签、一行覆盖多源的 validate 断言（92→60 级收缩的
  形状断言）。

### C. 自证显性化（mod.rs 挂号表条款重排）

- [ ] 挂号表条款头部加自查式禁令（显眼位）：「**自教行 ref ≠ 本行
  concept**——自己不能证明自己：`{concept:X, term:T, source:自教,
  ref:X}` 恒无效（T 是 X 定义里的词，不由 X 自己供给）；T 的出处必须
  落到既往批次的已教账、本节点 teaches 的**其他**概念、或常识」。负例
  直接嵌入条款（真机病历同形）。
- [ ] 真机病历同形测试（既有 verify 系测试补一条：自教 ref=concept 自身
  的响应被拒 + 重试反馈文案）。

## Non-goals

- 不加第四裁决值（总约束 2）；不动 `check_out_of_scope_row` 三件核。
- 不动 known 掩码/grounding_runs 切片本体（definition 源宁滥勿缺保持）。
- 不做候选集渲染（p03）。

## 验收

1. `cargo test -p nomifun-learning` 全绿 + 新测全过。
2. 真机 grow 臂 EXIT=0：`[fragments: total≈60±20 rows=...] unhandled=0
   degraded=false`——total 相对 127 显著下降；`[brief]`/`[names]`/
   provenance 判据零回归；评审调用耗时不高于基线（预期减半）。
3. 响应审计：噪声行的 note 不再出现与定义矛盾的「scope 覆盖」类话术
   （话术合法化后 note 应写出处而非伪装切词残留）。
4. 真机响应里无「自教 ref=concept 自身」行（显性化生效的观察项，一次跑
   不构成证明，连续两波留观）。

## Resolution（2026-11-05 关票）

- **实现提交**：`8e370fc80`（+`413749c6c` 并集成分支），经 `f3987d126` 并回；评审修复轮补形状钉 `ba742fff9`，经 `44b9fec2a` 并回。
- **验证**：nomifun-learning 859→863→869 全绿（新测：单字过滤三源三态/合并渲染一行多标签/一行覆盖多源 validate 断言/自证同形拒绝与反馈文案/92→60 机制等价形状钉 6 raw→3 unique→3 渲染行）；`validate_fragments`/`check_out_of_scope_row` 字节级零改动（Standards 轴复核确认）。
- **真机（grow run-1793859632428）**：合并多源标签渲染真机在场（`1. 发酵 [definition][node_title][scope]`）、三值裁决分布 噪声103/名词11/范围外7——第四出口与瘦身机制实弹。**fragments total 读数未取得有效值：评审回包撞 r01 存量契约矛盾（validate_review 空节点）被静默降级 PASS**——本票读数验收受 r01 阻塞，以单测+渲染实证为准；r01 关票后补跑 grow 臂读数（后续票）。
- 偏差 1 条（合理）：挂号表关系条款独立成 FRAGMENT_PROVENANCE_RELATION_CLAUSE 仅双面同开时织入（off 臂防泄漏，既有断言当场抓红后条件化）。

### 补跑读数（2026-11-06，随 r01 关票取得）

- r01 修复后真机 grow（全新库、与 absorb-8 基线同目标）取得 p01 读数：机械点名清单**合并后 138 行**（合并前原始来源标签 149，同词跨源合并省 11 行）；残余单字片段 11 行**全部来自 definition 源**（声明面单字已按设计全部滤除，definition 宁滥勿缺保持）；裁决分布 噪声124/范围外11/名词1；噪声行 note 开始引用出处（如「实质名词『可溶风味物质』已在 term_provenance 报缺失」）——第四出口条款与挂号表关系条款被模型实际使用。
- 说明：138 与基线 127 不可直接对比——清单规模随当批声明面文字量变化，本批 intent/scope 平均 78/72 字（基线 40/42）。机制面（合并渲染、声明面单字过滤、一行覆盖多源）真机实弹确认。
