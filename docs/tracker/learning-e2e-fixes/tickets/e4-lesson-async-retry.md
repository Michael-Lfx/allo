---
id: e4
title: "lesson/generate 后台化 + 瞬断重试加固"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# e4: lesson/generate 后台化 + 瞬断重试加固

**What to build:** 课时内容生成从「同步长请求挂 HTTP 生命周期」改为「任务
后台化 + 客户端轮询/事件」；瞬断流错误的重试纪律对齐 learnhub 三层（错误
两桶 → 门面带反馈环 → 显式超时=可重试）。

## allo 现状（E2E 实锤）

- `POST /lessons/{id}/generate` 分钟级同步阻塞：调用方 15s 断连 → 生成静默
  取消（draft 已建、调用方零感知）；弱网/移动端必疼。
- 任务注册表（j03）其实已记 lesson 任务（tasks.jsonl 有 begin/finish），但
  HTTP 语义仍是同步阻塞——两套模型并存，断连即取消。
- 瞬断流错误内部仅一次重试，第二次即 `BAD_GATEWAY` 裸抛调用方（e2e 实录：
  33s 失败尝试直接回错误页）。
- 一次性成功后无续跑语义：失败重发整课重付（draft 之后的段落进度全丢）。

## learnhub 参考（Reference 表）

| 件 | learnhub 路径 | 取什么 |
| --- | --- | --- |
| 分钟级任务一律 spawn_blocking（红线） | `app/commands.rs:7,128-135,170-176`、`docs/architecture.md:721` | 生成不占请求生命周期；命令 await 但跑在阻塞线程池 + 步骤级进度事件 + 协作取消旗标（LLM 调用前后双检查 :974-1015） |
| 任务注册表状态机 | `app/generation.rs:8-20,97` | queued→running→done/failed/cancelled、终态幂等、MAX_RUNNING_TASKS=5、资源级排他 |
| 错误两桶（可重试/数据性） | `infra/llm.rs:24,114-118,349-351` | 网络/限流/空响应/截断=可重试；「可重试语义由上层门面裁决」单源 |
| 门面重试环 ≤3 带反馈 | `decomposition/facade.rs:31`、`contentgen/facade.rs:67`（同预算） | 违约/上轮产物回灌 retry_feedback |
| 层内短退避（瞬断专属，不弃已得） | `contentgen/facade.rs:57-60,481-511` | SEARCH_ATTEMPTS=3、300ms×attempt、「一次瞬断曾炸掉整个内容生成」头注 |
| 显式超时=可重试失败 | `engine/questions/judge.rs:14-15,162-186` | 90s 独立限时、超时进重试环「线程分离不可撤销只弃其果」 |
| 同错误熔断 | `engine/coach.rs:57-59,584,2806-2843`† | step_guard：同步骤连续 2 次同指纹失败=CircuitOpen loud；成功清零 |
| 幂等续跑 | `commands.rs:166-169`（可重入生长：从覆盖前沿长出）、`coach.rs:1345-1378`（内容在→只补题库） | 失败重发不重付已付相位 |

## 实施要点

- [ ] `lessons/{id}/generate` 改双模：默认异步（返回 202 + 任务键，复用 j03
      注册表；前端轮询 generation-tasks 或 WS lesson 事件），保留 `?wait=true`
      同步臂给既有脚本/冒烟；断连**不取消**（任务脱离请求生命周期跑完）。
- [ ] 瞬断加固三件：①错误分类对齐两桶（流解码错误/超时/空响应=可重试）；
      ②课时循环入口重试环 ≤3 带反馈（非仅一次）；③显式超时（单调用限时，
      超时=可重试只弃其果——judge 90s 形态移植）。
- [ ] 段级续跑（依赖 e1 D1 第一步的段间幂等）：已落库 section 不重写。
- [ ] 同错误熔断（step_guard 同款）防重试环死循环；耗尽=结构化错误（任务
      failed + 原因），不裸 502。
- [ ] smoke content 档加「瞬断注入」臂（断 completer 一次再恢复 → 生成仍
      成功）；typecheck/learning/ai-agent 全绿。

## 验收

- 真课：发起生成 → 3s 内拿到任务键 → 断开连接 → 任务照常完成、课时落库
  （硬判据）；弱网模拟（代理掐流）一次后自愈。
- 轮询/事件前端面：i18n 双语进度文案；既有同步调用方（smoke/脚本）不破坏
  （wait 臂回归）。

## 非目标

- 课时循环内部形态改两遍式（e1 D1 管辖）。
- 取消 UI（j03 的 cancel 端点已有，前端暴露面另议）。

## Resolution（2026-10-30 关票）

- 实施 589ad0c53。双模：默认 POST → **202** + {task_key,state,poll}（spawn 脱离请求生命周期，断连不取消；已生成幂等短路 done）；?wait=true 同步臂逐字保留。瞬断三件：门面重试环 ≤3 带反馈（落 service 层一处覆盖图/传统两路径，resume 经 e1 段级原语承载回灌）+ 显式超时（每 attempt 660s 死人开关——粒度偏差已申报：每引擎调用而非每 LLM 轮，防误杀）+ step_guard 同错误熔断（连续 2 次同指纹 loud）。错误两桶（BadGateway/Timeout/RateLimited=可重试）。
- smoke content 瞬断注入臂（掐 2 次自愈，硬判据⑦ attempts_used=2/stop_reason=success）；registry 拆分 pre_register/begin_registered（m7 排队取消不变式跨拆分保持，测试钉住）；cancel 拒绝文案修正（断连不再中断）；flaky provider 无 feature 门（prompt_observer 同形，生产零装配）。
- 真机（acceptance.md）：**202 秒回实锤** + 后台 140s 完成 + 内容落库。
