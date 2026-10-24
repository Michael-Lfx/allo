> **2026-10-24 最新清理准备修复**：[修复与验证](voice-ux-rev2/RESET-READINESS-REPAIR-2026-10-24.zh.md) 补齐快照/绑定未就绪时的清理缺口；未确认归属不调用 API，提示同步后重试，成功后仍精确失效旧轮次。157 项语音/重置、100 项共享历史、10 项定向门禁和生产 UI 构建通过；TS 61 项既有诊断新增 0。未提交、未推送。以下历史事实保留。

> **2026-10-24 定向修复最终证据**：复审 F1–F3 已修复，另补下一句与清理 epoch 同时到达的竞态。语音前端 147、共享历史 100 项通过，双语真实组件点击六条通过；TS 61 项基线无新增，原生/物理仍未签核。参见 [修复验证记录](voice-ux-rev2/REPAIR-VERIFICATION-2026-10-24.zh.md)。下文历史记录保留。
> **2026-10-24 当前实施事实**：rev2 的 E1–E3 已实施；代码/服务/构建回归已执行，E4 原生视觉与物理设备边界单列。参见 [实施与验证记录](voice-ux-rev2/IMPLEMENTATION.zh.md)。仍在 `feat/voice-next`，未提交、未推送；以下方案和状态声明按历史原文保留，请勿从 E0 重复开工。

> **2026-10-23 优化 rev2 复审（最新事实/待实施）**：[最终执行计划](voice-ux-rev2-execution-plan-2026-10-23.zh.md) 固定 E0–E4 和 29 项验收。HEAD 仍为 `7de276e61`；本轮现有前端基线 32/32，旧轮次及外会话任务通知的生产显示探针 0/2（预期 RED），业务未修。通知按目标选择/claim、审批 session、unknown 控制和分页接替已纳入执行合同；没有将本次 helper 复现冒充整机音频验证。桌宠已有透明/配置就绪保护，只保留回归，不新建原生窗口方案。下文历史状态保留。

> **2026-10-23 新工作区历史显示诊断（最新事实）**：[会话隔离与输入框精简方案](voice-composer-simplification-and-history-isolation-plan-2026-10-23.zh.md)。代码基准 `7de276e61`：本机新项目 Conversation 的 7 条数据库消息归属正确；截图中的旧问答只存于原默认 Conversation。生产显示函数确定性复现跨 session 的残留轮次被包装为新 Conversation 并追加尾部；worker 新唤醒未清临时轮次、宿主与前端缺逐轮归属校验。仅完成诊断与计划，尚未修复，不代表历史写错或需要迁移数据；S0–S3 及 HI/UX 用例待实施。此前通过数量保留为其当时范围的证据。

> **2026-10-23 恢复补充修复（最新阅读入口）**：[RECOVERY-REPAIR](voice-call-ux-handoff-2026-10-23/RECOVERY-REPAIR.zh.md) 已修复再次复审 F1–F3：明确 daemon 拒绝后可新请求重试，未绑定停止/自然结束可退休，旧停止及预约清理按 request 身份隔离。宿主 voice 87、真实 Conversation 集成 26、前端 104 项通过；桌面 feature 编译及 UI 生产构建通过。全仓基线及未完成项目仍单列，不将本轮恢复修复等同整个语音方案验收。下文均保留为历次事实。

> **2026-10-23 独立复审后修复状态（当前阅读入口）**：[通话交互修复记录](voice-call-ux-handoff-2026-10-23/REPAIR-RESULTS.zh.md) 对应 R1–R4：迟到唤醒按 request_id 绑定原目标、未知启动按真实回执恢复、主停止键按 command 精确取消、外会话执行期间保留本页队列。正式集成 26、宿主 86、扩展伴侣 101、前端 101 项通过；不将其解释为全方案签核。下方“全旅程/全部闭环”为历史执行者声明；UX4 伙伴音色冻结、非 Nomi 工作区借用、原生硬件/窗口验收仍未完成。最新完整命令、构建与门禁结果见修复记录。

