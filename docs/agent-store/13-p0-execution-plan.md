# P0 收尾与 SDK/Web 对齐执行方案

> 日期：2026-09-04
> 前置：`开发计划.md`（Phase 0）、`agent-store-v1-test-cases.md`、`12-sdk-packaging.md`、`single-run-runtime-evidence.zh.md`
> 状态：起草（待开工 Step 1）
> 说明：本文件是执行层计划，不替代 `开发计划.md` 的阶段定义与 `09` 的门禁定义；
> 每个任务遵循开发计划 §11（REQ 编号/文件/契约/TC/失败场景/验证入口）

## 1. 现状基线（2026-09-04）

- TC-RT-001 PASS：临时实例 + mimo-v2.5 真实模型，`planning → running → completed`
 （`single-run-runtime-evidence.zh.md`）；附带修复 `runtime_adapter` actor 落库 500
 （`external_agent("app-server")` → `user(owner_id)`，已提交 `7f035f88a`）。
- TS 三包主体落地：`@agent-store/{protocol,client,sdk}` 0.1.0，tsdown 编译，
  npm dry-run 通过；`web/src/lib` 为 re-export 垫片 + Web 子类。
- 文档矛盾已消：TC-SDK-001 改为 spawn + 回环 WS；开发计划 TC 引用收缩到主表现有编号。
- 按开发计划 §12 停止条件：P0-A/B 关闭前不得扩展 Team/Web。

## 2. 目标与非目标

目标：关闭 P0-A（TC-RT-002/004/010）与 P0-B（TC-RT-005/006），补 P0-C/D 低成本验证项，
给出 Phase 0 门禁结论；并行把 SDK/Web 对齐到同一 client 实现。

非目标：Team planned DAG（Phase 2 Spike，需 P0 关闭后另行立项）、stdio 传输（V2）、
Python SDK（已决策延后）、Flowy 纵向闭环（Phase 5）。

## 3. 工作包 A：关闭 P0-A（Step 1，顺序 T3 → T1 → T2）

### REQ-P0A-01／TC-RT-004 取消（P1，先做，最快）

- 文件：点火脚本（临时）+ `nomifun-app-server`（若有缺口）
- 契约：`run/cancel`（10 §run 状态机）
- 操作：Run 进入 running 后调 `run/cancel`，轮询到终态
- 断言：终态为 `cancelled` 且版本递进；cancel 前为非终态（防伪造）
- 失败场景：cancel 后仍 completed → 查 engine cancel 语义，记 BLOCKED
- 验证：点火脚本 + `cargo test -p nomifun-app-server`

### REQ-P0A-02／TC-RT-002 版本冻结

- 文件：点火脚本 + 安装路径（`install/*` 重装同 Agent 新版本）
- 契约：receipt 的 `preset_revision`/`content_digest`（10 §不可变冻结）
- 操作：Run 运行中发布同 Agent 新版本，查 `run/get`
- 断言：运行中 Run 的两字段不变；历史记录可追溯；Preset ID 不被当作 Runtime Agent ID
- 失败场景：字段漂移 → 查 `create_for_app_server` 快照冻结，记 BLOCKED
- 验证：点火脚本

### REQ-P0A-03／TC-RT-010 规范化审计

- 文件：`nomifun-app-server`（`map_public_run_id` 周边）、`runtime_adapter::run_view/event_view`
- 契约：10 §公共 ID 与错误码（opaque ID、无内部 ID、无凭据）
- 操作：全字段扫描 `run/get`、`run/events`、`run/result`、`install` 响应
- 断言：无 allo 内部 ID、无凭据值；错误为稳定 code
- 失败场景：发现泄漏 → 修映射，转为可重复断言后再关
- 验证：新增断言脚本 + `cargo test -p nomifun-app-server`

出口：三项 PASS → P0-A 关闭，开发计划 §4 出口勾选第二轮。

## 4. 工作包 B：P0-B 事实源与重启（Step 2）

### REQ-P0B-01／TC-RT-005 重启恢复 + REQ-P0B-02／TC-RT-006 事件序

- 文件：点火脚本 + engine 恢复路径（`scheduler` 恢复扫描）
- 契约：技术方案 §6.2（sequence 服务端分配、迟到不覆盖终态）+ §6.3（`recovery_required`）
- 操作：运行中杀进程重启；查事件保留、未完成标记、`run/events` sequence 连续性
- 断言：事件保留；未完成标记 `recovery_required`；绝不伪装 completed
- 退出条件：若需改 engine 持久化超过 2 天，降级为只验“不伪装”，T4 记 BLOCKED 并写明原因
- 验证：点火脚本（杀进程版）

出口：P0-B 关闭 → Phase 0 出口条件全满足 → 按 §12 解锁 Phase 2 Team Spike 立项。

## 5. 工作包 C：P0-C/D 查漏（Step 3，纯验证）

- REQ-P0C-01／TC-API-002：同一 `idempotency_key` 重放 `agent/run` → 同一 run_id
 （实现已在 `execute_agent_run`，点火脚本加一段即可）。
- REQ-P0C-02／TC-API-003：`run/get` 与 `run/result` 终态一致（A1 已有一半）。
- REQ-P0D-01／TC-OAUTH-001/002/004 + TC-CONN-001/002：OAuth Spike 剩余用例（此前仅 003 有证据）。

出口：09 §9 快照更新，Gate 3/4 从“未执行”变为有结论。

## 6. 工作包 D：SDK/Web 对齐（Step 5，可与 A 并行）

