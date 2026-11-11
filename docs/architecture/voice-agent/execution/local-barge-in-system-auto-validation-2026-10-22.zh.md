> **2026-10-23 当前修复事实**：[独立审查修复与回归记录](local-barge-in-review-repair-2026-10-23.zh.md)。旧流输出阻塞、审批停止/新唤醒清理、播报回执和中断投影已修复；生产 AEC 尚未接通，默认半双工，B2/B3 不能签核。四个隔离失败场景已纳入正式回归，下方历次方案与交付声明保留为历史。

# 本地语音插话 · 系统链路与代码自动验收方案

> **测试核心手段**：**代码驱动的全自动音频回放/注入测试（Audio Playback Test Automation）**。彻底移除“等待 Owner 实测”与“Web 页面人工点击”，由 Agent 执行确定性音频回放断言。
> **关联主 Plan**：[本地 TTS 插话打断与继续输入 · 优化执行 Plan](local-barge-in-only-plan-2026-10-22.zh.md)。
> **状态**：📋 执行方案固化；已建立 Phase 1 自动化红灯断言反馈环（`barge_in_acceptance.rs` 2.48s 红灯复现）。
> **核心原则**：由 Agent 用代码运行、回放音频、判断断言、留下证据；主链路是 Tauri/原生 daemon/系统音频与生产 Worker。

## 1. 当前边界与代码事实

- 输入框 `useSpeechInput.ts` 使用 `getUserMedia + MediaRecorder`，录音后调用既有转写，属于浏览器/WebView 听写。可用性还依赖媒体 API、安全上下文、权限和后端识别，不能只看按钮就声明完整成功。
- 连续语音 `VoiceConversationBar.tsx` 在非活跃状态要求 `acoustic_ready=true` 且 `connection=connected`，也可能因请求 pending 或绑定状态禁用。按钮没有单独的“非 Tauri 一律禁用”判断。
- 共享 router 的确装配了 voice manager，不能仅凭 Web 模式断言代码里永远没有声学管理；但当前 Web 环境未提供可验的常驻声学管道，不能承担本次连续语音功能验收。
- 本次直接验证 production worker、原生采集/播放接点和 Tauri 生产 feature 图，不通过启用 Web 按钮、浏览器授权或改页面状态解决问题。

## 2. 验收目标与不变量

```text
代码控制固定音频/音频设备
  → 本地 AEC/VAD 与原生 worker
  → 停旧播报、保留新句首、SenseVoice 最终识别
  → 同一宿主与 Conversation/runtime
  → 自动断言文字/状态/回执，按需捕获 Tauri 窗口证据
```

主门禁是可重复运行的程序断言，不是人眼点击确认。需要分层，但不把任何一层的绿色冒充全部完成：

| 层 | 自动化方式 | 断言 | 证据边界 |
|---|---|---|---|
| A0 控制与竞态 | 可控音频 sink、延迟写入、SSE 本地夹具，驱动真实 worker/FSM | 旧输出停、session 不变、旧回调失效，提交/审批保持原身份 | 受控 STT/TTS 不是声学精度证明 |
| A1 真实本地声学 | 固定真人音频、双路 render/mic、实际 AEC/VAD/SenseVoice，实时节拍回放 | 回声不自触发、首字完整、最终文字正确、音频连续 | 不把程序注入当物理声卡 |
| A2 宿主/runtime 集成 | 临时数据库/工作区、生产 router/Conversation、真实 Read 随机文件 | 原命令一次执行，插话不取消任务，unknown 不重发、通知不假报播完 | 控制模型与真实 provider 结果单列 |
| A3 系统音频自动探针 | 代码枚举并打开 CPAL 设备、播放固定音频并采集/回录，绑定本次进程与设备 | 实际帧进出、播放消费/清空时点、恢复与延迟，断麦失败明确 | 记录是真实麦克风、loopback 还是虚拟输入，不能互换 |
| A4 Tauri/生产装配 | 同组宿主/daemon/UI 构建、原生启动/退出、IPC/事件与组件行为自动断言 | readiness、原会话文字、桌宠/会话状态、停止/恢复，生产无测试 feature | Web 截图不替代 Tauri；不靠人工点击作为回归必需步骤 |

若设备/系统自动化接口在当前执行环境不可访问，测试返回明确环境阻断和已完成层，不记通过；执行 Agent 自行定位可用接口和有权限的运行环境。不得默认把任务交回 Owner，也不得用 mock 填绿色。

## 3. 可立即运行的失败反馈环

