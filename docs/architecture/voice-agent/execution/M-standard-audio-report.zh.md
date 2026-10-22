# 标准音频回放与唤醒修复（2026-10-14）

## 实际发现

安装目录 `D:\tmp\flowy-dev\models\kws\keywords.txt` 是下载模型的上游示例词表，包含 Hello World、Hey Siri 等，没有 Hey Flowy。旧加载代码只检查是否含 BPE 分隔符，导致格式正确的示例词表被当成产品词表。原测试只用假 ASR 和声学波形，未捕获真实神经 KWS 的这个接线错误。

现在通过既有 Sherpa `keywords_buf` 传入固定产品词表，不写下载模型、不破坏 checksums。分数 1.5 和阈值 0.15 保持原值；本次修的是监听目标，不通过盲目降低阈值制造命中。

## 输入与测试边界

标准输入由现有 Edge-TTS 在线合成：Jenny/Guy 英文声音说 Hey Flowy，Xiaoxiao 中文声音说“今天天气怎么样？”及混合整句。原始 WAV 为 24 kHz 单声道；通过生产采集管线转换为 16 kHz 单声道 WAV。文件位于 [audio-fixtures](audio-fixtures/)，内容是测试句，没有用户录音。缓存存在时可离线重复回放。

运行真实安装的 KWS、SenseVoice ASR 和生产 worker；只有对话回答与输出 sink 用测试替身。未打开麦克风、未调用真实任务 runtime、未实际播音，不能把合成语音成绩称为真人唤醒准确率。

| 场景 | 修复前 | 修复后 |
| --- | --- | --- |
| Jenny/Guy × 原音量/0.25 倍音量（约 -12 dB）Hey Flowy，真实神经 KWS | 0/4 | 4/4 |
| 中文问题直接进入真实 SenseVoice | 今天天气怎么样？ | 保持完整识别 |
| 中文问题、Hello World / Hey Friend 非唤醒对照，神经 KWS | 未采样 | 均无命中 |
| 唤醒词加相邻中文问题，经生产 worker | 通过级联路径 | 完整问题、一次路由、零丢帧 |
| 一整句 Hey Flowy，今天天气怎么样？经生产 worker | 本次修词表后才采样 | 完整问题、一次路由、零丢帧 |

这组标准样本没有复现 ASR 首字丢失或模型加载失败，因此没有据此改 ASR 权重或音量阈值。真人口音、麦克风距离、设备增益与噪声仍需 Owner 复测。

## 证据与复现

- [原始失败](M-standard-audio-red.log)：真实 KWS 正样本 0/4，真实 ASR 可识别中文。
- [KWS 修复回归](M-kws-green.log)：同样样本 4/4。
- [完整回放](M-standard-audio-green.log)：显式运行 5 项真实音频测试，全部通过；负样本、完整问题、零丢帧与一次调用均有断言。
- [原会话回归](M-session-regression.log)：9 项通过。
- [生产 daemon 构建](M-default-daemon-build.log)：默认开发目录成功构建；后续 N 新增 DTO 时须同时更新宿主与 daemon，最终结果见 N-default-build.log。

PowerShell：

```powershell
$env:VOICE_REPLAY_MODEL_DIR = 'D:\tmp\flowy-dev\models'
cargo test --locked --target-dir target/voice-diagnostics --config 'build.build-dir="build.voice-diagnostics.noindex"' -p ai-mate --features voice-skeleton --test standard_audio_replay -- --ignored --nocapture --test-threads=1
```

测试默认 ignored，因为需要真实模型与首次联网生成音频；这份证据是上面的显式运行结果，不把 ignored 当作通过。

试听入口：[完整连说样本](audio-fixtures/continuous-command-16k.wav)、[分段唤醒与问题](audio-fixtures/wake-and-command-16k.wav)、[单独中文问题](audio-fixtures/command-16k.wav)。程序回放绕过麦克风，播放到实际房间后再录音属于另一项 Owner 真机证据。

## 更新后验收

完全退出旧应用后，重新启动已更新的宿主与 daemon。先说 Hey Flowy，等待 Listening，再说问题；随后尝试连说。核对面板里的 ASR 原文、完整模型输出和回复。标准音频通过不代表 Windows 安装包、Mac/Linux、嘈杂环境或真实任务/工具审批已经验收。
