# 语音套件全链路就绪与恢复 rev2 · 实施记录

> 状态：本地实施完成，等待父审查；未提交/未推送。
> 基准：`feat/voice-next`，HEAD `5875d361b3801a2fa287215c821b24b0e1fa0143`。
> 方案：[VOICE-SUITE-READINESS-AND-RECOVERY-PLAN-2026-10-25.zh.md](VOICE-SUITE-READINESS-AND-RECOVERY-PLAN-2026-10-25.zh.md)。
> 证据根：`build.noindex/voice-workspace-display-review/opencode-suite-readiness-20261025/`。

## 已落地

1. **启动小栈**：`compute_file_sha256` 的 64KiB 缓冲从 async 栈数组改为堆 `Vec`。独立进程、真实 246MB 套件、固定 1MB 栈的 provision 校验通过（`s1-provision-1mb-green.txt`）；正式回归 `apps/voice-companion/tests/provision_small_stack.rs` 走真实上层 `check_and_auto_download`，故障注入复现 `0xC00000FD` 后恢复 GREEN（`parent-gate-provision-red.txt` / `parent-gate-provision-green.txt`）。
2. **Edge TTS**：默认端点改为 `wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1`，查询参数使用 `TrustedClientToken`，保留 TLS/GEC/降级。真实中英短句均合成并解码为 24kHz PCM（`s1-edge-online-bilingual.txt`）。
3. **Range/恢复**：206 严格匹配起点、终点、总长、Content-Length；错误响应丢弃 `.part` 且不推广；416 不再凭最小大小判成功，改为从零重取；200 截断重写；超长/截断失败。
4. **删除如实**：逐个资产记录结果，`freed_bytes` 只统计实际删除；锁定/不可删资产保留并返回 `success=false`；前端读取 `success/state`，不再裸 2xx 覆写。
5. **运行占用**：宿主注入 in-use 判定（活动通话、未决任务、待审批）；安装/卸载命中时返回 HTTP 409 `voice_suite_in_use`。
6. **重启预算**：daemon 早退按 2/5/15 秒、最多 3 次自动恢复；稳定 60 秒重置；禁用/退出不重启；预算耗尽显示 `daemon_restart_exhausted`。模型未就绪不消耗预算。
7. **新通话门禁**：Connected 快照缺少 acoustic/dialogue/playback 时，在创建占位前返回 409 `voice_suite_not_ready`；首页与会话入口按原因提示；普通文字发送、停止、挂断不改共享 disabled。
8. **固定音频**：自检缺失 WAV 时明确失败，不再按需调用 Edge；真实 KWS 正/负样本、真实 Silero VAD、中英 ASR 用例通过。

## 证据

- RED：`s0-models-boundaries-red.txt`、`s0-edge-red.txt`；原 `startup-tts-diagnosis-20261025/provision-stack-1mb.txt` 保留。
- GREEN：`s1-models-boundaries-green.txt`、`s1-edge-green.txt`、`s1-edge-online-bilingual.txt`、`s1-provision-1mb-green.txt`、`s2-models-inuse.txt`。
- 回归：`s4-models-all.txt`、`s4-terminal-all.txt`、`s4-app-voice-lib.txt`、`s4-dialogue-integration.txt`、`s4-voice-companion-tests.txt`。
- 真实引擎：`s3-real-kws-asr.txt`、`s3-real-vad-english-asr.txt`、`s3-fixed-audio.txt`。
- 冷启动：`daemon-cold-start-stdout.txt`、`daemon-healthz.txt`（隔离端口 18099）。\n- 父门禁：`parent-gate-provision-red.txt`（0xC00000FD）、`parent-gate-provision-green.txt`、`parent-gate-provision-synthetic-green.txt`。
- 前端：`s4-i18n.txt`、`s4-typecheck.txt`（61 条既有、语音 0 新增）、`s5-build-ui.txt`、`s5-frontend-checks.txt`。

## 未签核