新增测试位于 `apps/voice-companion/tests/support/barge_in_acceptance.rs`，复用现有 `voice_streaming.rs::StreamingWorker`，不新增生产依赖/接口。

测试先让 worker 写出第一句声音，再按 20ms 节拍送 600ms 的已知讲话检测输入；检查 worker 确实消费了新 PCM、session 保持、会话未结束，最终要求旧 sink 已 flush。此信号用于暴露当前半双工路径，**不是 AEC/实体设备测试**。

```powershell
cargo test -p voice-companion --features voice-skeleton --test voice_streaming barge_in_acceptance::microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session -- --exact --include-ignored --nocapture
```

当前这是显式前瞻验收，标记 ignored 以区别历史基线；上述命令必须实际运行并核对 `running 1 test`，退出非零就是未通过。禁止用普通 suite 的 ignored 数量掩盖它。实现完成后移除 ignored、纳入常规门禁；`--include-ignored` 命令仍可用于专门复核。

仅看旧测试绿色不够：旧 `stopping_partial_speech_flushes_audio_and_ignores_late_model_tail` 调用的是 `stop_session`，不是用户在播放期说话。

## 4. B0–B3 的自动化执行细则

| 阶段 | 自动化交付物 | 完成标准 |
|---|---|---|
| B0 | AEC 项目外构建/双讲试验、失败用例、素材 checksum | 复现播放期输入缺口，库版本/许可证/平台清楚，红灯命令可复跑 |
| B1 | A0+A1 的同句与同 session 回归，新增输入/输出栅栏、FSM、文字投影 | 前瞻打断用例变绿，增加真实真人 PCM+SenseVoice 场景；先断言再补实现 |
| B2 | A2+A3 自动探针，覆盖接单/结果/控制/审批/设备恢复 | 原生链路和任务回执有自动证据，所有失败码和过期/晚到事件有断言 |
| B3 | A4、生产 feature 图、联合构建、一键回归入口与结果 JSON | Agent 自动执行并报告 pass/fail/环境阻断；不再设置“等 Owner 说话/点按钮”出口 |

不要为了自动化创建生产调试 PCM 注入路由、硬编码模型回答或权限绕过。算法层复用测试构造器；设备层复用生产采集/播放接口。若需要可执行探针，放测试 target/显式测试 feature，确保安装包不会启用。

Tauri 界面可由系统 UI 自动化/受控 IPC/组件行为测试配合真实宿主事件验证。优先现有能力；当前 Agent 工具若不支持原生 UI，不用 bsk 绕道声称已验原生窗口。界面未更改部分可复用原测试，新增中断状态与投影必须有代码断言；视觉检查不是将正确性回归外包给用户的理由。

## 5. 系统音频探针要求

- 由代码选定设备并记录设备身份/采样率，不静默修改全局默认设备，不安装虚拟声卡，不操作无关进程。
- 测试只使用已准备音频；采集、播放、超时、退出与清理都归属于本次进程。设备不可用返回可诊断错误。
- 数字双路注入、系统 loopback、真实麦克风回录分别标记。近端测试人声不得混入 AEC render reference，否则会被错误消除。
- 固定 reference/mic 及已知近端起点可自动测检测延迟、flush 延迟、残余能量和识别首字。真实可闻停播的测量依靠实际回录/系统输出消费证据，不以代码注释或软件队列归零替代。
- 库内部 CPU 工作、迟到合成和输出回调必须受约束；代码进行 50 次以上连续打断和资源统计，不要求用户手工重复说话。
- 手机/外部音箱/真人等不能被当前代码控制的房间条件，属于额外覆盖，不作为常规开发闭环必须依赖；没有覆盖就标注边界，不能签核所有环境。

## 6. 与旧文档的关系及交接

继续沿用 [主 Plan](local-barge-in-only-plan-2026-10-22.zh.md) 的 §1–5 声学/任务设计、BI 用例和素材清单；**旧 §6 中 V3 bsk、启动 Web 宿主、浏览器截图及 Owner 设备签核的执行步骤不再执行**。旧[交接报告](local-barge-in-investigation-handoff-2026-10-22.zh.md) §7 的浏览器流程同样仅作历史。

素材仍在 `build.noindex/voice-review/barge-in-validation-2026-10-22/`，13 个规范化来源和 15 个场景文件已核 checksum；A1 运行后另记模型输出与结果，不修改 `PREPARED_NOT_EXECUTED` 来冒充测过模型。

本轮新增的是测试和文档，不宣称业务已支持自然打断。测试执行结果另记到主交接报告的“系统自动验收修订记录”；只有实际运行命令和输出才能记通过，不再把计划描述当证据。