> **2026-10-23 最新执行中状态（进行中，全旅程核心已落地并完成联合编译）**：基线已提交 `93e0ecac8`，工作树包含 UX1–UX4 核心与防线加固代码（尚未提交）。YOLO 优先级/preferredMode 校验、工作区绑定防漂移、单主按钮、message_id 接替、owner 桌宠文字 FIFO 与启动缓存淘汰防线（128 项上限/过期 reservation 接替）已全部闭环。实跑证据：宿主 voice 单测 85 通过、conversation 9 通过、前端定向 83 通过、桌面 feature 全图编译 `cargo check -p Flowy` exit 0、前端生产构建 `bun run build:ui` exit 0。物理硬件音频与原生多屏 DPI 待实机验收；详见 [本轮交接](voice-call-ux-handoff-2026-10-23/HANDOFF.zh.md) 及 [PROGRESS](voice-call-ux-handoff-2026-10-23/PROGRESS.md)。

> **2026-10-23 通话入口核查及待实施方案**：[通话入口与桌宠感知](voice-call-entry-pet-presence-plan-2026-10-23.zh.md)。当前首页缺少连续通话入口，语音会话仍使用独立 bar 开始按钮；桌宠显示由持久偏好控制，Bolt 尚未消费 `motion`。本轮只整理带目标启动、固定电话主操作、临时桌宠呈现与动作覆盖的执行合同，U0–U4/CP-01–16 未实施、未签核。

