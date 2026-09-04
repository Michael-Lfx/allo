# Agent Store 单 Agent 真实 Run（Phase 0 P0-A）— 验证证据

> 日期：2026-09-04
> 目标：开发计划 §4 P0-A（单 Agent Run 真实完成），对齐 `agent-store-v1-test-cases.md` TC-RT-001
> 状态：✅ 通过（临时实例 + mimo-v2.5 真实模型：planning → running → completed）
> 方法：独立二进制 `agent-store --port 0 --data-dir <临时> --no-open` 就绪行建连，
> 注册 mimo provider → 导入并安装 `software-company` fixture → `agent/run`（mention 指向
> `wb-software-company-software-architect`）→ 轮询 `run/get` → `run/result` + `run/events`

## 1. 实现范围

| 组件 | 位置 | 内容 |
|---|---|---|
| Run 入口 | `nomifun-app-server::execute_agent_run`（`lib.rs:2102`） | 幂等 fingerprint/scope → preset resolve（`agent-store: ` 前缀门禁）→ `AgentRuntimeAdapter::start_agent_run` → 公共 ID 映射（失败则 best-effort cancel 防孤儿） |
| Adapter | `nomifun-agent-execution::runtime_adapter.rs` | PresetSnapshot → `create_for_app_server`（Single 模型池、max_parallel=1）；`get_run`/`get_result`（终态门禁）/`list_events`/`cancel_run` |
| 生产装配 | `nomifun-app::router::routes` + `apps/agent-store` | `create_router` 全量注入 `runtime: Some` + `preset_service: Some` + 幂等/映射仓储；model 取 owner 首个 enabled provider |
| 本次修复 | `runtime_adapter.rs`（`start_agent_run`/`cancel_run`） | `external_agent("app-server")` → `user(owner_id)`：前者自由字符串违反 executions.actor_id UUIDv7 CHECK，此前任何 `agent/run` 落库必 500；后者与 UI 两条创建路径一致（`routes.rs:448`、`template_routes.rs:116`） |

## 2. 验收结果（TC-RT-001）

| 断言 | 结果 |
|---|---|
| `agent/run` 返回异步 receipt（run_id/preset_revision/content_digest） | ✅ `status=planning, preset_revision=1, digest=sha256:b06a…` |
| Run 经 planning/running 进入 completed | ✅ `planning → running(v4) → completed(v6)`，轮询全程 2~5ms |
| 结果可查询且与终态一致 | ✅ `run/result` 200：中文一句话总结，`output_files=[]`，终态 completed |
| 事件可查询 | ✅ `run/events` 200（limit=50） |

## 3. 自动化证据

```text
cargo build -p agent-store（adapter fix 后）                        ✅ 3m14s（仅既有 nomifun-app 警告）
cargo test -p nomifun-app-server                                    ✅ 58/58（含新增 14 个 WS arms 门禁测试）
cargo test -p agent-store                                           ✅ 5/5
A1 点火脚本（临时实例真机链路，见 §5）                               ✅ planning→running→completed
```

## 4. 关键边界（已验证）

- **actor 归因**：App Server Run 的执行 actor 为连接用户本人（`user_id`），与 UI 创建一致；`system` 仍只保留给 scheduler/recovery。
- **版本冻结就绪**：receipt 已携带 `preset_revision` + `content_digest`，为 TC-RT-002 断言提供抓手（本次未执行）。
- **provider 隔离**：测试用临时 data_dir + 临时 provider 注册（`mimo-a1`），本机 `~/.agent-store` 零污染；key 只存在于脚本进程内存，全程未落盘明文、未进日志与文档。

## 5. 复现入口与已知边界（如实披露）

- 复现入口：点火脚本（临时目录，已归档会话；正式回归待 B1 固化进 `smoke --real`/sdk e2e）+
  `cargo test -p nomifun-app --test importer_e2e importer_mention_resolves_installed_preset_and_agents_run_gate`
 （import→install→mention 解析→run 门禁，模型/runtime 边界前）。
- 本次 goal 显式要求不调工具；工具调用、retry/replan、cancel、重启恢复均未覆盖（TC-RT-004/005/006 待 Step 2）。
- 点火脚本教训：读子进程 stdout 做就绪扫描后必须持续消费或重定向到文件，否则 64KB 管道背压会冻住服务端（曾误报为“服务端昏迷”，实为 harness bug，已在脚本内修复）。
- 原始日志随临时目录清理；本页 + 可重复入口为正式证据，符合测试主表 §1.2（输入摘要/公共 ID/终态/脱敏）。
