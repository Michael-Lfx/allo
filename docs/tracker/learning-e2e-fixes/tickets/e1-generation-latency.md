---
id: e1
title: "生成耗时优化——相位预算、白跑早停、调用形态收敛裁决"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# e1: 生成耗时优化

**What to build:** 把「建课→第一节内容就绪 ≈27 分钟」压下来。三刀：①结构
白跑早停（同装配同输出不再盲烧预算）②相位耗时读数（两仓首条）③课时循环
调用形态收敛的裁决与实施。每刀有 learnhub 参考锚。

## allo 现状（E2E 分相位，见 map 账目表）

- 结构 11m14s：**首次尝试 3m57s 白跑**（gd.no_schedule「每月」违约 → 双产物
  整体重生成）；好路径 7m07s；两次巨型调用 max_tokens=384000。
- 生长首批 7m55s（5 节点）。
- 单节内容 2m33s/1m22s：**9-15 次串行小调用**（agent 草稿循环，各 8192）。
- 瞬断处理脆（详见 e4；本票只管耗时维度）。

## learnhub 参考（Reference 表）

| 件 | learnhub 路径 | 取什么 |
| --- | --- | --- |
| 同指纹早停 + 守卫豁免 | `engine/node_growth/facade.rs:303-366`† | 「第 N 次失败指纹与 N-1 相同 = 反馈回灌已无法改变结果 → 提前 Exhausted 不烧剩余预算」；守卫违约（机械固定文案）豁免指纹比对——**allo 白跑 4 分钟的直接对症药**（结构/生长两环通用化） |
| 整批重试粒度（两份同跑是原裁决） | `engine/decomposition/facade.rs:7,31,137-160` | 「任一环节失败两份一起重跑」+ 反馈单源注释（单侧重跑的坑他们踩过）——**不要**试图改成单侧重跑 |
| 规模控制=条款数非 token | `goal-decomposition.md:32`（40–80/硬 100）、`node-growth.md:9,51`（批 1–5）、`infra/llm.rs` 全文（**零 max_tokens 设置**） | PromptSpec 只有 temperature/thinking/fence_words；allo 的 384000/8192 双天花板是自留形态，票内盘点谁在设、结构侧是否该收 |
| 两遍式内容管线（恒 2 次 LLM） | `engine/contentgen/facade.rs:1-30,171-380,410-475,517-560`、ADR-0014 + 契约 v1.7 | 结构字段住规划 JSON、正文面纯 Markdown 文本接缝（避 KaTeX 进 JSON 的转义雷区）；题目另面单发+元素级丢弃；工具只作 ≤8 轮有界只读查询（`llm.rs:63`、插件先例 129 回合收敛史 ADR-0032 补充裁决） |
| 步级幂等备料 | `engine/coach.rs:1345-1378`† | 内容在→只补题库：天然断点续跑，失败重试不重付已付相位 |
| 任务生命周期流水 | `app/generation.rs:8-20`（十类任务状态机/终态幂等/MAX_RUNNING_TASKS=5） | allo 已有 tasks.jsonl 但**无相位耗时读数**——两仓都没有，随本票立 |

## 实施要点（带裁决点）

- [ ] **D1（先裁决）课时循环形态**： allo 9-15 次串行小调用 vs learnhub 两遍
      式恒 2 次。收敛 = 大手术（草稿工具交互面是 allo 既有能力：用户可中途
      改稿）；**推荐分两步**——第一步不动形态，做「段间并行 where 安全 +
      失败段级续跑（已落段不重写，learnhub 备料幂等同款）」；第二步（独立
      后置票）评估两遍式迁移。裁决留 grilling 或推荐项默认。
- [ ] **D2（先裁决）结构白跑**： learnhub 无同指纹早停在结构侧（只有生长侧
      ）——推荐把生长侧指纹早停**通用化到结构重试环**（同装配同违约报告 =
      回灌无意义 → 提前 Exhausted）；守卫/机械文案豁免语义照搬。
- [ ] 相位耗时读数：structure/growth/lesson 三类任务在 tasks.jsonl 的
      finish 行补 `elapsed_ms` + LLM 调用次数 + 输入/输出字符量；smoke 各档
      读数行同步（两仓首条耗时基线）。
- [ ] max_tokens 盘点：结构 384000（接模型天花板）与课时 8192 的设定处、
      对网关排队/延迟的影响面（读数先行，收敛动作随 D1/D2 裁决）。
- [ ] 前后对照卡：同 fixture（同目标文本建课→首节内容就绪）改前/改后墙钟
      并排 + 白跑次数 + LLM 调用次数。

## 验收

- 结构白跑路径：人为注入同指纹违约（测试），早停生效、预算不烧完（红绿）。
- 相位耗时读数进 tasks.jsonl 与 smoke 读数行；对照卡两臂并排。
- `cargo test -p nomifun-learning` / `-p nomifun-ai-agent` / `-p nomifun-app`
  全绿；不新增迁移。

## 非目标

- 课时循环改两遍式（D1 第二步，后置独立票）。
- 换模型/路由档位（learnhub ADR-0035 经济学定调 flash 即前沿，不动）。
- 定时维护/自动补批节奏（m10 已落）。
