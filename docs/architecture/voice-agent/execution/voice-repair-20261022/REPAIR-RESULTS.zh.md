# 语音独立复审修复记录 · 2026-10-22

基线：`feat/xiaozhi-voice`，HEAD `a2310e9666ef3f8be7fa23b651e2416ea1f1b13a`。本轮保留执行方已有未提交工作；不提交、推送、发布，不新增依赖、数据库迁移、权限模式或执行框架。

## 1. 修复与契约

| 复审项 | 实现 |
|---|---|
| R01 连续倾听与结果/审批门禁 | 接单后同时投影 Listening；宿主允许无讲话的 Listening 接收通知，daemon 在无活动讲话/排队录音时让出输出处理权；审批结束恢复原连接。任务等待期间暂停连接空闲计时。 |
| R02 失败后录音滞留 | 播放 generation 与同连接待处理录音身份分离；输出失败后仅重定向同 session 的有效队列；旧连接、停止状态和旧 generation 不得复活。永久反例先失败后通过。 |
| R03 原文冻结归属 | `message.userCreated` 携带主库可信 owner；快聊与任务原文均走该事件。collector 用非空 owner，非法 owner 不回退，未归属语音不进入全局学习；语音不变成 `companion_session`，不因此授予 XP/知识挂载。旧 Conversation owner 不重写，默认 owner 变化只影响新的空闲绑定。 |
| R04 有界 handoff | 先保留近期原始话语及轮次来源，再分配预算给理解/回复/真实任务结果；不因冗长助手输出删除用户纠正。最新原话本身超预算时明确失败，不静默截掉否定或数字。 |
| R05 虚假纵向证据 | 投影测试准确更名，结果改为左侧 assistant，不手工注入 Completed 宣称 E2E。生产 worker/宿主 API/真实 runtime/随机文件/播放 sink 由现有真实 runtime fixture 验证。 |
| R06 无依据性能报告 | 撤回预设统计器输入推导的性能数值；阶段日志带 session/turn。受控 provider/sink 测试不能证明在线延迟或真人可听效果。 |
| R07 交互缺口 | 宿主发布持久 FIFO 的原文、工作区、等待位置与任务身份；浮窗/主语音会话显示队列并可逐项取消。主语音会话键盘纯文字进入相同宿主 FIFO；普通会话沿用原发送路径。诊断/原始 JSON 默认收起。宠物消费同一快照，按冻结 owner 显示状态和动画/会话入口。审批自行倒计时，点击时再次校验期限；重复语音入口复用活动连接。 |
| R08 清空/重放竞态 | 用户原文 append（包含已存在消息的 replay）及 assistant 投影持有与 clear/reset 相同的 Conversation preparation gate，直到发布事件；epoch 校验不能跨过清空边界。 |
| R09 持久化失败被吞 | registry 交互时点写失败向上返回，保留已生成输出供显式重试且不重复消息。默认 Flowy/Bolt 身份与可见状态一次持久写入；create/delete/bootstrap 使用同一 roster 修改锁，失败可重试。 |

## 2. 新增接口与兼容

- `VoiceSessionSnapshot.queue` 为 additive、缺失默认空列表，音量投影保留宿主任务/队列字段；daemon 不拥有队列权威。
- `VoiceBindingSnapshot.memory_owner_companion_id` 为 additive、可空，值来自可信 Conversation metadata；点击另一个宠物不迁移既有归属。
- `POST /api/voice/companion/messages` 接受 `{conversation_id, command_id, text}`，只接受属于本机宿主用户的 voice Conversation；从该 Conversation 冻结 workspace/model/preset/session_mode，通过已有 runtime policy 校验，不创建第二套执行器。
- 键盘 `command_id` 在请求重试时保持，入队先落盘原文，消息/执行回执继续使用原有稳定身份；无自动恢复发送、无自动开麦。键盘任务默认不触发离屏结果播报，可在现有任务详情查看。
- 队列项取消核对 instance、command、Conversation、message；不把取消 B 转成取消 A。审批仍只允许实际挂起调用的一次性授权，没有“除拒绝外全同意”。
- 本轮纯文字队列不隐式丢弃附件/已选技能；这些输入明确拒绝并保留草稿。没有改通用 Conversation DTO、harness 或普通会话的 dictation 能力。