- 本地中英离线 TTS 资源未锁定，当前只有 Edge 在线合成；纯离线完整通话不可用。
- 未执行真实付费 LLM 自检；未做物理听觉、AEC、真实托盘/快捷键/桌宠联调。
- 未新增逐能力证据 DTO/TTL 与显式自检入口；新通话门禁仍以 daemon 新鲜 snapshot 为宿主权威。
- 未做干净目录“删除→下载→安装→启用→全链”一次真实网络演练。
- 既有 61 条 TS 错误与 1 条 HEAD 同存在的 `NomiSendBoxLayout` 结构失败保留。

## 复跑

```powershell
cargo test -p nomifun-voice-models
cargo test -p nomifun-voice-terminal
cargo test -p nomifun-app --lib voice_
cargo test -p nomifun-app --test voice_dialogue_integration
cargo test -p voice-companion
cargo run -p voice-companion --example provision_stack_probe -- 1 D:/tmp/flowy-dev/models
$env:FLOWY_VOICE_PROVISION_MODEL_DIR='D:/tmp/flowy-dev/models'
cargo test -p voice-companion --test provision_small_stack
$env:VOICE_REPLAY_MODEL_DIR='D:/tmp/flowy-dev/models'
cargo test -p voice-companion --features voice-skeleton --test standard_audio_replay -- --ignored --test-threads=1
bun run gen:i18n; bun run check:i18n; bun run build:ui; bun run check:theme
```

## 第二轮补修（followup-r1，2026-10-26）

> 父复审 `PARENT-REVIEW-ROUND1` 指出的 RV1-01..05 已按序处理；证据根仍为 `build.noindex/voice-workspace-display-review/opencode-suite-readiness-20261025/`，本轮前缀 `followup-r1-`。本节优先于上方首轮状态。

1. **Range 契约**：206 即使 `total=*` 也核对实际正文长度与 `Content-Range` 长度；Content-Length/start/end/total 全校验；`checked_*` 防 u64 溢出；错误/超长丢弃 `.part`；416 从零重取。父探针两例已迁入 `unknown_total_206_body_must_match_its_content_range`，故障注入 RED→GREEN。
2. **资源租约**：`DirectoryWriteLease` 在 canonical 父目录用 `create_new` 锁文件实现跨 manager/进程写排他；安装任务持有到结束，卸载在 abort 后获取。宿主 `begin_model_write` 在通话/队列/任务/审批/reservation 占用时 409，空闲时先停止自有 daemon，写结束仅按当前启用意愿恢复；install/delete 路由已套 middleware。
3. **设置回执**：安装/删除响应只解码一次；`parseVoiceModelOperation` 保留 409 原因；操作错误与磁盘状态分离；删除 `success=false` 且仍 Ready 时通过 `VoiceModelOperationAlert` 可见。
4. **完整通话证据**：新增最小 `VoiceCapabilityReport`/self-check DTO。显式检查覆盖 fresh PCM、引擎加载、daemon 真实中英 TTS 解码与 sink 消费、可选固定无工具 LLM；证据绑定 instance/generation/models/目录/大小/dialogue/TTS，60 秒 TTL，旧 attempt/签名失效。daemon 新增 TTS 自检路由，宿主新增 `/self-check` 与 `/capabilities`（120 秒预算）。
5. **门禁与降级**：enabled 状态下新通话必须有当前 `can_start_full_call`，否则在 resolve 前返回 409（含 reason/requires_check/degraded_text_only）；设置页提供“检查并准备”、逐项原因与文字回复降级；已受理 request 先恢复。
6. **有界验证**：`followup-r1-isolated-check.ps1` 有界启动/健康/自检/清理，finally 只杀自有 PID 并检查端口；旧 304952 挂起事实保留。
7. **回归**：models 37、app voice 103、dialogue 34、voice-companion 107、terminal 43 通过；typecheck 仍 61 条既有（语音 0 新增）；build:ui、i18n/theme/motion/icons、Flowy check 通过。未签核项见 `followup-r1-BLOCKED.md`（真实付费 LLM、KWS/托盘/快捷键共用门禁、物理声学/AEC、真实干净目录下载、连续早退端到端、离线 TTS 资源）。
## 第三轮补修（followup-r2，2026-10-26）

