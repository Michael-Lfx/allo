---
id: j09
title: "结构修补与 findings：七型 ops + 修补回合"
type: task
status: closed
assignee: zhangyj
blocked_by: [j04, j08]
part_of: ../map.md
---

# j09: 结构修补与 findings（结构转活文档·下）

**What to build:** 结构从快照转活文档的另一半——findings 生产通道、七型 ops 应用引擎、
st.ops.* 契约、修补回合与级联、smoke-repair。

**实现决议:** 三轮 g1（下）。参考实现：learnhub `engine/maintenance/{ops,findings,repair}.rs`、
`prompts/maintenance/structure-repair.md`。

**Blocked by:** [j04]（findings 叠在评审门 schema 上——评审门在 j04 扩过裁决面）；
[j08]（挂载观察/级联的消费对象）。

**Status:** ready-for-agent

- [x] **findings 生产通道**：概念评审门输出扩可选 `findings[]`（warn 级、不阻断的数据
      缺陷观察——与 problems[] 阻断通道并行）→ 落 findings 表（j08 建的表；追加、坏行
      跳过不 Broken；「留痕是线索不是第二真相」）；第一轮生长即开始留痕
- [x] **七型 ops 应用引擎**（纯函数，零 LLM 零 IO 零时钟）：add_edge/remove_edge/
      flip_edge/add_block/split_block/rename_block/merge_blocks——「ops = 意图，引擎管
      自洽」：任一 op 无法应用 = 整体失败；级联引擎代管（改名跟改/拆块边退场/并块去重
      去自环 + 级联注记记数）
- [x] **st.ops.* 五条款**进契约文档（`shape/endpoints`（应用序模拟）`/reason/apply`
      （执法在门面：应用后 gd.* 全量重过零放宽）`/no_slug_churn`（rename 键闭禁 slug、
      新 slug 不得复用含退场））+ 互钉测试
- [x] **修补回合**：触发门槛 = findings 非空 ∪ 挂载观察非空（皆空 = skipped 零 LLM）；
      装配（宪章三项 + 结构快照投影 + findings 行 + mount_observations + retry_feedback）
      → RoundRetry ≤3（j03 骨架）→ ops 空 = noop（LLM 已跑不写盘）→ apply → gd.* 全量
      重过 → 结构快照原子写 → 算 new_hash + `mounts_stale`（本回合不动挂载，下回合
      全量重挂——级联由 hash 重评估单点兜住，不建逐 op 跟迁）→ 审计行落修补审计表
- [x] 提示词资产 `structure-repair.md`（「结构编辑是低风险维护」「每个操作一条依据」
      「不需要编辑就输出空数组」「编辑会被全量复核——宁可少改、改准」+ 地图自身下限）
- [x] 命令 `POST /courses/{id}/graph/structure/repair`（j03 注册表接入：kind =
      StructureRepair）+ `StructureRepairSummary{old_hash, new_hash, applied[],
      cascade[], mounts_stale, noop, skipped}` DTO；工作区结构图 Tab 修补入口
- [x] 结构修改后：结构图 Tab 的 `mounts_stale` 待重评估横条（读侧已有，接线确认）
- [x] learning-smoke 增 `repair` 子命令：回合成功（skip/noop 照印）+ **盘面复验**（磁盘
      重读独立跑全量 gd.* validator）+ **级联标记**（结构已改时挂载必须判 stale，否则
      EXIT=3；渲染一致判据无——allo 无渲染品）
- [x] 验证：`cargo test -p nomifun-learning`（七型 ops 逐型应用/级联注记/st.ops 红绿样/
      应用序模拟/no_slug_churn/findings 落表容错）+ 真机冒烟 repair

**Resolution:**

**速览**：结构转活文档闭环——findings warn 通道（评审门可选键 → 落表）、七型 ops 纯
函数引擎（级联代管 + 级联注记）、st.ops.* 五条款（契约 §10 新章 ↔ validator ↔ 提示词
三互钉）、修补回合（触发门槛/skipped/noop/applied 三态 + gd.* 全量重过 + 审计表）、
smoke repair（盘面复验 + 级联标记 EXIT=3）。结构故事完整闭环：挂载（j08）+ 修补
（j09）+ findings 回流。

1. **迁移 080**：`learning_structure_repairs` 修补审计表（一回合一行：old/new hash、
   result ∈ applied/noop、ops_json 逐 op 数组含 reason、级联注记在 note；skipped 零
   LLM 不落行）+ 具名索引；v3 注册（表计数 117→118，course_id → courses KeepHistory
   ——出处不追删）。**实施注记**：v3 schema 测试 grep 全 schema 禁词（CASCADE 等），
   迁移注释亦在盘内——注释措辞避让禁词（一个注释词触发的教训）。
