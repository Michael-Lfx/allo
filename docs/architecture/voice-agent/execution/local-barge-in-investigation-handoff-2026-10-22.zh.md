# 本地语音插话打断 · 调研与验证准备执行报告

> **最新交接修订**：以[本地 TTS 插话打断与继续输入 · 优化执行 Plan](local-barge-in-only-plan-2026-10-22.zh.md)与[系统链路与代码自动验收方案](local-barge-in-system-auto-validation-2026-10-22.zh.md)为准。全面确立**代码驱动的全自动音频回放/注入测试（Audio Playback Test Automation）**作为唯一验证手段，彻底移除“等待 Owner 实测”与“Web 页面人工点击”。
> **日期**：2026-10-22，Asia/Hong_Kong。
> **交付性质**：调研、方案收敛、测试素材准备与交接；**尚未实施自动打断功能**。
> **执行入口**：[本地 TTS 插话打断与继续输入 · 优化执行 Plan](local-barge-in-only-plan-2026-10-22.zh.md)，实现按 §5 B0–B3，验收按 §6 A0–A4 与 BI-01–14 映射表。
> **当前分支/HEAD**：`feat/voice-next` / `e9b1255362de909f3567450996ab0cb77def4d0e`。

## 1. 用户最终要求与范围

交付如下完整流程：

```text
助手正在播报 → 用户直接开口 → 停止旧播报 → 收完整句话
→ 现有 SenseVoice 句尾识别 → 同一个 Conversation 回复或委派 runtime
```

用户已明确：

- 暂不做流式字幕、Zipformer、双 ASR、额外 LLM 纠错调用；不根据单一机器或 16GB 容量推断所有用户性能。
- 打断判定走本地音频处理，不等待 ASR/LLM；不要求先喊“停”或重新说唤醒词。
- **浏览器用于观察真实文字和状态；音频输入可以且应当走本机真实输入设备/CPAL，不走浏览器假麦克风。**
- 语音仍对接软件现有工作区 Conversation/runtime。停播不等于结束语音会话，也不等于取消后台任务。
- 正式会话与桌宠承担交互，不恢复旧调试弹窗。

当前任务不要求迁移快模型/provider、保证全链路离线、变更审批权限、引入声纹或新增任务调度框架。音频前处理、AEC、VAD 和打断决定必须可本地完成。

## 2. 本轮实际完成与尚未完成

| 项目 | 状态 | 证据与边界 |
|---|---|---|
| 语音采集、状态机、播放、任务提交路径核查 | 已完成 | 代码入口与具体断点见 §3；不是业务修复 |
| 开源项目核查 | 已完成 | WebRTC APM、Pipecat、LiveKit、Qwen-Live-Harness、eidolon-sc；详见主 Plan §4 |
| 收敛执行 Plan 与 14 个 BI 验收项 | 已完成 | 主 Plan §5，流式字幕组合方案保留作历史 |
| 音频来源、下载、规范化、合成双路场景 | 已完成 | 2 段真人语音、既有 11 段夹具、5 场景/15 段双路夹具 |
| 素材完整性检查 | 已完成 | manifest 中 28 个文件 SHA256 匹配，5 组时轴长度一致，无削波 |
| 项目外 AEC 库编译、回声效果验证 | **未执行** | B0 工作，不得标记通过 |
| 真实 SenseVoice/KWS/VAD 回放 | **本轮未执行** | 已定位现有测试与历史模型目录，未加载模型 |
| 自动停播/同会话继续输入代码 | **未实施** | 当前仍为半双工 |
| 实体麦克风、扬声器、蓝牙测试 | **未执行** | 没有采集用户现场音频或播放测试声音 |
| bsk 页面操作/截图 | **未执行** | CLI 存在，daemon 不可用，宿主未运行；没有伪造页面结果 |
| 宿主、daemon、UI 联合构建 | **本轮未执行** | 未修改业务代码；不能复用历史构建作为新功能证据 |