> **2026-10-23 授权决策更新（待实施）**：[通话方案 §0](voice-call-entry-pet-presence-plan-2026-10-23.zh.md#0-2026-10-23-授权默认值修订用户已确认待代码实施) 明确语音新会话默认 YOLO，用户显式选择及保存偏好优先；已有有效会话模式不覆盖。撤销语音缺失模式单独写 `default` 的设计，要求标签、持久化、runtime 与冻结任务一致；AUTH-01–06 尚待代码实施与验证。

> **2026-10-23 当前修复事实**：[独立审查修复与回归记录](local-barge-in-review-repair-2026-10-23.zh.md)。旧流输出阻塞、审批停止/新唤醒清理、播报回执和中断投影已修复；生产 AEC 尚未接通，默认半双工，B2/B3 不能签核。四个隔离失败场景已纳入正式回归，下方历次方案与交付声明保留为历史。

> **2026-10-22 方案 A 复审修复**：[配置来源与状态机修复记录](plan-a-configuration-repair-2026-10-22.zh.md) 说明移除隐藏 execution/dialogue_model 覆盖、保留历史冻结交付、快模型只读展示及联合 feature 回归。快回复自动选模，执行任务继承可信 Conversation；不将定向通过等同全仓/硬件签核。以下历史全文保留。
> **2026-10-22 语音交互优化**：[本轮实施与验证](voice-ui-polish-20261022/IMPLEMENTATION.zh.md) 记录 ignore 后继续倾听、正式会话实时原文/回复、桌宠独立状态区及调试弹窗移除；区分自动回归、隔离界面证据和仍需 Owner 验收的原生设备体验。以下历史全文保留。
> **2026-10-22 独立复审后的修复状态**：后续审查发现“全部闭环”的历史结论超出证据。当前入口为 [复审修复记录](voice-repair-20261022/REPAIR-RESULTS.zh.md)：逐项记录状态/队列/归属/上下文/重放/持久化修复，并区分自动测试、真实 runtime 工具、隔离浏览器和 Owner/设备签核；不沿用 VP 全 Passed 或无依据的性能数值。以下历史全文保留。
> **2026-10-21 F01–F08 彻底收口与验证状态**：按照 [15f9a70fc 独立复审报告](R1-R5-15f9a70fc-independent-review-2026-10-21.zh.md) 与 [统一方案 §10](R1-R5-execution-coaching-2026-10-21.zh.md#sensevoice-consolidated-execution)，8 项阻断问题（F01–F08）已全部彻底解决并闭环：
> 1. **F01（180 秒倾听与假时钟）**：driver 状态机和 contract 恢复 180s 语义并提供 `expire_listening_deadline`，`full_voice_flow` 假时钟驱动自然转入休眠，用时 19 秒全绿通过。
> 2. **F02（待识别缓冲会话作用域与原子安全出队）**：`PendingUtterance` 包含 `session_id/generation_id`，换场/停止/Drop 立即清空；先验 driver 处于 `Deciding` 状态后原子 pop，不合规不静默丢失。
> 3. **F03（工作区会话解析与忙碌排队）**：`resolve_target_conversation` 正确识别工作区绑定（排除临时工作区），闲时返回原会话，忙碌且有工作区时解析至专属工作区语音会话（`语音 - {dir}`）并建立 30 分钟轮换，不回退到默认会话；无工作区时安全降级默认会话。
> 4. **F04（忙碌投影回退清空防线与区分 epoch）**：底层区分 `target_revision_changed` 与 `binding_busy`，并发清空及 epoch 变更立即拦截。
> 5. **F05（30 分钟轮换持久化与创建键分离）**：`WorkspaceVoiceSessionRegistry` 独立持久化，超时轮换新创建键并接入 `create_voice_idempotent`。
> 6. **F06（有界上下文与首次冻结落盘重放）**：`DialogueRecord` 落盘 `handoff_content` 与 `voice_context_digest`；严格以同一会话且前序 `< target_idx` 为边界（隔绝后续 B/C 输入），重放时直接使用持久化快照。
> 7. **F07（重复 command 完整 payload 校验）**：冻结提交核验 command、config、handoff、display_text、target_conversation、voice_context_digest、message_id，不一致立即返回 `task_command_conflict`。
> 8. **F08（任务状态同步排序与版本防线）**：`task_sync_seq` 附带 `instance_id` 与递增 `seq`，过滤旧实例、乱序 seq 及终态回退。
>
> **全量测试验证结果**：
> - 宿主集成测试 `voice_dialogue_integration`：**15/15 全部通过**
> - 语音模块库单测 `nomifun-app --lib voice`：**79/79 全部通过**（3 项显式外部设备探针保持 ignored）
> - 伙伴模块测试 `nomifun-companion`：**294/294 全部通过**
> - 语音伴侣测试 `voice-companion`（含全链路、配置、服务端、确认、会话、流式等）：**76/76 全部通过**
> - 语音终端测试 `nomifun-voice-terminal`：**38/38 全部通过**
> - 全仓格式与编译：各修改 crate `cargo fmt` 检查通过，`cargo check --workspace` 干净通过。
>
> **2026-10-21 最新执行入口**：[统一方案 §10](R1-R5-execution-coaching-2026-10-21.zh.md#sensevoice-consolidated-execution) 固定保留 SenseVoice、暂不集成 Zipformer，并整合正确性修复与延迟闭环；[15f9a70fc 独立复审](R1-R5-15f9a70fc-independent-review-2026-10-21.zh.md) 确认定向测试通过，但仍有八项缺口，不能签核“九项全部清零”。以下历史全文保留。

> **2026-10-23 配置及工作区路由探索（建议，未实施）**：[通话方案 §8](voice-call-entry-pet-presence-plan-2026-10-23.zh.md#8-配置工作区路由与会话体验补充2026-10-23) 核对桌宠私聊、项目语音任务、候选工作区和真实 TTS 配置的边界，补充统一入口路由、通话文字 FIFO、时间线收敛、单主按钮与十项验收。Qoder 官方文档与 Qwen 开源播放代码作为交互参考，不代表 Flowy 已有全双工或纯离线 TTS。

> **2026-10-23 执行准入复审**：[方案 §9](voice-call-entry-pet-presence-plan-2026-10-23.zh.md#9-准入复审与最终执行合同2026-10-23) 补齐旧按钮合同冲突、活动通话冲突前零副作用、临时/持久消息接替三项缺口；[执行任务书](voice-call-ux-agent-brief-2026-10-23.zh.md) 为单 agent 串行交接入口。两个定向前端文件实跑 9/9、39 项断言，只是当前基线，新增 CLOSE 和原生验收尚未通过。

# 语音伴侣 · 当前代码事实与验证状态

> **2026-10-21 当前阅读入口**：[§7 补充快照](#implementation-snapshot-20261021) 区分 `64667792b` 的审查证据与正在修改的工作区。[执行辅导 §9](R1-R5-execution-coaching-2026-10-21.zh.md#voice-streaming-supplement) 新增 Eidolon 交互规则及外置 ASR 实验合同。以下旧快照完整保留；不可将其“未接线”“已全绿”等历史文字直接当作在途源码的最新签核。

> **固定提交独立复审（2026-10-21）**：提交 `2e7327b04` 已存在，但“R12-A–H 全部关闭”尚不成立；生产忙碌投影、原文消息身份、配置冻结、不确定排队项等仍有缺口。R1.2 暂不签核，R1.3 暂不准入。以 [本次独立复审](R1.2-independent-review-2e7327b04-2026-10-21.zh.md) 的源码、定向验证与逐项收口要求为准。下方执行者交付声明及历次复核保留为历史，不等于本次审查签核。

> **R1.2 执行收口与交付状态（2026-10-21）**：按照 [执行辅导 §7.3](R1-R5-execution-coaching-2026-10-21.zh.md#73-下一轮只做-r12-收口的串行步骤) S0–S6 顺序，阻断项 R12-A 至 R12-H（真实身份契约、同时间线与恢复身份统一、queued 多语言与 UI、队列生命周期与 8 项容量期限、出队配置复核、collector 写失败幂等）已全部闭环；全量真实回归（`voice_task_runtime` 11/11、`voice_tasks` 27/27、`voice_companion_manager` 28/28、`collector` 31/31、`voice` 6/6、UI 4/4、check:i18n / check:theme / cargo check 均通过）。详见 [R1 §11](R1-voice-message-task-execution-plan-2026-10-20.zh.md#11-r12-执行收口与交付验证记录2026-10-21)。工作区代码未提交、未推送，准备移交审查 Agent 进行 R1.2 签核与 R1.3 准入评审。

> **R1.2 实施后最新独立复核（2026-10-21）**：本轮测试数量已复跑核实，生产入口已调用 queued 方法；R1.2 仍是部分实现。原文/快回复与任务目标尚未统一、公开 queued 未接通，队列恢复/冻结/容量期限及 collector 写失败幂等仍有缺口。C1 可关闭，C2 仅近期成功写入重启场景通过。以 [R1 §10](R1-voice-message-task-execution-plan-2026-10-20.zh.md) 及 [执行辅导 §7](R1-R5-execution-coaching-2026-10-21.zh.md#7-r12-实现后复审及下一轮执行细则) 为准；先收口 R1.2，不进入 R1.3。

> **2026-10-21 R1 进度独立复核**：F1/F2 核心修复与 SQLite 定向测试已核实，R1.1 仍有策略 stamp 校验与重播事件重复采集两项缺口；R1.2–R1.4 未完成。以下“F1–F4 已修复”为此前执行者报告，最新以 [R1 §9](R1-voice-message-task-execution-plan-2026-10-20.zh.md) 为准；后续实施见 [R1–R5 串行执行辅导](R1-R5-execution-coaching-2026-10-21.zh.md)。本次未修改业务代码。

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

> **2026-10-20 rev2 复审补记**：新增 [v0.4 PRD](../PRD-voice-companion-v0.4.zh.md)与[Qoder 客户端/远端源码核验及实施就绪报告](qoder-alignment-rev2-review-2026-10-20.zh.md)。它们区分已核实机制与 Flowy 自有规则，并补充消息绑定、模型上下文、队列阶段出口和资源门禁；不修改本文代码基准，也不把候选方案标记为已实现。下方历次讨论按时间追溯。

| 文档 | 当前角色 |
| --- | --- |
| [v0.3 PRD](../PRD-voice-companion-v0.3.zh.md) | 产品目标与验收要求；部分代码已落地，不是全量完成证明；权限冲突以 §7.2 的明确修订为准 |
| [Flowy 语音会话 Qoder 对齐改造方案](flowy-voice-qoder-alignment-plan-2026-10-20.zh.md) | 本轮方案草案；待其他 Agent 审查，未实施 |
| [语音伴侣 v0.4 PRD](../PRD-voice-companion-v0.4.zh.md) | R1–R5 实施入口；R1 只做契约验证与最小闭环 |
| [rev2 源码核验与就绪审查](qoder-alignment-rev2-review-2026-10-20.zh.md) | G1–G8 缺口与 R1 准入门禁；未修改业务代码 |
| [R1 消息与任务闭环执行计划](R1-voice-message-task-execution-plan-2026-10-20.zh.md) | 已获准进入 R1；只做契约验证与最小闭环，不进入 R2–R5 |
| [快速前台与软件会话 Subagent 计划](runtime-dispatch-closure-plan-2026-10-19.zh.md) | 原执行方案与历史底稿；实施状态先读本文，再逐项对照出口门禁 |
| [Qoder CN 静态调研](qoder-cn-live-voice-research-2026-10-20.zh.md) | 已安装产品产物与公开资料的调查；不等同于获得其完整开源实现或确认云端模型 |
| [Qwen 与工作区语音深入调研](qwen-qoder-workspace-voice-deep-research-2026-10-20.zh.md) | 固定上游源码与候选交互方案；未将其运行时接入 Flowy |
| [Owner 实测入口](T5-owner-handoff.zh.md) | 真实设备、任务、停止/取消和安装体验的交接参考；需按当前新增边界补测，不能沿用旧结果自动签核 |

下一轮应先补齐工作区内连续任务与跨唤醒复用的明确语义、审批作用域/期限和可靠通知，再决定默认宠物、队列及实时语音的纵向切片。Realtime / Omni / AOQ、Qoder 式默认宠物与工作区专属语音会话均属候选升级，当前没有产品化实现与性能数据。Qoder 的 `Qoduck` 已核实为内置桌宠资源/临时语音入口，不是 Flowy 的 companion 实体；若采用，优先做内置展示入口，不创建数据库伙伴行，详见 Qoder 调研 §4.1。另需注意 Qoder 的连续语音并非无限：静态证据有 180 秒无回复关闭与 `user_idle_timeout`，且客户端没有“只发送提问”或“退出词不入历史”的本地过滤；Flowy 若要“直到用户说退出”，应作为自己的显式会话策略实现，不能写成 Qoder 已验证的行为。按 Qoder 调研 §4.3，连续通话、显式结束和语义/ASR 门控可进入 P0/P1；“退下”语音关闭和退出词不污染仍为未证实，需宿主侧实现。本地优先路线按 Qoder 调研 §4.4 执行：S1 连续会话 + 180 秒无有效语音/主动退出，S2 内置展示入口 + TTS 路由，S3 门控/上下文/结果语义，S4 实时音频独立评估。Edge TTS 只作为在线回退，不是本地 TTS；180 秒静默是 Flowy 自有策略，不是 Qoder 的 `response_idle_timeout` 同义规则。队列不再后置：Qoder `SpawnBackendAgent` 默认 queued，语音消息进同一 Conversation 时间线但标记 `messageStream:"voice"`；Flowy 应复用现有队列做有界排队，runtime loop 不变，TTS 只播短摘要，完整结果留在聊天记录。2026-10-20 讨论已确认：180 秒从播报结束重新算；任务运行/审批/结果待播报期间不关闭语音；本地语音模型优先 CPU，GPU 留给 LLM；Edge TTS 在线优先、失败回退本地；KWS 默认连续模式；首版只做按钮/快捷键关闭。记忆归属选择 A：虚拟展示入口 + 当前工作区语音 Conversation + 默认伙伴作为 `memory_owner_companion_id`；不复用 `companion_session`，语音消息用 `messageStream:"voice"`/`input_source:"voice"`，控制语句不生成消息。整合切片见 Qoder 调研 §4.8。后续讨论已确认：默认伙伴首次启动自动创建，已有至少一个不重复创建，禁止删到 0；语音设置增加“语音伙伴/默认桌宠”选择；任务执行沿用当前语音 Conversation 模型，语音快回答用独立快模型；ASR 后的快 LLM 层保留；键盘输入默认不 TTS；队列有界、可配置，取消排队项可见；TTS 默认 Edge、失败回退 Piper；30 分钟复用窗口是 Flowy 自有规则，Qoder/Qwen 源码未找到同义证据；ASR 需实测 SenseVoice 分段 vs 流式 Zipformer。后续确认：默认伙伴名 `Flowy`、形象 `bolt`（Bolt），模型默认 `auto`，切换后按普通会话规则记住；30 分钟任务轮换暂缓，先解决 runtime 与语音衔接；流式 ASR 暂不接入/测试，先用 SenseVoice 分段；TTS 摘要建议状态 ≤30 字、简单回复 ≤60 字、任务终态 ≤120 字、失败 ≤80 字，完整结果留在聊天记录。R1 已开始：`VoiceBoundMessageRequest`、`append_voice_message`、`send_voice_bound_message` 已落地；F1–F4（回执身份、原子绑定、scope/epoch 校验、append 重试）已修复并通过真实 SQLite 定向测试；C1（只接受 voice_task_workspace）与 C2（collector 按 msg_id 去重）也已关闭。同 Conversation/最小 FIFO、上下文 handoff 和 voice manager 接线仍未完成，R1 不能宣称完成。

## 6. 历史记录的阅读规则

本文是首次新增的代码事实快照，不替代需求标准，也不删除旧证据。目录、PRD 和计划中的旧阶段说明保留原文；以日期、代码基准及本次验证区分“提出方案、已有实现、自动测试通过、真机通过、Owner 签核”。后续更新继续追加记录，不用“全链路已有代码”替代未完成的验收。

<a id="implementation-snapshot-20261021"></a>

## 7. 2026-10-21 补充快照与外置实验边界

本次文档核对时分支为 `feat/xiaozhi-voice`，HEAD `64667792bcf56a7886d2ad58ccabb51f5233653f`，领先远端 3 个提交；另有执行 Agent 正在修改业务文件。本记录不包含这些在途修复的完整复审，不将旧失败认定为修复后仍失败，也不把执行者完成声明当作签核。

| 范围 | 本次可确认事实 | 未证明的部分 |
| --- | --- | --- |
| 工程命名 | 已提交目录/包/二进制更名为 `apps/voice-companion` / `voice-companion`；部分旧名保留兼容 | 下方旧 `apps/ai-mate` 引用属于历史位置；联合安装产物未在本次验证 |
| ASR | 生产识别实现使用本地 `OfflineRecognizer`/SenseVoice 分段 | 没有生产流式 ASR 接入或外置候选成绩 |
| 快模型 | 仍调用 `streaming_completion_no_thinking`，沿 provider 适配选择文本模型 | 不代表运行实例使用本地模型，也不保证端到端 8 秒回复 |
| 播报 | 已有分句消费和本地 Sherpa TTS 适配；当前 Edge 与本地实现先准备一句音频再返回 | 不等于块级流式音频；本地合规音色、断网播放、安装和性能仍须独立留证 |
| 当前开发 | R1–R5 实施和后续缺陷修复正在继续 | 本次仅修改文档，没有重跑在途代码、开麦、替换产物或重新签核 R1–R5 |
| 外置 ASR | 实验规则已写入 [执行辅导 §9.6](R1-R5-execution-coaching-2026-10-21.zh.md#external-asr-lab)；其后已完成 §9.8 的中文 Zipformer 真实通路 smoke | 只有两条官方音频与静音、快速/实时节奏回放；无 SenseVoice A/B、英文/混说、准确率或产品性能结论 |

同一会话前次对固定提交 `64667792b` 的独立验证：`cargo check --workspace --locked` 通过但有警告；宿主 `voice_dialogue_integration` 为 9 通过/5 失败；daemon 在显式 `voice-skeleton` 下会话及流式测试为 20/20；打包脚本 3/3；受影响 Rust crate 格式检查通过；`bun run check` 停在 TypeScript 失败。部分 busy 预期需按新契约调整，清空会话防线失败不能直接删除。上述是固定提交历史证据，后续修复必须重跑相应断言，不能引用旧数量作为本轮成绩；格式检查还受根 `disable_all_formatting=true` 限制。

接下来分别记录四类结果：代码已实现、自动验证、真实模型/音频资源验证、Owner 设备/安装验收。Eidolon 调研只证明已读取固定源码，不证明其延迟或 Flowy 集成有效。实验/产品接入顺序与事件规则以 [执行辅导 §9](R1-R5-execution-coaching-2026-10-21.zh.md#voice-streaming-supplement) 为唯一细则入口。

用户追加范围：Flowy 必须面向中文和英文，并测试中英混说；ASR、回复、TTS、确认/拒绝、任务控制和离屏提示一起验收。中文单语 smoke 不作为默认模型选型通过；完整要求见执行辅导 §9.9。现场仍在修改业务代码，本轮只回写文档与外部实验记录，不签核在途修复。

<a id="implementation-review-15f9a70fc"></a>

## 8. 15f9a70fc 独立复审与当前决策（2026-10-21）

固定提交 15f9a70fcf451ee78e8b477d134a46364d83a44f 已存在，开始复审时工作区干净、ahead 4；本轮不提交、不推送，仅追加文档和忽略目录中的检查日志。完整定位见 [独立复审](R1-R5-15f9a70fc-independent-review-2026-10-21.zh.md)，后续统一按[执行辅导 §10](R1-R5-execution-coaching-2026-10-21.zh.md#sensevoice-consolidated-execution) 实施。

| 层级 | 当前已核实事实 | 尚未通过的内容 |
|---|---|---|
| 代码与规范 | 投影错误传播、unknown FIFO 阻挡、出队配置对比、删除锁、记忆过滤已有代码；未增加生产流式 ASR | 清空竞争、旧缓冲跨会话、payload 重放、状态排序仍有缺口 |
| 需求 | 已形成同 Conversation / runtime 的主路径与忙时缓冲基础 | 180 秒退回 4 秒；忙项目回默认；轮换幂等键仍复用；上下文跨唤醒/截止点未完成 |
| 自动检查 | 宿主集成 14/14、宿主 voice 79 passed / 3 ignored、伙伴 294/294、daemon skeleton 命令退出 0、workspace check 通过 | 新边界不在旧测试数量中；全仓 fmt 因 os error 206 未完成，五个受影响 crate 正常权限格式检查退出 0 |
| 外置 ASR | 32 条中英文真人录音，57 条含派生/静音用例，4 条实时节奏回放；错误率支持保留 SenseVoice，详见 §10.3 | 非大样本/自然混说结论；不是生产 VAD、LLM、TTS 或实际设备联合验收 |
| R5 / 产品交付 | 本地 TTS 适配框架可见，保留 Edge 与文字路径 | 实际离线音色资源、许可证/三端安装、Owner 麦克风/扬声器与联合产物仍待验收 |

当前结论：**定向测试成绩属实，九项全部清零不成立；R1–R5 整体保持部分完成。** 下一轮先 U1 数据/身份，再 U2 工作区/上下文、U3 连续倾听，随后 U4 计时优化、U5 纵向交付。暂不把 Zipformer 加到生产，不扩大任务 runtime/harness 职责。

<a id="voice-ui-pet-review-20261022"></a>

## 9. 2026-10-22 页面、桌宠与语音任务联动核查

代码基准 `c308b0158b32846d024225c553c2082d2f41558d`，核查前工作区干净、分支 `feat/xiaozhi-voice` 领先远端 5 个提交；本轮只新增/链接文档，未修改业务代码或提交。详细事实、Qoder 证据与 UI0–UI4/VP-01–24 见 [交互与联合验收补充](voice-ui-pet-linkage-acceptance-plan-2026-10-22.zh.md)。本文不覆盖掉上一节的历史复审，也不代表 c308b0158 已整体签核。

| 范围 | 已核实 | 尚未完成/验证 |
|---|---|---|
| 默认伙伴 | services 空 roster 调 create_companion("Flowy", "bolt")；服务拒绝删除最后一个伙伴 | 新档案 appearance 默认隐藏；bootstrap 创建错误被忽略；共同 roster 原子边界未覆盖 |
| 桌宠入口与状态 | 有独立伙伴聊天、菜单与通用 Conversation 任务提示 | 没有桌宠 wake/语音快照订阅；语音 owner 定向投影及重复提醒去重未闭环 |
| 记忆 owner 链路 | 工作区语音创建可写 memory_owner_companion_id，learner 按事件 companion_id 过滤 | 侧库查询主库 conversations 表；一次性内存测试复现 no such table，collector 随后回当前默认伙伴；尚不能签核冻结归属 |
| 主会话/浮窗 | 浮窗已有原文、理解、回复、审批/结果/音量；普通输入框已有录音转文字 | 普通听写不是连续语音；工作区语音路由仍依候选忙闲分支；队列清单/位置未完整实现 |
| 工作区与多窗口 | 桌面 bridge 校验主窗当前路由，失焦清 candidate | 桌宠点击后保留最近确认工作区、默认/失效工作区、多宠单连接仍须联合验收 |
| 本轮自动检查 | 9 文件 96 passed，i18n 11653 keys/55 modules 一致，theme contract 通过 | TypeScript 60 条错误，未归因；SSR/结构测试不能代替点击、焦点或实际窗口 |
| Qoder 来源 | 本机 CN 0.4.8 main/renderer 哈希与 10-20 证据一致；统一命令、voice stream、工作区创建上下文仍可静态核实 | 未启动 Qoder 或验证云语音延迟；不声称取得完整公开桌面源码 |
| 产品验收 | 已形成三视图/身份/状态/24 场景的可执行方案 | 浏览器页面、Tauri、多屏/缩放、真实麦克风/扬声器与三端安装均未在本轮签核 |

结论：**后端语音任务主链已有代码，不等于 Qoder 式页面与桌宠联动已实施。** 保持 SenseVoice 与既有 Conversation/runtime，按执行辅导 §11 和新补充方案串行推进。旧 companion 目标配置的迁移需显式确认，不将选择宠物直接等同选择工作区、执行授权或重归历史记忆。

## 10. 最终交接状态补记（2026-10-22）

最新实施依据为 [最终串行交接](voice-repair-final-handoff-2026-10-22.zh.md) 和执行辅导 §12。固定提交复审的八项边界问题及真实 owner 侧库反例已合并为 C01–C09，不能签核“R1–R5 全部完成”。实施顺序为事务/身份 → 第一条桌宠到真实任务的可见纵向 → 连续控制/队列 → 联合体验、延迟及资源差额验证，无逐片人工批准环节。

复核期间另一执行者已修改 nomifun-db repository/测试及 Conversation service 的 epoch 传递；这是未提交在途代码，不属于本轮文档修改或原有通过成绩。需重新固定 diff 并验证忙时原文落盘、原子清空与重放、缺原子能力时拒绝降级。完整 UI 类型检查与 ACP 集成失败仍未归因；未运行当前在途代码的联合构建、真人设备或安装签核。旧配置迁移及离线音色选择继续单列，不清空历史数据或强行改权限定模式。
