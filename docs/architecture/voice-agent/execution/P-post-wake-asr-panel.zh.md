# P：唤醒后的收音、ASR 与面板闭环（2026-10-14）

## 1. 需求与结论

> 唤醒很顺利，但是唤醒后的说话没有识别到，一直会错过唤醒时间导致休眠，检查唤醒后为什么识别不到文字？请排查后修复，我也看不到kws之后的ASR文字呈现，把面板交互和数据呈现 也补齐

当前实例的阶段日志显示多次成功进入 Listening，六秒后以 `speech_detected=false` 返回 Idle，没有进入 ASR。输入帧持续增加，因此不能把这些记录解释为 ASR 或 LLM 已经请求但没有返回。实例仍使用固定 RMS 0.01 的 EnergyVad；下载套件的 `vad/silero_vad.onnx` 未接入这条生产路径。证据见 [P-instance-stage-evidence.log](P-instance-stage-evidence.log)。心跳是低频采样，不能据此断言每个真人语音帧的音量。

已用同一段标准中文音频复现具体生产缺陷：缩放至 RMS 0.002671 后，直接调用真实 SenseVoice 能识别天气问题，经过生产 worker 则没有 ASR 文字并退回 Idle，采集零丢帧。[失败证据](P-post-wake-replay-red.log)。这证明固定能量门槛会错误拦截可识别的讲话；用户实际设备和口音仍需复测。

## 2. 最小实现与边界

- 生产 worker 复用现有 sherpa-onnx 依赖加载套件 Silero VAD，以语音概率判定，保持 800 ms 停顿、六秒内开始讲话和单句 30 秒上限。没有新增依赖、模型文件或平台系统声音。测试中没有模型资产的合成 worker 才使用原能量检测替身。
- 模型缺失或加载失败明确撤销声学就绪，不偷偷退回生产 EnergyVad。已检测到讲话时清除启动期限，后续发言不会因六秒期限而被截断。
- 快照增加倾听截止时间、VAD 就绪、本轮采集样本数与峰值；增加 `SpeechStarted` 事件。空 ASR 结果不会发布虚假的识别文字。只识别出唤醒词时明确说明没有后续问题。
- 超时分别报告“没有 PCM”和“收到音频但没有确认讲话”；设备采集中断也有具体错误。保留这些退出原因到下一次唤醒，日志记录样本数、峰值和阶段，不增加录音保存或用户内容日志。
- 设置面板和语音窗口共享原文、理解、回复与完整模型输出。等待阶段不留空白，明确说明停顿后才开始本地 ASR；本期仍是离线整句识别，不宣称流式转写。
- 语音窗口常显收音倒计时、音量、引擎状态及采集时长；Idle 可“再次开始说话”，沿用宿主唤醒入口。流水增加真实识别原文、需求理解、回复和错误原因，按快照变化去重。
- 新唤醒不清除最近三轮的文字阅读记录；新的作用域、控制请求和当轮原文仍按原规则重置。窗口关闭或停止语音仍不代表取消后台任务。

## 3. 验证

| 证据 | 结果与证明范围 |
| --- | --- |
| [P-post-wake-replay-red.log](P-post-wake-replay-red.log) | 同一真实 ASR 可识别的低音量音频被旧 worker 拦截，失败可复现 |
| [P-panel-red.log](P-panel-red.log) | 中英文面板在倾听状态没有原文标题或等待说明，2 项失败 |
| [P-post-wake-green.log](P-post-wake-green.log) | 同一低音量音频通过新 worker，真实 ASR 输出完整中文问题 |
| [P-ready-audio-regression.log](P-ready-audio-regression.log) | 8 项串行真实模型回放通过；低音量、4.5 秒后开口并继续超过六秒、连续/相邻唤醒加问题、静默退出、直接 ASR、KWS 四样本及两组误唤醒对照。命令一次路由、采集零丢帧 |
| [P-worker-tests.log](P-worker-tests.log) | 库 5 项、生产 worker 9 项通过；无 PCM 退出原因、停止作用域、迟到订阅、模型文字/TTS 失败、无真实任务时不假接单 |
| [P-ui-final-tests.log](P-ui-final-tests.log) | 21 项通过；双语空态、阶段说明、倒计时转为收音、电平/采集时长、完整文字保留及现有模型/接口契约 |
| [P-cargo-check.log](P-cargo-check.log) | `cargo check --locked -p ai-mate -p nomifun-app` 通过，包含生产路径 |
| [P-quality-check.log](P-quality-check.log) | 聚合检查在 60 个既有非语音类型错误处失败，无本次语音文件错误；不能宣称全仓门禁通过 |
| [i18n](P-check-i18n.log)、[theme](P-check-theme.log)、[按钮](P-check-button-layout-contract.log)、[CSS scope](P-check-css-scope.log) | 定向前端质量门禁通过 |
| [最终 UI 构建](P-final-build-ui.log)、[生产 feature 图](P-production-features.log)、[宿主/daemon 构建](P-production-build.log) | 以各日志结束状态签核构建；生产 feature 图要求没有 voice-skeleton |

