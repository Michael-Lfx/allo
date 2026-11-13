# m2 已会声明通道——双臂真机对照卡（开关臂 NOMI_LEARNING_BASIS_DECLARATION）

- 日期：2026-10-27；票：m2（ADR-0032 移植）
- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 两臂同构建同凭据（Flowy Cloud flash）、同 fixture（basis 冒烟内置种子：块甲 door_refs=[G1]、块乙 door_refs=[G1,G2]，门口 G1/G2）、各自全新数据目录

## 两臂命令原文

```
# off 臂（NOMI_LEARNING_BASIS_DECLARATION=off）
NOMI_CHANNEL=dev NOMI_LEARNING_BASIS_DECLARATION=off nomicore.exe --data-dir <data-m2-off> learning-smoke basis --db <data-m2-off>/flowy-backend.db
# on 臂（开关 unset，真管线）
NOMI_CHANNEL=dev nomicore.exe --data-dir <data-m2-on> learning-smoke basis --db <data-m2-on>/flowy-backend.db
```

## 读数行并排（off | on）

| 读数 | off 臂 | on 臂 |
|---|---|---|
| EXIT | **2**（拒绝路径即规格） | **0**（首跑被存档判据中断为 1——判据自身 bug，修复补跑后全绿，见下方「判据修复 + 补跑收尾」） |
| `[1/5]` 进程内 off 拒绝 | `off arm rejected: Conflict: 已会声明通道已关闭（NOMI_LEARNING_BASIS_DECLARATION=off）` | 同左（两臂都先验拒绝路径） |
| declare 读数 | `declare FAILED (no credentials?): Conflict: 已会声明通道已关闭（…=off）` | `declare read-out: doors=["G1", "G2"] blocks=["块甲", "块乙"] minted=2 reused=0 noop=false` |
| 登记表行数 | 0 → 0 | 0 → **2**（两行 `source='basis-declaration'`） |
| 锚行（learning_basis_declarations） | 0 | **1** |
| 节点数（本课 lessons） | 1（控制节点） | **1 → 1（零节点写入，计数守恒 ✅）** |
| 批次（learning_growth_batches） | 0 | **0（零批次写入 ✅）** |
| 声明概念数 | 0 | **2（minted=2 reused=0）** |
| 下游就绪投影翻转 | 未达（off 臂在 [2/5] 如实拒绝退出） | **达成 ✅**（补跑 [3/5][4/5][5/5] 真机走完，读数见下） |

## 人读抽判：声明概念粒度 ✅

铸出两条概念（存档响应逐字）：

- `{"name":"判据一","definition":"本课程中用于判断学习者是否已具备相应前置能力的第一项判定标准。","source":"声明自述"}`
- `{"name":"判据二","definition":"本课程中用于判断学习者是否已具备相应前置能力的第二项判定标准。"…}`

✅ **最小可教单元成立**：块名（块甲/块乙）被当作主题拆开，铸的是两条门口判据各自一条，**没有把块名整体铸成一条**；每条一句话定义、带出处说明（声明自述）。与提示词「一物一名，按最小可教单元；块名是主题、不是概念」的条款一致。

## 如实缺口

1. **缺口①（观测面，已修）**：首跑 on 臂的机械断言 `calls.contains("basis-declaration")` **按实现不可能为真**：任务字面量是观测面注册表键（`PromptSpec::task`），装配（`structure_prompts::assemble`）剥离携带它的头注后只发正文，Archiver 存档行只记方法名与模型面字段（system/user/response），`LearningCompleter` 特征不带任务上下文，声明又是同步端点（不进生成任务注册表，`learning-tasks.jsonl` 的 begin/finish 观测只覆盖长任务）→ on 臂在 [2/5] 末 EXIT=1。**修复（方案 b，系统提示词签名）**：`learning_smoke.rs` 判据改按存档行 `system` 字段逐行计 `DECLARATION_SYSTEM`（常量经 `nomifun-learning` 导出单源，不抄文本）——该立场句为声明调用独有，效果等价于「存档含声明调用」；(a) 方案（Archiver 加 task 槽位）需给 `LearningCompleter` 特征跨 crate 加任务上下文参数（nomifun-learning + nomifun-ai-agent + 全部测试 fake），对一条冒烟判据不成比例，不取。
2. off 臂注册表/锚表/节点/批次四项全零 ✅（零写面）。

## 判据修复 + 补跑收尾（同日，on 臂重跑）

- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 19:36:36**（含判据修复）；数据目录 `rm-matrix/data-m2-on-fix`（fresh 模板 + Nomi-dev 加密键，与矩阵同纪律）
- 命令原文：`NOMI_CHANNEL=dev nomicore.exe --data-dir <data-m2-on-fix> learning-smoke basis --db <data-m2-on-fix>/flowy-backend.db`
- 全读数（`rm-matrix/m2-on-fix-run.log` 逐字）：

```
observation archive: C:/Users/test/Desktop/work/allo-wt/rm-matrix/data-m2-on-fix\observation\learning-smoke\run-1793101018511
[1/5] off arm: switch rejects the declare endpoint…
  off arm rejected: Conflict: 已会声明通道已关闭（NOMI_LEARNING_BASIS_DECLARATION=off）
[2/5] declare G1+G2 (real pipeline)…
declare read-out: doors=["G1", "G2"] blocks=["块甲", "块乙"] minted=2 reused=0 noop=false
archive read-out: declaration_calls=1
[3/5] downstream readiness flips via the declared concept…
[4/5] idempotent rerun (noop, zero LLM)…
rerun read-out: noop=true llm_calls 1->1
[5/5] revoke (anchors deleted, projection restored, concepts kept)…
OK  course=01a17d26-3595-7681-97b1-c78d7e19d32c
EXIT=0
```

- **五条硬判据全过、EXIT=0**：首跑被卡住的 [3/5] 下游就绪翻转、[4/5] 幂等 noop 零 LLM（`llm_calls 1->1`）、[5/5] 撤销复原（锚行清零、投影复原、概念留池）均获真机读数；`archive read-out: declaration_calls=1` = 修复后判据读数（存档含且仅含该次铸造调用，method=`complete_json_with_thinking`，system 与 `DECLARATION_SYSTEM` 全等）。
- 声明读数与首跑逐字一致（doors=[G1,G2] blocks=[块甲,块乙] minted=2 reused=0），铸出概念同为「判据一 / 判据二」两条门口判据各一条——最小可教单元粒度跨跑稳定。

## 产物与存档

- 存档（首跑）：`rm-matrix/data-m2-on/observation/learning-smoke/run-1793098939800/calls.jsonl`
- 存档（补跑）：`rm-matrix/data-m2-on-fix/observation/learning-smoke/run-1793101018511/calls.jsonl`
- 日志：`rm-matrix/m2-off-run.log`、`rm-matrix/m2-on-run.log`、`rm-matrix/m2-on-fix-run.log`