原则：webui 与 SDK 共用 `@agent-store/client` 为唯一协议实现；
webui 不直接依赖 `@agent-store/sdk`（Node 专属，浏览器不可运行）。

- REQ-PAR-05（SDK 应用功能与工效包，用户 2026-09-04 定为优先于 PAR-01/02）：
  1. REQ-PAR-05a `run/steer`（协议 + client + handle）：runtime 能力已在
   `engine.steer_step`（durable effect + CAS + 恢复重投），App Server/协议/SDK 零暴露。
   补 `run/steer {run_id, text}`（单 Agent run 默认路由到当前活跃 step/attempt，step 不进公共契约），
   HTTP 薄适配双落；client `handle.steer(text)`；验证：协议测试 + live 脚本。
  2. REQ-PAR-05b `models/list`（协议 + client）：SDK/第三方无法枚举模型，唯一服务端真空缺；
   从 ProviderService 投影公共模型目录（不暴露 key）；client `models()`。
  3. REQ-PAR-05c `TurnResult` 聚合（client 层）：从事件流聚合 `final_response`（文本）
   + token usage（`context.usage` 事件）+ 事件/物品清单，`handle.finished` 升级返回聚合对象；
   webui 后续切同一聚合（消现有私有实现）。
  4. REQ-PAR-05d 多轮 `ConversationHandle`（client 层）：包装现有 conversation 域
   （create/send/cancel + 会话事件归并原语），≈Codex Thread 多轮形态；webui 切同一句柄。
  5. REQ-PAR-05e 重试辅助（client 层）：`retryable` + 指数退避 + 抖动的 `withRetry` 助手（≈Codex `retry_on_overload`）。
  顺序：05a → 05b → 05c → 05d → 05e（a/b 动协议，c/d/e 纯 client 层）。
  图片输入、sandbox 一等参数、archive/resume/fork 不列入本包（11 §6.3 或 V2）。
- REQ-PAR-01：抽 `@agent-store/browser`（12 §3 预留）：迁移 `web/src/lib/client.ts` 子类
 （serverRootUrl/browseDirectory/registerWorkspace + HTTP 底座）；`web/src` 只剩垫片。
- REQ-PAR-02：防漂移 contract test：`web/src` 禁止协议方法名字面量；
  新增协议方法必须 dispatch arms + client 方法 + 类型三件套（12 §7 入测试）。
- REQ-PAR-03 ✅ 2026-09-04 已落地（有修正）：Run 域事件路由直接在 `client` 包内以 `EventSubscription` 补齐
  （去重 seen 集 + `run/resync-required` 自动追平 + `follow` 静默建游标 + 手动 `resync()` 并发共享单 flight），
  单测 8 项全过（FakeTransport），真机 live（SDK spawn→WS 推送→completed）PASS（6 推送/0 重/游标覆盖）。
  注：`conversation-events.ts` 是会话域 transcript 归并（React 侧），不在下沉范围，保持 webui 私有。
- REQ-PAR-04 ✅ 2026-09-04 已落地（先行 AgentHandle 部分）：`client/run-handle.ts`——
  `launchRun()`（launch+follow）→ `AgentRunHandle`：`finished`（轮询兜底阻塞到终态，≈`thread.run()`）、
  `for await` 事件迭代（≈`runStreamed()`）、`cancel()`（带 receipt 版本）；纯组合零新协议方法。
  单测 4 项 + 真机 live（`scripts/sdk-live-handle.ts`：completed/6 事件/0 重）PASS。
  市场编排 helpers（`installFromMarket` 等）仍待做。

顺序：PAR-01 → PAR-02 → PAR-03 → PAR-04。门禁事项（工作包 A/B）优先于对齐；
PAR 与 A 无文件冲突，可并行。

## 7. 门禁判定与文档同步（Step 4，随做随更）

- 每项 TC 按测试主表 §1.2 存证据（输入摘要/公共 ID/终态/脱敏日志路径），体例见
  `single-run-runtime-evidence.zh.md`。
- 同步点：roadmap 进展段、12 状态表、09 §9 快照、开发计划 §4 出口勾选。
- 最终输出 Phase 0 “通过 / 附条件通过 / 阻断”结论；只有通过才立项 Team Spike。

## 8. 已定决策（不再讨论）

| 事项 | 结论 | 日期 |
|---|---|---|
| stdio 传输 | V2/deferred；TC-SDK-001 已改为 spawn + 回环 WS | 2026-09-04 |
| Python SDK | 延后，优先 TS | 2026-09-04 |
| TC 编号范围 | 收缩到主表现有（SDK 001~003、WEB 001~004），缺号待开工补 | 2026-09-04 |
| 包名 | `@agent-store/sdk`（07 冻结文档未改，以 12 为准） | 2026-09-04 |
| 证据体例 | `*-runtime-evidence.zh.md` + 可重复入口为正式证据 | 2026-09-04 |
| 点火脚本教训 | 子进程 stdout 必须持续消费/重定向（64KB 背压冻住服务端） | 2026-09-04 |

## 9. 风险与停止条件（继承开发计划 §12，增补）

| 风险 | 停止条件 | 处理 |
|---|---|---|
| 模型供给不稳（key/额度/网络） | A1 级链路连续失败 | 先修供给，计划整体后移；不记实现 BLOCKED |
| engine 持久化缺口（T4） | 改造超 2 天 | 降级断言，记 BLOCKED 写因（见 §4） |
| 规范化泄漏（T2） | 泄漏面超出映射层 | 升级为安全项，阻断 P0-A 关闭 |
| 对齐返工（D） | client 包 API breaking | 跨 web+sdk 同步改，禁止单边 fork |
