# 语音伴侣 · 当前代码事实与验证状态

> **状态**：🔧 部分实现；全链路已有代码与定向自动证据，尚未完成新版 PRD 全量验收。
> **核对日期**：2026-10-20。代码基准：`feat/xiaozhi-voice`，`10a1628abe600daf38ca1d3527073748d84637c0`。
> **用途**：回答“当前代码到底做到了什么”。旧计划、PRD 初稿中的“待实施 / 本轮只写文档”是当时状态，不能用来判断本次代码基准。产品目标和未完成的验收仍按相应 PRD 执行。

## 1. 当前流程与模块边界

当前仍是本地声学识别、文本模型、在线语音合成串联的架构。独立 `ai-mate` 负责采集、KWS、VAD、ASR、短对话和播放；宿主负责配置、会话绑定、持久化、窗口、公开快照及任务交付；复杂工作交给软件现有 Conversation/runtime。语音侧没有新增独立天气、文件或浏览器业务执行器。

| 环节 | 当前代码事实 | 主要入口 |
| --- | --- | --- |
| 唤醒与识别 | 生产 worker 使用本地 KWS / SenseVoice ASR / Sherpa VAD；已有音量、采集与阶段诊断、追问和停止路径 | [ai-mate voice](../../../../apps/ai-mate/src/voice.rs) |
| 快速理解与回复 | 原文经宿主对话入口调用已配置文本模型；区分聊天、澄清、忽略和委派；保留理解需求、回复及模型输出；已有流式句子消费路径 | [voice_dialogue](../../../../crates/backend/nomifun-app/src/voice_dialogue.rs)、[router](../../../../crates/backend/nomifun-voice-terminal/src/router.rs) |
| 真实任务 | 宿主冻结执行配置与交付正文，持久化后通过现有 Conversation 创建、发送、回执查询、取消及确认接口交付 | [voice_tasks](../../../../crates/backend/nomifun-app/src/voice_tasks.rs)、[voice_task_runtime](../../../../crates/backend/nomifun-app/src/voice_task_runtime.rs) |
| 结果与播报 | 接收真实任务投影，保留完整结果；提取适合口播的短文本，通知有去重、重播与播放回执；失败保留文字 | [voice_companion_manager](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs)、[ai-mate voice](../../../../apps/ai-mate/src/voice.rs) |
| TTS | 当前生产装配为 Edge TTS；不等于本地离线 TTS，也未切换到 Windows 系统声音 | [tts](../../../../crates/backend/nomifun-voice-terminal/src/tts.rs) |

配置与能力可用性不是同一件事；快轨聊天不需要先启动任务 runtime。启用语音、声学就绪和任务执行可用不能合并成一个“全部就绪”。以上是调用链事实，不代表所有网络、音频设备和安装场景已经签核。

## 2. 本次代码提交包含什么

| 项目 | 已落代码 | 当前边界 |
| --- | --- | --- |
| 交付上下文 | 冻结并保存任务交付正文；实际发送可使用该正文，目标与稳定命令身份仍独立保留；补充来源上下文和执行配置校验 | 不能将模型生成的目标当作用户已授权的一切工具操作 |
| 连续执行会话 | 相同声学 `session_id`、相同执行配置、已有任务 Conversation，且记录的最近活动距今不足 30 分钟时可复用创建身份 | 不是“任意跨唤醒、同工作区 30 分钟内必然复用”；具体差异见下一节 |
| 已有权限模式 | `VoiceExecutionConfig.session_mode` 可选；合法 `default / auto_edit / yolo` 透传，非法值归一为 `default`；仍禁用执行会话嵌套 delegation | 没有新增第四种权限模式，也没有将全部语音任务固定为 yolo |
| 工具授权 | 执行引擎对 `web_search / web_extract` 配置具体工具自动批准；语音策略允许相应搜索、提取别名；工具摘要补充搜索与网页信息 | 工具名允许不保证外部服务可用；未知工具和工作区越界仍拒绝 |
| yolo 下的语音策略 | 工作区内 Write / Edit / ApplyPatch 可直接允许；其他模式走审批；受支持的破坏性命令仍要求确认 | yolo 下 ApplyPatch 可包含删除；不能继续宣称“所有删除在所有模式下都必须口头确认” |
| 口头确认 | 拒绝优先，明确同意才允许；不明确时在总听取期限内最多澄清一次，听取上限 8 秒且受授权剩余时间限制 | 模糊语音不会默认同意；实际普通审批接缝的作用域与期限仍需专项回归 |
| 结果语义与通知 | 对明确表示未完成目标的文本增加失败判定；摘要提取保留否定信息；补通知身份冲突、去重、显式重播和失败提示处理 | 当前仍含文本启发式，不能将其视作完整的结构化目标达成判定 |
| 回归测试 | 覆盖上下文绑定、配置冻结、通知回执、审批、状态查询及任务不重复执行；同步模式测试以符合 v0.3 §7.2 决策 | 自动替身、真实工具执行和真人设备证据分别计数 |