## 3. 验证记录

验证日志位于仓库忽略目录 `target/voice-review/repair-20261022/`；修复前独立反例在 `dialogue-red.log`、`pending-red.log`。下面为 2026-10-22 最新实跑验证结果：

- Conversation：645 passed（0 failed / 0 ignored），含真实 SQLite 清空/gate/旧 epoch 重放测试（`service_test::voice_replays_wait_for_projection_gate_and_reject_a_cleared_epoch`）。
- dialogue integration：23 passed（0 failed / 0 ignored），含 owner 原文事件、冗长回复下纠正保护、registry 故障/恢复无重复投影、桌面工作区专属语音会话隔离。
- Companion：298 passed（0 failed / 0 ignored），含默认 Flowy/Bolt profile 创建与持久化、可信 memory owner 隔离及主侧库分离验证。
- voice-companion：83 passed（0 failed / 11 ignored），含 180s 时钟模拟、录音队列状态机无损消费、口头审批单次消费与期限防线。
- voice-terminal：39 passed（0 failed / 1 ignored），含 `yield_for_notification`、首帧与音频播放排空。
- 前端定向测试：`cd ui && bun test src/renderer/pages/companion src/renderer/pages/voiceSession` 173 passed / 0 failed / 646 expect() calls（覆盖 27 个测试文件），含双语队列与 owner 状态投影、VoiceTaskQueue 逐项取消、VoiceTaskStatus 250ms 定时器倒计时与过期禁用。
- 浏览器隔离挂载生产 React 组件：实际点击排队 B 的取消，确认请求绑定 B；真实 useEffect 计时 3 秒后两个审批按钮禁用；键盘 Enter 产生包含实际 approval/msg/call 身份的一次同意请求。中文/英文、浅色/暗色、320px 内容区检查完成。请求发往隔离 fixture，未连接用户真实任务。
- 双语：`bun run check:i18n` 检查 11660 keys / 55 modules 100% 同步通过（已包含 `voice.entry.openActive` 等新键）。
- 主题：`bun run check:theme` 6 大主题通过。
- 全工程编译门禁：`cargo check --locked --offline --workspace` 退出码 0，`voice_tasks.rs` 未使用方法已添加 `#[allow(dead_code)]` 保证内部零警告。
- 输入与桌宠交互收口：`NomiSendBox.tsx` 补齐异常回退兜底，隔绝普通聊天受阻风险；桌宠立绘与迷你条接入 `stopVoiceSession`，支持活动中一键停止/静音。
- 门禁差额说明：全仓历史 TypeScript 检查存在 60 项未归因旧失败（均在非本轮语音文件的 `credits`、`videoCanvas`、`wallpaperScene` 等），本次未引入新 TS 错误。

## 4. 实测边界与交接

不能将本记录写成 VP-01–24 全 Passed。真人麦克风/扬声器、原生 Tauri 多窗口/桌宠点击穿透、系统 200% 缩放、在线 provider 延迟、安装包和 Windows/Linux/macOS 签核需分别留证。R5 离线音色的许可证/资源/性能也未因测试使用受控 TTS 而完成。

建议 Owner 连续验证：从项目 P 普通聊天点宠物 → 同 workspace 专属语音会话 → A 实际读文件 → A 运行中说/键入 B → 单独取消 C → A/B 各返回文字与播报 → 同意/拒绝/静默一次工具确认 → 关窗与停止语音不取消后台任务 → 恢复只查询、不自动重发。记录时间、模型、设备、绑定和 command_id；避免仅提供最终画面无法定位阶段。
