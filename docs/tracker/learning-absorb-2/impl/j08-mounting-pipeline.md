---
id: j08
title: "挂载管线：挂载表 + 维护回合 + 三态投影"
type: task
status: open
assignee:
blocked_by: [j01]
part_of: ../map.md
---

# j08: 挂载管线（结构转活文档·上）

**What to build:** 挂载事实层完整落地——挂载表、st.mount.* 契约、挂载维护回合、三态
读侧投影、smoke-mount。

**实现决议:** 三轮 g1（上）。参考实现：learnhub `engine/maintenance/{mounting,mod}.rs`、
`prompts/maintenance/structure-mount.md`。

**Blocked by:** [j01]（门位口径提示词与契约文档约定）；[j03]（RoundRetry 同源合并——
挂载回合直接用 j03 的重试骨架）。

**Status:** ready-for-agent

- [ ] 迁移：挂载表一课一行（`course_id UNIQUE, mounts_json, structure_hash, updated_at`）
      + findings 表（追加行：`ts/course_id/source/node/field/issue/advice`——j09 消费，
      本票建表）
- [ ] **structure_hash = 手写 FNV-1a 64**（勿用语言默认 hash）对 `structure_json` 规范化
      字节
- [ ] **st.mount.* 六条款**（`st.mount.top.keys / entries.required / entries.align /
      blocks.resolve / blocks.dedup / no_concept_fields`）进契约文档新章 + validator +
      schema 单源互钉（违规报告原文注入重试）
- [ ] **挂载回合**：输入七件（宪章三项 + 结构参考行 + pending_nodes + existing_mounts +
      retry_feedback）；待挂集 = 非归档 ∧ (未评估 ∪ 失效 slug)，stale = 全量；
      **待挂集空 = skipped 零 LLM 零写**；失败整回合重试不降级（RoundRetry ≤3，同错
      早停——j03 骨架复用）；落盘 = 单行原子覆盖（merge upsert：本轮替换、未评估保留、
      孤儿清出）
- [ ] 提示词资产 `structure-mount.md`（头注五字段 + 金样 + 围栏词表 + 契约互钉；
      temperature 0.0 + thinking；「挂载是导航事实」「0 条是合法答案」「slug 逐字」）
- [ ] 命令 `POST /courses/{id}/graph/structure/mount`（j03 注册表接入：kind =
      StructureMount，可取消）+ `MountRoundSummary` DTO
- [ ] **读侧三态投影**：NotEvaluated/Outside/Mounted 单源；悬挂 slug 逐条降级不 Broken；
      `mounts_stale` 标记；`mount_index`（块 slug → 已挂节点清单）；结构图 Tab 节点三态
      段 + 区块已挂清单 + 已挂数徽标（读侧 join 零第二真相）
- [ ] **挂载观察派生**（修补输入源）：结构外单元 + 空块 + 超载块（≥5 节点）——纯读函数
- [ ] learning-smoke 增 `mount` 子命令：落位 + hash 锚定 + 条数一致 + **重跑幂等
      （第二次必须 skipped，否则 EXIT=4）**
- [ ] 验证：`cargo test -p nomifun-learning`（六条款红绿样/待挂集/merge 幂等/三态降级/
      hash 稳定）+ 真机冒烟 mount

**Resolution:**（收口时写）