2. **findings 生产通道**：`ng.review.findings` 可选键（`[{node?, field?, issue, advice?}]`
   ——issue 非空是唯一硬形状）进 `validate_review` + 评审系统提示词【findings——数据
   缺陷留痕】段（与 problems 并行：该打回的仍走 problems，拿不准的观察放 findings）；
   `ReviewOutcome.findings` 随 verdict 解析（reject/pass 都可携带）；生长循环在评审
   后逐条落 `learning_structure_findings`（source = concept-review，issue 空跳过 warn、
   插入失败 warn 不阻断生长）——**第一轮生长即开始留痕**。
3. **七型 ops 引擎**（`structure_ops.rs` 纯函数）：`StructureOp` 七型解析（严格形状，
   坏条目报清单不静默）；`apply_ops` 依序应用、任一失败整体失败——add_edge（同三元组
   已存在拒）/remove_edge（唯一命中，多命中拒盲删）/flip_edge（端点对调 type/tier/note
   保持）/add_block（名冲突拒）/split_block（旧块退场 + **随块边一并退场**——需要的关系
   由加边补回）/rename_block（slug 终身不变 + 边端点跟改记数）/merge_blocks（端点改指
   新块 + 同三元组去重 + 自环退场）——级联注记逐条记数进 cascade[]。
4. **st.ops.* 五条款**（`validate_ops`，应用序模拟——块名集与 slug 集（含退场）随 op
   推进）：shape（顶层 {ops} 可空 + 七型词表 + 逐型必填）/endpoints（边 ops 两端 ∈ 该
   op 时刻块名集；rename 的 to 是本 op 引入的新名不算端点引用）/reason（无依据的编辑
   不做）/no_slug_churn（rename 键闭禁 slug——raw 键回查点名 + 新 slug 不得复用当前含
   退场）/apply（执法在门面，无静态代码）。契约文档 §10 新章（七型表 + 五条款 + 回合
   形态 + ADR-0017 级联口径）↔ validator 互钉测试。
5. **修补回合**（`run_repair_round` 零 IO LLM 面 + `run_structure_repair` 服务编排）：
   - 触发门槛 = findings 行非空 ∪ 挂载观察非空；**皆空 = skipped（零 LLM 零写、审计
     无行）**。**挂载观察只在挂载回合已跑过时派生**（无挂载行的课程没有挂载事实，
     「空块」无从谈起——否则修补触发门槛被未挂载课程常驻点燃，测试钉住）。
   - 装配七件（宪章三项 + 结构快照投影（块参考行 + 边表）/findings 行/挂载观察/
     retry_feedback）→ RoundRetry ≤3 同指纹早停 + 取消双检查；LLM 错误报「生成通道
     不可用」不与条款违约混报。
   - **ops 空 = noop**：LLM 已跑不写盘；审计行落 result='noop'（结论合法：评估过、
     本轮无修补动作）。
   - applied：apply_ops → gd.* 全量重过（validate_structure + reconcile_doors，零放
     宽，任一红拒整回合）→ 结构快照原子写 → 审计行（old/new hash + 逐 op 标签与
     reason + 级联注记）。**本回合不动挂载**：mounts_stale = old ≠ new hash（读侧
     hash 比对单点兜住，不建逐 op 跟迁）。
6. **命令与前端**：`POST /courses/{id}/graph/structure/repair`（provider/model 可选
   成对；kind = structure_repair 入 j03 注册表 + WS structure_repair_* 事件）+
   `StructureRepairSummary{status, old_hash, new_hash, applied[], cascade[], mounts_stale,
   noop, skipped}`；结构图 Tab 挂载段补「运行修补」按钮（noop 弹本轮无需修补）+
   mounts_stale 待重评估横条（j08 已有 stale 标记，接线确认）；i18n 双语 +4 键。
7. **smoke `repair`**：种子课程 + 一条 findings 留痕 → 真实回合（skip/noop 照印）→
   **盘面复验**（磁盘重读信封独立跑全量 gd.* validator + 门口对账）→ **级联标记**
   （applied 而 hash 未变 = EXIT=3；结构已改时 structure_view 判 stale 断言）。
8. **测试**（+18）：七型逐型应用与级联注记、remove/flip 无命中拒、st.ops 红绿样、
   应用序模拟（add_block 后引用合法/直接引用越界）、no_slug_churn（复用退场 slug 红条）、
   ops 空合法、条款码 ↔ 契约互钉、提示词三闸（金样/围栏/条款覆盖扩 repair 短语）、
   服务层 skipped 零 LLM + 触发门槛。
9. **验证**：learning 310 全绿、db 全套绿（080 + schema contract 23——禁词注释教训
   见上）、`cargo check --workspace` 干净、`cargo fmt` 过、typecheck 60 = 存量基线
   （learning 零新增）、check:i18n 过（11393 键）、学习页单测 19 过、smoke repair
   真机跑至 provider 解析（无凭据如实 Conflict 非零退出）。
