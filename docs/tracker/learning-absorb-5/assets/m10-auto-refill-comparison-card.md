# m10 自动补批——双臂真机对照卡（开关臂 NOMI_LEARNING_AUTO_REFILL）

- 日期：2026-10-27；票：m10（verdict 显式化 + 工作区加载触发 + 开关与重入集）
- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 跑次：on 臂复用 m3 grow-on 跑次（readout 同打印面）；off 臂独立跑一次

## 两臂命令原文

```
# on 臂（开关 unset）
NOMI_CHANNEL=dev nomicore.exe --data-dir <data-grow-on> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
# off 臂
NOMI_CHANNEL=dev NOMI_LEARNING_AUTO_REFILL=off nomicore.exe --data-dir <data-grow-off-refill> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
```

## 读数行并排（off | on）

| 读数 | off 臂（AUTO_REFILL=off） | on 臂（缺省 on） |
|---|---|---|
| r1 pre | `[auto-refill] round 1 pre  verdict=disabled ready=0 trigger=3 enabled=false stalled=true` | `[auto-refill] round 1 pre  verdict=due ready=0 trigger=3 enabled=true stalled=true` |
| r1 post | `[auto-refill] round 1 post verdict=disabled ready=4 (对照 pre 行看批后水位)` | `[auto-refill] round 1 post verdict=below_threshold ready=4 (对照 pre 行看批后水位)` |
| r2 pre | `[auto-refill] round 2 pre  verdict=disabled ready=4 trigger=3 enabled=false stalled=false` | `[auto-refill] round 2 pre  verdict=below_threshold ready=4 trigger=3 enabled=true stalled=false` |
| r2 post | **无此行**（自动臂被闸死，批二未落地，见下） | `[auto-refill] round 2 post verdict=below_threshold ready=5 (对照 pre 行看批后水位)` |
| ready 水位 | 0 → 4（批一直落）；此后不再自动上涨 | 0 → 4 → **5**（批二落地后回升） |
| EXIT | 2（`round 2: growth batch did not land (applied) in time`） | 0 |

## 判读

- verdict 显式化 ✅：`verdict=due / below_threshold / disabled` 三态在真机读数行全部在场；`enabled=true|false` 如实分叉。
- **off 臂 EXIT=2 是设计的的行为信号，不是故障**：补批开关只管自动臂（`kick_growth` 非 force 路径消费 `auto_refill_verdict`，off = Disabled 恒早退，learning_graph.rs:1438-1446）；off 臂 r2 由替身驱动完成后**不再自动补批**，smoke 的 `--batches 2` 阶梯等不到批二落地而如实报 `did not land`。`verdict=disabled` 读数行 + 「无自动补批」的行为后果两证齐全。✅
- on 臂水位语义 ✅：`due ready=0` → 批后 `below_threshold ready=4` → 消费后批二落地 → `ready=5`。

## 如实缺口

- **工作区加载触发补批（UI 面）**：该触发点在桌面端工作区加载路径，CLI 冒烟不可达——记 **UI-manual 待办**，不算失败。
- stalled=true 在 r1 pre 两臂同现（空登记表地板期即停摆旗），两臂一致，非开关差异。

## 产物与存档

- off：`rm-matrix/data-grow-off-refill/observation/learning-smoke/run-*/`；日志 `rm-matrix/grow-off-refill-run.log`
- on：同 `data-grow-on` 跑次（`rm-matrix/grow-on-run.log`）