## 3. 当前代码事实与实施必修点

核查基线最初为 `648277420`，结束 HEAD 为 `e9b125536`。两者之间，核查的 voice-companion、nomifun-audio、nomifun-voice-terminal 及宿主 voice manager 无新增差异；其他执行者的模型 UI 改动不是本报告的业务交付。

### 3.1 声学与播放

- `apps/voice-companion/src/voice.rs::spawn_acoustic_loop`：播放态没有正常进入讲话识别；Deciding/Dispatching 有有界待处理输入，但不等于自然打断。
- 快回复可在内部仍为 `Deciding` 时开始播报，不能只以 `SpeakingFast/Control/Summary` 枚举触发检测。
- `crates/backend/nomifun-audio/src/speaker.rs`：CPAL 回调取队列并输出，尚无接通的 AEC render reference。须提供真正送到设备的 PCM、时钟/延迟和设备代次，包含提示音。
- `PlaybackPipeline::interrupt`：增加输出 generation 并 flush。注释的 `<50ms` 不是本轮硬件证据。
- `stop_session` 会 reset driver、abort active turn、清 pending 并结束 session，**不得拿来实现插话**。

### 3.2 取消、回执与通知

- 当前播放 generation 被多个处理路径用作整轮有效性栅栏。需隔离旧输出失效与新输入身份，阻止旧 PCM/token/结束/错误回调污染下一句。
- 任务提交、控制请求、接单播报在同一处理路径里。不能对整轮无差别 abort，造成已经发出的请求/回执丢失；原 command_id 的 unknown 必须先核回执，不换身份重发。
- `play_response` 有 generation 不匹配仍以 `Ok(())` 退出的路径，而结果通知会将 `Ok(())` 记为 `played`。必须区分真正播完、中断、未播/静音和失败，并核对宿主重试策略。
- 当前审批回调比较 `pending_confirmation.generation` 与 playback generation。插话后需正确衔接新输入与原审批，保留 approval 身份、response 通道和截止时间，不能丢新答复、重新签发授权或多给时间。

### 3.3 Web 与真实输入

- shared router `crates/backend/nomifun-app/src/router/routes.rs` 已装配语音 manager、Conversation runtime 和 HTTP/SSE 端点；Web 可观察正式会话结果。
- `ui/src/common/adapter/voiceCompanion.ts` 的非 Tauri 路径支持 HTTP/SSE 与 Conversation 跳转；但 `reportVoiceCandidate` 在非 Tauri 环境直接返回。原生窗口聚焦、桌宠窗口与工作区跟随另做 Tauri 验收。
- 麦克风由 daemon/CPAL 采集，浏览器 `getUserMedia` 假输入不会进入该链路。不向 DOM/store 注入快照，也不伪造 SSE 作为验收。

## 4. 推荐实现与关键取舍

采用：**实际输出参考 → 本地 AEC → 已有 VAD → 确认人声 → 停旧输出并保留句首 → SenseVoice 句尾识别**。

