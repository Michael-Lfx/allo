---
id: j08
title: "挂载管线：挂载表 + 维护回合 + 三态投影"
type: task
status: closed
assignee: zhangyj
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

- [x] 迁移：挂载表一课一行（`course_id UNIQUE, mounts_json, structure_hash, updated_at`）
      + findings 表（追加行：`ts/course_id/source/node/field/issue/advice`——j09 消费，
      本票建表）
- [x] **structure_hash = 手写 FNV-1a 64**（勿用语言默认 hash）对 `structure_json` 规范化
      字节
- [x] **st.mount.* 六条款**（`st.mount.top.keys / entries.required / entries.align /
      blocks.resolve / blocks.dedup / no_concept_fields`）进契约文档新章 + validator +
      schema 单源互钉（违规报告原文注入重试）
- [x] **挂载回合**：输入七件（宪章三项 + 结构参考行 + pending_nodes + existing_mounts +
      retry_feedback）；待挂集 = 非归档 ∧ (未评估 ∪ 失效 slug)，stale = 全量；
      **待挂集空 = skipped 零 LLM 零写**；失败整回合重试不降级（RoundRetry ≤3，同错
      早停——j03 骨架复用）；落盘 = 单行原子覆盖（merge upsert：本轮替换、未评估保留、
      孤儿清出）
- [x] 提示词资产 `structure-mount.md`（头注五字段 + 金样 + 围栏词表 + 契约互钉；
      temperature 0.0 + thinking；「挂载是导航事实」「0 条是合法答案」「slug 逐字」）
- [x] 命令 `POST /courses/{id}/graph/structure/mount`（j03 注册表接入：kind =
      StructureMount，可取消）+ `MountRoundSummary` DTO
- [x] **读侧三态投影**：NotEvaluated/Outside/Mounted 单源；悬挂 slug 逐条降级不 Broken；
      `mounts_stale` 标记；`mount_index`（块 slug → 已挂节点清单）；结构图 Tab 节点三态
      段 + 区块已挂清单 + 已挂数徽标（读侧 join 零第二真相）
- [x] **挂载观察派生**（修补输入源）：结构外单元 + 空块 + 超载块（≥5 节点）——纯读函数
- [x] learning-smoke 增 `mount` 子命令：落位 + hash 锚定 + 条数一致 + **重跑幂等
      （第二次必须 skipped，否则 EXIT=4）**
- [x] 验证：`cargo test -p nomifun-learning`（六条款红绿样/待挂集/merge 幂等/三态降级/
      hash 稳定）+ 真机冒烟 mount

**Resolution:**

**速览**：挂载事实层全链落地——迁移 079（挂载表 + findings 表，v3 契约全量注册，
表计数 115→117）、FNV-1a 64 手写 hash、st.mount.* 六条款（契约 §9 新章 ↔ validator
↔ 提示词三互钉）、挂载回合（单 LLM 调用 + RoundRetry + merge upsert）、三态投影 +
mount_index + 观察派生、结构图 Tab 挂载段、smoke mount（重跑幂等 EXIT=4 判据）。

1. **迁移 079**：`learning_structure_mounts`（course_id UNIQUE + mounts_json 裸数组
   CHECK + structure_hash + updated_at）与 `learning_structure_findings`（追加只增，
   source ∈ concept-review/mount-observation，j09 接生产与消费）双表 + 具名索引；
   v3 注册（PRODUCT_TABLES/逻辑引用 course_id，挂载 Cascade、findings KeepHistory，
   表计数 +2）。
2. **FNV-1a 64**（`mounting::fnv1a_64`，手写 + 公开测试向量）：对 structure_json 列
   **原样字节**锚定；导出 `nomifun_learning::learning_graph_hash` 供冒烟复算。
3. **挂载产物形态**：`{ mounts: [{node, block, reason}], note? }`——节点级条目，
   `block: null` = 已评估、声明结构外；存储列为裸数组（列 CHECK 同形），
   `parse_mount_entries` 对象/数组双读、坏条目跳过不 Broken。
