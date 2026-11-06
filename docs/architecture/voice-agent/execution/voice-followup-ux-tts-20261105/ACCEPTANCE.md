# 第二轮语音 UX/TTS · 验收报告

> **2026-11-06 E 轮追加**：语音 runtime 冷启动与模型继承另见 [RUNTIME-WARMUP.md](RUNTIME-WARMUP.md)。新建自动语音会话 gateway-only，日志实测首次 build 364–490ms；旧会话不迁移。A/B/C/D 验收结论保持原文，E 的边界见该文档。

- 日期：2026-11-05（UTC+8）
- 分支：`fix/voice-missing-service-component`；交接 HEAD `efe34e6e7`；业务基线 `a2519cb99`；未 push
- 本轮工作目录：`docs/architecture/voice-agent/execution/voice-followup-ux-tts-20261105/`
- 本地生成物（未提交）：`build.noindex/generated-vocal/`

## 1. 结论

| 需求 | 状态 | 结论 |
| --- | --- | --- |
| A 语音会话页打字后左侧立即等待 | 🟡 计时/reset/stop/自动刷新/英文页面回归已完成；主题视觉/真机待验 | 提交时先发布本地等待行，按 command 轮询真实任务，结果出现后接管；见 §2 |
| B 首页声学唤醒跳到本次新建会话 | 🟢 代码/状态序列测试完成 | 无 binding 的 active 快照不再消费激活；后续同 session binding 只导航一次；见 §3 |
| C TTS 默认 1.1x + 六段本地 WAV | 🟡 默认速度完成；WAV 已生成但受发行许可阻断 | 未替换仓库六段资产；见 §4、`TTS-GENERATION.md`、`BLOCKED.md` |
| D 本轮证据 + 上轮真实 TC12 | 🔴 真实设备项未完成 | 代码/单测/构建已记录；真实麦克风、扬声器、TC12 两轮天气缺证；见 §5 |

## 2. A · 左侧等待行

实现：

- `nomiSendDispatch` 在既有 command_id 建立后、`await sendVoiceKeyboardMessage` 前同步调用 `onVoiceCommand`；返回/失败再报 `onVoiceCommandResult`。
- `NomiSendBox` 将回调接到 `NomiChat`。
- `NomiChat` 以 `conversation_id + command_id` 持有本地等待状态：
  - transcript 只追加右侧用户预览；左侧等待行由独立 `VoiceKeyboardWaitRow` 在 `MessageList` 外渲染，使用 standalone `MessageThinking` 自有 1s timer（`startedAt` 固定），不再被 process/turn disclosure 关闭。
  - 用 `getVoiceTask(commandId)` 轮询真实 task 状态，映射 sending/queued/accepted/delivering/syncing/unknown/failed。
  - Completed 先于消息时刷新本会话；真实左侧文本消息出现后一次接管，不重复插入。
  - 失败 5s 有界清除；切换会话清除；**command 级绝对 108s 上限**（phase 变化不重置）；未知投递错误（`task_delivery_pending/unknown`、网络错误）继续查同 command，硬拒绝才 failed；不使用全局 current 或最后一条 assistant 猜归属。
  - 新增 `voice.keyboardWait.*` 中英 key、`aria-live` 状态文本。

测试：

- `voiceKeyboardWait.test.ts`：右侧用户预览、权威 user 行去重、真实左侧文本接管、任务阶段映射（含 cancelling）、投递错误分类、Completed 先于消息、absolute 108s 上界、foreign command 忽略：8 pass。
- `nomiSendBoxDispatch.test.ts`：等待回调严格先于 host await、回执带同 command、失败仍先报同 command：8 pass。
- 定向 6 文件（含页面/Router 集成）：31 pass / 0 fail / 158 expect。

已补：`NomiChat.wait.integration.test.tsx` 挂载实际 composer+list，经 suggestion→Enter→延迟 host 回执→56s 虚拟时钟→完成先于消息→**不手动 emit，自动刷新持久化历史**，断言独立等待行存在且**不在** `process_group`/`turn_process_disclosure` 内、56s 显示 56、57s 显示 57、真实回执阶段、同步中保留、最终回复只出现一次；第二个页面用例覆盖 reset 立即清理和 en-US/dark 上下文，IME composing Enter 已覆盖。根因修复见 PROGRESS 的“计时冻结修复”。stop 已由 clear 事件覆盖；仍缺浅深主题视觉人工验收，真机页面待验。

## 3. B · 声学唤醒导航

实现：

- `voiceWakeNavigation` 新增 `nextVoiceWakeNavigation`：只有 `shouldFollowVoiceSession` 得到有效 target 才推进 followed session；active 无 binding 保留激活。
- Router 的 GlobalVoiceWakeListener 改用该纯状态转移；已在目标页仍消费一次；订阅改为跨路由稳定，pathname 用 ref 读取，避免每次导航重置 consumer 水位。
- 保留 self-check、非 connected、idle/error、设置/模型/companion 保护。

