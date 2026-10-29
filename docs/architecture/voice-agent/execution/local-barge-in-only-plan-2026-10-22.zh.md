> **2026-10-23 当前修复事实**：[独立审查修复与回归记录](local-barge-in-review-repair-2026-10-23.zh.md)。旧流输出阻塞、审批停止/新唤醒清理、播报回执和中断投影已修复；生产 AEC 尚未接通，默认半双工，B2/B3 不能签核。四个隔离失败场景已纳入正式回归，下方历次方案与交付声明保留为历史。

# 本地 TTS 插话打断与继续输入 · 优化执行 Plan

> **核心测试手段（已固化）**：**代码驱动的全自动音频回放/注入测试（Audio Playback Test Automation）**。彻底移除“等 Owner 手动实测/人声喊停/Web 点击”的外部依赖，由 Agent 运行可复跑的双路音频回放、PCM 注入、竞态断言与系统音频探针，以可度量的程序断言作为唯一闭环标准。
> **系统自动验收与交接文档**：
> - 自动化链路细则见 [系统链路与代码自动验收方案](local-barge-in-system-auto-validation-2026-10-22.zh.md)。
> - 调研、基线红灯与素材清单见 [完整交接与红灯基线报告](local-barge-in-investigation-handoff-2026-10-22.zh.md)。
>
> **当前状态**：📋 执行 Plan 优化完成。已建立 Phase 1 自动化红灯断言反馈环（`barge_in_acceptance.rs` 2.48s 稳定复现半双工缺陷）；已吸纳对抗审查（laoyeye 评审）的 4 项关键防御设计（游荡音频原子令牌、AEC 启动自激防护与构建容灾、高危工具审批语境保护、全链路音频回放自动化矩阵）。
> **当前范围**：用户已暂缓流式字幕、Zipformer、双 ASR 和额外 LLM 纠错。本文件为唯一权威执行入口。
> **核心原则**：播放期间本地检测用户开口，停止旧声音并保留新输入；句尾仍用现有 SenseVoice。播放停止、语音会话停止和后台任务取消是三种独立操作。

## 1. 背景、代码事实与目标

本轮目标是：**助手正在播报 → 用户直接说话 → 助手停播 → 收完整句话 → SenseVoice 识别 → 同一个工作区 Conversation 回复或交给 runtime**。无需先说“停”，无需再次唤醒，也不增加一个流式 ASR 模型。

硬件负担是需要验证的风险，不是已经证明所有 16GB 机器都不能流式识别；本轮按用户的优先级收敛范围，不对未知用户机器作统一性能结论。

| 入口 | 当前已核事实 | 本次必须处理 |
|---|---|---|
| daemon `voice.rs::spawn_acoustic_loop` | 播放态未执行讲话识别；Deciding/Dispatching 有待处理音频队列 | 播放时持续送经回声处理的音频给讲话检测 |
| 快回复流式播放 | LLM 句子可以在 `Deciding` 阶段开始播放；UI `Speaking` 也不等于内部声学状态 | 不能仅检查三个 `Speaking*` 枚举判断可插话，要识别实际输出及其归属 |
| `PlaybackPipeline::interrupt` | 增加 generation 并 flush sink | 保留此入口，验证旧写入与 flush 并发；注释的 `<50ms` 不是硬件测量 |
| `stop_session` | reset driver、abort active turn、清空 pending，并结束 session | 禁止用它实现自然插话 |
| `process_utterance/start_utterance` | 多处用播放 generation 判断整轮有效性 | 输出取消不能让新输入无效，也不能让旧结束回调重置新一轮 |
| task submit/control | 提交、回执处理及接单播报在同一处理路径中 | 只停声音，不丢已经发出的请求/回执，不重发副作用 |
| CPAL `speaker.rs` | 硬件输出回调取队列并转换采样格式，尚无 AEC render reference | 在实际输出边界提供参考，包含提示音，标明时间/设备身份 |
| UI 与数据 | 已有原文、理解、任务投影与 `reply_state` | 保留文字，显示播报中断/正在听；不恢复调试弹窗 |

本次不更换快模型/provider，不实施全离线 LLM/TTS 迁移，不改任务权限，不引入声纹身份识别。AEC/VAD/打断判定在本地完成，之后的既有 LLM/TTS 是否联网仍由原配置决定。

## 2. 总体架构与不可变规则

```text
实际输出 PCM + 输出时钟/设备身份 ─→ 本地 AEC 参考
                                       ↓
麦克风连续 PCM ──────────────────→ 回声消除 → 讲话检测
                                       ↓
                              经处理的句首环形缓存
                                       ↓
                         确认开口：停旧播报、继续收音
                                       ↓
                         句尾 SenseVoice → 现有会话链路
```

AEC 是本地音频信号处理，不要求增加 Zipformer、本地 LLM 或云端语音服务。VAD 只判断是否有人声，不证明说话者身份，也不理解是否有意插话。首版不做“嗯/啊”语义附和分类。

固定规则：

