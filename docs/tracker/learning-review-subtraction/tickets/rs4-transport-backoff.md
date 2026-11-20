---
id: rs4
title: 评审传输重试退避——0s 瞬时双败改 1.5s 退避
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

评审调用传输失败的重试**零间隔**：11-16 09:34 / 10:03 两次实录均为
「首次失败（SSE 掉线，failed-provider-sse 留痕）→ 重试 0 秒瞬时失败 →
Degraded 保守放行」——批**未经任何评审**落库，仅 findings+note 留痕。
SSE 掉线通常是瞬时故障，0s 重发大概率撞同一故障窗口；1-2s 退避可能把
两次静默降级放行救回一次。

## 已核验事实（2026-11-20 对码）

- 重试分支：`mod.rs` concept_review 传输臂（~:1795），
  `REVIEW_TRANSPORT_ATTEMPTS=2`（:1155）——失败后 `continue` 直接重发，
  无 sleep。
- 病例：11-16 09:34 growth_review 60s ok=F out=0 + 0s ok=F →
  "concept review call failed twice (transport budget) — degrading to
  PASS"；10:03 同形态（78s + 0s）。
- Degraded 语义本身不动（ADR-0026，上游同守）：退避只是降低双败概率。
- 重试族测试 11 个（mod.rs:3639-4507，含 transport_failure_retries_
  then_rescues / transport_failure_twice_degrades_with_signal /
  transport_then_contract / contract_then_transport 两臂）——真实
  sleep 会拖慢测试。

## R（用户条件批准 2026-11-20）

- **R-a 退避常量**：传输重试前 `tokio::time::sleep` 1.5s；常量
  cfg(test) 置 0（测试零减速，生产行为不变语义）。
- **R-b 只挂传输臂**：契约违约臂（有回包、模型侧问题）不加退避——
  瞬时故障假设只对传输成立。
- **R-c 范围外**：draft/redraft 调用无传输重试（失败=整 attempt 作废
  走 redraft 环，天然有间隔）——不在本票；多 provider fallback 另立
  候选。

## Acceptance

- 既有 11 个重试族测试零改动绿（cfg(test) 常量=0）。
- 新增一个断言：非 test 构建下退避常量 >0（编译期检查或文档说明，
  从简）。
- `cargo test -p nomifun-learning` 全量绿 + fmt。
- 负优化判定：本票不碰契约与提示词，无可负优化面；若真机再现 0s 双
  败降级（sleep 未生效）= 实现有误，返工。

## Resolution

已施工收口（2026-11-20，implement-spec 并行波）。

- **落地**（wave/rs4 fa132bf84，并入 feat/learn1116 merge 5d5b5cfcc）：
  单文件 mod.rs +18 行——常量区 `REVIEW_TRANSPORT_RETRY_BACKOFF`
  （`#[cfg(not(test))]`=1500ms 带编译期断言恒为正；`#[cfg(test)]`=
  ZERO）+ 传输臂 `continue` 前 sleep；契约违约臂/其它行零触碰。
- **检查**：重试族过滤测试 `transport` 15 绿 + `contract` 76 绿
  （0.26s 完结即证 test 构建退避=0 生效）；合并点全量
  `cargo test -p nomifun-learning` **1022 passed / 0 failed**；
  `cargo check --workspace` 过（nomifun-app 2 个 appearance 既有
  warning，非本票引入）。fmt 干净。
- **真机观察项**：下次 live 传输双败时核 sleep 生效（日志两行间隔
  ≥1.5s）；成功救回则降级放行少一次。
