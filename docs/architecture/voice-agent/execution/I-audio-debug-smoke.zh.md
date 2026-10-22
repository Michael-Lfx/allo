# 语音输入闭环实机核对

2026-10-14，Owner 执行。以下是步骤，不是通过证据。

## 更新前提

退出旧开发实例及其受管 daemon，释放 Cargo/可执行文件锁后，使用当前分支的宿主和 ai-mate 重新构建，再启动开发应用。不能仅刷新页面，也不能把带 voice-skeleton feature 的测试二进制当作生产 daemon。

## 核对步骤

1. 启用语音，观察 ASR/KWS 各自显示已加载；后台音频帧应显示送达。若引擎未加载，查看宿主打印的 Voice companion diagnostics log 路径，定位 voice_asr_load_failed / voice_asr_model_not_found 或 KWS 初始化错误。
2. 保持安静，查看音量与 VAD；开始普通音量讲话，音量应变化、dBFS 上升，出现已检测到讲话。音量来自 daemon 消费的 PCM，不是浏览器新开麦克风。
3. 点击试用唤醒，六秒内开始说“现在几点”，再停顿约一秒。观察 Listening → Transcribing → Deciding，出现识别原文。未配置对话模型时明确显示模型未接通，不能算真实模型回答通过。
4. 若无音量/无新帧，检查 voice_microphone_stream_started 的实际设备、通道与采样率，以及 voice_capture_no_pcm / closed_or_failed；界面检测设备成功不等于后台音频已经送达。
5. 若音量变化但始终未检测讲话，对照 voice_vad_configured 和 voice_capture_heartbeat 的 RMS/峰值。当前伴侣门槛 RMS 0.01，仅为首轮校准，需要当前设备和背景噪声样本验证。
6. 若出现 Transcribing 后无文字，查看 voice_asr_started / voice_asr_finished 的样本数、耗时和字数，确认是否有 asr_empty_result。无需分享录音原文、token 或 API key。
7. 拔掉设备后，界面应撤销声学就绪、音量归零；设备自动恢复不在本轮签核。无发言六秒回到 Idle；连续发言最多三十秒提交。
8. 在明暗主题各检查一次：窗口底部无白色空区，标题、状态、音量和停止按钮可读。确认关闭/停止及新唤醒的竞态仍按后续切片回归。

记录版本、实际设备、背景噪声、声学状态、阶段日志和结果。音量/回放通过不代替真实 ASR 准确率、模型回答、物理播放或任务验收。