4. **六条款验证器**（`validate_mounts`，全红——违规报告原文注入重试）：红绿样 +
   互钉测试（条款码 ↔ 契约 §9 文档 include_str 比对）。条款与.learnhub 命名一一对应
   （tickets 票面原码）。
5. **挂载回合**（`run_mount_round` 零 IO 纯 LLM 面 + `run_structure_mount` 服务编排）：
   七件输入装配 STRUCTURE_MOUNT 资产（temperature 0.0）；契约门全绿才解析条目；LLM
   错误（无凭据/网络）与条款违约分报——前者 Conflict「生成通道不可用，挂载表未改动」
   （配置问题不因重试收敛，不伪装成契约失败），后者 UnprocessableEntity 带逐条款
   报告；同指纹早停（j03 normalize_report/should_stop_early 复用）+ CancellationCompleter
   真取消（循环头 + 写前双检查）。**待挂集空 = skipped**：零 LLM 零写、挂载行不动。
6. **merge upsert**：本轮替换（pending 节点以本轮为准）、未评估保留、孤儿清出（节点
   不存在或块 slug 失效）；`INSERT … ON CONFLICT(course_id) DO UPDATE` 单行原子。
7. **读侧三态**（`structure_view::build_mounts_view` 纯函数，单源）：Mounted（块可
   解析）/ Outside（block null）/ NotEvaluated（未评估或悬挂降级）；`mount_index` =
   slug → 节点清单（只含可解析块，结构顺序）；`stale` = 存储 hash ≠ 当前 hash；
   挂载观察派生（结构外单元/空块/超载块 ≥5——纯读不打点）。`structure_view` 服务
   读扩展：节点标题 + 挂载行 + 当前 hash 一次行读，`LearningStructureView.mounts`
   可空（从未跑过回合）。
8. **命令与前端**：`POST /courses/{id}/graph/structure/mount`（provider/model 可选
   成对；同步返回 `MountRoundSummary`：status/pending/mounted/outside/total/stale/
   hash）；kind = `structure_mount` 入 j03 注册表（slot 锁 + 可取消 + WS
   structure_mount_started/completed/cancelled/failed 事件）；结构图 Tab 挂载段 =
   三态计数徽标 + stale 显影 + 已挂清单（块 → 节点串）+ 观察标签 + 运行挂载按钮
   （i18n 双语 +9 键）；types.ts/api.ts 镜像同步。
9. **smoke `mount`**：从零库立结构快照课程（两块 + 三节点）→ 真实回合 → 机械判据
   ①mounted 且三节点全评估 ②挂载行 hash = FNV-1a(结构字节) ③条数一致 → 重跑必须
   skipped（否则 **EXIT=4**）且挂载行未被触碰；无凭据临时库如实非零退出（本会话真
   机验证至 provider 解析，LLM 腿需凭据——structure 冒烟同纪律）。
10. **测试**（+16）：六条款红绿、FNV 公开向量、待挂集（未评估∪失效 slug）、merge
    （替换/保留/清出）、观察三形、parse 容错、条款码 ↔ 契约文档互钉、提示词三闸
    （金样/围栏/条款覆盖扩 mount 短语）、三态投影（含 stale）、服务层挂载→幂等
    skip→stale 显影全链。
11. **验证**：learning 299 全绿、db 全套绿（079 + schema contract 23）、ai-agent 不受
    触、`cargo check --workspace` 干净、`cargo fmt` 过、typecheck 60 = 存量基线
    （learning 零新增）、check:i18n 过（11389 键）、smoke mount 真机跑至 provider
    解析（无凭据如实 Conflict 非零退出）。

**Resolution 增补（2026-10-16，对账修复 76c2bca8d 后续）：** learnhub 实码对账后条目
形态从单块（`block: <slug>|null`）改为**多块数组**（`blocks: [<slug>…]`，空 = 结构外；
learnhub `MountEntry.blocks` 明文无唯一性限制，且引导偏向为「拿不准时宁可少挂——挂
错比挂空更有害」，已同步翻转提示词）；`entries.align` 补缺失半边（待挂集每节点恰一
条）。存储三读兼容（新数组 / j08 单块 / null）。契约 §9.2 已同步改写。
