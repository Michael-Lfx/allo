# 本地插话 · 独立审查修复与回归记录

> **状态**：🔧 审查中的五项缺陷已修复；生产保持半双工，真实 AEC 与系统音频验收仍待 B2/B3。
> **代码范围**：`feat/voice-next`，基于 `e9b125536` 的工作区修改，未提交、未推送。
> **原则**：停声音、结束语音、取消 runtime 任务分别处理；生成完文字不代表播放完成；没有 AEC 不开放原始麦克风自动插话。

## 1. 本轮范围与历史证据

依据[主 Plan](local-barge-in-only-plan-2026-10-22.zh.md)及[系统自动验收方案](local-barge-in-system-auto-validation-2026-10-22.zh.md)，修复独立审查发现的输出阻塞、审批清理、回执误报、投影错误与 AEC 门禁缺失。

审查时已有流式、审批与 FSM 共 22 个测试通过，但额外四个隔离探针全部失败。隔离探针的源码及输出位于忽略目录 `build.noindex/voice-review/independent-review-2026-10-23/`；它们不是可随 Git 交接的正式回归。现已将这四个场景纳入仓库测试，不依赖该目录或预编译 exe。

不增加 ASR/LLM、AEC 库、数据库迁移、权限模式或生产测试注入端点；不启动、替换或发布桌面安装产物。

## 2. 修复后的调用链与边界

```text
经过明确允许的无回声测试 PCM
  → production worker/VAD → 原输出 interrupt/flush
  → 输出代次通知：取消旧前台模型、合成及通道等待，释放输出锁
  → 新输入代次 → 保留句首 → 既有 ASR → 同一会话后续理解

生产原生麦克风（尚无 render reference/AEC）
  → barge_in_ready=false / aec_unavailable
  → 播放期间保持半双工，不将回声作为新输入
```

`with_test_barge_in` 仅存在于测试/显式 `voice-skeleton` feature，且只对 replay capture 生效。普通 `from_capture` 也默认关闭；生产装配不提供环境变量或用户开关绕过门禁。

中断取消的是前台生成与声音。已交付的 `VoiceTaskClient` 任务不调用 cancel，不改变 command、Conversation、工作区或真实执行回执。任务控制/提交仍沿既有宿主接口处理。

## 3. 五项修复及 API 契约

| 项目 | 当前实现 | 正式回归 |
|---|---|---|
| 旧流占用输出 | `PlaybackPipeline::interrupted` 在 flush 后通知；模型 select 和 TTS 等待均可取消；`speak_text_for_generation` 固定旧输出身份，防止旧调用借用新代次 | `barge_in_releases_output_before_old_model_finishes`；terminal 的 50 次连续中断用例 |
| 旧结束回调干扰新输入 | 输入代次独立递增；旧 `start_utterance` 收尾不清理或启动新输入的 pending 队列 | 同一 session 后续请求/实际新声音回归 |
| 审批停止后重启 | 停止时清除 pending；清理在 driver 锁内校验审批与当前 session、终止状态；停止或新唤醒后旧请求只返回拒绝，不 reset/abort 新工作 | `stop_during_prompt_does_not_restart_a_previous_live_session` 包含停止与立即新唤醒两种情况 |
| 审批计时 | 第一次进入听取时冻结 `min(执行期限, now+8s)`；插话/迟到 PromptDone/澄清不重置期限；提示播放失败继续返回失败而非吞错 | 原审批正常/拒绝/静默/长提示回归，审批提示插话回归 |
| 任务播报回执 | `played` 只用于实际消费完成；增加 `interrupted` / `muted` 及对应 reason；宿主 ledger 接受并持久化，重启不自动重播，用户显式重听仍可用 | `interrupted_task_result_is_not_reported_played`、`muted_result_is_not_reported_played`、宿主持久化回归 |
| 文本与音频状态 | 生成已完成但仍在播放时，插话同样标记 `reply_state=interrupted`；保留原文字；正常追问不会把已播完回答改成中断 | `completed_text_still_marks_interrupted_audio` |
| AEC 门禁 | 只有明确的无回声测试装配可接通自动插话；生产默认半双工；`has_pending_audio` 检查实际 sink 队列，不只看 FSM/TTS 网络状态 | `raw_pcm_cannot_enable_barge_in_without_echo_cancellation` |

