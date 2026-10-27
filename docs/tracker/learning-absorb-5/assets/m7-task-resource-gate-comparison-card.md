# m7 学习任务资源闸——双臂真机对照卡（开关臂 NOMI_LEARNING_TASK_GATE）

- 日期：2026-10-27；票：m7（#144/#145 缩编）
- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 两臂同构建同凭据、同 fixture（maintenance 冒烟内置种子：图课程 ×2 + 两条 findings + 无挂载行，跑两轮 `run_maintenance_tick`）

## 两臂命令原文

```
# on 臂（缺省 on）
NOMI_CHANNEL=dev nomicore.exe --data-dir <data-maint-on> learning-smoke maintenance --db <…>/flowy-backend.db
# off 臂
NOMI_CHANNEL=dev NOMI_LEARNING_TASK_GATE=off nomicore.exe --data-dir <data-maint-off> learning-smoke maintenance --db <…>/flowy-backend.db
```

## 读数行并排（off | on）

| 读数 | off 臂（TASK_GATE=off） | on 臂（缺省 on） |
|---|---|---|
| EXIT | 0 | 0 |
| tick1 闸读数 | `gate read-out: enabled=false limit=5 running=0 running_peak=1 queued=0 queued_peak=0` | `gate read-out: enabled=true limit=5 running=0 running_peak=1 queued=0 queued_peak=1` |
| tick2 后闸读数 | `gate read-out after tick2: running=0 running_peak=1 queued=0 queued_peak=0` | `gate read-out after tick2: running=0 running_peak=1 queued=0 queued_peak=1` |
| 收尾 running / queued | 0 / 0 ✅ | 0 / 0 ✅ |
| 回合语义 | `[2/2]` 重读同一批留痕并重跑 LLM（off = 无水位语义仍在，watermark read-out enabled=true value=Some(2)） | tick2 零工作零 LLM（`llm_calls_before=2 llm_calls_after=2`，`idle=1`） |
| 其余判据 | `tick read-out: scanned=1 idle=0 worked=1`；归档课不在扫描面；`repair=Some("noop") mount=Some("mounted")` | 同左全绿 |

## 判读

- on 臂硬判据 ✅：`running_peak (1) ≤ limit (5)` 恒成立；收尾 `running=0 queued=0`；闸排队的机械语义（queued_peak=1 入队后清零）在场。
- off 臂直跑形态 ✅：`enabled=false`、`queued` 恒 0（无排队面）、running_peak 同样收在 1——直跑不排队，与「off = 无上限直跑 = 现行为」一致。
- **如实缺口（>5 并发夹具卡）**：maintenance 冒烟的天然并发只有 1–2（单课工作面），`running_peak > 5` 的排队/取消窗口**真机未触发**——14 项闸单测覆盖该机械面（并发上限、排队、取消窗口），本轮记「单测覆盖、真机未自然达峰」。不硬造夹具。

## 产物与存档

- 存档：`rm-matrix/data-maint-on|data-maint-off/observation/learning-smoke/run-*/`
- 日志：`rm-matrix/maint-on-run.log`、`rm-matrix/maint-off-run.log`
