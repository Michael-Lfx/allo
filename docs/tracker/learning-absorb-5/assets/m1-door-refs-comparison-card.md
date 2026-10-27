# m1 门口梯队 door_refs 块注解——双臂真机对照卡（版本臂）

- 日期：2026-10-27；票：m1（纯资产面，无运行时开关 → 版本臂，k06 先例）
- 新 tip 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 旧 tip 二进制：`vbase-target/debug/nomicore.exe`，mtime **2026-10-27 18:01:54**，tip `ccb08d14e`（独立 CARGO_TARGET_DIR 冷构建，未 stash）
- 凭据：`NOMI_CHANNEL=dev`；Flowy Cloud 缺省解析（flash），双臂同源（各自模板库注入同一 provider 行）

## 两臂命令原文

```
# 旧臂（ccb08d14e）
nomicore.exe --data-dir <data-m1-old> learning-smoke structure --db <data-m1-old>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用"
# 新臂（bd7db6188）——同一 fixture 跑了两个独立样本（new / new2，新数据目录各一）
nomicore.exe --data-dir <data-m1-new|data-m1-new2> learning-smoke structure --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用"
```

## 读数行并排（旧 | 新）

| 读数 | 旧 tip（ccb08d14e） | 新 tip（bd7db6188）样本1 | 新 tip 样本2 |
|---|---|---|---|
| EXIT | 0 | 0 | 0 |
| smoke 尾行 | `OK  course=01a17d12-be57…  spines=9  blocks=54  edges=83  doors=7` | `OK  course=01a17d06-cdd9…  spines=9  blocks=51  edges=59  doors=6` | `OK  course=01a17d13-4f0d…  spines=9  blocks=51  edges=59  doors=6` |
| 快照含 door_refs 键 | **false**（旧契约无此键，块键=五必填） | true（键合法，契约 v2.14） | true |
| door_refs 标注块数 | 0（无此键） | **0 / 51（入门档 0 / 23）** | **0 / 51（入门档 0 / 23）** |
| 覆盖门口数 / 不可解析引用 | — | 0 / 0（零标注 → 可解析性空真） | 0 / 0 |

## 人读抽判

- 结构产物两臂同形合法：EXIT 均按既有判据绿（双契约门 + 门口对账），新臂无劣化。✅
- **如实缺口（主要发现）**：门位标注面在真机**两样本均未自发**——存档 `calls.jsonl` 中 `door_refs` 仅出现在 user 提示词的契约文本里（各 1 次），两轮 LLM 原始响应零次写出 `door_refs`。密度读数 **0/23 入门块**，对比 learnhub 先例（21 块入门档 3 块标注）偏低至零。提示词「宁缺勿滥——错标比漏标严重」的纪律可能使模型在该 fixture 上整体弃标；两次独立采样都为零，倾向系统性而非随机。
- 读侧消费机制由 m2 的 basis fixture 真机验证（块甲→G1、块乙→G1、G2 注入→命中→铸造），m1 的解析/存储/对账通路本身有真机证据；缺的是「生成端自发标注」的真机非零样本。
- 判据面（door_refs 引用越界 → 打回重试）为单测覆盖（structure_gen.rs:383-410 叉积核验），真机未自然触发（零标注无从越界）。

## 产物与存档

- `rm-matrix/data-m1-old|data-m1-new|data-m1-new2/observation/learning-smoke/run-*/calls.jsonl`
- 运行日志：`rm-matrix/m1-old-run.log`、`m1-new-run.log`、`m1-new2-run.log`