兼容性：原 JSON 结构追加默认字段 `audio.barge_in_ready` 和 `audio.barge_in_reason`；旧快照缺字段仍可读取。通知状态仍使用既有字符串字段，新增 `interrupted` 与 `muted`，旧状态不重写。宿主、daemon、UI 同步支持新状态；没有迁移持久化 schema。

UI 在声学诊断中显示 AEC 未就绪和半双工原因，任务诊断区分别显示播报中断/静音；中英词条与类型生成同步。测试状态明确标为测试，不显示成生产 AEC 已就绪。

## 4. 关键决策

| 决策 | 采用 | 边界 |
|---|---|---|
| 取消机制 | 复用 Tokio watch 和现有 generation，不新增调度器 | 等待模型、TTS、输出锁或句子通道时都能退出；不按 active_turn 全量取消后台任务 |
| AEC 尚未就绪 | 明确半双工，而非裸 VAD 默认上线 | 这是修复不安全开放的门禁，不是 AEC 功能交付 |
| 通知中断 | 终态保留文字，由显式重听创建新 attempt | 不自动重播用户刚打断的结果，不重新执行任务 |
| 证据 | 正式 Cargo/Bun 测试，自动运行 | 不要求 Owner 对麦克风说话，不将受控 STT/TTS 当真实声学证明 |

## 5. 验证记录

下列记录只针对本轮命令和工作区，不沿用历史“全绿”签核。最终命令结果见本节后续补记。

已实际通过：

- `cargo test -p voice-companion --features voice-skeleton`：99 passed，11 个显式资源/在线测试 ignored。
- `cargo test -p nomifun-voice-terminal --features voice-skeleton`：48 passed，1 个在线 Edge probe ignored；新增 50 次中断测试另行运行。
- 两个 UI 测试文件 `VoiceConversation.test.tsx` / `VoiceTaskDiagnostics.test.tsx`：30 passed，覆盖双语状态及明暗主题包装。
- `bun run gen:i18n`：生成 11,708 keys，55 modules。

`bun run check` 在 TypeScript 检查处失败：报错位于 credits、wallpaper、普通会话、视频画布及 Vite 配置等未修改文件，没有报告本轮语音文件的类型错误。不能记全仓通过，也未扩大范围修改这些模块；其余门禁独立复跑。

仓库 `.rustfmt.toml` 当前设置 `disable_all_formatting=true`。格式命令即使退出成功也不代表自动重排发生；本轮另核对 diff 空白，保留历史代码排版。

## 6. 后续阶段与交接

本轮关闭五项审查缺陷，**没有完成 B2/B3**。现有自然插话正例是控制链路证明，不是生产自然插话可用性证明。

下一步仍需：本地 AEC 项目外构建与许可证验证、CPAL 实际 render reference、双路固定真人音频与真实 SenseVoice、echo-only 零打断/零派单/零自我审批、实际 runtime 随机文件任务在插话期间继续执行、系统设备探针和物理停播延迟、生产宿主/daemon/UI 联合构建与退出验证。B2 通过前保留半双工门禁，不增加凭空可用的设备/耳机绕过开关。

主 Plan 中原“转绿闭环”记录作为历史保留，只证明当时的窄测试；当前完成范围以本文件和后续实际验收输出为准。

### 6.1 最终验证补记

