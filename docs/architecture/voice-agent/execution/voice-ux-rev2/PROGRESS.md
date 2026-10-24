> **2026-10-24 清理准备缺口修复**：空快照/未绑定/错 session 不再执行清理 API，状态恢复后显式重试；clearContext 与 required reset 共用成功提交屏障。正式回归 157、共享历史 100、10 项定向门禁通过。详见 [修复记录](RESET-READINESS-REPAIR-2026-10-24.zh.md)。未提交、未推送；下方历史保留。

> **2026-10-24 接续修复完成**：已执行独立复审 F1–F3 的定向收尾；147 项语音前端、100 项共享历史通过；六条双语真实组件交互通过；原 61 项 TS 诊断未新增。未提交、未推送。见 [接续修复验证](REPAIR-VERIFICATION-2026-10-24.zh.md)。

> **2026-10-24 当前进度**：E1 独立审查问题已修复，并已实施 E2 输入框/列表和 E3 设置精简。E4 已完成代码/服务/构建验证，原生视觉/物理设备边界未签核。以 [IMPLEMENTATION.zh.md](IMPLEMENTATION.zh.md) 和 [当前 29 项证据](RESULTS.zh.md) 为准；下面是原始开工与过程记录，不代表当前仍停在 E1。

# 语音交互优化 rev2 开工回执

- **基准**: HEAD `7de276e61e93836365c5a1c7e006c557ce3e3c6d`, dirty 范围仅限既有设计与架构文档
- **目标**: 跨会话轮次与外任务通知彻底隔离、输入框单主控接替并移除旧栏、侧栏声波、设置精简与桌宠回归
- **顺序**: 严格串行推进 E0(基准与迁移) -> E1(正确性与隔离) -> E2(输入框与侧栏) -> E3(设置精简) -> E4(全量验证)
- **最大风险**: 异步时序交错（持久消息晚于回执到达、外任务候选被误认领/阻塞、旧栏移除丢失审批/取消操作）
- **基线实跑**: 原方案 6 文件 32 pass / 0 fail / 155 assertions (3.04s)；TS 基线 61 项无关既有错误 (0 语音错误)
- **隔离探针**: 两个反例（旧工作区跨会话串台、外任务通知冒充当前助手消息）已确认 RED

## 2026-10-24 接续修复与 E2/E3 实施（进行中）

本轮继承并保留 E1 未提交改动；分支仍为 `feat/voice-next`，未提交、未推送。

- 修复审查 F1/F2：宿主 Option 会话 ID 比较、前端 turn 的 session_id 类型。
- 修复 F3/F4：原始 session 缺失或不符时拒绝临时轮次；投影缓存按 session/conversation/turn 三元组查询；KWS 和按钮启动均清理易失轮次；不因全局外任务屏蔽本会话合法输入。
- 修复 F5：以最新页请求序号及 epoch 接替 live 文本；回执前发起的旧 GET 不算接替证明；用户/助手各自退休；状态限定于挂载视图和 worker 三轮窗口；同步失败提供显式重试。
- 实跑发现漏收 SSE 后绑定未恢复会阻断播报；状态查询与 SSE 现复用同一绑定恢复路径，恢复仍核对 generation/session。
- E2：删除独立 VoiceConversationBar，输入框 footer 接替阶段、审批、unknown 核对及队列逐项取消；大主键优先级不变，必要时增加小挂断；保留未知启动 request_id 的恢复入口；严格类型谓词共用于侧栏、聊天、发送，右侧静态声波不依赖名称；语音附件、文件提及与显式技能入口收口。
- E3：快速模型 UUID 诊断移入高级折叠；删除长唤醒容错宣告及概览重复模型/音色配置；仅保留缺模型 onboarding。私聊选模和 profile.voice.tts 契约保留；首页标签为“语音通话 / Voice call”。

已实跑：宿主 voice_ 87 pass/0 fail/3 ignored；worker 包各 suite 通过；真实数据库 voice_dialogue_integration 26 pass/0 fail；前端 18 文件 138 pass/0 fail/891 assertions。最终构建、门禁、类型基线对照及新增宿主反例仍在收口，不能将这些中间结果作为 E4 已全部签核。

## 最终收口补充

- 共享 MessageList/历史加载链额外运行 9 个测试文件：86 pass / 0 fail / 283 assertions，覆盖 merge、loadOlder epoch、reset、刷新通道与 retry、流式缓存、折叠及滚动入口。日志：`build.noindex/voice-workspace-display-review/fix-history-shared.txt`。
- TS 最终诊断集合为 61 项，与原有 61 项逐条一致；Rust 6 文件格式检查及 12 项最终静态门禁通过；桌面 feature 编译与最终 UI 构建均 exit 0。
- 保留全部原始文档与未提交代码；未创建提交、推送、切换分支或启动新的自动任务。

最终队列条目级防线追加后，定向 3 文件 8 pass/0 fail/168 assertions；TS 仍为同一组 61 项既有诊断，新增 0；`bun run build:ui` 最终交付构建 exit 0（1m 48s）。未以定向重复用例增加“唯一通过项”计数。