> 父复审 RV2-01..04 的修复；本轮证据目录原有 r1/TASK/carried 与部分父 RED 被外部清空，原日志不可恢复，详见 `followup-r2-BLOCKED.md`。以下为当前代码事实，旧历史不删除。

1. **OS 锁**：`DirectoryWriteLease` 改为持有 `File::try_lock()`，进程硬退出自动释放；锁文件可保留但不等于占用；canonical 目录别名/尾分隔符统一；真实 child `process::exit(0)` 后新 manager 可恢复。
2. **TTS 证据**：daemon 校验有效采样率、100ms–30s、有限 PCM、peak/rms 能量；零/NaN/极短/空音频失败且不播放；DTO 增加 duration/rms/peak；宿主要求中英覆盖与数据一致，顶层 ok 不足以通过。
3. **报告新鲜度/并发**：signature 绑定模型文件 identity、provider/model/base_url/updated_at/params、instance/generation、capture_state、sink/asr/vad；发布与读取重验 2 秒 PCM；自检与 start/write 原子互斥；取消通过 Notify + daemon cancel 停止播放；KWS/VAD/ASR 标 `unknown/inference_not_verified`，不冒充推理验收；daemon admission 60 秒过期，projection 每 2 秒推送。
4. **入口**：新增 `POST /api/voice/companion/prepare`（显式启用/启动/等待 PCM/120 秒检查，无模型零占位）；首页第二次点击执行 prepare+check；设置页逐项原因和普通聊天听写入口；KWS/手动 wake 经 daemon admission，start_call 仍统一门禁。
5. **回归**：models 37、app voice 108、dialogue 34、voice-companion 111、terminal 43；typecheck 61 既有（语音 0 新增）；build:ui、Flowy、i18n/theme/motion/icons 通过；真实隔离 daemon 自检 exit 0。
6. **边界**：文字回复是普通聊天听写入口而非独立 text-only 通话协议；真实付费 LLM、物理声学/AEC、托盘/快捷键/桌宠、离线 TTS 资源、连续早退端到端未签核。
## 第四轮补修（followup-r3，2026-10-26）

> 父复审 RV3-01..04；证据根 `build.voice-review-evidence.noindex/opencode-suite-readiness-20261025/`，前缀 `followup-r3-`。原 r2/父 RED 部分丢失，不伪造。

1. **播放完成**：`play_wav_for_self_check` 在写入后执行 `sink.drain()`，成功必须真实消费；drain 错误/超时/取消失败关闭并 flush 丢弃；无 runtime 的立即 sink 探针走直接 drain，不 panic。新增真实计数、drain 错误/挂起、取消单测与无 runtime 单次 poll 探针；旧 flush 语义故障注入为 RED。
2. **当前实例推理**：daemon `self_check_engines` 使用固定 16k PCM 真实跑 KWS 正负、Silero VAD、中英 SenseVoice ASR；缺合法固定样本返回 `unsupported/fixed_audio_missing`，不用 loaded/云文字代证。宿主拉取 `/api/voice/engine-self-check`，KWS/VAD/ASR 通过才 Ready；完整通话需 capture+VAD+ASR+LLM+TTS+sink，wake 需 KWS+VAD+ASR+capture。
3. **准入 scope**：daemon 分 wake/call admission，KWS 使用 wake，start_call/manual 使用 call；host 60 秒过期推送，projection 每 2 秒复核；KWS 未验证不显示 wake 就绪。
4. **旧读归属**：`current_capabilities` 克隆记录 attempt/generation/signature，条件清除只作用于同一归属，返回前重读；确定性 barrier 首次读失败但新报告保留。
5. **文字回复**：设置页导航首页后点击现有 `data-layout-part=microphone` 听写按钮，进入普通语音输入/文字回复，不建通话、不调用 TTS；去掉字面量换行；helper 行为测试。
6. **回归**：terminal 46、models 37、app voice 111、dialogue 34、voice-companion 112；typecheck 61 既有（语音 0 新增）；build:ui、Flowy、i18n/theme/motion/icons、真实隔离 daemon 自检通过。
7. **边界**：固定音频生产分发许可、真实付费 LLM、物理声学/AEC/原生窗口、离线 TTS、连续早退端到端未签核。
## 第四轮补修（followup-r4，2026-10-26）

