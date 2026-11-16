# m9f 碎片裁决面（fragments readout + LEXICON_ENFORCE）——真机对照卡

- 日期：2026-10-27；票：m9 补接碎片面（`NOMI_LEARNING_LEXICON_ENFORCE`，v1 缺省 off）
- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 跑次：缺省臂复用 m3 的 grow-on 跑次（readout 同打印面）；ENFORCE 臂独立跑一次

## 两臂命令原文

```
# 缺省臂（ENFORCE unset = off；与 m3/m5/m10/m11 共用 on 跑次）
NOMI_CHANNEL=dev nomicore.exe --data-dir <data-grow-on> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
# ENFORCE 臂
NOMI_CHANNEL=dev NOMI_LEARNING_LEXICON_ENFORCE=on nomicore.exe --data-dir <data-grow-enforce> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
```

## 读数行并排（缺省 off | ENFORCE=on）

| 读数 | 缺省（enforce off） | ENFORCE=on |
|---|---|---|
| EXIT | 0 | 0 |
| r1 碎片读数行 | `[fragments: total=27 rows=27 unhandled=0 degraded=false]` | `[fragments: total=27 rows=27 unhandled=0 degraded=false]`（同形） |
| r2 碎片读数行 | `[fragments: total=0 rows=0 unhandled=0 degraded=false]`（该轮评审契约退化，表未产出） | `[fragments: total=0 rows=0 unhandled=0 degraded=false]`（该轮挂号表两轮不可用退化，碎片表未产出） |
| unhandled/total | 0/27（全部有裁决） | 0/27 |
| 阻断行为 | 无（report-only，缺省 off） | 无阻断（unhandled=0，无可阻断项；`NOMI_LEARNING_LEXICON_ENFORCE` on 且全部裁决可用 → 未走到阻断分支） |

## report-only findings 抽 3 行人工核（r1，27 条全部 verdict=噪声）

| 行 | 抽判 |
|---|---|
| `{"term":"知数","verdict":"噪声","note":"「未知数」切词碎片，非独立名词"}` | ✅ 切词噪声（「未知数」的碎片） |
| `{"term":"数相乘","verdict":"噪声","note":"跨词边界碎片，出自「两个相同的数相乘」"}` | ✅ 跨词边界噪声 |
| `{"term":"运算","verdict":"噪声","note":"通用数学活动词，常识放行"}` | ✅ 通用词，非本课缺口 |

3/3 判「噪声」成立，**本轮无真缺口行**（评审 problems 里的真缺口走的是挂号表 `缺失` 行 + findings，不走碎片面）；切词噪声未混入任何「名词」误报。

## 如实缺口

- 「ENFORCE 臂 unhandled/total 可能偏高」的预期未出现：flash 在两轮给出的碎片裁决全部可用（unhandled=0），阻断分支真机未走到——ENFORCE 的阻断语义仍以单测 + 观测计划为准（触发条件：定义用语表真机审计成熟后开开关）。
- r2 两臂的碎片表都因上游退化未产出（total=0），如实记录；碎片面 r1 单轮样本足够支撑读数行在场性判断。

## 产物与存档

- 缺省臂：`rm-matrix/data-grow-on/observation/learning-smoke/run-1793095259231/calls.jsonl`（r1 响应含 27 条 fragments 全文）
- ENFORCE 臂：`rm-matrix/data-grow-enforce/observation/learning-smoke/run-*/calls.jsonl`；日志 `rm-matrix/grow-enforce-run.log`
