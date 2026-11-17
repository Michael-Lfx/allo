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

- core：`cargo test -p flowy-agent-core` → **34 passed / 0 failed / 2 ignored**（真实 loop 执行宿主工具、错误终态、工具历史无损往返、传输构造、闸门、引擎持久化、并发拒绝、历史保存失败上报、推理增量、模型切换、pi 结构化错误映射、catalog 限制、标题持久化、平台 shell 选择；pi bash / Windows PowerShell 各以 ignored 用例单独验证通过）。
- agent：`cargo test -p flowy-agent`（TEMP 重定向到工作区可写目录）→ **24 passed / 0 failed / 1 ignored**（恢复、中断修复、损坏索引/历史保护与隔离、保存失败上报、mock 回显、标题回退、工具注册与路径约束、推理投影、index 重建、guard 豁免）。
- 依赖树：`cargo tree -p flowy-agent --depth 1` → `flowy-agent-core` / `serde` / `serde_json` / `tokio`，无 `dsh-rs`；core 额外依赖同 SHA 的 `rpi-tools`（bash 工具来源）。
- 删除：`dsh-rs` 目录与 workspace 引用已移除，回滚标签 `archive/dsh-rs-20261117`；验证见 [dsh-rs-removal-plan.md](../plans/dsh-rs-removal-plan.md)。
- 上层：`cargo check --workspace` 退出 0；`cargo build -p flowy-desktop` 退出 0，`target/debug/flowy-app.exe` 已更新。
- 收尾项与验收标准：[agent-core-migration-handoff.md](../plans/agent-core-migration-handoff.md)。
- 多端 shell 策略：[agent-shell-strategy.md](agent-shell-strategy.md)。

## 上层 API 审计

- `flowy-studio` 使用：`with_config`、`lines`、`turn_status`、`working_elapsed_secs`、`has_messages`、`drain_updates`、`create_session`、`select_session`、`send`、`stop`，全部保留。
- `flowy-app` 使用：`list_sessions`、`active_session_id`、`apply_config`、`turn_status`、`has_messages`、`stop`；字段 `SessionSummary { id, title, updated_at }` 未变。
- `cargo check --workspace` 与 `cargo check -p flowy-app --tests` 均退出 0。

## 审查修复（2026-11-17）

1. 工具接线：生产 registry 注册 `bash`／`read_file`／`write_file`／`edit_file`／`glob`／`grep`；`todo_write` 暂无宿主面，不对外暴露。工具 cwd 固定为 `{store_root}/sessions/cwd`，文件工具 guard 限制在该目录内。
2. 标题 sidecar：首轮后异步生成模型标题；失败或超时保留本地回退标题，不阻塞发送。
3. 历史保存：`Engine` 写失败通过 `TurnObserver::storage_error` 上报，UI 收到 `StorageError`。
4. mock 分支错误：与 live 分支一样进入 `mark_failure`，不再永久停在 `Running`。
5. history 损坏：重命名为 `history.corrupt-<时间戳>.json` 后用 index 投影恢复，避免整机不可用。
6. 传输：连接 15s 超时；请求/流空闲超时交给 pi 的默认值（600s/300s），不再自设 120s 总超时。
7. bash：直接使用 pi-rust `rpi-tools` 的 bash 工具（POSIX bash、Windows git-bash 探测、默认 120s、流式尾部截断 2000 行/50KB、完整输出落临时文件），不再维护自研 shell 捕获。
8. stop 竞态：`mark_failure` 只覆盖 `Running`，不再把 `Stopped` 改成 `Error`。
9. 模型元数据：`LlmConfig`／`ModelRoute` 增加 `context_window`／`max_tokens`，catalog 值传入 pi `Model`。
10. 推理投影：`ThinkingDelta` 进入 `ChatLine::Reasoning`，运行中实时可见，结束后落为持久行。

### 复审修复（2026-11-17 第二轮）

1. pi 工具失败保留 `StructuredTool` 的 content 与 `failure_kind`，不再只显示 `structured tool error`。
2. 标题 sidecar 改为自由函数，只捕获 `Arc<UiState>`；stop 取消其 token，且只在 `LoopOutcome::Completed` 后触发。
3. 损坏 `index.json` 隔离为 `index.corrupt-*.json`，并从结构化 history 重建可见会话；history 仍只在 `InvalidData` 时隔离。
4. create/select/stop/ensure/hydrate/title 的索引写入失败统一上报 `StorageError`。
5. guard 对已存在路径做 canonicalize、Windows 大小写不敏感比较；bash 明确豁免并有测试。
6. 推理计时在首个文本 delta 停止；适配器降为私有，`description` 进入工具行目标，metadata 映射有测试。
7. 标题同时写入结构化 history（`Engine::set_session_title`），启动时把 index 中的标题回填到 history，index 丢失后仍能恢复模型标题。
8. 多端 shell：Windows 默认注册 pi-rust `powershell`，macOS/Linux 默认注册 `bash`；宿主只暴露一个平台 shell，详见 [agent-shell-strategy.md](agent-shell-strategy.md)。

## 已知行为差异

- 标题 sidecar 在无 key 或调用失败时使用首轮回退标题。

## 已知环境限制

- 依赖解析需要可达 registry：本机 `flowy18_wip/.cargo/config.toml` 指向的本地镜像缺少 `eventsource-stream`，`--offline` 构建会失败。
- 通过 CLI 覆盖 `source.crates-io.replace-with="rsproxy-sparse"` 可解析并构建；未修改该配置文件。
- 生产分发需要让镜像包含 pi 依赖，或统一改用可达源；这是上线前必须解决的环境问题。

## 待办

1. 真实云端冒烟：验证工具调用与模型标题在真实网关下的行为（当前仅有离线测试与一次手工文本回复证据）。
2. 观测接入：见 [agent-observability-plan.md](../plans/agent-observability-plan.md)。
3. 镜像分发仍待处理。

## 约束

- 不提交、不推送，除非用户明确要求。
- 迁移完成前不删除 dsh-rs 或旧历史文件。
- 工具权限、取消、结构化历史三项必须有测试，不能因“薄”删减。