> 对应 `PARENT-REVIEW-ROUND4` 的 RV4-01..03。证据根为 `build.voice-review-evidence.noindex/opencode-suite-readiness-20261025/`，前缀 `followup-r4-`；父审查文件与旧 RED 保持原样，未用新日志冒充原基线。

1. **安装版自检资源**：新增 `apps/voice-companion/src/self_check_audio.rs`。解析顺序为显式 override、可执行文件旁 `audio-fixtures`、用户数据目录缓存；不再默认读取仓库文档路径。缓存带 schema/model revision/生成时间/SHA256/source/online 清单，缺失、损坏、过期或模型代次变化均拒绝。无合法固定资产时，使用现有 TTS provider 生成三条公开测试句并原子写入用户缓存；Edge 在线路径标注 `edge_online_generated(_cached)`，离线 provider 标注 `offline_tts_generated`；生成物不随安装包分发。缺失/损坏 override 继续失败关闭。
2. **实际 worker 引擎**：删除 HTTP 内临时新建 KWS/VAD/ASR 的诊断路径。daemon 通过 worker 命令通道把固定 PCM 送入采集 worker 自己的 `kws`、`vad`、`transcriber`；worker 在 ASR 尚未加载时于同实例懒加载，不再用临时重载结果授予能力。响应绑定 request_id、instance_id、worker_epoch、model_revision、fixture source/online。自检期间 worker 丢弃实时采集并重置 KWS/VAD，阻止 wake、TTS 自检与模型写入并发；取消/超时在阶段间和 ASR 边界检查，超时后释放 active，迟到结果丢弃。
3. **有归属听写入口**：`VoiceSettings` 不再 `navigate` 后固定 400ms 全文档查找。新增 route-owned `dictationIntent`，仅首页挂载的现有 `SpeechInputButton` 消费一次；离开路由撤销，disabled/unsupported 给出明确提示，重复激活不停止已有录音。转写仍进入现有输入框/文字回复路径，不触发 TTS。
4. **验证**：实际 worker 失败但临时 ASR 重载成功的 `#[ignore]` 定向反例通过；worker 取消/截止时间、TTS/engine 互斥、host 迟到 request/instance/revision 拒绝均有确定性测试。两个仓库外 cwd 的新二进制自检均返回 `kws/vad/asr=true`、`fixtures_source=edge_online_generated(_cached)`、`worker_epoch=1`；缺失/损坏 override 返回 false。回归：terminal 46、models 39、app voice 112、dialogue 34、voice-companion 119（14 ignored）。typecheck 61 条既有（语音 0 新增）、build:ui/Flowy/i18n/theme/icons 通过；dead-css 仍为既有非语音文件失败，button-layout 需要运行中的 dev server，本轮未作为通过证据。
5. **依赖边界**：`jsdom` 仅加入 `ui` devDependencies，用于真实 mount 的听写入口测试；不进入生产 bundle。未新增生产依赖、AEC、离线 TTS 资产、鉴权/计价/迁移/Agent loop 变更。
6. **剩余边界**：真实付费 LLM、物理声学/AEC、原生窗口/托盘/桌宠、连续崩溃 60 秒、离线 TTS 资产许可仍未签核；固定资产的生产分发仍采用“运行时生成、不分发”策略。
## 第五轮定向补修（followup-r5，2026-10-26）

