# Qoder 对齐 rev2 · 源码核验与实施就绪复审

> **日期**：2026-10-20。业务代码基准 `5800811c7e3fb76e43b32afdda9916357f70d900`。
> **审查对象**：[对齐方案 rev2](flowy-voice-qoder-alignment-plan-2026-10-20.zh.md)，SHA256 `2c124198be6e5240ad1eddaec2df6a6b49fb341448a4db6adac187f367d40779`。
> **结论**：方向可行，上一轮主要缺口已补写；可以进入 R1 的接口与最小闭环验证，不能据此将 R1–R5 全部标成实施就绪。消息绑定、上下文双视图、队列冻结/回执兼容仍需按下述接缝落实。配套产品要求见 [v0.4 PRD](../PRD-voice-companion-v0.4.zh.md)。

## 1. 核验范围与方法

本轮重新读取 rev2 与相关 Flowy 生产代码，并读取本机 Qoder CN `resources/app.asar` 中的客户端 JS、AOQ 包清单。只解析静态数据，不启动 Qoder、不执行提取代码、不登录或请求其语音服务。它是已发布产品的编译产物，不是完整开源仓库。

对远端重新打开 Qoder 官方文档、QoderAI 仓库与 Qwen-Live-Harness；网页读取源码失败时，使用公开 raw URL 下载固定版本文本到本地忽略目录，再与已有固定提交比对。没有把 prior agent 的断言或 JSON 检查布尔值当作源码证明。

不修改 rev2、不修改业务代码、不添加生产依赖或迁移、不执行任务/麦克风/扬声器验证。第三方源码片段与检查脚本只在 `target/voice-review/`，不作为分发资源加入仓库。

## 2. 哪些借鉴依据属实

### 2.1 Qoder 本机客户端

安装位置：`C:/Users/38788/AppData/Local/Programs/Qoder CN/resources/app.asar`。原记录版本 0.4.8；本轮读取的三个关键文件与此前哈希一致：

| 文件 | 字节数 | SHA256 |
| --- | --- | --- |
| `out/main/electronMain.js` | 17016488 | `1a382db7062b9acdb910ed78fcebb2a16a8a6900b5d44965f9c8a3cbb4260996` |
| `out/renderer/assets/DIR8HOYO.js` | 15529793 | `52b8425ff40f90ef76782847291e61183ad0a2eb4a6c44df443494466b8eccef` |
| `node_modules/aoq-electron-sdk/package.json` | 982 | `261d26fd237e1d176c690349e8e412fdaf1ec57a5da6ebcf0de3bca53e6dd8a8` |

下表位置均为 renderer **零基 UTF-16 字符偏移**，不是源码行号或字节偏移。哈希固定后可复查：

| 观察 | 定位与实际分支 | 能支持的结论 |
| --- | --- | --- |
| 委派既有聊天 | `7673710` 解析 SpawnBackendAgent；`7713026` 附近调用原聊天提交并完成工具回执 | 前台语音和软件任务执行分工，成立 |
| 默认排队、显式替换 | `7674800` 的 interruptCurrent 判断；`7713026` 附近选择 queued/replace | 非中断请求默认排队，成立 |
| 回执基于接收结果 | `7665455` 及 `7713905` 附近处理 accepted 与 started/queued/replaced；只在接受时提交待用上下文 | 应先有真实调度结果，再更新前台；不是模型说已接单就算接单 |
| 同聊天、独立语音身份 | `5144595` 语音消息转换带 messageStream、voiceSessionId 等字段；`7656444` 启动独立连接 | UI 时间线与实时连接身份分开，成立 |
| 工作区来源 | `11954090` 区分路由/默认/最近工作区并校验目录；`13698247` 新语音会话带 workspace/model/permission；`11307739` 已有 voice 会话直接提供目标 | 工作区解析和独立 voice 会话有客户端依据；不能概括为始终借用当前普通会话 ID |
| 传输选择 | `7658421` 优先 aoq-first，能力不足走 legacy-only | 客户端有实时媒体传输；不能证明其等于本地分段 ASR + 普通文本模型 |
| 180 秒 | `7638281` 附近的无回复文案；`7688063`、`7688415` 分别处理 user_idle_timeout/response_idle_timeout | 存在超时，但客户端文案不能证明“180 秒无用户语音”的同义行为 |
| 结果口播 | `7689324` 构造终态结果上下文，要求简短口播并避免再调用函数 | 结果与动作能力分开；不证明其完整 system prompt 或云端模型是什么 |