测试：

- 无 binding active → 同 session binding → 一次 target；重复快照不导航；已在目标页只消费不重跳；旧 unbound 不清已消费 session：`voiceWakeNavigation.test.ts` 7 pass；实际 Router 订阅 `voiceWakeRouter.integration.test.tsx` 2 pass（含受保护路由/self-check）。

未验：真机首页声学唤醒/拨号实测待现场；实际 Router 订阅集成测试已补。

## 4. C · 默认速度与六段人声

默认 1.1x：

- `VoiceFeedbackConfig::default().tts_speed`、`default_tts_speed()`、`SherpaOfflineTtsConfig` 的 `Default`/`from_dir`、`VoiceFeedbackSettings` 回退统一为 1.1。
- `VoiceUserPreferences` 空 JSON 默认 1.1；显式 `{"tts_speed":1.3}`、1.0、1.1 反序列化保留。
- 测试：API 713 pass；TTS cache 7 pass（含 default=1.1）；UI 侧加后端权威注释，C 默认值多处加同步注释。

六段 WAV：

- 使用本机 `C:\Users\38788\AppData\Local\Flowy\models\tts` 的 Matcha/Vocos local provider，1.1x，生成 6 段到 `build.noindex/generated-vocal/`。
- 格式均为 16kHz/mono/16-bit/30ms 首尾静音；peak ≤ 0.498；时长 382–870ms；完整 SHA-256/模型指纹及 1.0/1.3 速度对照见 `TTS-GENERATION.md`。
- 生成期间 sherpa 打印 `Unknown token: shei2`；听感/可懂度未验。
- **未替换仓库资产**：中文模型 README 明确 non-commercial，仓库既有架构文档记录模型资产无显式发行许可；按任务“许可受阻保留原资产交证据”处理。

## 5. D · 证据与真实 TC12

已完成：

- 见 `PROGRESS.md` 验证表：定向 UI 31 pass、API/TTS、cargo check workspace、UI build、daemon/Desktop build、joint trace 6 pass、i18n/theme/dead-css、rustfmt、diff check。
- 安装版本：FlowyAIPC 5.2.2.731 / NomiFun 0.7.5；核验时无运行中的 Desktop/daemon，实际运行版本与生效模型待现场。

未完成：

- 修复版 Desktop/daemon 真机场景、真实麦克风/扬声器、TC12 两轮天气、物理播放/首句识别。
- 上轮真实 TC12 本来就缺实测，本轮不能补造。

## 6. 复跑命令

```powershell
bun test ui/src/renderer/pages/conversation/platforms/nomi/nomiSendBoxDispatch.test.ts `
  ui/src/renderer/pages/conversation/platforms/nomi/voiceKeyboardWait.test.ts `
  ui/src/renderer/pages/conversation/platforms/nomi/NomiChat.wait.integration.test.tsx `
  ui/src/renderer/components/layout/voiceWakeNavigation.test.ts `
  ui/src/renderer/pages/settings/VoiceFeedbackSettings.test.tsx
cargo test -p nomifun-api-types --lib
cargo test -p nomifun-voice-terminal --features local-tts --lib tts_cache_tests
cargo check --workspace
bun run build:ui
```

## 7. E · 语音 runtime 冷启动与模型继承（2026-11-06）

| 项 | 状态 | 证据/边界 |
| --- | --- | --- |
| 新建自动语音会话 gateway-only | 🟢 | `01a1aefb` 490ms / `01a1af01` 364ms，`mcp_count=1`；对比旧 5 MCP 27,549ms |
| 继承用户默认模型 | 🟢 | `01a1af89` / `01a1af8c` 新语音会话 model = `AIPC-deepseek-v4.1-flash`；偏好日志 `voice_default_model_preference_selected` |
| P1 显式 fallback | 🟢 | 已实现 fallback 日志 + `model_fallback` 响应；用户确认 fallback 后仍有模型、UI toast 非必需；preset 不可用直接失败 |
| 旧语音会话 | ⚪ | 按用户范围不迁移；复用旧 retained/workspace 会话仍可能走旧 MCP 配置 |
| 真机后验 | 🟢 | 新语音第 2 次 312ms、MCP=1、模型继承；普通会话 0ms；旧会话未变；app 集成测试仍受 `sherpa_onnx_sys` 阻断 |
| E6 显式/preset 优先 | 🟢 | `explicit_voice_model_overrides_saved_default_preference` 1/1；`preset_model_reconciliation_*` 8/8；全量 662/662 |

详细实现、日志和边界见 [RUNTIME-WARMUP.md](RUNTIME-WARMUP.md)。
