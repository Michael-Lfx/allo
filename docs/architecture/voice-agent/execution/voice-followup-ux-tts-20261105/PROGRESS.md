# 第二轮语音 UX/TTS · PROGRESS

- 任务书：2026-11-05；分支 `fix/voice-missing-service-component`；交接 HEAD `efe34e6e7`，业务基线 `a2519cb99`；交接时 ahead 3、干净；当前为本轮未提交 diff，未 push。
- 任务 0：已核 HEAD/规则/旧验收文档；相关 UI 基线 21 pass / 0 fail；未发现运行中的 Desktop/daemon（安装版 FlowyAIPC 5.2.2.731 / NomiFun 0.7.5，运行版本待现场核）；本地 TTS 模型位于 `C:\Users\38788\AppData\Local\Flowy\models\tts`。
- 顺序：A 会话页左侧等待（UI sub-agent 并行）→ B 唤醒导航 → C 1.1x 默认与六段 WAV → D 证据/TC12。
- 风险：A 的真实页面 56 秒慢请求需保留等待但不缩短推理；六段固定 WAV 的中文 Baker 模型 README 标注 non-commercial，分发许可待核；真实语音/桌面验收仍缺设备。
- 规则：编辑期最小验证，代码冻结后串行全量门禁；不提交/push，除非用户明确。

## 当前改动

- B：`voiceWakeNavigation` 新增 `nextVoiceWakeNavigation`，Router 仅在有效 binding 时消费激活；新增 helper 测试和实际 Router 订阅集成测试（unbound→bound→仅一次，仍为 29 pass 内）。
- C：产品默认速度 1.1x（API、VoiceUserPreferences、SherpaOfflineTtsConfig、VoiceFeedbackSettings 回退），补显式 1.3/1.0 保留测试；WAV 尚未替换。
- A：修复计时冻结。等待行不再作为 synthetic `thinking` 混入 `MessageList` 的 turn/process 管线；`NomiChat` 在 `MessageList` 下方独立渲染 `VoiceKeyboardWaitRow`，用 `MessageThinking` standalone 自有 1s timer，`startedAt` 固定；transcript 只保留右侧用户预览。`nomiSendDispatch` 仍在 await 前同步回调；真实 task 轮询、Completed 先于消息刷新、回复一次接管、失败/切会话/108s 清理保持不变。测试：wait view 8 + dispatch 8 + B 7 + feedback 4 + NomiChat 页面集成 1 + Router 集成 1 = 29 pass / 0 fail；页面测试断言等待行不在 process_group/turn_process_disclosure 内、56s 显示 56、57s 显示 57、刷新后真实回复只出现一次。仍缺 stop/reset、浅深主题视觉的页面级断言（Enter/IME 已覆盖）。
- C：默认速度全链路 1.1x（`VoiceFeedbackConfig`、`VoiceUserPreferences` 缺省、`SherpaOfflineTtsConfig` 两处、设置页回退），显式 1.3/1.0 保留测试；API 713 pass、TTS cache 7 pass。六段 WAV 已用本机 local provider 1.1 生成到 `build.noindex/generated-vocal/`，指标通过；因中文模型 non-commercial/发行许可未核，未替换资产。另生成 1.0/1.3 对照：除 `dismiss_zh` 13ms 边界差异外，时长随速度单调缩短，见 `TTS-GENERATION.md`。

## 验证记录（编辑期/冻结后）

| 命令 | 结果 |
| --- | --- |
| `bun test` A/B/C 六个定向 UI 文件（含页面/Router 集成） | **29 pass / 0 fail / 145 expect** |
| `bun run check:i18n` | exit 0 |
| `bun run check:theme` | exit 0 |
| `bun run check:dead-css` | exit 0 |
| `cargo test -p nomifun-api-types --lib` | **713 passed / 0 failed** |
| `cargo test -p nomifun-voice-terminal --features local-tts --lib tts_cache_tests` | **7 passed / 0 failed** |
| `cargo check --workspace` | exit 0（1 个既有 unused imports warning） |
| `bun run typecheck` / `bun run check` | exit 2；**71 个既有基线错误，改动文件 0 error**；复审后复跑仍 71 |
| `bun run build:ui` | exit 0（独立等待行最终重跑 2m2s） |
| `bun run test:voice-joint-trace` | 首次 180s 超时（编译争用）；空载重跑 **6 pass / 0 fail**，joint trace SUCCESS |
| `cargo build -p voice-companion --bin voice-companion` | exit 0 |
| `cargo build -p Flowy` | exit 0（最终等待行布局重跑 exit 0；dev 构建提示不嵌入静态 WebUI，真机需 Vite dev） |
| `cargo check -p nomifun-voice-terminal --example generate_vocal_assets --features local-tts` | exit 0 |
| `rustfmt --edition 2024 --check` 改动 Rust 文件 | exit 0 |
| `git diff --check` | exit 0 |

## 仍未完成

- A 已挂载实际 NomiChat composer+list，独立等待行不在 process 分组内，覆盖延迟回执/56s 虚拟时钟/阶段/完成先于消息/真实回复只出现一次；**仍缺 stop/reset、浅深主题视觉的页面级断言（Enter/IME 已由实际 composer 覆盖）**。
- C 六段 WAV 生成通过指标，但未替换资产；等待模型/输出分发许可确认。
- 真实 Desktop/daemon、麦克风、扬声器、TC12 两轮天气、物理播放全部待现场。

## 复审修复（只读 reviewer）

- Required 1：轮询改为 command 级绝对 deadline（108s），不再因 phase 变化重置 attempts；失败分支独立 effect 5s 清理。
- Required 2：投递失败按 adapter code 分类；`task_delivery_pending/unknown/creation_unknown/status_unavailable` 与网络错误继续查同 command，其余 failed。
- Recommended：persisted 同文右侧行去重；Router 唤醒订阅改为稳定订阅 + pathname ref；C 默认值加交叉注释；`generate_vocal_assets` 显式 `cargo check --example` 通过。
- 已补：NomiChat 实际 composer+list 页面集成（延迟回执/56s 虚拟时钟/完成先于消息/持久化回复只出现一次）+ 独立 live 等待行；仍未补 stop/reset、主题视觉（Enter/IME 已由实际 composer 覆盖）。

## 复审放行（只读 reviewer）

- Required 1/2、Recommended 3/6 全部关闭；无新增 Required，可进入下一阶段。
- 仍未处理（已声明、不阻塞）：C7 旧架构文档仍保留历史 1.3x 表述（属历史记录，不回改）；A 的 stop/reset、浅深主题视觉页面断言仍未补（Enter/IME 已覆盖）。

## 计时冻结修复（按 first-principles 方案 A）

- 根因：等待行是 synthetic `thinking`，无 `message_id/turn_id`，被 `MessageList` 归入 process_content；键盘任务未设置 turnActivity/isProcessing，tail 被视为 closed，`ProcessTraceItem` 把 completed/forceDone 传给 `MessageThinking`，其 interval 不启动或只算一次。
- 修复：新增 `VoiceKeyboardWaitRow`，在 `MessageList` 外、聊天列内独立渲染；transcript 只保留右侧用户预览；`MessageThinking` standalone 以固定 `startedAt` 自有 1s timer，不再依赖 process/turn disclosure。
- 回归：页面测试断言等待行不在 `process_group`/`turn_process_disclosure`，56s 显示 56、57s 显示 57，真实历史刷新后回复只出现一次；29 pass / 0 fail / 145 expect。
