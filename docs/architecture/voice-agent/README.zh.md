# voice-agent 文档目录 (AI Mate Voice Agent Docs)

> **2026-10-22 当前收敛方案**：[本地 TTS 插话打断与继续输入](execution/local-barge-in-only-plan-2026-10-22.zh.md)。用户已暂缓流式字幕、Zipformer 与额外纠错；只做 AEC/VAD、本轮输出取消、句首保留及现有 SenseVoice 同会话继续输入。B0–B3/BI-01–14 待实施，下方组合方案保留为历史。

> **2026-10-22 本地语音演进计划**：[实时字幕、最终转写与自然打断](execution/local-streaming-barge-in-transcript-plan-2026-10-22.zh.md)。推荐实验验证双语流式预览加 SenseVoice 最终识别，LLM 理解不覆盖用户原话；先项目外实验，再 S1 字幕、S2 打断、S3 联合验收。当前仅方案，不宣称已支持全双工或严格离线。

> **2026-10-22 页面与桌宠联合验收补充**：[语音页面、工作区会话与桌宠联动方案](execution/voice-ui-pet-linkage-acceptance-plan-2026-10-22.zh.md)。固定 c308b0158 核查：Flowy/Bolt 初始化和最后伙伴删除保护已有代码，但新装默认显示、桌宠统一语音按钮、主会话连续语音态和可视队列尚未闭环；新增 UI0–UI4、24 个 VP 验收用例及 Qoder 0.4.8 静态复核。定向前端 96/96、i18n/theme 通过，TypeScript 仍有 60 条未归因错误；浏览器/Tauri/设备未签核。本轮只更新文档，不将后端测试等同界面联动完成。

> **2026-10-21 R1 进度与后续实施**：[R1–R5 串行执行辅导](execution/R1-R5-execution-coaching-2026-10-21.zh.md)。含当前修复核验、策略/采集缺口及每片代码入口、边界、测试与命令；最新审查见 [R1 §9](execution/R1-voice-message-task-execution-plan-2026-10-20.zh.md)。历史正文保留，局部编译通过不等于 R1 全流程完成。

> **2026-10-20 rev2 源码复审**：[v0.4 工作区连续会话 PRD](PRD-voice-companion-v0.4.zh.md)与[Qoder/远端源码核验及实施就绪结论](execution/qoder-alignment-rev2-review-2026-10-20.zh.md)。补齐可信消息绑定、时间线与模型上下文分离、最小队列阶段出口；R1 可进入契约验证，R5 资源交付仍有门禁。均为文档，未实施业务代码；原方案和历史记录保留。

> **当前代码事实（2026-10-20）**：[实现状态与验证记录](execution/current-implementation-status.zh.md)，基准 `10a1628ab`。真实任务交付、上下文、部分连续会话、审批及结果播报已有实现；新版 PRD 全量验收未完成。下方“待实施 / 本轮仅文档”等为原阶段历史。默认不可删除宠物、统一工作区语音会话及 Realtime/AOQ 仍属调研候选。

> **2026-10-20 方案草案**：[Flowy 语音会话 Qoder 对齐改造方案](execution/flowy-voice-qoder-alignment-plan-2026-10-20.zh.md)。覆盖连续会话、工作区语音 Conversation、队列、默认 Flowy/Bolt 伙伴、memory owner、本地快模型与 Edge/Piper TTS；待其他 Agent 审查，未实施。

> **2026-10-20 v0.4 实施入口**：[语音伴侣 v0.4 PRD](PRD-voice-companion-v0.4.zh.md) 收敛 R1–R5 交付边界；[rev2 源码核验与就绪审查](execution/qoder-alignment-rev2-review-2026-10-20.zh.md) 给出 G1–G8 缺口。当前结论：只批准 R1 契约验证与最小闭环，不批准全量 R1–R5。

