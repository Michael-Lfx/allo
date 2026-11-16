# m5 生长收敛梯子对账加固（有状态复审）——双臂真机对照卡（开关臂 NOMI_LEARNING_REVIEW_STATE）

- 日期：2026-10-27；票：m5（#175 三断层 + 有状态复审）
- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 两臂同构建同凭据、同 fixture：`learning-smoke grow --goal "掌握一元二次方程的解法与判别式应用" --batches 2`，各自全新数据目录

## 两臂命令原文

```
# on 臂（开关 unset；与 m3 on 臂同一跑次，readout 同打印面）
NOMI_CHANNEL=dev nomicore.exe --data-dir <data-grow-on> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
# off 臂
NOMI_CHANNEL=dev NOMI_LEARNING_REVIEW_STATE=off nomicore.exe --data-dir <data-grow-off-rstate> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
```

## 读数行并排（off | on）

| 读数 | off 臂（REVIEW_STATE=off） | on 臂（缺省 on） |
|---|---|---|
| EXIT | 0 | 0 |
| round 1 状态行 | `[review_state: off injections=0]` | `[review_state: on injections=0]` |
| round 2 状态行 | `[review_state: off injections=0]` | `[review_state: on injections=0]` |
| retry 读数 r1 | `[retry: attempts_used=1, stop_reason=success]` | 同左 |
| retry 读数 r2 | `[retry: attempts_used=1, stop_reason=success]` | 同左 |
| r2 自然重试 | **有**：`provenance validation failed — retrying… reason=自教行「二次项系数」的 ref 指向提名概念自身（循环自证，该词仍无出处）` → `[provenance: rows=16 invalid=1 retried=1 degraded=false]` | **有**：`…reason=挂号行「方程」的名词未逐字出现在「方程的根」的定义里…` → `[provenance: rows=0 invalid=1 retried=1 degraded=false]` |
| r2 退化面 | 碎片裁决退化：`fragment verdicts unusable twice — degraded pass death=碎片裁决不可用:片段「高次数」判「名词」却放行或未在 problems 指名（自相矛盾）` → `[fragments: … degraded=true]` | 评审契约退化：`concept review reply violated its contract — degrading to PASS`（批次仍落地判据绿） |
| 早停次数 | 0（stop_reason 恒 success，指纹早停未自然触发） | 0（同左） |
| 重试后通过率 | 1/1（重试后批次落地、criteria green） | 1/1（同左） |

## 判读

- 开关读数面 ✅：`[review_state: on|off injections=N]` 两臂如实分叉；`switches.rs` 解析生效。
- **自然重试两臂都真实发生且判得对**：off 臂死因「自教 ref 指向自身（循环自证）」与 on 臂「名词未逐字出现在定义里」都是 m3 机械校验器的正确红牌；重试恰一次、批次保守落地——梯子的机械面在真机走通。✅
- **如实缺口（injections=0）**：两臂四轮全部 `injections=0`——前轮注入面只在「前一轮评审失败留痕」时才有东西可注；fixture 的失败都发生在批内重试（当轮消化），从未跨轮留痕。**前轮注入语义本轮真机未触发**（脚本化单测已覆盖机械面）——如实记录，不硬造。
- >3 次预算与指纹早停：未自然触发（每轮一次重试即过或保守放行）；14 项 gate/梯子单测覆盖，真机读数记「未触发」。

## 产物与存档

- off：`rm-matrix/data-grow-off-rstate/observation/learning-smoke/run-*/calls.jsonl`；日志 `rm-matrix/grow-off-rstate-run.log`
- on：同 m3 卡 on 臂跑次（`data-grow-on`，readout 同打印面，复用合法——同一运行同时打印两票读数行）