- 宿主 `cargo test -p nomifun-app --lib voice_tasks`：30 passed；包含中断/静音通知持久化、重启不自动重播、显式重听不重新执行。
- `cargo test -p nomifun-voice-terminal --features voice-skeleton interruption_releases_a_stalled_sentence_stream_fifty_times`：1 passed，50 次控制链连续中断；不是物理声卡延迟或资源泄漏签核。
- `bun run build:ui`：通过，Vite 6.4.3，14,399 modules；第三方 eval、静态/动态混合导入及 chunk 大小警告保留。
- 独立门禁通过：i18n、theme、motion、error/support surface contract、button layout contract、icons、CSS scope、CodeMirror runtime、process/browser platform boundary、Windows console-hide、Agent Store market self-test（17/17）。
- 独立门禁失败（未修改其规则或出错文件）：dead-css 在 codeBlock、WallpaperSection、CloudLogin、视频等现有文件及基线清单处失败；fingerprint 因文档站 SDK 页面 fp-1 / fp-17 不一致失败；release-sync 因文档站版本 beta.4 / beta.11 与功能数 48/71 / 56/83 不一致失败。这些不属于本次语音修复，不能把独立复跑的进程最终退出码当作所有门禁通过。

### 6.2 播放投影补充

同一中断标记覆盖自然讲话和用户主动停止。只要原文字已完整生成但声音仍处于合成/播放状态，保留完整文字并标记中断；新增长期回归 `completed_text_keeps_an_interrupted_audio_marker_on_speech_and_stop`。正常播完后开始下一句不倒改已完成的回答。播放失败沿用既有错误投影，完整生成文字保持 Complete；这是已有 `a_later_synthesis_failure_keeps_the_complete_answer_and_enters_text_follow_up` 锁定的契约。

### 6.3 全仓基线归因

使用 TypeScript Compiler API，以只读 CompilerHost 把本轮 8 个 UI 类型/组件/词条文件替换为 HEAD 原文，分别检查 HEAD 输入与当前输入；不切分支、不 stash、不改原文件。两边均为 **61 条相同错误，新增 0 条**。比较脚本和完整诊断 JSON 位于忽略目录 `build.noindex/voice-review/independent-review-2026-10-23/compare-typecheck.cjs` / `typecheck-comparison.json`。该结论仅证明本轮没有增加类型错误，不代表全仓类型检查通过。

`bun run help --check` 通过；脚本注册表与 README 入口对齐。`git diff --check` 通过。

### 6.4 声学与原生推理限制

本轮取消回归覆盖异步模型连接、合成等待、通道和输出锁；不将其视为原生 CPU 推理可被强制抢占的证明。既有 Sherpa TTS 存在同步原生生成调用，实际本地模型的取消时延与 CPU 占用必须在 B2 中测量并评估其回调取消接口。生产自动插话门禁继续关闭；50 次受控回归不替代这些测量。

### 6.5 默认生产编译与格式命令

`cargo check -p voice-companion -p nomifun-app -p nomifun-voice-terminal -p nomifun-api-types` 实际完成，`Finished dev profile`（首次重建约 5 分钟），无编译错误；保留 app/agent-trace 中未使用成员的既有警告。全仓 `cargo fmt --check` 随后因 Windows 命令行长度限制（os error 206）失败，改用上述四个受影响 package 的格式检查；仍需注意仓库配置关闭了自动格式化。

### 6.6 最终源码收口

最后一次全量复跑发现：将完整文字在播放错误时也改成 Interrupted，违反了原有“生成完成与播放失败分别记录”的契约。已撤回这项扩大处理，保留原断言，没有修改测试来掩盖失败。对最终源码重新运行：

```powershell
cargo test -p voice-companion --features voice-skeleton --lib voice_events
cargo test -p voice-companion --features voice-skeleton --test voice_streaming --test voice_confirmation
cargo fmt -p voice-companion -p nomifun-app -p nomifun-voice-terminal -p nomifun-api-types --check
```

结果：投影 **4/4**、流式 **14/14**、审批 **6/6** 通过，包含原先失败的后半句合成错误用例；受影响包格式命令通过（自动格式化仍由仓库配置禁用）。默认 feature 的四包编译复核也已通过。首次完整 suite 的 99 passed 记录保留为当时快照，不冒充最后一次全量命令成功。

最终工作区保留所有已有改动和本轮修复；未暂存、未提交、未推送。真实 AEC、原生设备性能和安装产物仍按第 6 节单独验收。