> **2026-10-20 优化 PRD**：[v0.3 连续任务、低打断授权与可靠播报](PRD-voice-companion-v0.3.zh.md)。依据本机会话日志和 GitHub 项目经验，建议复用现有权限模式与具体工具白名单，不统一启用 yolo；补齐执行会话连续性、确认澄清、结果语义和播放回执。本轮只交付文档，待实施及验收；较早方案与历史状态保留。

> **当前后续方案（2026-10-15）**：[纵向闭环与 runtime 接口方案](execution/runtime-vertical-contract-plan.zh.md)。先打通真实低风险任务，再补控制、恢复、工具确认及体验优化；明确模块职责、拟定 API、状态/错误/幂等契约和逐片验收。本轮只整理方案，未实施业务代码；下方较早阶段状态保留作历史。

AI Mate 无头伴侣主机语音交互方案的权威文档集。两层结构：**一份架构基线 + 两份子规范**，外加一份历史底稿归档。

| 文档 | 定位 |
| :--- | :--- |
| [interaction.zh.md](interaction.zh.md) | **架构基线（唯一生效标准）**：背景与痛点、五模块边界、无锁声学管线、双状态机、单次四路路由契约、工具策略安全红线、ADR 矩阵与 Phase 0-4 验收门禁 |
| [model-management.zh.md](model-management.zh.md) | 子规范：模型资产统一目录拓扑、双重强校验、双源探针降级链、`ModelManager` 生命周期状态机与桌面/无屏/离线三场景分发 |
| [ui-motion.zh.md](ui-motion.zh.md) | 子规范：动效令牌总线、设置页行契约与状态视觉映射、无屏配对页契约、无障碍降级与反模式红线 |
| [interaction.archive.zh.md](interaction.archive.zh.md) | 历史全量底稿归档（仅供追溯审计，不构成执行依据） |
| [execution/](execution/README.zh.md) | **执行计划**：骨架先行五波次拆分、多 Agent 并行分工、TDD 纵向测试集与 bsk 浏览器验证规程（面向执行，不定义验收指标） |

阅读顺序：先读基线建立全局，再按需深入子规范；测试用例（TC-AM-xxx）与阶段门禁（Gate S ~ Gate 4）统一收敛在基线 §5，子规范只做映射引用；落地执行按 execution/ 波次推进。

## 桌面语音伴侣产品规划（2026-10-14）

以下为新增的桌面产品规划，业务尚未实施。用户已明确本轮只交付 PRD 与技术方案；既有无屏基线和历史正文完整保留。后续实施时补充基线中的宿主窗口边界、隐私说明和新增验收，不把旧波次计划视为新产品已验收。

| 文档 | 定位 |
| :--- | :--- |
| [PRD-voice-companion.zh.md](PRD-voice-companion.zh.md) | 详细产品需求：用户与价值、首次启用、唤醒会话、追问、任务与确认、控制语义、降级、数据使用、26 项功能、20 项验收和发布切片 |
| [voice-companion-integration.zh.md](voice-companion-integration.zh.md) | 整合技术方案：产品职责、独立进程、常驻窗口、共享快照、具体 runtime Adapter、真实回执与确认、任务恢复及七个实施工作包 |
| [execution/2026-10-14-desktop-voice-session-handoff.zh.md](execution/2026-10-14-desktop-voice-session-handoff.zh.md) | 原始探索证据与修复建议，保留首次分析全文，供对照追溯 |

## 已授权实施

2026-10-14 用户明确启动实现，执行入口为 [vertical-execution-plan.zh.md](execution/vertical-execution-plan.zh.md)。按 V0–V8 串行推进，代码、自动测试、真机和 Owner 验收分别记录；上面的文档阶段状态保留作历史。

授权计划的完整历史原文见 [vertical-execution-plan.original.zh.md](execution/vertical-execution-plan.original.zh.md)；当前 V1 真机验证入口为 [V1-desktop-smoke.zh.md](execution/V1-desktop-smoke.zh.md)。V1 尚未通过完整出口门禁，聊天、任务、工具确认和安装包仍未签核。

