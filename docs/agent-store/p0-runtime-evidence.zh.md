# P0 运行时证据（WP-3）

> 状态：📎 证据（运行时实测快照，非契约）
> 日期：2026-09-09
> 脚本：`web/scripts/sdk-live-p0a.ts`（P0-A）、`web/scripts/sdk-live-p0b.ts`（P0-B）
> 模型：mimo-v2.5（key 仅脚本内存）

## 摘要

| 工作包 | 用例 | 结果 |
|---|---|---|
| P0-A | TC-RT-004 / TC-RT-002 / TC-RT-010 / TC-API-002 / TC-API-003 | **18/18 PASS** |
| P0-B | TC-RT-005 / TC-RT-006 | **10/10 PASS** |

**P0-A/B 已关闭**（无 engine 持久化改造，未触发降级条件）；按 `13` §4 出口与
`15` §7 门禁，Phase 0 出口条件满足，Team Spike（Phase 2）可立项。
剩余 P0-C/D：OAuth 运行时证据（TC-OAUTH-001/002/004）待补；TC-CONN-002 已在 WP-2 覆盖。

## P0-A（TC-RT-004 / TC-RT-002 / TC-RT-010 / TC-API-002 / TC-API-003）

最近一次：**18/18 PASS**（`RESULT PASS`，data `agent-store-p0a-1788933195275`）。

| 用例 | 判据 | 实测 |
|---|---|---|
| TC-RT-004 取消 | cancel 前非终态；终态 `cancelled`；版本递进 | `planning@v0` → `cancel-accepted` → `cancelled`，版本 **v0→v1** |
| TC-RT-002 版本冻结 | 运行中发布同 Agent 新版本，冻结字段不变；历史可追溯；preset_id ≠ runtime agent id | 重装 v9.9.9（9 组件）后 `preset_revision:1` + `content_digest:sha256:1fd66d36…` 在 run/get 与 run/result 均不变；`preset_id=01a084ba-…` ≠ `agent_id=wb-software-company-software-architect` |
| TC-RT-010 规范化 | 无内部 ID、无凭据；错误为稳定 code | run/get、run/result、run/events、store/install-entry、install/status、agents/list 六处扫描零泄漏；未知 run → `not_found` |
| TC-API-002 幂等重放 | 同 key + 同请求 → 同 run_id；同 key + 不同请求 → `idempotency_conflict` | 重放返回同一 run_id `01a084ba-537e-…`；改 goal 后返回 `idempotency_conflict` |
| TC-API-003 终态一致 | `run/get` 与 `run/result` 终态一致 | 两者均 `completed@v6` |

### 语义澄清（本轮 live 校准）

- **version 从 0 起算**：`agent_executions` INSERT 显式 `version=0`，每次状态迁移 +1。
  TC-RT-004 的「版本递进」应断言**相对取消前严格递增**（v0→v1），而非绝对值 >1。
- `run/cancel` 走 CAS（`expected_version`），并发失配返回 conflict；脚本按「重读→重试」处理。

### 判据与用例原文的差异

`13-p0-execution-plan.md` 对 TC-RT-004 写「终态为 cancelled 且版本递进」；用例原文
（`agent-store-v1-test-cases.md` §TC-RT-004）为「取消请求不直接伪造终态；最终收到
`run.cancelled` 或明确无法取消的结果」。两者均满足。

## P0-B（TC-RT-005 重启恢复 / TC-RT-006 事件序）

最近一次：**10/10 PASS**（`RESULT PASS`，data `agent-store-p0b-6X0Zum`）。

脚本 `web/scripts/sdk-live-p0b.ts`：自持进程（硬杀 pid 模拟崩溃）→ 同一 data dir
重启 → 协议面仍走 SDK client。

| 用例 | 判据 | 实测 |
|---|---|---|
| TC-RT-005 重启恢复 | 事件与终态保留；不伪装 completed；无法证明安全时 `recovery_required` | 杀前 `running@v4`（seq1–5）→ 重启后可解析且首读 `running`（非 completed）→ 安全收敛 `recovery_required@v5` |
| TC-RT-006 事件序 | 已持久化事件保留、序列无缺口、重启后续号单调 | 崩溃前 seq1–5 完整保留；重启后新增 seq6；最终 [1..6] 单调无缺口 |

```text
PASS P0B.pre-kill.running :: {"status":"running","version":4}
PASS P0B.pre-kill.events-exist :: ["1:run.started","2:run.plan_changed","3:run.status_changed","4:attempt.updated","5:attempt.updated"]
KILLED pid=24464
PASS TC-RT-005.run-resolvable-after-restart :: "running"
PASS TC-RT-005.not-fake-completed :: "running"
PASS TC-RT-006.events-preserved :: {"pre":[1,2,3,4,5],"post":[1,2,3,4,5]}
PASS TC-RT-006.sequence-gapless :: [1,2,3,4,5]
PASS TC-RT-005.settles-safely :: {"status":"recovery_required","version":5}
PASS TC-RT-006.final-sequence-monotonic :: [1,2,3,4,5,6]
```

说明：恢复路径 `reconcile_recovered_attempt` 判定无法证明安全 → `review_blocked`
（`runtime_state.reason=process_restart`）→ run 投影为 `recovery_required`；
未做任何 engine 持久化改造，未触发 §4 的 2 天降级条件。

### 方案选择（P0-B 前置）

SDK `SpawnedServer` 只暴露 `close()`（优雅停），不暴露 pid/child。采用**方案 1**：
脚本自持进程（`Bun.spawn` + 持续排空 stdout/stderr 防背压），协议面经
`AppServerClient` + `WebSocketTransport` 连回环 WS——**不动 SDK 公共面**。
若后续需要 App 侧进程管理能力，再单独评估给 `SpawnedServer` 增补 `pid`。

## 复现

```bash
cd web
AGENT_STORE_BIN=<repo>/target/debug/agent-store.exe bun scripts/sdk-live-p0a.ts
# CHAIN_KEEP_DATA=1 保留实例数据目录供取证
```

退出码即判据：0 = 全部 PASS。