1. **导航归属与原子消费**：设置页通过 `navigate('/', { state: { dictationIntentId } })` 把意图绑定到目标历史项；Router 内常驻 `DictationIntentRouteGuard` 在路径/导航状态不匹配时撤销 pending。`SpeechInputButton` 在调用 `startRecording` 前执行 `claim()`，只有 store 的原子 `consumeDictationIntent` 成功才启动；过期/已撤销返回 `stale` 且不打开麦克风。真实 `MemoryRouter + guard + 挂载控件` 测试覆盖未挂载前离开、普通返回、匹配状态延迟挂载、TTL 过期、重复激活、disabled 和正常手动麦克风。
2. **自检取消/所有权**：deadline 从请求开始覆盖资源生成、模型懒加载和 ASR；取消同时设置 AtomicBool 与 Notify。TTS 生成与 ASR 等待均为 cancel/deadline select；ASR 在独立任务中持有 `NativeBusyGuard`，取消/超时后 HTTP 有界返回失败，但 `engine_check_native_busy` 保持，旧任务退出前拒绝新诊断、旧结果不授予能力。底层 blocking 不可被安全终止时不做 unsafe kill，保持失败关闭；完成后恢复。
3. **验证**：companion 123 passed / 14 ignored，app voice 112 / 3 ignored，terminal 46，models 39，dialogue 34；实际 worker 取消/超时/恢复、合成阶段 deadline/notify、native busy 二次拒绝、实际 worker 失败而重载成功反例、1MB provision、两仓库外 cwd daemon 自检均通过。typecheck 61 条既有（语音 0 新增）；build:ui、i18n/theme/icons、workspace/Flowy、rustfmt/diff check 通过。
4. **边界**：button-layout 未运行（需 dev server）；dead-css 既有非语音失败；底层 native 任务若不返回会一直占用隔离门禁；物理/AEC/真实 LLM/离线 TTS 许可仍未签核。
## 第九轮所有权/恢复（followup-r9，2026-10-27）

1. **R9-01**：`VoiceEventHub::update_task_versioned` 返回接受结果；runtime 仅在接受且 command/session/turn 与当前 snapshot 一致、playback generation 重新校验后推进 `DispatchingTask`。`play_response` 与 Chat/TaskControl/AgentTask 的 generation 不符分支不再修改 driver。父 owner 探针 GREEN（B driver 不变）。
2. **R9-02**：deadline/cancel 覆盖资源生成、ASR lazy load 与推理；native 任务由 `NativeBusyGuard` 保持所有权，取消/超时只失败关闭，旧结果不授予能力；supervised daemon 在 native busy 超 grace 后 exit75，由既有 manager restart budget 有限重启并最终 `daemon_restart_exhausted`。
3. **R9-03/04**：入口读取 capabilities；loading 禁用，setup 先 prepare，失败跳设置，活动通话不隐藏停止/挂断；离页 abandon start request，回包按 request_id 查询并 stop。设置页 dictation intent 绑定导航 state + Router guard + 原子 claim，TTL 过期不启动麦克风。
4. **验证**：companion 125/14、app voice 114/3、voice_tasks 31、UI 40、terminal 46、models 39、dialogue 34；typecheck 61 既有（语音 0 新增）、build:ui/workspace/Flowy/i18n/theme/icons/rustfmt/diff 通过。未完成项见 `followup-r9-BLOCKED.md`。
## 第十轮准备/取消/native 定时器（followup-r10，2026-10-27）

1. **R10-01**：Guid 在 active 判断后、任何 capability/prepare/start/retry 异步判断前取得单飞锁；setup/TTL 双击共享一次 prepare。生产 callback 探针由 prepare=2 变 prepare=1。
2. **R10-02**：未知查询失败保留原 request_id 与 abandoned 状态；再次 start 返回 `voice_call_cancel_unknown`，不生成新 UUID；新增原身份重试，只有匹配 request 的 stop 成功/确定拒绝才清理。
3. **R10-03**：`NativeBusyGuard` 绑定 `native_job_epoch`，任务开始/结束递增；supervised watchdog 只有在同一 job epoch 仍 busy 时 exit75。父 timer helper 从 A 旧 timer 误杀 B 变 GREEN；host 继续用既有 manager restart budget 做有限恢复。
4. **R10-04**：Guid 读取 `models_ready/autostart`，缺失/损坏/校验中/停用只跳设置，不 prepare/start；已安装启用但 TTL 过期一次 click prepare；活动通话控制保留。
5. **验证**：companion 125/14、app voice 114/3、voice_tasks 31、UI 42、terminal 46、models 39、dialogue 34；typecheck 61 既有（语音 0 新增）、build:ui/workspace/Flowy/i18n/theme/icons/rustfmt/diff 通过。真实 mounted 两轮、永久 native 监督和资源状态矩阵仍列 BLOCKED。
## 第十二轮无设备 native 证据（followup-r12，2026-10-27）