回放使用既有标准 WAV、真实 KWS/Silero/SenseVoice；仅模型回答与音频输出端使用测试替身，没有麦克风、物理扬声器或真实云模型请求。原始中间结果保留：最初默认 daemon 被运行实例锁住 [P-post-wake-red.log](P-post-wake-red.log)，改用独立诊断输出；回放先等待引擎就绪，再沿固定 20 ms 时间轴投递，避免模型初始化和 Windows 每帧 sleep 累积误差污染六秒门禁。早期并发/计时失败见 P-audio-regression、P-audio-final-regression、P-audio-paced-regression 日志，未降低原文、单次路由、零丢帧或时间断言。

## 4. Owner 实机验收

1. 完全退出 Flowy 与托盘，更新默认开发产物后重启。确认 ASR、KWS 和讲话检测引擎均已加载。
2. 说 Hey Flowy，确认窗口显示六秒倒计时；用普通或偏轻的音量说完整问题。检测到讲话后应转为“正在收音，停顿后识别文字”，不继续倒数关窗。
3. 停顿后核对面板和窗口中的 ASR 原文、需求理解与回答。若没有文字，应明确区分未收到 PCM、未检测讲话、ASR 空结果和只识别唤醒词。
4. 四到五秒后才开口，继续说到六秒之后，核对完整句子仍被识别；静默六秒应退出并保留原因，“再次开始说话”应能重新进入倾听。
5. 核对最近三轮文字不因下一次唤醒消失，流水不因音量快照重复刷屏。检查浅色、深色、停止、追问和实际声音。

当前代码和合成回放不能替代上述设备验收，也不能签核 Mac/Linux、任务确认、重启恢复、安装包、性能 P95 或 72 小时挂机。

## 5. 最终生产产物状态

生产 feature 门禁发现语音库原默认仍含 `voice-skeleton`，[原图证据](P-production-features-red.log) 保留。现已将默认 feature 设为空，只有需要 StubVoiceRouter/NullTts 的 VT-0 集成测试要求显式开启该 feature；VT-2/VT-3 默认测试覆盖保留。默认库测试和显式替身测试均执行，而非降低或省略断言：[默认测试](P-terminal-default-tests.log)、[显式完整测试](P-terminal-all-tests.log)。完整语音库共 31 项通过，既有在线 Edge 探针保持显式 ignored，本轮未额外调用网络播报。

最终 SPA 构建、生产 feature 图检查和 Flowy/ai-mate 构建均退出 0，独立产物位于 `target/voice-staging/debug/Flowy.exe` 与 `target/voice-staging/debug/ai-mate.exe`。构建没有开启 voice-skeleton。

最终进程检查仍发现 Flowy 与 ai-mate 正在运行，因此**默认 `target/debug` 产物尚未替换，当前应用仍是旧版本**。退出请求已交给 Owner；退出包括托盘后再同步替换宿主与 daemon，核对两份二进制哈希。没有停止用户进程、自动启动应用或进行实机录音/播音。

## 6. Owner 退出后的默认产物更新（2026-10-14）

Owner 随后确认已完全退出。再次检查无 Flowy/ai-mate 进程后，已将独立生产构建的 `Flowy.exe`、`ai-mate.exe` 和对应 PDB 同步到默认 `target/debug` 目录，四份文件 SHA256 均与源产物一致。[更新回执](P-default-artifact-update.json) 保存时间、源/目标路径、大小和哈希。上述第 5 节的未替换记录保留为历史状态；当前默认开发产物已更新，可由 Owner 按原启动方式复测。未启动应用或进行实机录音/播音，实机门禁仍待签核。
