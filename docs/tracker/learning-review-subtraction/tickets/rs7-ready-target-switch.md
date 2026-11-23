---
id: rs7
title: 就绪目标/批上限可配 + 真机 A/B 定缺省——单轮时长与返工爆炸半径的直接杠杆
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

单轮生长 9-18 分钟的痛由**每批缺口大小**驱动（缺口 = READY_TARGET − 当前
就绪，本波实测缺口 3 → 5 调用 → 18 分钟）。弱模型的错误面随批大小**平方
级**收缩：①单批错误暴露率下降（1 节点有缺陷 3 节点批必打回）；②「批内兄
弟不供给」约束随批变小趋平凡满足（11-16 病历的预设未教概念类消失）；③评
审碎片清单变小 → 格式错位暴露面与思考量同降（97 片段=58.5k reasoning 实
测）。批内互不供给契约保证批大小**不是结构旋钮**（先后依赖从来跨批表达）
——唯一损失是"同批对照"教学形态（对照对变先后两批）。

## 已核验事实（2026-11-23 对码）

- 三常数：`READY_TARGET=5`（补货目标）、`READY_TRIGGER=3`（触发水位）、
  `MAX_BATCH_NODES=5`（单批上限）；数量契约段渲染 `{READY_TARGET}`（模板
  已参数化）。
- 全链路已参数化：verdict API 返回 `trigger`、图 API 返回 `ready_target`、
  前端 `graph.ready_target` 显示（零 UI 改动）。
- 常数先例：learnhub 同位数字=7，allo 吸收时已降至 5（代码注释在案）——
  本常数已存在被调先例，非神圣值。
- 上游教训适配：批变小不减弱质量门——门抓的是缺口与供给，批越小错误面
  越小、点修越轻（11-23 点修 25s 样本）。

## R（用户裁决 2026-11-23：可配 + A/B 定缺省，不硬改）

- **R-a 单开关**：`NOMI_LEARNING_READY_TARGET`（沿 NOMI_LEARNING_* 词表，
  switches.rs 单源）。未设 = 5/3 现行为（零变化）；设 N（2-5 合法域，越
  界回退缺省）→ `READY_TARGET=N`、`READY_TRIGGER=max(N-2, 1)`（保持
  trigger=target-2 的既有关系，单开关不双调）。
- **R-b A/B 协议**（正优化验收纪律）：冻结二进制，同一课程快照/重置法，
  两档各 2-3 轮真实生长（11-23 同款流程：删未开课 → skip 触发 → 轮询）。
  读数：单轮墙钟、attempts/打回率、死因重试次数、评审 reasoning_tokens、
  就绪饿死次数（学习者可学池空转）、批均节点数。
- **R-c 胜者定缺省**：读数进 Resolution；若 3 档显著缩短单轮且打回率/饿
  死不劣化 → 缺省改 3/2 并更新注释（learnhub=7 → allo=5 → 调参史续写）；
  若无显著差异或饿死出现 → 维持 5/3，开关留作运维调参面。

## Acceptance

- 缺省（未设 env）：全部既有测试零改动绿（prompt 数量契约段渲染测试、
  auto_refill verdict 测试按常数写的零影响）。
- 设 3：prompt 渲染 3/1、verdict 返回 trigger=2、growth 单轮批 ≤2——
  单测三件。
- 真机 A/B 读数表进 Resolution（两档各 ≥2 轮）。
- `cargo test -p nomifun-learning` 全量绿 + fmt。

## 回退条件（负优化判定）

- A/B 出现就绪饿死（学习者无节点可学等待补货）或打回率不降反升 → 维持
  5/3 缺省；开关保留为运维面（模型升级后可再试）。
- 本票不治：格式误伤重试（rs6 靶）、思考固有延迟、传输 flake（rs4 靶）。
