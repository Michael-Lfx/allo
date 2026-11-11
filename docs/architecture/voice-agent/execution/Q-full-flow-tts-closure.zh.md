# Q：TTS 故障与完整流程测试闭环（2026-10-14）

## 1. 原始需求与根因

> 出问题，合成语音失败，tts没有播报；为什么你没有验证过？把全流程mock和测试闭环再给我实测

当前实例日志记录了成功的 ASR（143 ms）和模型回复（4534 ms），随后语音工作任务在 Rustls 初始化处 panic。完整桌面依赖图同时启用了 `ring` 与 `aws-lc-rs`，默认 TLS 构建器无法自动选择 CryptoProvider。工作任务退出后没有发送失败/结束事件，因此页面停在原来的“正在合成并播报”。这是合成连接初始化故障，不是本轮 ASR 没有识别内容。

之前的在线合成探针使用独立语音包的依赖图，未覆盖宿主与 daemon 联合构建的 feature 合并；输出端的替身验证也不能证明物理扬声器播放。这是之前验证范围的缺口。本轮在 `-p ai-mate -p Flowy` 的完整依赖图中重现原始 panic，保留 [生产 TLS 失败测试](Q-production-tls-red.log) 和 [完整流程行为失败测试](Q-full-flow-behavior-red.log)。

## 2. 实现与关键决定