[官方语音说明](https://docs.qoder.cn/qoder/voice-input)描述连续交流、当前任务、麦克风/播放分别控制；[官方宠物说明](https://docs.qoder.cn/qoder/desktop-pet)将宠物定义为轻量入口，并说明不改变当前工作区。它们不证明不可删除伙伴、memory owner、30 分钟复用和本地 Piper，这些仍是 Flowy 自定设计。

### 2.2 Qwen-Live-Harness 固定源码

固定提交：`b2d17b8706b226ff7792683ecd8f050f18f9752e`，仓库 [QwenLM/Qwen-Live-Harness](https://github.com/QwenLM/Qwen-Live-Harness)。本轮重新下载 ACP 适配器，与缓存 git 提交逐文本比对一致；工作树 CRLF 与 raw LF 造成字节哈希不同，未据此误报上游漂移。raw 文件 40320 字节，SHA256 `060c6986d7f3180da973279b6cedebc53839748ae7932120ea754fe379225001`。

| 文件/定位 | 本轮阅读到的行为 | 对 Flowy 的意义 |
| --- | --- | --- |
| [config.ts](https://github.com/QwenLM/Qwen-Live-Harness/blob/b2d17b8706b226ff7792683ecd8f050f18f9752e/packages/qwen-live-harness/src/config.ts#L149) | 默认使用 Omni Realtime 模型 | 不能用其交互体验证明普通文本 Flash 加 TTS 的相同延迟 |
| [realtime-session.ts](https://github.com/QwenLM/Qwen-Live-Harness/blob/b2d17b8706b226ff7792683ecd8f050f18f9752e/packages/qwen-live-harness/src/realtime/realtime-session.ts#L3520) | 同一连接配置音频/文本、转写、semantic VAD，create_response=false | 是流式音频会话协调，不是先等独立 ASR 全文再串行调用文本模型 |
| [acp-adaptor.ts](https://github.com/QwenLM/Qwen-Live-Harness/blob/b2d17b8706b226ff7792683ecd8f050f18f9752e/packages/qwen-live-harness/src/adaptor/acp-adaptor.ts#L324) | 空闲接收；忙碌默认最多 8 个 pending prompts；每项有 jobRef | 借鉴有界准入和身份；该缓存队列本身不证明持久化与断电恢复 |
| [cancelJob](https://github.com/QwenLM/Qwen-Live-Harness/blob/b2d17b8706b226ff7792683ecd8f050f18f9752e/packages/qwen-live-harness/src/adaptor/acp-adaptor.ts#L418) | 排队项直接移除；运行项返回 stopping 并等后续事件 | “发出取消”与“取消完成”分开 |
| [injector.ts](https://github.com/QwenLM/Qwen-Live-Harness/blob/b2d17b8706b226ff7792683ecd8f050f18f9752e/packages/qwen-live-harness/src/orchestrator/injector.ts#L16) | 讲话、模型响应或播放未完成时关闭注入窗口；正文/短口播分离，处理去重与撤回 | 通知需要真实播放收据，不能只依赖网络音频接收完成 |
| [permission-broker.ts](https://github.com/QwenLM/Qwen-Live-Harness/blob/b2d17b8706b226ff7792683ecd8f050f18f9752e/packages/qwen-live-harness/src/permissions/permission-broker.ts) | Live 全局策略与后端实际权限分离，普通允许选择非持久授权 | 借鉴单次审批及请求绑定，不从实时模型口头判断直接赋予权限 |

本轮未执行上游测试、未连接 DashScope 或实际 ACP backend。以上为源码机制核验，不是性能和跨平台验收。

### 2.3 QoderAI 公开远端示例

本轮从 [forward-quickstart](https://github.com/QoderAI/forward-quickstart)取得固定提交 `03ad58890438dde77e430e1078706874cf8ddcb3` 的两个文件：

- [server/src/voiceProxy.ts](https://github.com/QoderAI/forward-quickstart/blob/03ad58890438dde77e430e1078706874cf8ddcb3/server/src/voiceProxy.ts)：7525 字节，SHA256 `eb6219264151ed69bb7fa6b73022dee47e44cc9083ecaf52ac2c633d21e0caeb`。使用 qca-sdk 的实时连接，检查入口、认证/初始化期限、收发事件和清理，不重复建立无界发送队列。
- [client/src/voice/useVoiceSession.ts](https://github.com/QoderAI/forward-quickstart/blob/03ad58890438dde77e430e1078706874cf8ddcb3/client/src/voice/useVoiceSession.ts)：11975 字节，SHA256 `4713b0f258c51485606a2651226b8ac044c6c5a54999e00750ac28f3031f920a`。管理实时连接、转写/任务事件、播放、重连和结束。

因此此前“语音中继示例”定位成立，本轮补上了实际文件读取。但它不是 Qoder CN 桌面源码，不能用它证明桌面宠物、离线模型或内部排队持久化。公开源码检索未获得 Qoder 桌面完整工程，不作“整个产品绝未开源”的穷尽性断言。

## 3. Standards 与 Spec 复审结果

Standards：backend 边界可实现目标，未发现必须改动 `crates/agent` 的要求；公共发送兼容、可信消息绑定和资源分发仍需下列门禁。

Spec：rev2 已覆盖上一轮九类风险，但写入一个原则不等于已有可实现接缝。以下反馈只针对当前 rev2 的遗漏，不要求先做泛化框架。

| 编号/标签 | rev2 定位 | 当前代码证据、影响和最小收敛 |
| --- | --- | --- |
| G1 [Required] 已投影消息绑定 | §3.3，约第 209–211 行 | `SendMessageRequest` 没有可信 existing message 绑定；[service.rs](../../../../crates/backend/nomifun-conversation/src/service.rs#L10885)从真实 delivery 决定 message ID，再校验现存正文。提前投影 B 后直接普通 send，不能保证绑定 B。需 backend 内受宿主调用的绑定/准入接缝，校验身份、epoch 和正文，不开放任意客户端指定消息归属。 |
| G2 [Required] 时间线不等于模型上下文 | §3.3 同一 Conversation 与 voice_context | [factory/nomi.rs](../../../../crates/backend/nomifun-ai-agent/src/factory/nomi.rs#L641)从 SessionManager 恢复模型历史，快速聊天使用另一路数据库投影。只合并 UI，runtime 不会自动知道快聊中的城市或指代；需按 Conversation 构造有界 handoff 并记录来源，覆盖跨声学连接追问。 |
| G3 [Required] 消息/回执存储与兼容 | §3.8，约第 417–424 行 | [MessageRow](../../../../crates/backend/nomifun-db/src/models/message.rs#L12)没有独立 extra，但 content 为 JSON；旧 [receipt payload](../../../../crates/backend/nomifun-conversation/src/service.rs#L867)严格解析，且比较完整 payload。优先公开原文 JSON + 私有冻结交付正文，证明读写/继续回答兼容；不能只加可选字段，或只存 digest 丢掉恢复正文。必要迁移另审。 |
| G4 [Required] 阶段出口存在循环依赖 | §5.2 R1/R2，约第 546–547 行 | R1 要 B 排队，但队列被整体放到 R2，无法串行过门。R1 纳入最小持久 FIFO 和基本顺序派发，R2 扩展取消/故障恢复；不以未持久的临时队列验收 R1。 |
| G5 [Required] 冻结配置与身份需使用现有契约 | §3.4、§3.8 VoiceQueueItem | 示例仅保存 model 字符串/preset ID，无法冻结 provider 与预设 revision；当前已有 [prepare_configuration](../../../../crates/backend/nomifun-app/src/voice_task_runtime.rs#L69)和 ResolvedPresetSnapshot。复用它们；入队仅预留消息身份，真实 runtime_turn_id 接受前为空。未发送配置失效与已发送 unknown 分开处理。 |
| G6 [Question] 本地快模型不可用时如何仍可使用 | §3.6，约第 293–305 行 | 当前 [ProviderVoiceDialogue](../../../../crates/backend/nomifun-app/src/voice_dialogue.rs)已复用用户 provider。rev2 改为只选本地且缺失时规则/纯文字，没有说明复杂任务的分流来源。建议 R1–R4 继续现有配置，本地只作可选补全适配；如要求强制本地，应单独锁定供应与不可用行为。不能把生产 skeleton 当规则回退。 |
| G7 [Required，R5 门禁] 引擎与音色一起审查 | §3.6 Piper 资源 | [piper1-gpl](https://github.com/OHF-Voice/piper1-gpl)上游标记 GPL-3.0 并包含音素处理依赖。当前只列 voice 许可证不足以决定分发；需选实现、调用方式、依赖、voice、版本及各平台产物后审核。不是认定不可用，也不阻塞前四片。 |
| G8 [Required，R3 门禁] 后台任务队列不等于输入缓冲 | §3.1 recognizing、TC-VOICE-05 | 当前 [voice.rs](../../../../apps/ai-mate/src/voice.rs#L1775)按声学状态采集句子，并由单 active_turn 处理；只补 voice_tasks 无法保证快模型思考时继续说 B/C 不丢音。需限定非播放阶段的有界待识别缓冲与过载提示，串行消费并保留每句身份；播放阶段仍半双工。不得靠中断旧 active_turn 接收新句。 |

### 3.1 已补齐但仍须保留的门禁

- 语音任务的新 Conversation 携带可信 `voice_task_workspace`，发送、取消和审批同步适配；不能通过普通 extra 或隐藏上下文扩大权限。
- 语音/键盘共用队列；unknown 不重发；restart 先核对；replace 等真实终态；等待与执行预算分开，另外给准备/发送过程有界期限。
- owner 归属按每条消息/任务冻结，collector 和 learner 同步处理；过滤仅针对新增语音来源，不顺手改变所有旧工作事件；遵守已有采集/学习开关。
- 180 秒计时与声学/任务状态分开；设备故障不等待空闲期限；半双工期间无普通 ASR 自触发。
- 空最终正文不等于伪造回执；不能把合法成功但无正文的操作强制判失败。应准确呈现结果来源、无正文或目标未验证的差别。

## 4. 对四个未决问题的建议

| 未决项 | 建议与适用阶段 |
| --- | --- |
| metadata 或 migration | R1 优先验证公开 JSON 原文/非敏感 voice 元数据与私有冻结正文分离。现有 MessageRow 无 extra 列，不能臆造。验证不成立再提最小迁移；原文/回执/继续回答测试必须先过。 |
| 队列等待是否 30 分钟 | 可作为首版建议值；容量 8 指等待项，恢复默认暂停。不要把等待预算占用运行预算，配置失效也不要伪装成 unknown。 |
| Piper voice/许可证 | 只阻塞 R5 资源交付。引擎、phonemizer、音色与分发方式一并选定；暂不新增资源或依赖。 |
| 摘要时长 | 120 字是文本上限，20 秒是按音色测量的体验目标。暂不加两档设置；禁止机械截断导致否定、关键数字或审批目标缺失。 |

PRD 给出了这些建议的产品表达，尚未把用户未确认事项写成已批准决策。

## 5. 可执行状态与交接

| 范围 | 本轮判断 |
| --- | --- |
| 方向、模块和来源核验 | 通过静态复核；不等于实现通过 |
| R1 | 可开始 G1/G2/G3/G4/G5 的契约验证与最小闭环实现；采用 PRD 补充门禁，不能只照 rev2 DTO 示例直接铺业务代码 |
| R2–R4 | 顺序和目标可用；等待前片出口通过，保留真实失败路径与 Owner 验收 |
| R5 | 资源/实现/许可证/三端产物与性能未锁定，不具备完整交付就绪条件 |
| 自动/设备/安装与发布 | 本轮未执行业务测试、未运行第三方应用或模型、不做任何完成签核或发布 |

本轮本地证据文件：`target/voice-review/rev2-qoder-verification.json`、`rev2-qoder-workspace-verification.json`、`rev2-remote-verification.json`；原始报告仍保留。配套 PRD 继承 22 个 TC，增加 9 个跨身份、上下文、持久化、输入缓冲与恢复用例，作为后续实施者的核对表。

本轮文档检查：6 份相关文档的 95 个本地链接可解析；PRD 8 节、12 个功能编号、9 个补充验收及 rev2 的 22 个原验收编号检查通过；rev2 哈希保持不变，`git diff --check` 通过。未运行 Mermaid 渲染、业务测试或上游测试，不把文档检查当成功能验证。
