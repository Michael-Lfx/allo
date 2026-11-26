---
id: m2
title: 让修补真正跑起来——定时臂 bug + 修补后自动重挂
type: task
status: closed
assignee:
blocked_by: [m1]
part_of: map.md
---

## Question

修补回合（ADR-0012 / j09）在代码里完整存在，但**三条触发路径全死**：定时臂
有 bug 永不触发、手动按钮从没人点过（动作簿零行）、sb2 的唤醒只挂载不修补。
结果 16 条 findings 从未被任何消费者读过。

## 已核验事实

- **W1**：`nomifun-app-server/src/lib.rs` 的调度循环里，`last_run` 在**每个
  30 秒 tick 之后**被无条件重置，而到期判断要求 `elapsed() >= 间隔`（下限
  30 分钟）——任何运行时长下都不成立。定时维护自上线从未运行。
- 修补输出只有 7 种结构 op（`add_edge / remove_edge / flip_edge / add_block /
  split_block / rename_block / merge_blocks`）；不能新增节点、不能改别名。
- 修补应用 = 结构 hash 变 = **全部挂载失效**（`mounts_stale`）。现状没有
  「修补后重挂」的链路：只能手动点「重新检查」，或靠下一次生长唤醒。
- 水位 = **已读**（applied/noop 都推进）。读过的 findings 不会重复触发——
  定时臂跑起来不会空转燃烧。
- sb2 已有唤醒通道：`take_growth_wakes` / `growth_wake_notified` /
  `spawn_learning_growth_wakes`（仅挂载回合）。

## R

- **R-a 修定时臂**：`last_run` 只在**定时臂真的跑完一轮**时重置（把重置挪进
  「到期并执行」分支）。唤醒分支维持现状（不碰 `last_run`、不刷新节拍）。
  失败/跳过按既有语义（失败也重置，防热循环）。**这是 bug 修复，不是行为
  设计变更**；但它是定时维护第一次真正运行，须走 R-d 的验证顺序。
- **R-b 修补后自动重挂**：`run_structure_repair_with_trigger` 在
  `applied`（**不含 noop**）后，登记一次仅挂载唤醒（复用 sb2 通道）。noop
  不重挂（结构没变，挂载没失效）。失败/取消不登记。
- **R-c 修补输入不变**：仍读「水位之后的 findings + 挂载观察」。信任层已经
  有全量复核与「repair 永不删 historical」两条护栏；不在本票扩它的输入契约。
- **R-d 验证顺序（硬性）**：
  1. 先手动 POST `structure/repair` 跑一次本课（16 条 findings 在场），
     检查产出的 ops 是否只动结构、是否通过全量复核、结构 hash 是否变化；
  2. 若 ops 为空（noop）：接受，说明这 16 条不是结构问题（与 map 的判断
     一致：概念级缺口修补改不了）；
  3. 只有 1/2 都符合预期，才把定时臂的修复留给后续真机自然运行观察。
- **R-e 读数**：修补回合的 `applied/noop/skipped`、op 条数、结构 hash 前后
  值、触发的重挂是否成功——落动作簿与日志（现有通道，不新建）。

## Acceptance

- 单测：
  ① 定时臂到点后跑且 `last_run` 推进；未到点不跑且 `last_run` **不变**
  （W1 的正面钉：把旧行为写成会红的断言）；
  ② 唤醒分支不改变 `last_run`（现有语义回归）；
  ③ 修补 `applied` → 登记一次仅挂载唤醒；`noop` → 不登记；失败 → 不登记。
- `cargo test -p nomifun-learning` 全量绿 + `cargo test -p nomifun-app-server`
  + `cargo check --workspace`。
- **真机**（冻结二进制 + dev 库）：
  1. 手动跑一次修补，记录 ops / hash / 动作簿行；
  2. 若 applied：观察自动重挂是否在数秒内完成、挂载表是否恢复一致；
  3. 若 noop：如实记录，并在 Resolution 写明「这 16 条不是结构问题」。
- 读数进 Resolution。

## 回退条件（负优化判定）

- 定时修补在生产把关过的批次改坏（结构 ops 应用后全量复核仍放过明显错误）
  → 回退 R-a（恢复 `last_run` 每 tick 重置），定时臂退回不运行；m1 不受影响。