- Edge 连接复用项目频道 WebSocket 的 TLS 装配方式：显式选择已有默认 provider，未设置时使用已有 ring，并加载平台根证书、继续验证服务器证书。没有全局修改 provider、跳过证书验证或改用 Windows 系统声音。相关依据为 [Rustls CryptoProvider 文档](https://docs.rs/rustls/0.23.45/rustls/crypto/struct.CryptoProvider.html) 和 `nomifun-channel/src/plugins/qqbot/gateway.rs` 的既有实现。
- 语音 crate 引用仓库已锁定的 `rustls`、`rustls-native-certs`；测试引用已有 `async-trait`。没有新第三方包、版本或数据库迁移。下载声学套件仍包含 KWS、VAD、ASR，不包含本地 TTS 模型；本轮播报继续使用跨平台 Edge 在线服务。
- 区分 `Synthesizing` 与 `Speaking`：等待网络合成时显示“正在合成语音”，有效音频写入输出端之后才显示“正在播放语音”；消费完成后才开启四秒追问。
- 轮次任务发生 panic 时恢复 Idle，保留已识别文字、需求理解和模型回复，发送明确错误；旧轮次故障不重置新会话。空音频、网络错误和中途停止继续走真实失败/取消语义。
- 窗口改为可调整大小的 440×520，最低 360×340；电平和收音状态常显，引擎详情折叠，正文独立滚动，避免回复被诊断信息挤出阅读区。保持双语与语义主题令牌。
- 实际 KWS 就绪值来自引擎初始化状态，测试回放不再把“启用了回放”当成 KWS 已加载的证明。

## 3. 测试层次及证据

| 检查 | 实际覆盖与结果 |
| --- | --- |
| [完整 worker 回归](Q-final-worker-tests.log) | 20 项通过：库 5 项、完整流程 6 项、原会话 9 项。完整流程覆盖正常音频、空音频、HTTP 503、panic 恢复后再次唤醒、合成中停止及迟到音频抑制；TLS 初始化在桌面完整依赖图中不再 panic |
| [语音库和宿主测试](Q-terminal-tests.log) | 语音库 31 项、桌面宿主 101 项通过。在线探针默认 ignored，必须单独显式运行 |
| [真实 Edge 合成](Q-real-edge-greeting.log) | 固定句“你好呀，很高兴听到你的声音。”真实在线合成、解码成功；24 kHz、71424 个非零 PCM 样本，约 2.976 秒音频，合成探针耗时 2461 ms |
| [真实声学与在线 TTS 完整回放](Q-real-entire-flow-final.log) | 实际 KWS/Silero/SenseVoice → 测试模型 HTTP → 实际 Edge/解码 → 可控制的输出消费确认。模型请求 1 次、采集零丢帧；顺序为 Idle → Listening → Transcribing → Deciding → Synthesizing → Speaking → FollowUp → Idle |
| [前端行为测试](Q-ui-tests.log) | 23 项、90 个断言通过；中英文合成/播放阶段、错误文案、正文保留、折叠诊断及现有接口/模型选择回归 |
| [生产编译检查](Q-cargo-check.log) | `cargo check --locked -p ai-mate -p Flowy` 退出 0 |
| [最终 UI 构建](Q-final-build-ui.log) | `bun run build:ui` 退出 0 |
| [生产 feature 图](Q-production-features.log) | 宿主与 daemon 联合生产图不含 `voice-skeleton` |
| [聚合质量检查](Q-quality-check.log) | 在 60 个既有非语音 TypeScript 错误处失败；没有语音模块错误，不能宣称全仓检查通过 |
| [i18n](Q-check-i18n.log)、[theme](Q-check-theme.log)、[按钮](Q-check-button-layout-contract.log)、[CSS scope](Q-check-css-scope.log) | 本轮定向门禁通过；定向 Rust 格式和 `git diff --check` 退出 0 |

离线完整测试使用测试范围的 ASR、模型和输出替身，经过生产 worker、真实 HostDialogueClient 请求契约、Edge 传输/音频解析代码和播放管线。在线完整回放换成实际声学引擎与 Edge，仅模型回复及输出消费由测试控制。**两者均没有开启麦克风、物理扬声器或实际 Flowy 云模型请求；不能把可控制的 drain 确认写成物理播放验收。** 本轮在线固定句无用户录音。

测试环境装配和早期编译失败保留在 `Q-full-flow-red.log`、`Q-real-entire-flow.log`；它们不是产品运行失败证据。行为失败与最终成功分别按上表日志判断，没有降低单次调用、阶段顺序、文字保留和零丢帧断言。

音频产物：

- [固定回复在线合成](Q-edge-greeting.wav)。
- [标准完整输入：唤醒词及中文问题](Q-full-flow-input.wav)。
- [完整 worker 实际 Edge 输出](Q-full-flow-output.wav)。

## 4. 可重复执行命令

保持宿主与 daemon 同时参与 feature 解析，避免独立包检查再次遗漏桌面 TLS 组合。

```powershell
cargo test --locked -p ai-mate -p Flowy --features ai-mate/voice-skeleton --target-dir target/voice-review --lib --test full_voice_flow --test voice_session
cargo test --locked -p ai-mate -p Flowy --target-dir target/voice-review --test full_voice_flow greeting_synthesis_with_desktop_tls_graph -- --ignored --nocapture
$env:VOICE_REPLAY_MODEL_DIR = 'D:\tmp\flowy-dev\models'
cargo test --locked -p ai-mate -p Flowy --features ai-mate/voice-skeleton --target-dir target/voice-review --test full_voice_flow real_acoustics_and_edge_complete_the_entire_worker_flow -- --ignored --nocapture
cargo test --locked -p nomifun-voice-terminal -p Flowy --features nomifun-voice-terminal/voice-skeleton --target-dir target/voice-review
cargo check --locked -p ai-mate -p Flowy
bun run build:ui
cargo build --locked -p Flowy -p ai-mate --target-dir target/voice-staging
```

`voice-skeleton` 只用于显式测试命令；生产构建不启用。上述 ignored 在线测试需要模型资产/网络，不默认消耗网络调用。Windows 工具链路径按本机环境配置。

## 5. Owner 实机签核

Owner 已确认完全退出 Flowy 和托盘，本轮不自动启动应用。程序更新状态以第 6 节为准；使用原来的启动方式重新启动。

1. 唤醒后说“你好啊”，确认原文、理解和回复均可读；网络合成时显示合成中，音频送到输出端后显示播放中。
2. 必须实际听到回复；说完声音后四秒内继续讲话，确认开始说话后允许完成整句。播放中麦克风电平暂时静默不能单独作为输入设备故障结论。
3. 合成或播放中按停止，确认没有残留声音和迟到结果；再次唤醒应可重新完成对话。
4. 故障时保留文字、显示具体错误；不能永久停在合成/播放状态。核对深色与浅色模式，以及窗口缩放后的正文滚动。

若仍无声，提供流水中的最终错误、`D:\tmp\flowy-dev\logs\voice` 最新日志中 `voice_playback_failed` / `voice_playback_finished` / `edge_tts_*` / `voice_speaker_*` / `voice_turn_panicked` 行；无需提供任何 token 或 API key。

实机声音、完整真实云模型语音链、Mac/Linux、安装包、任务控制与确认、恢复、性能 P95 和 72 小时挂机仍未签核。此前产品计划 V0–V8 的门禁状态不因本轮 mock 或在线音频回放而自动变为完成。

## 6. 生产产物更新

生产构建与默认产物更新完成后在此追加结果；当前构建证据为 [Q-production-build.log](Q-production-build.log)。不将独立目录构建成功等同于默认运行文件已经更新。

最终宿主与 daemon 联合生产构建退出 0（3m56s）。Owner 确认退出后，再检查无 Flowy/ai-mate 运行实例，将 `target/voice-staging/debug` 中的 `Flowy.exe`、`ai-mate.exe` 及两份 PDB 同步到默认 `target/debug`；四份文件 SHA256 均与源构建一致，见 [默认产物更新回执](Q-default-artifact-update.json)。默认产物已更新，未启动应用、录音或物理播音。请按第 5 节进行实机复测，结果仍待 Owner 签核。

## 7. Owner基础链路反馈与新增规划（2026-10-14）

Owner随后反馈“起码kws到asr到llm到tts没问题了”，并确认runtime尚未接通。记为基础链路定性试用通过，不升级为全部AC、物理时延/准确率、安装或三平台签核；第5节的剩余门禁保留历史。新增需求为离屏完整声音反馈，用户选择轻量状态反馈，详见 [v0.2 PRD](../PRD-voice-companion-v0.2.zh.md) 与 [R计划](R-auditory-feedback-plan.zh.md)。新声音行为尚未实施。
