# 完整回复与 Edge 播报闭环核对（2026-10-14）

## 已确认的原因与实现

用户截图已经显示非零麦克风音量、ASR 原文和思考阶段，不能再把这一阶段的无声解释为没有麦克风。代码中的 Edge 地址、公开客户端参数和握手校验与当前服务不符；旧播放链路忽略空音频与播放错误，提前显示追问。UI 只显示当前识别原文，缺少完整模型输出和历史回复。

现在使用 Edge 在线合成，复用现有 sha2、chrono、tungstenite、Symphonia 和 CPAL，不调用 Windows 系统语音、不添加离线 TTS 模型。模型套件的权威清单 `nomifun-voice-models/src/layout.rs` 只有 KWS、VAD、ASR 与校验文件，尚无 TTS 模型。Edge 仍依赖网络和服务协议；跨平台代码兼容不等于三端实测通过。

当前协议参考：[Readest Edge 客户端](https://github.com/readest/readest/blob/main/apps/readest-app/src/libs/edgeTTS.ts)。没有引入其包或 vendor 文件。

- 合成等待真实 turn.end，拒绝空/截断音频，保留实际采样率后送入扬声器；公开客户端握手校验与桌面私有控制凭据、模型 provider 的用户凭据分别处理。
- 生产播放直接连接物理 sink，输出初始化失败和流失败不能伪装成功；追问在输出队列消费后开始。仍须实机核对系统音频延迟与声音。
- 采集持续消费 PCM，模型/播报作为当前轮次工作执行；停止后的迟到结果不写入新会话、不清空新一轮声音。
- 用户原文、模型的 message.content 和解析后的回答保留最近三轮；窗口滚动显示全文，诊断区域可展开。播报失败保留回答并明确进入文字追问状态；模型网络、认证、响应格式和输出设备错误分别显示。
- 增加握手拒绝 HTTP 状态、解码音频字节数、模型/播放阶段耗时及成功标记，不打印用户语音原文、模型全文或私有控制凭据。

## 自动证据与边界

- [在线探针](K-edge-online.log)：固定测试句返回 24 kHz、54,720 个非零 PCM 样本；运行约 1.96 秒。只调用网络合成，不录音、不播放，也不是完整聊天首音延迟指标。
- [生产 worker 回放](J-session-final-tests.log)：普通音量到 ASR、单轮路由、无 PCM 超时、旧窗口停止冲突、模型正文到快照、TTS 失败后的文字追问。
- [音频/语音库回归](J-final-rust-tests.log)：空合成拒绝、采样率、播放等待输出、迟到旧合成不清空新输出；网络探针默认 ignored，以上在线记录是单独显式运行的结果。
- 独立目录最终 [库回归](J-diagnostics-rust-tests.log)：音频 73 项、语音 10 项通过，在线探针默认 ignored；定向 Rust 格式检查通过。完整语音库集成结果见 [J-terminal-all-tests.log](J-terminal-all-tests.log)。
- [前端行为及双语](J-ui-tests.log)、[前端构建](J-final-ui-build.log)。[全量类型检查](J-typecheck.log)仍有 60 个其他路径错误，无本次语音模块错误；i18n 与主题契约通过。未代替双主题视觉验收。
- 最终默认开发路径 `cargo build --locked -p Flowy -p ai-mate` 退出 0，见 [J-final-default-build.log](J-final-default-build.log)，宿主与 daemon 已一起重建，不使用 voice-skeleton。完整语音库 31 项（含 21 项既有集成测试）、音频库 73 项、worker 9 项、前端行为/双语 12 项通过。没有启动应用、录音或物理播音；Owner 须重新启动旧开发实例。首次受限环境构建访问失败与后续成功日志分别保留，不能将失败记录当作构建成功。

## Owner 实机步骤

1. 完全退出旧开发应用，再启动当前分支；宿主和 ai-mate 必须一起更新，单独刷新 UI 不足以更新后台逻辑。
2. 开启语音，试用唤醒，说一句普通问题。核对识别原文、完整模型输出、回答文字是否都显示，等待声音结束后继续追问。
3. 在回答较长时滚动阅读；下一轮开始后，上一轮回答应继续保留。展开诊断核对 PCM 音量仍刷新。
4. 在思考或合成时停止，再次唤醒；旧结果不得写入新窗口，也不得清空新一轮声音。
5. 若无声，记录界面的具体错误和 daemon 日志中 `edge_tts_*`、`voice_model_*`、`voice_playback_*` 行；日志位置由宿主的 `Voice companion diagnostics log` 行给出。不要提供 token 或 API key。
6. 单独验收 Windows 扬声器、4 秒追问、停止清音和双主题；Mac/Linux、安装包及任务/审批仍未签核。
