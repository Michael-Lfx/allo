# Agent 内核解耦进度

目标：`flowy-agent` 只保留门面，引擎能力迁入自有 `flowy-agent-core`，最终移除 `dsh-rs` 依赖。顺序固定，每步独立可编译，旧测试不弱化。

## 已完成

1. 新建 `crates/flowy-agent-core`（workspace 成员，无 kernel/plugin 依赖）。
2. 类型层：`ContentBlock` / `Message` / `MessageSource` / `Role` / `ToolSchema` / `ToolCallArgs` / `ToolExecutionResult` / `TodoItem` / `TodoStatus` / `ToolRunContext` / `CancelToken`。
3. 工具层：registry 去掉 cordis waterfall，改为显式 `ToolPolicy` + 单调 guards；7 个内置工具从 dsh 移植（MIT 归属保留），`todo_write` 改走 `TodoSink`。
4. 结构化历史：`session` 模块保存完整消息、工具调用原始参数字符串、结果配对、todos、状态，文件 `sessions/history.json`。
5. loop：`loop_runner.rs` 双向转换消息并调用真实 pi `run_agent_loop`；错误终态不会映射成成功。
6. 传输：`transport.rs` 用 pi-ai `OpenAiCompletionsProvider`（`providers` feature）构造同步 `StreamFn`，凭证按 run 注入。
7. 工具适配：`tools/adapter.rs` 把宿主 `ToolDefinition` 包成 pi `AgentTool`，调用仍经过 registry 的 policy/guards/取消。
8. 运行闸门：`run_fence.rs` 保证单会话只有一个活动 run；取消会触发 token 并释放闸门；过期 ticket 不能发布结果。
9. 会话引擎：`engine.rs` 组合历史、闸门、工具与 loop，提供 `run`／`run_with_stream`、会话增删、配置与取消；`TurnObserver` 把文本增量、工具生命周期和终态推给 UI；运行结果持久化，取消或过期结果不写回。
10. `AgentHandle` 切换：内部改为 `Engine` + `TurnObserver`；公开方法不变；`index.json` 继续作为 UI 投影存储与兼容恢复源，结构化历史写入 `history.json`；旧会话可降级迁移；存储失败的报错语义保留。
11. 依赖移除：`flowy-agent` 不再依赖 `dsh-rs`／`cordis`／`futures`，投影不再引用内核事件类型。

## 证据

- core：`cargo test -p flowy-agent-core` → **26 passed / 0 failed**（真实 loop 执行宿主工具、错误终态、工具历史无损往返、传输构造、闸门、引擎持久化与并发拒绝）。
- agent：`cargo test -p flowy-agent`（TEMP 重定向到工作区可写目录）→ **17 passed / 0 failed / 1 ignored**（恢复、中断修复、损坏索引保护、保存失败上报、mock 回显、标题回退）。原测试全部保留并通过。
- 依赖树：`cargo tree -p flowy-agent --depth 1` → `flowy-agent-core` / `serde` / `serde_json` / `tokio`，无 `dsh-rs`。
- 删除：`dsh-rs` 目录与 workspace 引用已移除，回滚标签 `archive/dsh-rs-20261117`；验证见 [dsh-rs-removal-plan.md](dsh-rs-removal-plan.md)。
- 收尾项与验收标准：[agent-core-migration-handoff.md](agent-core-migration-handoff.md)。

## 上层 API 审计

- `flowy-studio` 使用：`with_config`、`lines`、`turn_status`、`working_elapsed_secs`、`has_messages`、`drain_updates`、`create_session`、`select_session`、`send`、`stop`，全部保留。
- `flowy-app` 使用：`list_sessions`、`active_session_id`、`apply_config`、`turn_status`、`has_messages`、`stop`；字段 `SessionSummary { id, title, updated_at }` 未变。
- `flowy-studio`／`flowy-app` 编译检查仍在后台运行（GPUI 依赖首次构建），结果决定收尾。

## 已知行为差异（需确认）

- 会话标题改为首轮提示的本地回退标题，不再走第二路 LLM sidecar；`title_system_prompt` 暂未使用。
- 实时投影只显示助手文本增量与工具开始/结束；推理增量（Thinking）未进入 `ChatLine::Reasoning`，恢复后的历史行仍原样保留。

## 已知环境限制

- 依赖解析需要可达 registry：本机 `flowy18_wip/.cargo/config.toml` 指向的本地镜像缺少 `eventsource-stream`，`--offline` 构建会失败。
- 通过 CLI 覆盖 `source.crates-io.replace-with="rsproxy-sparse"` 可解析并构建；未修改该配置文件。
- 生产分发需要让镜像包含 pi 依赖，或统一改用可达源；这是上线前必须解决的环境问题。

## 待办

12. 收取 `flowy-studio`／`flowy-app` 编译结果。
13. 全量验证：workspace check + 相关测试串行跑一次；核对依赖树不含 `dsh-rs`。
14. 确认标题 sidecar 与推理增量两项行为差异的处理方式。

## 约束

- 不提交、不推送，除非用户明确要求。
- 迁移完成前不删除 dsh-rs 或旧历史文件。
- 工具权限、取消、结构化历史三项必须有测试，不能因“薄”删减。