1. AEC 首选实验候选是 [WebRTC AudioProcessing Rust 绑定](https://github.com/tonarino/webrtc-audio-processing)，其 render/capture 使用固定 10ms 帧；先验证版本、许可证、C++ 构建及三端交付，不直接新增生产依赖。
2. 近端讲话确认时间可从 150–250ms 开始实验，保存 300–500ms 经 AEC 处理的句首缓存；都是待测参数。不要停播后再静音麦克风一段时间，造成首字丢失。
3. 停播仅处理旧前台生成与音频；后台任务继续。旧迟到写入、旧 flush 与新音频之间必须有可验证的有序交接。
4. 无识别内容/ignore 的误停播继续倾听，保留旧文字和中断标记；首版不自动重播旧段落，不新增精确断点恢复调度。
5. AEC 故障、参考丢失或设备变化时明确退回半双工与现有停止入口；不能静默用带回声的原始音频派单。

开源经验：[Pipecat](https://docs.pipecat.ai/pipecat/fundamentals/interruptions)用于取消/清队列及已播内容；[LiveKit](https://docs.livekit.io/agents/logic/turns/)用于 VAD/误打断边界；[Qwen-Live-Harness](https://github.com/QwenLM/Qwen-Live-Harness/blob/main/packages/qwen-live-harness/README.md)用于前后台分离和回执/通知。后两者及 eidolon 的现成服务组合不能直接当成纯本地方案。借鉴机制，不整体引入另一套语音框架。

## 5. 已准备音频与复跑方式

目录：`D:/workSpace/git_clone_test/allo/build.noindex/voice-review/barge-in-validation-2026-10-22/`。

| 文件/来源 | 内容与用途 |
|---|---|
| `english-1272-128104-0000.wav` | 真人英文 5.855 秒，LibriSpeech 小样本；来源/原文在 `english-source.json` |
| `chinese-minds14-60351ea44c449c80694dc254.wav` | 真人中文原始 8kHz PCM8；CC BY 4.0，来源在 `chinese-source.json` |
| `chinese-minds14-16k.wav` | 上述中文离线转换为 16kHz PCM16，约 7.765 秒；ffmpeg 仅为测试素材准备工具 |
| `prepare-corpus.mjs` | 无第三方 JS 依赖，仅解析/生成固定夹具；不载模型、不操作设备、不启动服务 |
| `manifest.json` | 状态、样本身份/SHA256、采样率、近端起点、回声参数与明确证据限制 |
| 五组 `*.render.wav / *.mic.wav / *.near-reference.wav` | 纯回声、中文真人、中文合成、英文真人、回声加噪声；固定 800ms 插话起点与合成回声路径 |

现有 11 段仓库夹具约 1.15MB，属于历史合成语音为主的覆盖；新真人样本每种语言只有一个说话者，不能证明人群覆盖。合成回声不能证明真实房间、蓝牙或声卡延迟。

来源固定：

- [中文 MINDS-14](https://huggingface.co/datasets/PolyAI/minds14)，revision `40ce77cb32a384e4d50a568e1ec39ac804019d33`。
- [英文镜像](https://huggingface.co/datasets/hf-internal-testing/librispeech_asr_dummy)，revision `5be91486e11a2d616f4ec5db8d3fd248585ac07a`；[原 LibriSpeech](https://www.openslr.org/12) CC BY 4.0，镜像没有单列 license 字段。
- 所有新素材留在忽略目录，没有加入生产包。跨机器需移交音频、脚本及 manifest；进入版本库/安装资源前再核许可和归属。

当前文件快照：

```text
prepare-corpus.mjs SHA256
374f0001d95fd18d98fa7ebd4db2e9cf3af7895c20d85d1466e5a742b9d15390

manifest.json SHA256
b3037e50b902c7cce08bb318efe14f66e718224bbe83d2ed7623baa8272e55d0
```

上面 hash 针对本报告生成时文件。重跑会更新时间戳，manifest hash 随之变化；固定输入和生成音频的 SHA256 应保持一致。

```powershell
node build.noindex/voice-review/barge-in-validation-2026-10-22/prepare-corpus.mjs
```

已运行上述准备脚本与 `node --check`，并独立读取 manifest 核验 28 个文件的 SHA256、5 组三路样本长度一致、零削波。状态始终是 `PREPARED_NOT_EXECUTED`，没有执行 AEC/ASR/browser/device。

中文银行场景只用于音频识别，不连接银行/账户；英文叙述也不触发真实外部动作。真实任务用隔离工作区的随机只读文件验证，不使用这些语音叙述做业务执行指令。

## 6. 执行 Agent 的串行工作清单

| 阶段 | 工作 | 必须通过的出口 |
|---|---|---|
| B0 | 复查分支/dirty；校验素材与完整模型；项目外编译 AEC 并回放双路输入；补当前 generation/播放期输入的失败复现 | 库/许可证/平台证据清楚，测试能稳定重现断点；不新增生产替身 |
| B1 | 打通输入身份、输出取消、句首缓存、合法 FSM 转换、真实 SenseVoice、原会话显示；覆盖 Deciding 早播 | 固定回放一次识别/准入；旧回调不污染；任务回执与审批不因插话失效 |
| B2 | 实际 CPAL render reference、AEC/VAD、采样/设备恢复及所有播报类型接通 | TTS-only 不自触发；真实中英插话停播且首字完整；任务继续、通知不假报已播完 |
| B3 | bsk 正式页面文字/状态，原生桌宠/Tauri，资源长时、联合构建与 Owner 脚本 | 14 个 BI 用例逐一留证，设备/OS/模型/产物身份明确，未验项单列 |

原则：同一人串行打通，不要求每个字段反复审批；在 B1/B2 集成节点验收，不将缺少端到端证据的局部绿色当成完成。按当前授权与项目规则处理新依赖/资源，高风险 migration/auth 不在本次范围。不自动提交、推送或发布。

## 7. 完整验证链路与 bsk 配合

必须区分五层：

1. **V0 素材预检**：本轮完成；不等于模型正确。
2. **V1 确定性算法/worker 回放**：以真实时间送 render/mic，运行真实 AEC/VAD/SenseVoice，检查首字与时序。
3. **V2 真实输入设备**：录音从 OS 麦克风/CPAL 进入原生 daemon，助手走真实扬声器。真人或外部设备播放录音，明确设备/位置。虚拟音频线与程序注入不签核物理麦克风。
4. **V3 bsk 观察真实结果**：浏览器连接同一个宿主的真实 API/SSE，检查 Conversation 内原文、回复、播报中断、后续原文及任务状态；不模拟浏览器麦克风。
5. **V4 真实 runtime**：实际 Read 运行时随机创建的文件，结果包含新 marker；在接单/结果播报时插话，一次执行、任务不中断。

V2+V3 是用户希望看到的正式演示。外部播放的近端录音不得加入 Flowy 的 AEC render reference，否则会把测试输入一并消掉。物理扬声器/麦克风回录测可闻静音；软件队列清空不算实际停播。

当前环境核查：

- `C:/Users/38788/.local/bin/bsk.exe` 存在；本报告生成前重新执行 `BSK_AUTO_START=0 bsk status --json`，仍报告 daemon 不可用。没有启动/重启共享 daemon，没有借用用户标签或创建浏览器 session。
- 上轮探测 localhost 8787/1420/5173 不可连接；本轮未发现 Flowy/voice-companion/nomifun-web 运行进程。
- `target/debug/nomifun-web.exe` 不存在；Flowy/voice-companion 二进制存在但未核验为未来打断功能产物。
- 历史 `C:/Users/38788/AppData/Local/Flowy/AI-Mate/models` 有 asr/kws/vad 目录，未做本轮完整性/加载验证。

开始浏览器验收前，要先提供隔离宿主、同组 UI/daemon 产物及真实音频服务。不得以启动第二个宿主/daemon争抢设备的方式绕过装配问题；桌面嵌入端口是否能直接提供 SPA 必须实查。不要用会清理 5173/8787 端口进程的 `bun run dev:web` 抢占用户开发环境。

执行 bsk 前阅读 `C:/Users/38788/.agents/skills/browser-skill/SKILL.md`。在其所属持久宿主准备 daemon、确认扩展连接，再创建独立 Agent Window；所有 session 在成功/失败时都 stop。页面交互使用最新 observe refs，不改 DOM/store，不读取凭据。

```powershell
$env:BSK_AUTO_START = '0'
bsk status --json
bsk session start --no-focus --json
bsk navigate '<实际隔离宿主URL>' --session '<返回的session_id>'
bsk observe --session '<返回的session_id>'
# 原生链路完成真实音频输入后，继续观察页面与宿主同身份事件。
bsk screenshot --session '<返回的session_id>' --out '<证据目录>/voice-result.png'
bsk session stop '<返回的session_id>'
```

以上是模板，未作为已运行记录。

## 8. 既有测试复用与命令

`standard_audio_replay.rs` 可复用真实 KWS/VAD/ASR 和 20ms 送音频节拍，但用 StubVoiceRouter、NullTts、测试 sink；不是 AEC/物理设备证据。`standard_pcm` 夹具缺失时会联网 Edge 合成，离线回归应先冻结夹具、缺失明确失败。

`voice_task_runtime.rs::installed_acoustics_reach_a_real_read_and_return_the_result` 已有真实声学模型、宿主/Conversation/runtime Read 接缝，但对话/合成受控且无实体设备。扩展现有测试，不重造另一套执行引擎。

```powershell
$env:VOICE_REPLAY_MODEL_DIR = '<已校验完整模型目录>'
cargo test -p voice-companion --features voice-skeleton --test standard_audio_replay standard_wake_and_command_reach_the_production_worker -- --exact --ignored --nocapture
cargo test -p nomifun-app installed_acoustics_reach_a_real_read_and_return_the_result -- --ignored --nocapture
cargo test -p nomifun-audio --features local-stt
cargo test -p nomifun-voice-terminal --features local-tts,voice-skeleton
cargo test -p voice-companion --features voice-skeleton
cargo test -p nomifun-app voice_
cargo test -p nomifun-app --test voice_dialogue_integration
bun test ./ui/src/renderer/pages/voiceSession
bun run typecheck
bun run check:i18n
bun run check:theme
```

本轮未跑这些业务测试。新增双路回放/设备执行命令必须由实现提交提供，当前没有现成 `--barge-in-replay` CLI，不要向 Owner 提供虚构命令。

集成后执行 workspace 编译、聚合门禁及宿主/daemon/UI 联合构建；确保生产 feature 图没有 `voice-skeleton`。历史失败单列，不删除旧测试或降低断言。目标设备至少含耳机、扬声器与蓝牙/外接设备；OS 逐项留证，单机结果不签核三端。

## 9. 最终交付必须包含

- B0–B3 与 BI-01–14 的状态、命令、日志和证据路径；缺项写未验，不写“全面通过”。
- 同身份 session/turn/Conversation/command/runtime_turn 的音频时序、原文、实际任务结果、页面截图。
- 人声起点→检测→实际可闻静音的 P50/P95、首字完整性、TTS-only 负例、晚到事件竞态；初始 P95 ≤400ms 是目标而非当前成绩。
- 30 分钟/至少 50 次插话的线程、内存、CPU、队列曲线；卡顿/设备失效有透明降级。
- 库/模型/音色/设备/OS、安装和产物 build identity；代码/自动化、真实模型、实体设备和 Owner 验收分别陈述。

本报告生成前工作区只有主 Plan 的验证章节处于未提交修改；报告本身为新增文件。调查期间没有由本 Agent 执行提交/推送，没有覆盖其他执行者的业务改动。

## 10. 系统自动验收修订记录（2026-10-22 实测基线与红灯证据）

按照 [系统链路与代码自动验收方案](local-barge-in-system-auto-validation-2026-10-22.zh.md) 及 `diagnosing-bugs` Phase 1 要求，先固化代码驱动的自动验收反馈环：

### 10.1 历史基线运行（Baseline Green）

```powershell
cargo test -p voice-companion --features voice-skeleton --test voice_streaming
```

**运行结果**：`7 passed; 0 failed; 1 ignored; finished in 1.96s`。
证实既有局部流式、TTS 试听与静音保护机制基线完好，未受破坏。

### 10.2 播放期插话红灯断言（Barge-in Red-Capable Loop）

测试用例位于 `apps/voice-companion/tests/support/barge_in_acceptance.rs`，让生产 worker 进入播报态后按 20ms 节拍送入 600ms 麦克风 PCM，断言旧音频必须停止且 session 不变：

```powershell
cargo test -p voice-companion --features voice-skeleton --test voice_streaming barge_in_acceptance::microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session -- --exact --include-ignored --nocapture
```

**运行结果**：**FAILED (如期红灯)**。
- **失败定位**：`apps\voice-companion\tests\support\barge_in_acceptance.rs:44:5`
- **断言日志**：
  ```text
  thread 'barge_in_acceptance::microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session' panicked at apps\voice-companion\tests\support\barge_in_acceptance.rs:44:5:
  barge-in missing: the worker consumed 600ms of microphone PCM while the old answer remains queued; phase=Speaking, frames_before=163, frames_after=223, model_calls=1, pending=0
  test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 7 filtered out; finished in 2.48s
  ```
- **核心事实**：
  1. `frames_received` 由 163 增至 223，证明生产 capture worker 确实消费了新的 PCM；
  2. `session_id` 保持一致且 `ended_reason.is_none()`，证明会话没有被非法结束；
  3. 但 `flow.sink.frames` 依然非空，旧播报队列未被中断或 flush，暴露出当前半双工机制的硬缺口。

**结论**：已完成代码级闭环门禁绑定，不再将“等待人工实测”作为验收先决条件。后续 B1 实施只需通过最小可行改动（ponytail 模式）使该测试断言转绿。

## 11. 执行 Plan 优化与对抗审查收敛（2026-10-22 方案定稿）

针对对抗审查（laoyeye 评审）提出的 5 项系统性攻击点，已全面更新并固化至 [主 Plan](local-barge-in-only-plan-2026-10-22.zh.md)：

1. **游荡音频并发竞态（Zombie Audio Race）**：确立全局原子代次令牌（`turn_generation`）机制，打断发生时原子递增代次，不仅 flush 输出队列，更直接中止正在消费的 LLM Token 流和异步 TTS 切片合成队列，晚到分块进入 Speaker 必须比对代次，杜绝打断后冒出迟到旧音频。
2. **AEC 自激打断防护（Anti-Self-Interruption Guard）**：设立播报起始 100ms 保护窗口与近端双讲能量比门限，避免扬声器突发冲击导致系统自己把自己打断。
3. **AEC 构建容灾矩阵（AEC Fallback Matrix）**：建立 Plan A（WebRTC AEC3）/ Plan B（SpeexDSP 或纯 Rust Ducking 闪避降噪）/ Plan C（耳机直通模式）三级容灾，不被 Windows C++ 构建依赖卡死。
4. **高危工具审批语境保护（Tool Confirmation Context Preservation）**：高危操作审批播报被打断时，仅停止播报声音，状态机保持 `ListeningConfirm`，`command_id` 与 `approval_id` 完整保留，识别到“确认”后精准放行，无关闲聊保持挂起。
5. **全链路音频回放自动化矩阵（Audio Playback Test Automation）**：以 28 个已校验固定 WAV 语料和 5 组双路对齐场景为驱动，建立 A0–A4 五层纯程序断言门禁，彻底取代人工对麦克风说话或 Web 页面手工操作。

## 附录：可直接转交执行 Agent 的说明

请阅读本报告、`local-barge-in-only-plan-2026-10-22.zh.md` 与 `local-barge-in-system-auto-validation-2026-10-22.zh.md`。目标只做本地 TTS 插话打断与现有 SenseVoice 同会话继续输入。以代码驱动的音频回放与 PCM 注入作为唯一验收手段，按 B0→B1→B2→B3 串行实施：B1 优先让 `barge_in_acceptance.rs` 转绿，杜绝游荡音频；B2 接入真实 AEC 参考与高危审批保护；B3 完成系统音频回放探针与全量门禁。每步留下代码断言日志，不自动提交、推送或发布。