- 修补后重挂失败或把新节点挂错块 → 回退 R-b，重挂退回手动。
- 定时臂跑起来后出现「每 6 小时空转烧一次 LLM」→ 检查水位是否被正确推进
  （若 noop 未推进会重复触发），修水位后重试。

## 范围外

- 修补的输入契约扩展（例如把手动「移出本批」的节点回收进结构）。
- 修补 op 的 dry-run 模式（先把 ops 给用户看再应用）——若 R-d 第 1 步发现
  ops 质量不可接受，另立票做 dry-run。

## Resolution

**真机验证通过**（2026-11-26）。定时臂到点运行、修补 `applied` 后自动重挂，
两段都在真二进制上观察到（库快照）；真库手跑一次 = noop，水位第一次推进。

### 落地

- **提交** 093100705。15 文件 +1719/−92；新模块 `service/repair_followup.rs`
  （153 行）。
- **R-a 定时臂（W1）**：生产循环抽成 `learning_maintenance_loop(wait, on_wake,
  settings, timer_tick)`——四件外部依赖经闭包注入，测试在 tokio 暂停时钟下驱动的
  就是**这一个**循环（此前循环只能 spawn 真服务、测试只能另抄一份循环体，W1 因此
  漏网）。节拍收进 `MaintenanceCadence`：`last_run` 只在类型内部推进，到期时先记
  「到点时刻」并发出 `TickPermit`，回合结束才凭凭证记「完成时刻」——循环体里再没有
  一处能顺手改写节拍。
- **R-b 修补后重挂**：独立意图 `RepairFollowupWake` + 独立登记表，共用 sb2 的
  `Notify` 与调度器的同一唤醒分支；登记点在 `applied` 的 Done 账本行之后、水位推进
  之前（水位写失败也不能让改过的结构没有重挂）。执行前四道复核，各有 info 日志：
  自动维护运行期关 / 不在维护面 / **从未挂载**（无挂载行）/ 已 fresh 且无待挂。
  不设待挂阈值、不去抖。
- **R-c**：修补输入契约未动。
- **R-e 读数**：`applied` 的 Done 行 basis 追加 `ops={n} stale={0|1}`；每个 Ok
  出口一行 `structure repair round finished`（`trigger / status / ops / old_hash /
  new_hash / mounts_stale`）；重挂任务体留 finished / failed / panicked 与
  not-started（带 reason）日志。
- 触发源 `repair_followup`：Rust 枚举 + 前端 `systemActions.ts` 映射 + zh/en
  「修补后重挂」+ 新增 `systemActionsLocales.test.ts`（此前 `ui/` 里没有系统动作
  键的双语断言）。

### 偏离票面（读码取证后由执行方裁决）

| # | 票面 | 实际 | 理由 |
| --- | --- | --- | --- |
| 1 | R-b「登记一次仅挂载唤醒（复用 sb2 通道）」 | 复用通道**设施**（同一个 `Notify` + 调度器同一唤醒分支），但给修补后重挂独立意图、独立登记表、独立准入，不受 stale / 待挂阈值 / 去抖三道闸约束 | sb2 的准入**第一道就拒挂载失效**（`growth_wake.rs` 的 `growth_wake_verdict`，ADR-0018 决定 4，三条测试钉着）；修补 `applied` 后挂载必然失效——原样复用等于空操作 |
| 2 | 已核验事实「或靠下一次生长唤醒」 | 不成立 | 修补后的生长唤醒同样被判 stale 拒绝 |
| 3 | 自动重挂的范围 | 从未挂载（无挂载行）不重挂 | ADR-0018「首次对齐走手动」的边界保留 |
| 4 | R-e「落动作簿与日志（现有通道，不新建）」 | op 条数与 hash 此前不在账本 / 结果日志里：给 Done 行 basis 追加读数 + 新增结果级 info 行 | 现有通道里没有这些读数可读 |
| 5 | R-a 未分类的三条早退 | 运行期关 → 不推进；属主解析失败 / id 非法 → 算尝试过（推进） | 「到期并尝试过就推进，不论结果」；关了很久再打开，下一个 tick 就跑 |
| 6 | 票面外追加 | 结构图页签的结构视图监听修补 / 挂载**终态帧**重取（B7） | 定时臂复活后，后台回合会让页签停在修补前的图与过期的「待重新检查」横幅上 |
| 7 | 循环 panic 兜底的位置 | 兜底放在循环里，不在生产的回合闭包里；panic 日志尾句改为「the next attempt waits one interval」 | 测试驱动的是真循环，兜底放在闭包里等于只测自己的闭包；旧句「retries next tick」在新节拍下不真 |