1. 停播不结束 session、不重新选择工作区/Conversation、不默认取消已接受任务。
2. 新输入一旦确认，旧回答的后续音频、token、`PlaybackDone`、失败重置不能覆盖它。
3. 一句话只创建一个输入身份、执行一次最终识别和准入；重复打断事件幂等。
4. partial API/字幕/双 ASR 不属于本次改动；保留原有最终转写、ignore 和审批契约。
5. 原审批的任务/调用/approval_id/截止时间不变；插话不是同意，提示未播完也不能延长授权。
6. 后台结果仍由真实回执确认；插话导致前台放弃等待，不代表提交失败或任务取消。

## 3. 核心实现规范

### 3.1 音频参考与本地 AEC

首选实验候选是 WebRTC AudioProcessing 的 AEC3，先验证 [Rust 绑定](https://github.com/tonarino/webrtc-audio-processing)的版本、许可证、C++ 构建和三端依赖，再考虑接入。其固定 10ms render/capture 帧接口已核实；不需要首先启用 experimental AEC3 参数。

- 参考来自送往设备的真实 PCM，经过 Flowy 自身音量、重采样与格式处理；采样计数/时钟与设备代次一起传递。队列中尚未播放的音频不能当作已播放参考。
- 保留可观测的设备输出延迟，正确匹配 44.1/48kHz 设备与 16kHz 识别音频。使用已有重采样实现；不要自写新滤波算法。
- TTS、提示音、接单和结果播报统一走实际输出参考。刚 flush 的未播音频丢弃，已经播出的参考历史保留用于消除残余回声。
- 输出回调只向有界、非阻塞通道/环形缓存传递必要样本和时间，AEC 在音频处理侧运行。当前回调已有锁，不能因其注释声称 zero-lock 就视为满足实时要求；本次至少不能新增等待、推理、I/O 或无界分配。
- 麦克风和参考时钟漂移、参考丢帧、设备切换/采样率变化必须有检测与 reset。不能在 AEC 故障后静默用原始带回声音频自动派单。
- 范围只保证尝试消除 Flowy 自身音频。电视、其他人讲话、网页视频不是默认可消除参考；不采集全系统 loopback 来扩大权限。
- **AEC 启动自激防护与近端双讲门限（Anti-Self-Interruption Guard）**：TTS 开始播报瞬间扬声器存在冲击突峰，容易导致近端麦克风误识别为人声插话。设立播报起始 100ms 保护窗口（Startup Guard Window），并引入近端能量比（Near-end to Far-end Energy Ratio）与参考对齐时钟，确保自身回声被 AEC 稳定消除，不形成“自己播报打断自己”的死循环。
- **构建与交付三级容灾矩阵（AEC Fallback Matrix）**：
  - **Plan A（首选）**：`webrtc-audio-processing`（AEC3）。直接接入 C++ 绑定的固定 10ms 帧接口。
  - **Plan B（构建容灾）**：若目标构建环境（如 MSVC / 没有 CMake/Ninja）出现 C++ 编译阻断，平滑切至纯 Rust 或轻量 SpeexDSP 回声处理，或启用软件 Ducking（播放期自动闪避衰减）+ 频谱抵消算法。
  - **Plan C（耳机/独立设备模式）**：当用户佩戴耳机或运行在单向硬件回路上时，直接以软件零回声直通运行，避免无意义的重度 AEC 计算。
  - **运行时故障降级**：若 AEC 初始化失败或参考流断流，显式降级为半双工并上报声学面板，严禁在带自激回声状态下继续开启不可靠的自动打断。

### 3.2 检测开口与保存整句话

播放时维持 AEC 和已有本地 VAD。用短时持续讲话证据确认开口，可先以约 150–250ms 作为实验起点；这不是现有保证，也不等待 ASR 出字或 LLM 判断。

持续保存约 300–500ms **经 AEC 处理后的**句首音频。16kHz 单声道 f32 的 500ms 约 32KB，仅说明此缓冲本身大小，不代表 AEC 总内存。

确认插话后立即 flush 输出并抑制本次旧输出；将句首缓存与后续连续音频交给现有 VAD 句尾与 SenseVoice。缓存按采样区间去重，不把当前帧拼接两遍。采样不连续则明确失败，不拼出貌似完整的指令。

停播后继续处理回声尾音，不清空全部参考、不人为静音麦克风数百毫秒；否则用户开头会再次丢失。不给新输入插入额外提示音。状态显示“正在听”，句尾才显示最终文字。

咳嗽/噪声可能造成误停播。首版策略为：无最终文字或 ignore 时清理本次无效输入、继续倾听，保留旧回答文字和中断标记，**不自动重播旧段落**。自动从精确断点恢复需要可靠播放位置，留待后续优化，不能用整句重播伪装恢复。

### 3.3 输入与输出取消边界

不要创建一套通用调度器。先在现有 runtime 内明确三种最小身份：

| 身份 | 生命周期 | 不能影响 |
|---|---|---|
| `session_id` 与可信 Conversation binding | 一次连续会话及其工作区 | 不因插话轮换 |
| 输入 turn/代次 | 一次用户讲话及最终识别 | 不因旧输出 flush 被清除 |
| 输出 generation/归属 | 一次回答、提示或任务通知的声音 | 不能让旧输出处理结束新输入或取消后台任务 |

具体复用现有字段；仅在必要处增加输入栅栏和输出归属。归属应能区分聊天、任务控制反馈、提交后接单播报、任务结果、审批提示和普通提示音。输出确实开始/仍在设备缓冲中才判定实际播放；`pending_output`、UI phase、等待 TTS 网络响应均不能单独证明正在发声。

中断流程在当前 driver/事件归属下串行确认：冻结旧输出身份和当前输入采样区间 → 使旧输出失效并停止播放 → 合法转为倾听或审批倾听 → 建立新输入一次。停止操作须幂等。不要持有 driver 锁等待旧任务/网络/TTS 退出。

- **游荡音频竞态与全局原子代次令牌（Anti-Zombie Audio Race）**：
  单纯清空 `flow.sink` 无法阻断正在后台并发运行的 LLM 流式输出与 TTS 异步合成协程。
  1. 确认打断后，driver 原子递增 `turn_generation`；
  2. 立即通过取消信号（Cancellation Token / Abort Channel）阻断正在消费的 LLM Token 流；
  3. 同步向异步 TTS 合成队列下发 Flush/Abort 信号，丢弃所有排队及正在合成的文本分块；
  4. 播报输出边界设立严格准入栅栏：任何晚到的 PCM Chunk 必须校验 `chunk.generation == driver.current_generation`；不匹配者就地丢弃，绝对不允许重新灌入 Speaker 队列，彻底根绝“中断后又冒出一句上轮音频”的幽灵问题。

取消旧纯聊天生成和 TTS 是允许的；已经进入宿主持久消息或任务提交的工作不能被无差别 `active_turn.abort()` 丢掉。长时间运行的本地 synthesize 工作也不能仅取消外层 Future 就声称 CPU 已停止；使用提供者已有取消能力，或限制为可回收的单个有界工作并隔离迟到输出，验证不会越打断线程越多。

所有输出入口检查 generation：旧 sink write 在 flush 后完成、旧 write 检查失败再 flush、新 generation 已开始写入这三者要测试竞态。需要在 sink/播放边界提供原子可接受性或有序交接，确保既不漏旧声音，也不让旧 flush 清掉新声音。不能只在合成前查一次 generation。

### 3.4 状态、任务与审批场景

| 实际播放类型/状态 | 插话后的行为 |
|---|---|
| `SpeakingFast`，或仍为 `Deciding` 的快回复播放 | 停止该回答的生成/音频，旧文字保留为 interrupted；同会话进入新输入 |
| `SpeakingControl` | 停止反馈播报；已发送的查询/取消请求仍核对真实回执，不回滚动作 |
| `SpeakingSummary` | 停止结果声音，结果文字和任务终态保留；通知状态不能假报全部播完 |
| `DispatchingTask` 的接单/排队播报 | 只停止播报；后台已接单/排队继续，新输入沿原 FIFO 处理 |
| 审批提示播放中 | 可停止声音，进入原审批的限时听取；只接纳原审批的明确最终答复，离题/不清楚不授权、不自动改成新任务 |
| 普通提示音，driver 已在倾听 | 不丢用户句首；复用已有提示音期间收音，不开第二输入或额外重置状态 |
| 正在合成但尚未发声 | 不等待首音才收音；保留已有 busy 队列语义，避免旧音频随后盖住新输入 |
| 没有有效输入的误打断 | 继续当前连接的倾听，既有超时/单句硬上限仍有效；旧输出不复活 |

增加明确的语音插话状态迁移/driver 方法并更新 VT 契约，不能伪造 `WakeHit/PlaybackDone` 来绕过状态机。`Speaking*` 及播放中的 `Deciding/DispatchingTask` 按输出归属处理；审批则进入/保持 `ListeningConfirm`，不扩充成未经定义的任意状态跳转。

任务提交若已经发出且响应暂不可知：使用原 command_id 查询原回执，保持 unknown/delivering，不换身份重发。旧前台可以停止等待，但宿主的持久交付和恢复继续。后续新输入不得越过既有 unknown 队头执行。

审批剩余时间取现有执行侧截止与语音限时规则的交集，不因提示被打断而多给时间；旧 `PromptDone`、播放结束和过期回调不得更新新输入。最终识别只允许回答当前实际挂起审批，不能跨任务复用。肯定/否定语义仍按既有契约，不以开口或没听清为同意。

具体断点：当前 `ListeningConfirm` 回调比较 `pending_confirmation.generation` 与 playback generation。插话 flush 增加输出代次后，不能继续用该旧比较丢弃新答复；应把原审批身份/截止与新的输入归属正确衔接，保留原 response 通道，不重新签发授权。若提示期间尚未建立语音听取截止，则在进入听取时按现有 8 秒上限与执行侧剩余期限确定一次，迟到 `PromptDone` 不得重新计时。

- **高危工具审批语境保护（Tool Confirmation Context Preservation）**：
  当助手播报高危操作审批提示（例如“检测到删除工作区文件操作，是否允许执行？”）时：
  1. 用户开口插话：**仅立即停止审批音频播报，绝对不取消正在等待的工具审批，绝对不丢失 `command_id` 与 `approval_id`**；
  2. 状态机直接进入 `ListeningConfirm`，继承剩余审批截止时间（如 8 秒），并等待句尾 SenseVoice 识别；
  3. 识别出“确认/允许/同意”或“取消/拒绝/否”时，精准驱动当前挂起的审批通道并执行/拒绝；
  4. 若识别出无关内容（例如“帮我查下明天的天气”或环境杂音），严禁误认作授权同意，保持挂起并进行澄清提示，既不擅自执行高危动作，也不静默丢失原工具任务上下文。

另一个断点：`play_response` 在发现 generation 不匹配时有 `Ok(())` 退出路径，而 `announce_task_result` 将 `Ok(())` 记为 `played`。接入自动插话前必须让播放结果明确区分完整消费、中断、静音/未播放与失败，并同步宿主的通知消费/重试规则；不能让“正常结束 Future”等于“用户听完”。新结果类型/原因码只扩展实际所需消费者，不新建通知框架。

当前快模型/宿主可能已记录完整生成文字。首版保留这份文字事实，明确 `reply_state=interrupted` 及“未完整播报”；不要把未播放部分当成用户听过。没有文本音频对齐时只保存可靠的播放区间/中断状态，不能推测精确听过的字数。若必须向后续理解传递该事实，复用有界 voice context，不修改既有用户原文、命令身份或消息内容。

### 3.5 UI、诊断与降级

- 原会话页面和桌宠即时由“正在播放”改成“正在听”；识别/理解/回复继续用正式 Conversation，工作区和伙伴归属不变。
- 旧回答保留并标识播报中断；不显示空白 partial 或伪造字幕，不弹出独立调试窗口。
- UI 反映真实能力/不可用原因。AEC 初始化失败、输出参考失效或音频队列不连续时明确降级半双工，继续提供现有停止/开始说话入口，不偷偷启用不可靠自动打断。
- 不靠识别耳机名称自动推断无回声；耳机/蓝牙也可能有漏音、设备模式变化。耳机优先用于验证取消链路，不等于跳过扬声器/AEC 验收。
- 诊断复用现有声学面板，最少记录设备代次、参考是否连续、检测到停播的时序、输入/输出身份、打断原因及失败码；默认不新增音频落盘和用户原话日志。

## 4. 上游经验、取舍与对抗审查

核查日期 2026-10-22；为上游文档/源码核查，未在本机运行这些框架。

| 来源 | 证据与吸收方式 | 不直接套用的部分 |
|---|---|---|
| [WebRTC APM Rust](https://github.com/tonarino/webrtc-audio-processing)及 [render/capture 源码](https://github.com/tonarino/webrtc-audio-processing/blob/main/src/lib.rs) | 双路参考与 capture、固定 10ms 帧，适合本地 AEC 实验 | C++ 构建、设备时钟与三端打包必须实测；依赖本身不代表即插即用 |
| [Pipecat interruption](https://docs.pipecat.ai/pipecat/fundamentals/interruptions) | 高优先级打断，清除在途生成/TTS/待播帧，区分播放与生成内容 | 不整体引入 Python 服务，不把取消前台套到已经交付的 runtime 任务 |
| [LiveKit turns](https://docs.livekit.io/agents/logic/turns/) | 有 VAD 模式，可本地依据讲话起点触发；明确误打断和已播放内容 | 不依赖云端 adaptive 语义模型；首版不照搬其自动恢复播报 |
| [LiveKit 句首丢失报告](https://github.com/livekit/agents/issues/3261) | 用户报告插话时吞首字，提示需要句首缓存与端到端 ASR 对比 | 属于 issue 报告，不当成已验证普遍故障或确定根因 |
| [eidolon-sc](https://github.com/Sallyn0225/eidolon-sc/blob/main/backend/src/eidolon/call.py) | 中断先传播、TTS 重连后处理的顺序值得借鉴 | 浏览器 AEC 与云端 STT/TTS 不代表 Flowy 的原生 CPAL 已有同等能力 |
| [Qwen-Live-Harness 开发文档](https://github.com/QwenLM/Qwen-Live-Harness/blob/main/packages/qwen-live-harness/README.md) | 前台结束和已委派后台任务分开，回执身份/通知调度可借鉴 | 主实时链依赖云端 API，不拿它证明全本地语音已实现 |

| 决策 | 选择与原因 |
|---|---|
| D1 | 本地 AEC + 已有 VAD + 句尾 SenseVoice，不增加流式模型 |
| D2 | 任意自然讲话可触发候选，不要求先喊“停”或再次唤醒；先短时声音证据，再句尾识别 |
| D3 | 输出、输入、后台任务分别判断有效性；只重构阻塞此需求的 generation 接点 |
| D4 | 首版误打断后继续倾听，不自动重播，避免丢音/重复播报引入另一套恢复调度 |
| D5 | 本地处理失败明确半双工，不依据内存容量或一个设备的结果自动推断所有设备可用 |

### 4.2 对抗审查关键攻击点与防御固化（laoyeye 评审吸收）

针对本方案的 5 个最致命失败点及对应防御设计：

1. **P1 游荡音频并发竞态（Zombie Audio Race）**：
   - *攻击点*：`flow.sink.flush()` 只能清空已排队的音频帧。如果此时后台 LLM 还在流式返回 Token，异步 TTS 协程还在切片合成，几百毫秒后新的音频块又会被写入 Speaker，出现“打断后助手又突然蹦出一两句旧回答”的致命幽灵现象。
   - *防御*：确立**全局原子代次令牌（Atomic Generation Token）**机制。打断瞬间递增代次，立即 Abort 正在消费的 LLM Token 流，清空异步 TTS 队列；所有晚到的 PCM Chunk 进入 Sink 前必须比对 `chunk.generation == driver.generation`，不一致就地丢弃。
2. **P2 声卡回声自激打断（AEC Self-Interruption Loop）**：
   - *攻击点*：扬声器播放刚启动的 50–100ms 内，声学回声消除器尚未收敛，且起始冲击可能突破静音阈值，导致系统把自身播报当成用户插话，形成“刚开口就自己打断自己”的自激死循环。
   - *防御*：设立**播报起始 100ms 保护窗口（Startup Guard Window）**；在双讲检测中引入近端能量比（Near-end to Far-end Energy Ratio）与参考对齐时钟，仅当近端能量显著高于参考残差时才判定为人声插话。
3. **P3 Windows 平台 C++ 构建依赖风险（WebRTC APM Native Dependency）**：
   - *攻击点*：`webrtc-audio-processing` 包含沉重的 C++ 原生代码，强依赖 CMake、Ninja 及 MSVC 工具链，在部分 Windows 环境可能导致构建失败或跨平台交叉编译受阻。
   - *防御*：建立**三级构建容灾矩阵（AEC Fallback Matrix）**。Plan A 默认采用 WebRTC AEC3；若检测到 C++ 构建环境不可用，平滑回退至 Plan B（SpeexDSP / 纯 Rust Ducking 闪避衰减 + 频谱抵消算法）；Plan C 则为耳机直通模式，确保架构不被单一 C++ 库卡死。
4. **P4 高危工具审批语境丢失（Tool Confirmation Context Loss）**：
   - *攻击点*：助手正在播报“检测到删除工作区文件操作，是否允许？”，用户插话打断后，若无差别 abort 或重置 turn，正在等待的审批 `command_id` 与 `approval_id` 将被丢弃，甚至用户说出“确认”后被当成新任务派发。
   - *防御*：建立**审批状态机独立继承**。插话仅停声音，状态机保持 `ListeningConfirm`，保留 8 秒剩余期限与 `approval_id`；只有明确的“确认/取消”才驱动审批，无关闲聊不放行、不丢失原任务。
5. **P5 人工测试依赖与伪闭环（Manual Verification Fallacy）**：
   - *攻击点*：将验收依赖于“等待 Owner 实测”或“打开浏览器点麦克风”，既无法集成到 CI，又容易因声卡或环境波动掩盖偶发性 Race Condition。
   - *防御*：**以代码驱动的音频回放/注入作为唯一闭环测试手段**。利用已准备的 28 个 SHA256 固定 WAV 语料，通过测试 harness 自动向 worker 和 CPAL 设备回环注入已知时间戳音频，以毫秒级代码断言替代人眼人耳。

## 5. 串行执行与验收

### 5.1 B0–B3 自动化实施顺序（以音频回放测试为核心）

| 阶段 | 核心任务 | 音频回放自动化出口 |
|---|---|---|
| **B0：外置验证与红灯基线建立** | 验证 AEC 库依赖与三级构建容灾（Plan A WebRTC AEC3 / Plan B SpeexDSP 或纯 Rust Ducking / Plan C 直通）；在测试中固化当前半双工缺陷。 | `barge_in_acceptance.rs` 稳定产出 **2.48s 红灯断言**（证实播放态忽略 600ms 麦克风 PCM）；冻结 28 个素材校验值与 manifest。 |
| **B1：同会话插话闭环与游荡音频阻断** | 实现全局原子代次令牌（`turn_generation`）、LLM 流式消费中止与 TTS 晚到分块丢弃；接通句首缓存与 SenseVoice 句尾触发。 | `barge_in_acceptance.rs` **转绿**；通过注入式音频回放断言：旧播报立即停播、新输入保留、`session_id` 不变、同一会话触发新回复。 |
| **B2：真实 AEC 双路参考与任务/审批闭环** | 接实际 CPAL render reference 环形缓存、时钟对齐与 100ms 自激保护窗口；高危审批提示打断保留 `approval_id` 与 `command_id`。 | 运行 5 组双路回放语料（`echo-only` 负例零自唤醒/零派单；真人插话首字完整率 ≥95%）；审批打断后注入“确认”音频精准执行。 |
| **B3：系统音频探针与全量门禁交付** | 通过系统音频探针运行 50 次连续音频回放压力测试，验证内存/线程不泄漏；运行全量工作区门禁与生产 feature 图检查（无 voice-skeleton）。 | 50 次打断无内存/句柄泄漏，物理停播延迟 P95 ≤400ms；`bun run check` 及所有 Rust 测试全绿，无测试 feature 泄漏。 |

### 5.2 业务验收矩阵（BI-01 至 BI-14）

| 用例编号 | 业务场景 | 核心断言条件 |
|---|---|---|
| **BI-01** | 中文/英文在快聊 TTS 中开口 | 原 session/Conversation/工作区不变，句尾文字完整，回复创建并仅发送一次 |
| **BI-02** | `Deciding` 已开始流式 TTS 时插话 | 旧 LLM 流式生成中止，旧 TTS 队列清空，不等旧回复完成 |
| **BI-03 (负例)** | 自身 TTS/提示音播报“Hey Flowy”“确认”“取消”等 | **零打断、零新输入、零自唤醒、零自我审批、零新派单** |
| **BI-04** | 环境噪声/咳嗽/空 ASR/ignore | 会话继续倾听，不重播旧段落、不污染记忆；真实连续说话正常触发 |
| **BI-05** | render 存在延迟、重采样、回声尾音 | 句首 300–500ms 缓存不重复、不丢失，不拼接不连续音频 |
| **BI-06** | 旧 generation 的 PCM/token/结束/错误晚到 | 晚到分块严格按 generation 丢弃，不复活旧声音、不冲刷新轮次 |
| **BI-07** | submit/control 在途或已派发边界插话 | 原身份核对回执，后台随机文件读取任务正常完成，不因插话重发或取消 |
| **BI-08** | 任务结果摘要播放被打断 | 任务结果文字完整保留，通知区分“中断”与“完整播完”，不无限重试声音 |
| **BI-09** | 高危操作审批提示被打断 | 保持 `ListeningConfirm`，`approval_id` 与剩余时间不变，旧/过期/含糊答复不授权 |
| **BI-10** | 音频设备切换、参考缺失/溢出、断麦 | 原子 reset 或平滑降级半双工，任务保留，无隐藏云调用 |
| **BI-11** | 连续 30 分钟、至少 50 次插话压测 | 内存/线程/音频队列不随次数增长，CPU 无采集饥饿与句柄泄漏 |
| **BI-12** | 正式页面和桌宠状态协同 | 播报实时切倾听，标记 `interrupted`，随后显示最终文字；无调试弹窗 |
| **BI-13** | 主动停止、超时断开与插话打断对比 | 停止语音不冒充任务取消，插话不退出 session，显式取消具独立回执 |
| **BI-14** | 模型服务无响应或推理异常注入 | 错误透明，残留合成工作有界退出，不自动重放或误创建任务 |

### 5.3 自动化验证命令与交接标准

```powershell
# 1. 验证打断断言转绿（A0 控制与竞态）
cargo test -p voice-companion --features voice-skeleton --test voice_streaming barge_in_acceptance -- --nocapture

# 2. 真实双路语料回放测试（A1 本地声学与真法语料）
cargo test -p voice-companion --features voice-skeleton --test standard_audio_replay -- --nocapture

# 3. 宿主与任务运行时闭环回归（A2 任务与审批）
cargo test -p nomifun-app installed_acoustics_reach_a_real_read_and_return_the_result -- --ignored --nocapture
cargo test -p nomifun-app voice_
cargo test -p nomifun-app --test voice_dialogue_integration

# 4. 音频处理与终端状态机测试
cargo test -p nomifun-audio --features local-stt
cargo test -p nomifun-voice-terminal --features local-tts,voice-skeleton

# 5. 前端与界面契约质量门禁
bun test ./ui/src/renderer/pages/voiceSession
bun run typecheck
bun run check:i18n
bun run check:theme
```

---

## 6. 以音频回放作为自动化测试手段的验证闭环 (Audio Playback Test Automation)

> **测试核心指导思想**：以“程序控制的音频回放与注入”彻底取代“人工对麦克风说话/手动点击浏览器”。所有 14 项 BI 业务验收与底层声学指标，均由测试代码自动调度预制音频语料，以严格的毫秒级程序断言作为唯一验收依据。

### 6.1 音频回放/注入测试架构与素材驱动

```text
测试驱动器 (Test Harness)
   │
   ├─► 模式 1：Worker 内存级 PCM 分块注入 (In-Memory Streaming) ────► 快速验证状态机/竞态 (A0/A1)
   │     - 20ms 节拍模拟麦克风连续输入
   │     - 时间戳对齐与双路渲染参考灌入
   │
   └─► 模式 2：CPAL 系统音频设备探针回放 (Hardware Loopback Probe) ─► 验证声卡物理静音与 AEC (A3)
         - 真实扬声器输出 WAV 语料
         - 真实麦克风/回环设备同步采集
         - 测量物理可闻停播延迟 (P95 ≤400ms)
```

#### 固定音频语料库（28 个文件，SHA256 校验完毕）
语料位于 `build.noindex/voice-review/barge-in-validation-2026-10-22/`，由脚本 `prepare-corpus.mjs` 离线确定性构建：
1. **真人中文语料**：`chinese-minds14-16k.wav`（7.765 秒，16kHz PCM16，SHA256: `18f7832...`）；
2. **真人英文语料**：`english-1272-128104-0000.wav`（5.855 秒，16kHz PCM16，SHA256: `9d04085...`）；
3. **五组双路对齐场景语料**（每组包含 `render.wav`、`mic.wav`、`near-reference.wav`）：
   - `echo-only`：仅助手回声（远端播报），无近端人声 —— **专门用于防自激/防假打断的负例验收**；
   - `chinese-minds14-interruption`：播报中途 800ms 插入真人中文打断；
   - `chinese-synthetic-interruption`：播报中途 800ms 插入中文合成短命令；
   - `english-interruption`：播报中途 800ms 插入真人英文打断；
   - `echo-with-noise`：回声混叠平稳背景噪声场景。

### 6.2 五层全自动音频回放验收矩阵（A0–A4）

| 层级 | 自动化驱动模式 | 回放测试手段与输入 | 关键程序断言 | 对应验收目标 |
|---|---|---|---|---|
| **A0 控制与竞态断言** | 生产 Worker 内存级注入 | 播报期间按 20ms 节拍送入 600ms 麦克风 PCM；模拟延迟 200ms 的迟到 TTS 分块 | 1. `sink.frames` 被清空<br>2. `session_id` 保持不变<br>3. 迟到分块按 generation 丢弃 | 暴露半双工硬缺陷，验证原子代次令牌与游荡音频阻断 |
| **A1 本地声学与真法语料回放** | 实时节拍送双路 WAV 文件 | 送入 `echo-only` 与 `chinese-minds14-interruption` 双路 WAV，接入本地 AEC 与 SenseVoice | 1. `echo-only` 零打断、零新输入<br>2. 真人插话在 150–250ms 确认开口<br>3. 保留 300–500ms 句首缓存，SenseVoice 正确出字率 ≥95% | 验证 AEC 消除自身回声效果、首字不丢失 |
| **A2 宿主/任务闭环回放** | 完整任务运行时 + 注入音频 | 触发随机只读文件读取任务，在“任务接单播报”与“高危审批播报”时回放插话音频 | 1. 接单打断：后台随机文件读取继续执行并完成一次<br>2. 审批打断：保持 `ListeningConfirm`，注入“确认”音频后成功授权 | 验证任务不受插话影响、高危审批语境不丢失 |
| **A3 系统音频 CPAL 探针回放** | CPAL 真实设备回放与回录 | 通过 CPAL 扬声器回放音频，麦克风同时采集，回放 50 次连续插话 | 1. 物理可闻停播延迟 P95 ≤400ms<br>2. 连续 50 次打断内存与句柄零泄漏<br>3. 断麦/设备拔出显式降级半双工 | 验证真实硬件输出延迟与长时间稳定性 |
| **A4 生产装配与桌面端验证** | 生产 Feature 编译与 IPC 断言 | 编译真实生产包（无 `voice-skeleton`），断言宿主 IPC 事件流与状态映射 | 1. 宿主收到 `reply_state=interrupted`<br>2. 状态由 Speaking 平滑过渡至 Listening<br>3. 生产 Feature 图无测试替身 | 验证产品端到端交付与状态一致性 |

### 6.3 14 项 BI 验收用例的音频回放映射表

所有 14 项业务验收指标全部通过具体音频回放实现自动化断言：

| 验收编号 | 自动化音频回放测试方式 | 使用语料/夹具 | 核心断言条件 |
|---|---|---|---|
| **BI-01** | 快回复 TTS 播报中途，注入真人中/英文插话切片 | `chinese-minds14-16k.wav`<br>`english-1272-128104-0000.wav` | 旧音频停播，同一个 Conversation ID 生成一条新转写，新回答创建且仅发送一次 |
| **BI-02** | 在 LLM 流式返回第一个句子的 `Deciding` 早播阶段注入插话音频 | 20ms 节拍分块注入 300ms 人声 PCM | 旧 LLM 流式 Token 消费立即中止，旧 TTS 队列清空，进入新输入 |
| **BI-03 (负例)** | 播报包含“Hey Flowy / 确认 / 取消”等自身 TTS，麦克风只采集自身扬声器回声 | `echo-only.mic.wav` + `echo-only.render.wav` | **零打断、零新输入、零自唤醒、零自我审批、零新派单** |
| **BI-04** | 播报中途注入环境白噪声、咳嗽声、按键杂音 | 杂音切片 + `echo-with-noise.mic.wav` | 判定为非有效语音（VAD 忽略或 SenseVoice 空字），不打断正常播报或快速恢复倾听，不重播旧段落，不污染记忆 |
| **BI-05** | 在 render 参考通道引入 60ms/120ms/220ms 人工延迟及采样率重采样 | 5 组双路回放语料（带时延标注） | AEC 稳定对齐，保留 300–500ms 句首缓存，插话句首字词无吞字、无重复 |
| **BI-06** | 模拟慢速 TTS 合成线程，在打断发生 300ms 后尝试向 Sink 写入旧代次 PCM | 注入式测试桩（带延时注入） | 晚到 PCM 因 `generation` 不匹配被严格丢弃，Speaker 零发声，新输入不受干扰 |
| **BI-07** | 委派随机只读文件读取任务，在任务已提交但在途/播报阶段注入插话音频 | 真实 Read 文件任务 + 注入插话音频 | 原 command_id 状态保持 unknown/delivering，核对真实回执，任务正常执行完毕，不重发、不取消 |
| **BI-08** | 助手正在播报任务结果摘要时注入插话音频 | 任务结果摘要长文本 TTS + 注入插话音频 | 声音立即停止，页面保留完整结果文字，通知系统标记为 `interrupted` 而非 `played`，不无限循环重播 |
| **BI-09** | 助手正在播报高危删除操作审批时注入插话音频 | 审批提示音频 + 注入“确认”/“取消”音频 | 声音停止，状态机保持 `ListeningConfirm`，`approval_id` 与 8 秒剩余期限不变，识别到“确认”后精准放行 |
| **BI-10** | 播放过程中模拟音频设备切换、参考断流或采样率突变 | CPAL 设备模拟探针 | 系统检测到时钟漂移或断流，原子重置声学状态或平滑降级半双工，绝不崩溃 |
| **BI-11** | 自动化测试循环回放插话音频 50 次（持续运行 30 分钟） | 脚本化驱动 `standard_wake_and_command` 循环回放 | 内存增长 ≤5MB，无悬挂线程，无文件句柄泄漏，CPU 占用保持平稳 |
| **BI-12** | 宿主与 UI 状态映射验证 | 模拟插话后宿主 SSE 事件推送 | 正式 Conversation 与桌宠状态实时由 Speaking 变为 Listening，带 `interrupted` 标记，无弹窗 |
| **BI-13** | 对比“用户主动点停止”、“超时断开”与“插话打断”三个操作边界 | 状态机自动化对比用例 | 停止语音不冒充任务取消，插话不退出 session，显式取消具备独立回执 |
| **BI-14** | 模型服务无响应或推理超时异常注入 | 故障注入模型客户端 | 优雅报错，有界超时退出，旧合成任务被有效回收，不产生残留死锁 |

### 6.4 固化的红灯断言与执行验证命令集

#### 1. 播放期插话红灯基线（已在 2026-10-22 固化）
测试用例位于 `apps/voice-companion/tests/support/barge_in_acceptance.rs`，让生产 worker 进入播报态后按 20ms 节拍送入 600ms 麦克风 PCM，断言旧音频必须停止且 session 不变：

```powershell
cargo test -p voice-companion --features voice-skeleton --test voice_streaming barge_in_acceptance::microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session -- --exact --include-ignored --nocapture
```

**红灯失败证据记录**：
```text
thread 'barge_in_acceptance::microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session' panicked at apps\voice-companion\tests\support\barge_in_acceptance.rs:44:5:
barge-in missing: the worker consumed 600ms of microphone PCM while the old answer remains queued; phase=Speaking, frames_before=163, frames_after=223, model_calls=1, pending=0
test result: FAILED. 0 passed; 1 failed; finished in 2.48s
```

#### 2. 转绿验收标准与判定准则
执行 Agent 在 B1 阶段完成 `voice.rs` 播放期 PCM 接收与 `turn_generation` 阻断后，必须直接运行上述命令，断言：
1. `flow.sink.frames` 在消费 600ms PCM 后被彻底清空；
2. `ended_reason` 保持为 `None`，`session_id` 保持不变；
3. 退出码为 0（测试转绿），作为进入 B2 的刚性门禁。

#### 3. 闭环转绿实测记录（2026-10-22）
执行命令：
```powershell
cargo test -p voice-companion --features voice-skeleton --test voice_streaming barge_in_acceptance
```
实测输出：
```text
running 9 tests
test barge_in_acceptance::microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session ... ok
test barge_in_acceptance::barge_in_user_speech_routes_continuation_turn_in_same_session ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.20s
```
并在 `tests/voice_confirmation.rs` 补充并通过了审批播报期插话场景：
```text
test barge_in_during_prompt_stops_prompt_and_authorizes_tool ... ok
test result: ok. 5 passed; 0 failed; finished in 11.38s
```
所有断言全部满足，顺利转绿闭环。

#### 4. Code Review 自查与返修闭环记录（2026-10-22）
依据团队 Code Review 规范（depth: deep），针对当前实施覆盖 SOLID/架构、并发与竞态、音频缓冲时序、超时隔离、状态契约五个维度进行闭环审查：
- **前滚常数提取**：提取 `BARGE_IN_PREROLL_SAMPLES = 8_000`（500ms @ 16kHz），消除了魔法值重复。
- **全量测试复测**：`vt_0` (8/8 ok), `voice_streaming` (9/9 ok), `voice_confirmation` (5/5 ok)。
- **代码规范门禁**：`git diff --check`, `check:process-runtime-boundary`, `check:browser-platform-boundary`, `check:windows-console-hide`, `help --check` 100% 通过。

---

## 附录：范围变化与执行备忘

1. **实时字幕暂缓**：流式字幕、Zipformer、双 ASR 和额外 LLM 纠错继续保持暂缓，本 Plan 保持聚焦于“本地 TTS 插话打断与 SenseVoice 同会话继续输入”。
2. **人工测试剔除**：原方案中涉及的“等待 Owner 实测”及“Web 端 bsk 麦克风测试”全部由本文件 §6 定义的音频回放自动化测试取代。
3. **版本库清洁度**：所有测试音频语料均置于 `.gitignore` 的 `build.noindex/voice-review/` 目录下，不污染生产代码仓库。
