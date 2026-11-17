---
id: f1
title: 课时生成轮次呈现语义失真 + 14 轮构成取证
type: task
status: closed
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

## 裁决（2026-11-17 grilling，全按推荐收口）

- **Q2 相位化呈现**：学习面进度行改相位化（规划节清单 → 写第 k/N 节 →
  出题 → 审校 / 修复审校 → 发布 + 已耗时），裸轮次数字退出学习者面。
  后端在 handler 侧新增节进度帧（帧词汇表现无 plan/section，`events.rs`
  补相位于 const + ADR-0004 翻译差异表纪律 + 测试）；audit 帧已带
  `loop` 可区分修复审校。帧上 `loop`/`max_rounds`/`tools` 学习面不再消费
  裸 round。
- **Q3 取证先行**：施工期开 `NOMI_LEARNING_CALL_ARCHIVE=on` 用同模型
  （AIPC-deepseek-v4.1-flash）重放一个 5 节课时生成，逐轮表（轮号/工具/
  usage/耗时）落 `assets/`；仅当证据显示可治理浪费才另立效率票；不预设
  砍 GENERATE_MAX_ROUNDS。

## Acceptance

- 呈现面：双 locale 词条 + 现有渲染测试更新；修复环轮次有可辨识标记
  （不再与生成环计数混淆）。
- 取证：assets/ 报告含逐轮表（轮号 / 工具 / usage / 耗时），并给「合同固
  有 vs 可治理」结论。
- 全闸：typecheck / i18n / theme / dead-css 绿。

## Resolution

已施工收口（2026-11-17，feat/learn1116：`09f1495d6`+`cec0127db`+审查修复
`45e9b22ff`）。

- **相位化落地**：后端 handler 侧四帧——`plan`（manifest 定稿，N=节数）、
  `section`（k/N+title，清单经 `lesson_draft_manifest` 单源直读，断点续跑
  正确）、`activities`（出题批次有 op 接纳即发）、`publishing`（ls_finish
  门禁尝试前发）；ADR-0004 翻译差异表同步 + wire 放行测试。前端
  `lessonProgressLine` 相位化（规划节清单 → 写第 k/N 节 → 出题中 →
  审校/修复审校（带计数）→ 发布中，全程「· 已 {{elapsed}}」），completed/
  failed 清行；裸轮次退出学习者面（`lessonGenRound` 词条双语退役；旧帧兜
  底「生成中」不带数字）。
- **审查收口**（双轴：Standards 0 硬违规 / Spec 缺口两项）：出题+发布相位
  端到端补齐（M1）、耗时全程挂（M2）、计时常量复用（S6）；跳过项留痕——
  `max_rounds`/`tools` 不上学习面（Q2 相位化裁决覆盖 R-a 草案措辞）、
  started 帧缺失降级无耗时（已测试接受）、宠物面相位别名 creep 接受。
- **验证**：nomifun-ai-agent 1169 绿 / nomifun-learning 969 绿 / learning
  前端 428 绿 / typecheck+i18n+theme+dead-css 绿 / `cargo check --workspace`
  + `bun run check` 广谱绿。
- **R-b 取证重放：待真机**。装置确认支持（learning_smoke `archive` 开关 +
  lesson observer，`learning_smoke.rs:210-213`）；需真模型凭据与配额（5 节
  课时 ≈10–30 分钟真实调用）。取证报告落 assets/ 后，若见可治理浪费（空
  转/重写轮）再立效率票；不预设砍 GENERATE_MAX_ROUNDS。
