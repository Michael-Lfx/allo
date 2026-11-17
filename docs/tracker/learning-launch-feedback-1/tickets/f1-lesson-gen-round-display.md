---
id: f1
title: 课时生成轮次呈现语义失真 + 14 轮构成取证
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

课时生成进度显示「生成中 · 第 14 轮」——用户读作「失败重试了 14 次」。
轮到底是什么？14 轮是异常吗？呈现该怎么改？

## 已核验事实（2026-11-17 对码 + dev 库/日志）

- **轮 = 工具环的原始模型调用计数**：`loop_core.rs:224` 起 `round` 手工计
  数，每轮 = 一次 LLM 调用（可带工具调用）；上限 `GENERATE_MAX_ROUNDS = 50`
  （loop_core.rs:23），修复环 `REPAIR_MAX_ROUNDS = 20`（:25）。同轮损坏重试
  不推进计数（round_retries，:280-295）。
- **轮数是分节合同的固有成本**：课时合同（ADR-0002）= 规划节清单
  `ls_set_section_manifest` ×1 + **每节一次** `ls_set_section_body` + 活动
  + `ls_finish`（lesson_content_loop.rs:152-155、系统提示 :204-209）。5 节
  课时零失败也需 ~8–10 轮；审计门打回进修复环再 +N 轮。
- **帧上数据齐全但前端丢弃**：round 帧带 `loop`（generate/repair）、
  `round`、`max_rounds`、`tools`（learning_loop.rs:292-299），前端迷你进度
  行只取裸 `round`（model.ts:162-163、LessonStudy.tsx:761-764）→ 无总数、
  无相位、无耗时；修复环轮次与生成环轮次呈现无差别（计数还可能回跳——两环
  各自从 1 起数）。
- **实测耗时**（dev 库课程 01a1e364，AIPC-deepseek-v4.1-flash）：
  - 判定类型检查时机（5 节，截图课时）：2026-11-17 08:59:26Z 开跑 →
    09:09:46Z 下一课时开跑，≈ **10min20s**；截图时已到第 14 轮。
  - 参数传递（5 节）：2026-11-16 09:31:21Z → 10:00:35Z 下一课时开跑，
    ≥ **29min**。
  - 单轮分钟级与 growth 病历（thinking-ON 云端方差）同族；零重试零超时
    ——未见本地故障证据。
- **观测缺口**：课时流 accept/publish 在 nomicore.log 无日志（只有
  `lesson content generation start` 一行）；o2 dev call archive
  （`NOMI_LEARNING_CALL_ARCHIVE`，挂整个 `LearningCompleter`，含课时流，
  services.rs:2784-2808）已在 feat/learn1116 落地但默认关，走查时的构建
  未含 → 14 轮的逐轮构成（每轮写了什么/有无空转/修复环占几轮）**尚无法
  分解**。

## Terms（修复方向草案，施工前过裁决）

- **R-a（呈现相位化）**：迷你进度行改为相位化文案——「规划节清单 → 写第
  k/N 节 → 出题 → 审校 → 发布」+ 已耗时；帧上 `max_rounds`/`loop`/`tools`
  先用起来（tools 摘要 `summarizeTools` 已有现成函数）。「写第 k/N 节」需
  帧补充节序号字段（现 `tools` 只带 name+is_error，不带 args；后端在
  `ls_set_section_body` handler 或 agent_round 帧加 args 摘要）。裸轮次
  数字退出学习者面，归 dev/观测面。
- **R-b（取证先行）**：开 `NOMI_LEARNING_CALL_ARCHIVE=on` 用同模型重放
  一个 5 节课时生成，逐轮分解轮构成（每轮工具 + usage + 耗时），报告落
  `assets/`。取证结论决定是否另立效率票（合同减轮 / 空转轮治理）——**本票
  不预设 14 轮是病**，5 节课时的合同成本本就在 10 轮量级。
- **R-c（不做）**：不先砍 GENERATE_MAX_ROUNDS（无证据支撑下调）；不把轮次
  明细搬回学习者面。

## Acceptance

- 呈现面：双 locale 词条 + 现有渲染测试更新；修复环轮次有可辨识标记
  （不再与生成环计数混淆）。
- 取证：assets/ 报告含逐轮表（轮号 / 工具 / usage / 耗时），并给「合同固
  有 vs 可治理」结论。
- 全闸：typecheck / i18n / theme / dead-css 绿。

## Resolution

（未结案）
