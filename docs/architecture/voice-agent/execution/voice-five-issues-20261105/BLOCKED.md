# 语音五组问题修复 · BLOCKED / 未验项

## 硬阻断

无。未新增持久化格式、公共协议、依赖、鉴权/进程边界改动；代码改动均在任务书白名单内。

## 环境 / 端到端未验

| 项 | 原因 | 影响 | 替代验证 |
| --- | --- | --- | --- |
| TC12 真实两轮天气 + runtime execution/result + 物理扬声器 | 当前无可用真实麦克风、上游 runtime 账户与设备复跑条件 | 只能宣称“代码验证完成，端到端未完成” | host 141、integration 53、daemon 109、UI 72、joint 6、真实构建 |
| 现场 chip phase/seq | 原现场未同时抓 host snapshot 与 SSE | 最后一跳未现场闭环 | hook 探针覆盖 phase 更新/清理；reader 已移除 await 清理阻塞 |
| R4-1 投影超过 3s | 安全 fail-closed：宿主 delayed retry 仅覆盖约 1s；超过后 watchdog 进入 TextFollowUp，scope guard 拒绝迟到 submit | 该窗口不执行任务，只给未确认 text follow-up；避免越权/迟到副作用 | R4 pending 与自然投影恢复测试；scope guard 测试 |
| R2 TaskControl 专项并发红灯 | 缺 gate + 双会话并发夹具 | 代码已锁外 append，但无单独 integration | 全量 host/integration 回归 |
| R5 真实 Completed 播报端到端 | 需真实 task-result + 物理扬声器 | R-2 真实回执释放 + `announce_task_result` 单测绿，物理播报未真机验证 | `real_task_receipt_prevents_pending_watchdog...`、TextFollowUp gate |
| R3 registry 另一半 RMW | `resolve_target_conversation` 仍未纳入 workspace_registry_lock | 可能丢 last_user_interaction_at/复用指针，不影响消息/任务/标题 | 已登记；update_workspace_interaction 已加锁 |

## 基线红

- `bun run typecheck`：71 个既有错误（未改文件：videoCanvas/vite.config/jointTraceReplay.test），改动文件 0 error。
- `bun run check`：被同一批基线 typecheck 阻断。
- `cargo fmt --all` 在 Windows 环境报文件名过长；使用改动文件 `rustfmt --check` 替代并通过。

## 产物

- `ACCEPTANCE.md`：需求→修复→测试→结论。
- 本地未提交 diff：14 个修改文件 + `VoicePetInput.test.tsx` + 本目录文档。