边界例外的立册见 ADR-0019。

### 检查

- 过滤测试：`cargo test -p nomifun-learning repair_followup` **10/10**；
  `cargo test -p nomifun-app-server` 全量 **203/0**（基线 196 − 1 条循环复刻 + 8）。
- 全量 `cargo test -p nomifun-learning` **1094/0**（基线 1081 + 13）。
- `cargo check --workspace` 通过；`bun run typecheck` 通过；`bun run check:i18n`
  通过（12,589 键）；9 个 UI 测试文件 **77/0**。
- **W1 的正面钉**：先只把循环原样抽出（节拍语义不动），跑
  `timer_arm_runs_only_when_due_and_never_before` → 红（3 小时 15 分虚拟时间里
  零运行，「last_run 又被每个 tick 无条件重置」）；修节拍后转绿。
- 另四条循环级测试：唤醒不移定时节拍、运行期关后重开即跑、回合 panic 不热循环、
  改间隔下一拍生效。

### 真机读数

**A. 修补回合的采样分布**（同一门课、同一结构；每个独立样本 = 新快照 + 提示词
逐字不同，避开服务端提示词缓存）

| 批次 | 二进制 | 模型 | 独立样本 | applied | noop |
| --- | --- | --- | --- | --- | --- |
| 彩排 a/c/f/g | m1 | v4-flash | 4 | 1 | 3 |
| 彩排 d/e/h | m1 | v4.1-flash | 3 | 0 | 3 |
| 采样 N=11/12/13 | m2 | v4-flash | 3 | 1 | 2 |
| 页面实时观察 | m2 | v4-flash | 1 | 0 | 1 |
| 定时臂观察 | m2 | v4.1-flash（缺省） | 1 | 1 | 0 |
| **合计** | | | **12** | **3** | **9** |

- 耗时 4.6–75.9 秒；思考 token 958–10,470。
- 三次 `applied` 都是纯加法（只动结构，均过全量复核）：样本 c 加 1 条
  `contrasts|静态检查|类型错误识别`；样本 N=13 加块「容器」「返回类型」+ 2 条
  assumes 边；定时臂那次加块「容器」+ 1 条 assumes 边。后两次的理由都引用留痕 #13
  ——修补对「前置底座缺口」的结构回应是**补结构块**。

**B. 真库手跑修补**（2026-11-26 01:10 本地；m2 二进制；模型 v4-flash）

| 项 | 读数 |
| --- | --- |
| 结果 | `noop`，`applied: []`，hash 前后同为 `717843304089389345` |
| 性质 | **服务端缓存回放**：提示词与彩排样本 a 逐字相同，用量指纹 4934/1680/1674、1.4 秒。对真库真实生效，但**不是一次独立推理**（真库数据不为换提示词而改动） |
| 水位 | `learning_maintenance_state` 无行 → `findings_watermark=16`（修补回合第一次在真库消费留痕） |
| 结构 / 挂载 / 账本 | 均无变化（noop 零账本行、零审计行） |
| 结果日志 | `structure repair round finished · trigger=manual · status=noop · ops=0 · mounts_stale=false` |
| 自动重挂 | 按设计未触发（noop 不登记） |
| 运行前备份 | 仓外 `allo-m-notes/db-backup/20261126-pre-m2-run/`（quick_check ok） |

**C. 修补后自动重挂闭环**（m2 二进制；快照；2026-11-26 01:12 UTC）

| 时刻 (UTC) | 事件 |
| --- | --- |
| 01:12:45.870 | 修补调用完成（15.9 秒），4 条 op |
| 01:12:45.894 | 账本 `maintenance_repair · manual · done · basis=unconsumed=16 observations=36 ops=4 stale=1` |
| 01:12:45.897 | `structure repair followup remount registered` |
| 01:12:45.902 | `structure repair round finished · trigger=manual · applied · ops=4` |
| 01:12:58.131 | 挂载调用完成（12.2 秒）——登记后同一秒即被唤醒启动 |
| 01:12:58.150 | 账本 `maintenance_mount · repair_followup · done · basis=pending_mount=32 stale=true` |
| 01:12:58.154 | `learning repair followup mount round finished · status=mounted pending=32 mounted=32` |