## 语音输入诊断实施入口

2026-10-14：新增普通音量 VAD 校准、daemon PCM 音量及分阶段日志。实机核对步骤见 [音量与识别闭环核对](execution/I-audio-debug-smoke.zh.md)，实施证据见 [纵向执行记录 I](execution/vertical-execution-plan.zh.md)。这些代码/回放证据不代表完整语音聊天、任务或安装验收已签核。

## 完整回复与在线播报修复入口

2026-10-14：修复 Edge 握手、空音频假成功、输出完成与追问竞态；保留完整模型输出和最近三轮回复。依据用户授权复用已有 sha2 和 chrono，不使用 Windows 系统声音。验证记录与 Owner 步骤见 [回复及播报闭环](execution/J-K-reply-playback-smoke.zh.md)；在线合成已验证，实际扬声器和 Mac/Linux 尚待验收。

## 标准音频与唤醒词接线

2026-10-14：真实神经 KWS 标准样本从 0/4 修复为 4/4；下载示例词表不再被当作产品词表。音频、生产 worker 回放、真实 ASR 和误唤醒对照见 [标准音频报告](execution/M-standard-audio-report.zh.md)。这是合成语音小样本验证，真人与设备准确率仍待 Owner 测试。

## Flowy provider 与本地模型接口

2026-10-14：语音伴侣复用宿主模型目录、引用与既有补全客户端；新增模型选择与可替换的文本补全接口。真实 Flowy 固定句探针已通过，接口位置、实现和验收边界见 [provider 接入记录](execution/O-provider-integration.zh.md)。

## 唤醒后的 ASR 与面板闭环

2026-10-14：修复固定能量门槛拦截低音量讲话的问题，复用套件 Silero VAD；补齐收音倒计时、ASR 等待说明、具体退出原因、再次开始和识别/回答流水。八项真实声学回放通过，证据与 Owner 步骤见 [唤醒后 ASR 排查与修复](execution/P-post-wake-asr-panel.zh.md)。

## TTS 故障与完整流程回归

2026-10-14：完整桌面 TLS feature 图复现合成任务 panic，修复 provider 装配与异常恢复，区分合成/播放并扩大正文阅读区。完整 mock、真实 KWS/VAD/ASR 与在线 Edge 回放及 Owner 实测入口见 [TTS 完整流程闭环](execution/Q-full-flow-tts-closure.zh.md)。音频文件和可控制的输出消费不替代物理扬声器验收。

## 当前产品规划：v0.2 离屏声音反馈

2026-10-14 Owner确认基础KWS→ASR→LLM→TTS链路正常，追加完整声音反馈，并选择轻量状态反馈。本版文档已制定，新增音效行为未实施；原VC/AC及任务/安装门禁继续保留。

| 文档 | 用途 |
| --- | --- |
| [v0.2 PRD](PRD-voice-companion-v0.2.zh.md) | 当前产品演进：离屏旅程、12类声音场景、设置/干扰规则、16项新增验收；继承原完整产品范围 |
| [声音反馈技术方案](voice-feedback-integration.zh.md) | 真实事件、唯一发声者、输出所有权、计时、取消、回声、配置与runtime接线边界 |
| [R串行执行计划](execution/R-auditory-feedback-plan.zh.md) | R0–R6切片、验收映射、证据格式及Owner离屏剧本；下一实施入口R0 |

v0.1、原无屏基线与执行历史全文保留。旧D9禁止固定4秒滴答；当前轻量模式仅按用户新增需求提供有限且可取消的状态反馈，不启用持续背景音。

2026-10-22 最终复核与下一轮唯一执行导航：[语音修复与桌宠联动交接](execution/voice-repair-final-handoff-2026-10-22.zh.md)。既有主链可保留；数据/身份、连续倾听和页面/桌宠联动仍按 C01–C09 与 VP-01–24 验收。本轮只更新文档，另有在途 epoch 代码修改，未签核其结果。
