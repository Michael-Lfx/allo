---
id: j09
title: "结构修补与 findings：七型 ops + 修补回合"
type: task
status: open
assignee:
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

- [ ] **findings 生产通道**：概念评审门输出扩可选 `findings[]`（warn 级、不阻断的数据
      缺陷观察——与 problems[] 阻断通道并行）→ 落 findings 表（j08 建的表；追加、坏行
      跳过不 Broken；「留痕是线索不是第二真相」）；第一轮生长即开始留痕
- [ ] **七型 ops 应用引擎**（纯函数，零 LLM 零 IO 零时钟）：add_edge/remove_edge/
      flip_edge/add_block/split_block/rename_block/merge_blocks——「ops = 意图，引擎管
      自洽」：任一 op 无法应用 = 整体失败；级联引擎代管（改名跟改/拆块边退场/并块去重
      去自环 + 级联注记记数）
- [ ] **st.ops.* 五条款**进契约文档（`shape/endpoints`（应用序模拟）`/reason/apply`
      （执法在门面：应用后 gd.* 全量重过零放宽）`/no_slug_churn`（rename 键闭禁 slug、
      新 slug 不得复用含退场））+ 互钉测试
- [ ] **修补回合**：触发门槛 = findings 非空 ∪ 挂载观察非空（皆空 = skipped 零 LLM）；
      装配（宪章三项 + 结构快照投影 + findings 行 + mount_observations + retry_feedback）
      → RoundRetry ≤3（j03 骨架）→ ops 空 = noop（LLM 已跑不写盘）→ apply → gd.* 全量
      重过 → 结构快照原子写 → 算 new_hash + `mounts_stale`（本回合不动挂载，下回合
      全量重挂——级联由 hash 重评估单点兜住，不建逐 op 跟迁）→ 审计行落修补审计表
- [ ] 提示词资产 `structure-repair.md`（「结构编辑是低风险维护」「每个操作一条依据」
      「不需要编辑就输出空数组」「编辑会被全量复核——宁可少改、改准」+ 地图自身下限）
- [ ] 命令 `POST /courses/{id}/graph/structure/repair`（j03 注册表接入：kind =
      StructureRepair）+ `StructureRepairSummary{old_hash, new_hash, applied[],
      cascade[], mounts_stale, noop, skipped}` DTO；工作区结构图 Tab 修补入口
- [ ] 结构修改后：结构图 Tab 的 `mounts_stale` 待重评估横条（读侧已有，接线确认）
- [ ] learning-smoke 增 `repair` 子命令：回合成功（skip/noop 照印）+ **盘面复验**（磁盘
      重读独立跑全量 gd.* validator）+ **级联标记**（结构已改时挂载必须判 stale，否则
      EXIT=3；渲染一致判据无——allo 无渲染品）
- [ ] 验证：`cargo test -p nomifun-learning`（七型 ops 逐型应用/级联注记/st.ops 红绿样/
      应用序模拟/no_slug_churn/findings 落表容错）+ 真机冒烟 repair

**Resolution:**（收口时写）