本提交没有增加数据库迁移、生产替身或新 runtime。`ai-mate` 的回放 feature 只用于测试；正常依赖图中未启用 `voice-skeleton`。

## 3. 会话选择的实际规则及尚存差距

目前宿主优先使用已上报且属于当前本机用户的合法 `nomi` 会话。候选会话忙碌时，将语音对话记录放到默认“语音助手”入口，并保留来源会话信息。未选中合法会话时，可按配置选择宠物会话；否则确保默认“语音助手”Conversation 存在。具体规则见 [resolve_target_conversation](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs)。

**对话记录落在哪里，与后台任务 Conversation 是否复用，是两个判断。** 任务侧当前根据声学 `session_id` 寻找历史创建身份，并以 `last_receipt_at_ms / last_observed_at_ms / send_started_at_ms` 计算活动时间；它不是独立的、按工作区和最后用户交互时间维护的 30 分钟会话租约。因此：

- 新唤醒如果产生不同声学会话身份，不能仅凭“同工作区且不足 30 分钟”保证复用。
- 查询和回执会影响当前活动时间口径；它与“最后一次用户发言后 30 分钟”不完全相同。
- 新目标仍有自己的 command、message、runtime turn；复用执行会话不等于复用任务身份。
- 当前没有把“项目工作区中唤醒，必定创建该工作区专属语音会话”实现为统一规则。
- 宠物仍是可选实体，可以没有或存在多个；没有交付“安装自带、不可删除但可隐藏的唯一语音入口宠物”。

另一个待核实边界是普通审批的投影：当语音作用域审批列表为空时，适配层会读取普通待审批列表，并生成 `now + 60 秒` 的展示期限，见 [manager/nomi/agent](../../../../crates/backend/nomifun-ai-agent/src/manager/nomi/agent.rs)。这不是证明原授权期限不变的证据；错轮次、重复查询及临近过期场景仍须验证。本文只记录现状，不在文档同步任务中修改审批实现。

## 4. 当前验证记录

本节记录本次提交前实际执行结果。完整日志在本地忽略目录 `target/voice-review/`，不作为安装包资源或产品状态提交。