- 修补 Done 行 → 重挂 Done 行：**12.3 秒**。挂载表 hash `-4779015556844282879`
  （= 修补审计行的 new_hash），32 条——恢复一致。
- 结构对比：块 63→65、边 76→78，无删除无改写。

**D. 定时臂**

- **夜间的真库**（m2 服务 01:08→09:01 连续运行约 7 小时 53 分，跨过缺省 360
  分钟间隔）：全库账本零新行、挂载表零更新。该库只有一门启用的图课程，其留痕已被
  手跑消费、挂载 fresh → 到点回合为**空转**；空转按设计不落账本、不打结果日志 →
  **无法从留痕判定是否到点**。
- **可控的正面验证**（快照：间隔设下限 30 分钟、水位归零、采样标记 9；m2 二进制，
  缺省模型）：

| 时刻 (UTC) | 事件 |
| --- | --- |
| 01:04:34 | 服务就绪 |
| ≈01:34:35 | 到点（就绪后 **+1801 秒** = 30 分钟 + 1 秒），定时臂启动一轮维护 |
| 01:35:06 | 修补首稿被条款门打回：`[st.ops.reason] ops[0](add_block) 缺 reason——每个操作一条依据`（30.5 秒） |
| 01:35:25.309 | 带反馈重试完成（19.2 秒），2 条带依据的 op |
| 01:35:25.333 | 账本 `maintenance_repair · **timer** · done · basis=unconsumed=16 observations=36 ops=2 stale=1`（**有史以来第一行定时修补**） |
| 01:35:25.335 | `structure repair followup remount registered` |
| 01:35:39 | 定时臂内联挂载的首稿被契约门打回：`[st.mount.top.keys] 未知顶层键「type」`（14.0 秒） |
| 01:36:21.341 | 重试完成（41.9 秒） |
| 01:36:21.364 | 账本 `maintenance_mount · timer · done · basis=pending_mount=32 stale=true` |
| 01:36:21.413 | `learning auto maintenance tick scanned=1 idle=0 worked=1` |
| 01:36:21.554 | `repair followup remount round not started reason="nothing_to_remount"` |

  - op：`add_block 容器` + `add_edge 迭代器协议 -[assumes]-> 容器`。
  - 挂载表 hash `-9202531790991277596`（= new_hash），32 条；水位 16。
  - 到点 → 收尾 1 分 46 秒（修补 50 秒 + 内联挂载 56 秒）。回合内登记的重挂意图在
    回合结束后被取走，复核为无事可重挂，**不重复调用**——与票面「定时臂跑起来后不
    会空转燃烧」一致。

**E. 页面实时观察**（m2 带页面二进制；快照；同一份文档、全程未刷新）

| 时点 | 读数 |
| --- | --- |
| 修补前 | 评审留痕展开 16 行；标题标签「最近 16 条 / 修补未读 16」；「修补已读」0 个；挂载卡「已对应 32 个，待检查 0 个」 |
| 修补结束瞬间（01:06:18.256，noop，28.9 秒） | 页面自行发出 `…/graph/structure`（B7）与 `…/graph/structure/findings`（m1 的 WS 重取） |
| 修补后 | 标题标签只剩「最近 16 条」；「修补已读」16 个；挂载卡不变 |

### 边界

- **空转的定时 tick 零留痕**：日志只在有课干活时打一行。定时臂是否到点在「全库
  空转」时不可观测（D 的第一段即此）——评审修复批候选：到点即打一行
  `scanned / idle / worked`。
- B7（结构视图跟随后台终态帧）没有专项单测（不在预先约定的缝上；m1 的工作区测试
  不模拟 WS 帧）——真机页面观察（E）补上。
- app-server 侧的日志行（重挂 finished / failed / panicked、调度器 panic 行）未被
  测试断言；service 侧结果行与未启动原因有断言。
- `start_learning_auto_maintenance` 本身（环境闸、每进程一枚、spawn）仍无直接测试。
- 新标签「修补后重挂」未做双主题目验。
- dev 库里有 24 个 `learning_growth_batches` 行引用已不存在的课时（历史批次孤儿，
  服务启动时报 `logical-reference orphans present`）——与本波无关，留作观察项。