1. R11 未证明无设备：main→assemble 仍尝试 MicSource/PhysicalAudioSink。R12 在非默认 `native-fault-injection` feature 下增加 headless 装配（内存 capture/Channel sink、固定 PCM、真实 worker/HTTP/manager），feature 无 `NOMIFUN_VOICE_FAULT_HEADLESS=1` 时装配前拒绝；硬件构造计数 HTTP 端点断言为 0。
2. `FaultBlockingStt` 使用真实 `spawn_blocking` 120s；集成断言 `fault_asr_entered>0`、HTTP 200 `reason=engine_check_timeout`/`asr_ok!=true`、daemon exit75、host 重启 4 轮后 `daemon_restart_exhausted`；成功/失败/超时使用临时 data dir、ephemeral port、finally stop。
3. 默认 daemon 重建后不含故障字符串，SHA `5a3d5fc12f3a885fe925efb3b444c90795e5163095eb10364237f3e66529c4eb`；feature daemon SHA `3ac66696843d14ac5304f5e06d99c5c79b3ab1b9a00b8ee3f627d8d290275fba`。
4. 事故：默认二进制误跑已披露在 `followup-r12-BLOCKED.md`；正式无设备结论仅基于 feature headless。
## 第十三轮无设备 fail-closed（followup-r13，2026-10-27）

1. `startup_mode` 在任何 assemble 前决定 Default/HeadlessTest/拒绝；默认构建出现 FAULT/HEADLESS 标记即拒绝，feature 必须 headless。8 组合单测不使用设备。
2. feature headless 产物复制到 E 独立路径并核验文件名/SHA 后才允许 launch；默认二进制本轮未启动。默认/feature SHA 分列，默认产物不含 fault 字符串。
3. 集成每轮断言 hardware=0、fault ASR entered、HTTP timeout 体、真实 PID/instance/worker、子进程 exit75、host 重启和预算 `daemon_restart_exhausted`；catch_unwind+timeout 后必执行 stop/端口验证；forced panic/timeout cleanup 单测通过。
4. R12 默认二进制误跑事故保留并披露，不把其算入无设备证据。
## 第十四轮 env 标记与因果闭环（followup-r14，2026-10-27）

1. `parse_startup_markers` 区分变量存在与值：默认构建只要 HEADLESS/BLOCK 变量存在（空/0/false/未知）即在硬件装配前拒绝；feature 必须 headless 严格为 `1`。无设备单测覆盖。
2. feature headless 独立复制到 E 并核验绝对路径/文件名/SHA；默认 daemon 未启动。`fault_asr_entered` 在实际 `spawn_blocking` 闭包记录，exit 观测绑定 spawn PID。
3. 正式 ignored 集成 4 轮：HTTP timeout 体、hardware=0、entered>0、运行 PID/instance/worker、子进程 exit75、host 重启、预算 `daemon_restart_exhausted`；catch_unwind+timeout 后 stop/端口验证；forced assertion/timeout 测试实际 exit101 且 residual=0。
4. 事故/边界见 `followup-r14-BLOCKED.md`。
## 第十五轮共同保护边界（followup-r15，2026-10-27）