| 检查 | 本次结果 | 证据与限制 |
| --- | --- | --- |
| `cargo test --locked -p ai-mate -p nomifun-api-types -p nomifun-app -p nomifun-conversation -p nomifun-ai-agent --lib voice` | 首轮 91 通过、3 忽略，Conversation 有 1 项旧模式断言失败 | `prepush-rust-voice.log`；旧断言要求 yolo 变为 default，与已修订权限决策不符 |
| `cargo test --locked -p nomifun-conversation --lib voice` | 修正旧断言并增加合法/非法模式覆盖后，1 通过 | `prepush-conversation-retest.log`；两次合计当前覆盖 **92 通过、3 忽略**，不是完整 workspace 测试 |
| `cargo test --locked -p nomifun-app --test voice_dialogue_integration` | 14 通过，0 失败 | `prepush-dialogue-integration.log`；包含无宠物落盘、忙碌候选、冻结工作区、导航后绑定不漂移、前轮城市上下文、正文重放不重复生成 |
| `cargo test --locked -p nomifun-api-types --test c2_voice_dto_golden` | 7 通过，0 失败 | `prepush-voice-dto.log`；含序列化契约与提前输出的聊天句子不能转为任务 |
| `cargo test --locked -p nomifun-voice-terminal --lib` | 21 通过、1 忽略，0 失败 | `prepush-voice-terminal.log`；含 TTS 限流重试、冷却、空音频失败、原始采样率、播放完成与停止竞态；真实 Edge 在线探针未执行 |
| 生产 feature 图 | `cargo tree --locked -p ai-mate -e features,normal,build` 通过，未启用 `voice-skeleton` | `prepush-production-features.log`；不代表全部桌面 feature 已构建 |
| 相关仓库门禁 | process-runtime-boundary、windows-console-hide、error-surface-contract 均通过 | `prepush-boundary.log`、`prepush-console.log`、`prepush-error-surface.log` |
| 聚合 `bun run check` | UI 类型检查失败，报告 60 项 TS 错误 | `prepush-quality.log`；本批没有 UI 变更，失败文件在提交前已存在；后续聚合门禁未运行，不能宣称全绿 |
| `cargo fmt --all -- --check` | 未有效完成 | Windows 命令长度错误 206；仓库 `rustfmt.toml` 另有 `disable_all_formatting = true`，定向 rustfmt 返回 0 也不能证明格式合格 |
| 文档静态核对 | PRD 8 节、18 个唯一 AC、相对链接与三份历史报告保留检查通过 | Mermaid Node 解析受 `DOMPurify.sanitize is not a function` 阻断；没有图形渲染通过的证据 |

本次新增快照及关联导航共 7 份文档另经静态核对：79 个相对链接可解析，3 份已跟踪文档的历史行及 PRD 内 3 份原始报告保留。`git diff --check` 通过。

以上定向测试按修正后结果合计 **134 通过、4 忽略**。忽略用例不计为通过；不把库级 TTS 回归当作在线 Edge 或实际扬声器验证。

主测试中的真实工具用例经过现有 Conversation/runtime 读取测试时新生成的随机文件，也覆盖停止语音不取消任务、实际删除/危险命令等待审批等场景。声学或模型替身只属于测试控制，不等于真人麦克风、在线模型及物理扬声器实测。

本轮未重新启动桌面应用，未做安装包、Mac/Linux、真人录音/扬声器或性能签核。历史上用户已反馈基础语音与任务全链路可用，但这不能代替本次提交产物的 Owner 验收。完整 workspace 编译、桌面联合构建和发布门禁也不能由以上定向检查推定通过。

## 5. 后续方案与验收入口

| 文档 | 当前角色 |
| --- | --- |
| [v0.3 PRD](../PRD-voice-companion-v0.3.zh.md) | 产品目标与验收要求；部分代码已落地，不是全量完成证明；权限冲突以 §7.2 的明确修订为准 |
| [快速前台与软件会话 Subagent 计划](runtime-dispatch-closure-plan-2026-10-19.zh.md) | 原执行方案与历史底稿；实施状态先读本文，再逐项对照出口门禁 |
| [Qoder CN 静态调研](qoder-cn-live-voice-research-2026-10-20.zh.md) | 已安装产品产物与公开资料的调查；不等同于获得其完整开源实现或确认云端模型 |
| [Qwen 与工作区语音深入调研](qwen-qoder-workspace-voice-deep-research-2026-10-20.zh.md) | 固定上游源码与候选交互方案；未将其运行时接入 Flowy |
| [Owner 实测入口](T5-owner-handoff.zh.md) | 真实设备、任务、停止/取消和安装体验的交接参考；需按当前新增边界补测，不能沿用旧结果自动签核 |

下一轮应先补齐工作区内连续任务与跨唤醒复用的明确语义、审批作用域/期限和可靠通知，再决定默认宠物、队列及实时语音的纵向切片。Realtime / Omni / AOQ、Qoder 式默认宠物与工作区专属语音会话均属候选升级，当前没有产品化实现与性能数据。

## 6. 历史记录的阅读规则

本文是首次新增的代码事实快照，不替代需求标准，也不删除旧证据。目录、PRD 和计划中的旧阶段说明保留原文；以日期、代码基准及本次验证区分“提出方案、已有实现、自动测试通过、真机通过、Owner 签核”。后续更新继续追加记录，不用“全链路已有代码”替代未完成的验收。
