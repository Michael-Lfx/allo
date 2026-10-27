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
| EXIT | **2**（拒绝路径即规格） | **1**（见下方「如实缺口②」——冒烟判据 bug，非管线失败） |
| `[1/5]` 进程内 off 拒绝 | `off arm rejected: Conflict: 已会声明通道已关闭（NOMI_LEARNING_BASIS_DECLARATION=off）` | 同左（两臂都先验拒绝路径） |
| declare 读数 | `declare FAILED (no credentials?): Conflict: 已会声明通道已关闭（…=off）` | `declare read-out: doors=["G1", "G2"] blocks=["块甲", "块乙"] minted=2 reused=0 noop=false` |
| 登记表行数 | 0 → 0 | 0 → **2**（两行 `source='basis-declaration'`） |
| 锚行（learning_basis_declarations） | 0 | **1** |
| 节点数（本课 lessons） | 1（控制节点） | **1 → 1（零节点写入，计数守恒 ✅）** |
| 批次（learning_growth_batches） | 0 | **0（零批次写入 ✅）** |
| 声明概念数 | 0 | **2（minted=2 reused=0）** |
| 下游就绪投影翻转 | 未达（off 臂在 [2/5] 如实拒绝退出） | **未达**（见缺口②：冒烟在 [2/5] 末被存档判据中断，[3/5][4/5][5/5] 未执行） |

## 人读抽判：声明概念粒度 ✅

铸出两条概念（存档响应逐字）：

- `{"name":"判据一","definition":"本课程中用于判断学习者是否已具备相应前置能力的第一项判定标准。","source":"声明自述"}`
- `{"name":"判据二","definition":"本课程中用于判断学习者是否已具备相应前置能力的第二项判定标准。"…}`

✅ **最小可教单元成立**：块名（块甲/块乙）被当作主题拆开，铸的是两条门口判据各自一条，**没有把块名整体铸成一条**；每条一句话定义、带出处说明（声明自述）。与提示词「一物一名，按最小可教单元；块名是主题、不是概念」的条款一致。

## 如实缺口

1. **缺口①（观测面）**：on 臂观测存档 `calls.jsonl` 含**且仅含**该次声明铸造调用（system=登记表铸造执行者、user=已具备块行+门口判据、response=两条概念 JSON，76.8s）——「存档含声明调用」的**实质判据人工核验为真**；但 smoke 的机械断言 `calls.contains("basis-declaration")` **按实现不可能为真**：装配时 `split_template` 剥掉携带 `id: basis-declaration` 的头注（单测自己断言「头注可剥离」），Archiver 存档行也无 task 字段 → 该断言恒假，on 臂因此在 [2/5] 末 EXIT=1。**这是 m2 冒烟判据自身的 bug（不可满足断言），留待修**；管线本身（声明→铸概念→锚行→计数守恒）全部绿灯。
2. **缺口②（连带）**：因缺口①中断，[3/5] 下游就绪翻转、[4/5] 幂等 noop 零 LLM、[5/5] 撤销复原三步**未获得真机读数**——脚本化单测已覆盖（红绿清单），本轮记为「单测覆盖、真机未走完」。
3. off 臂注册表/锚表/节点/批次四项全零 ✅（零写面）。

## 产物与存档

- 存档：`rm-matrix/data-m2-on/observation/learning-smoke/run-1793098939800/calls.jsonl`
- 日志：`rm-matrix/m2-off-run.log`、`rm-matrix/m2-on-run.log`