1. manager cfg(test) `run_headless_case` 统一覆盖启动、health、主体、超时与 cleanup：stop 有 10s 期限，端口 2s 内关闭检查，stop 后 2.5s 确认无 projection 延后重启；cleanup 失败单独报错，成功后才传播原 panic/timeout。
2. 正向监督与 health/主体断言/主体超时三类故意失败都调用同一函数；故意失败测试实际 exit101，保留原错误，`cleanup_case_ok` 打印 owned PID/port，外部核对 residual=0。
3. 负对照改为持有句柄 spawn，5s 内层期限+finally kill/reap，核验 feature headless 绝对路径/SHA；未启动默认 daemon。
4. 证据见 `followup-r15-*`；UI/AEC 与物理/云验仍排队。
## 第十六轮 native 作业归因与反向验收（followup-r16，2026-10-27）

1. feature headless 的 blocking ASR 入场绑定真实 worker 命令的 request_id/worker_epoch/instance_id，并在 blocking 闭包内记录当前 native job_epoch、PID、进入时间、still_blocking；诊断端点返回 `fault_observation`。
2. 正式监督每轮断言 request/worker/instance/PID/job_epoch/still_blocking/native_busy，再要求同一 PID exit75；缺注入和无关 code1 用同一断言实测变红，均 exit101 且 cleanup 保留原因。
3. 旧 `headless_cleanup_survives_forced_panic_and_timeout` 已由 R15 共同边界完全替代并删除；其他反例保留。
4. UI/AEC/物理/云仍排队；未交付 Git。
## 第十七轮普通 native caller 所有权（followup-r17，2026-10-27）

1. startup marker 统一纳入 EXIT_CODE，任一 HEADLESS/BLOCK_ASR/EXIT_CODE 存在即默认拒绝，feature 仍要求 HEADLESS=1。
2. `still_blocking` 由当前 real lease/job_epoch 计算，完成或换代后历史 job 不活跃。
3. 普通 `process_utterance`、待机级联 ASR、确认 ASR 经 `transcribe_with_lease` 共用 `transcribe_cancelable`/`NativeBusyGuard`/deadline/fence；普通取消/再开实测 A 租约保留、B 不假释放、exit75 换实例并 clean。
4. 初始化 KWS/VAD 的 deadline/lease 仍未闭环，失败尝试已撤回，见 BLOCKED。
## 第十八轮普通单作业 watchdog 归属（followup-r18，2026-10-27）

1. 成功 CAS 后立即分配唯一 job epoch，再由该 job 所有者安排 watchdog；busy 拒绝不安排 timer，避免 B 为 A 排 timer。
2. spawn 使用 `NativeBusyGuard::attach`，完成 drop 才递增 epoch；预约窗口、spawn 未入场和取消后仍 blocking 都由同一 job watchdog 管理。
3. 新产物单次 A 无 B exit75 并换非空新 instance/新 PID；A/B、诊断四轮、健康 B 和 cleanup 均实跑。
4. 初始化 KWS/VAD 与逐帧 native 仍排队，未宣称整体完成。
## 第十九轮普通租约收口（followup-r19，2026-10-27）

1. watchdog 增加 `VOICE_COMPANION_SUPERVISED=1` 合同；未受管理不擅自终止宿主。
2. guard drop 先 epoch++ 再 busy=false，避免旧 A 覆盖新 B；100000 次并发单元回归通过。
3. 成功预约时按 job 不可变绑定 request/worker/instance，FaultBlockingStt 入场读取对应 job；B 忙拒绝不重标 A。
4. 单 A、A/B、诊断四轮、race 已用新产物通过；完整普通矩阵和初始化仍队列。
## 第二十轮观察交接与不可变入场（followup-r20，2026-10-27）

1. guard 保存 job；drop 先 epoch 退休、再只清自己的 observation、最后开放 busy，旧 A 不再清 B 的 FAULT_NATIVE_BUSY。
2. 真正两线程 Barrier 100000 次并发回归通过；R19 顺序循环改名，不再冒充并发。
3. BoundStt 在预约时捕获 job/request/worker/instance，实际 ASR 入口直接按该 job 记录；B 忙拒绝不能重标 A。
4. 单 A、取消无 B、健康 A/B、A/B、诊断与 cleanup 实跑；受控排队、A 后新 stuck B、完整反向链仍队列。