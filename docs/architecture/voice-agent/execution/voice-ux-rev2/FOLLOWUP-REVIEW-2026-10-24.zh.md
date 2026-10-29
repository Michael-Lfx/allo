> **2026-10-24 修复状态**：下述 Required 项已增加清理前准备检查；快照/绑定未就绪时不调用 API、提示稍后重试。正式反例先 RED 后修复，157 项语音/重置与 100 项共享历史通过。完整证据见 [清理准备修复记录](RESET-READINESS-REPAIR-2026-10-24.zh.md)。以下复审原文保留。

# rev2 修复后的边界复审

2026-10-24；分支 `feat/voice-next`，HEAD `7de276e61e93836365c5a1c7e006c557ce3e3c6d`。范围为当前未提交的语音 UX 改动及上轮 F1–F3 修复，重点检查清理生命周期、首页启动恢复、跨会话异步隔离、历史接替与类型标记。本轮没有修改业务代码或正式测试，没有提交、推送或切换分支。

## 结论

- **Standards：** 本轮检查未发现新的模块边界、依赖或双语规范阻断；`git diff --check` 通过。该结论不等同于重新运行全部仓库门禁。
- **Spec：** RV-07 尚未完全闭环。发现一个确定性问题，包含两个独立失败输入；既有 147 项语音前端测试仍全部通过，说明现有覆盖缺少此类时序。

## [Required] 快照或绑定未就绪时，成功清理没有留下语音失效屏障

所属：Spec / RV-07、HI-06；也涉及异步正确性。

代码位置：

- `ui/src/renderer/pages/voiceSession/liveVoiceTurn.ts:17-26`：只有传入快照已绑定当前 Conversation 才冻结 window；否则成功提交只更新 Conversation coordinator，完全不记录语音失效身份。
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx:2029`、`:2061`：清理与重置入口在请求前直接捕获当前组件的 `voiceSnapshot`。
- `ui/src/renderer/pages/voiceSession/useVoiceSnapshot.ts:10`：每次组件挂载从 null 开始，订阅/初次 GET 异步完成。清理操作没有等待有效快照或绑定的条件。

触发与结果：

1. 打开一个仍有未投影语音轮次的会话，当前页面的语音订阅尚未完成，`snapshot=null`；或者快照已到达但宿主 binding 尚未返回。
2. 用户清理上下文，后端成功，调用已捕获的清理提交函数。
3. 原 session 的合法快照/绑定稍后到达。由于全局失效 window 没有写入，本地前一视图也没有观察到这些轮次，旧问题与旧回答被重新生成两行 live 消息。

该问题在同一个 renderer、同一次挂载即可复现，不依赖完整 renderer 重启；因此不属于上轮已经披露的“跨 renderer 不提供新协议”限制。此处复现的是旧文字重新显示，不据此推断发生了任务重执行或数据库串写。

最小修复方向：

- 让语音清理具有明确的、Conversation 归属的准备条件。快照/绑定尚未能确认时，应先完成归属核验并冻结待清理轮次，或暂缓清理并提供可恢复状态，不能在没有屏障时显示清理成功。
- 若采用待决失效记录，必须区分清理前旧轮次与清理后新输入；不能直接把“成功后收到的全部轮次”都删除。
- 保留失败清理不失效、不同 session 可正常显示、新一句与首个清理 epoch 同时到达不被误删的既有正例。
- 增加入口级异步回归：延迟订阅/延迟 binding → 点击清理/重置 → 成功 → 旧快照到达，并覆盖卸载重进。当前纯函数反例已证明屏障缺失；入口级用例还需随修复纳入正式套件。

## 实跑验证与证据

日志目录：`build.noindex/voice-workspace-display-review/`。

| 检查 | 本轮结果 | 证据 |
| --- | --- | --- |
| 正式语音前端 18 文件 | 147 pass / 0 fail / 1161 assertions，5.05s | `review-20261024-followup-formal.txt` |
| 新增隔离反例与对照 | 2 pass / 2 fail；null 快照清理、未绑定快照清理均重新出现两行旧文字 | `review-20261024-followup.test.ts`、`review-20261024-followup.txt` |
| 对照 | 快照已就绪时清理后重挂载仍隐藏；新独立 session 正常显示 | 同上 |
| 差异检查 | exit 0；仅既有 CRLF 提示 | `git diff --check` |
| 关键源文件哈希 | 已记录，便于下一轮比对 | `review-20261024-followup-source.json` |

反例调用生产的 `prepareVoiceHistoryReset`、coordinator、`reconcileLiveVoiceHistory`、`mergeVoiceTranscript`，没有复制一套模拟修复逻辑；每例均释放 consumer。反例位于忽略目录，未修改正式测试来制造失败或放宽断言。

复跑：在仓库 `ui/` 目录执行 `bun test ../build.noindex/voice-workspace-display-review/review-20261024-followup.test.ts`。当前应为 2 pass / 2 fail；修复后必须同时保留全部对照通过。

## 已确认边界与未重复签核范围

- 上轮首页 `starting` → 目标 Conversation 原请求恢复及折叠语音标记，本轮正式回归通过；没有重新进行原生像素验收。
- 本轮没有重跑 Rust 宿主、Tauri 构建、生产 UI 构建、完整 TypeScript 或所有质量门禁；上轮结果仍是历史证据，不能算本轮新通过。
- 生产 `apps/voice-companion/src/voice.rs:1594` 附近的 `barge_in_ready` 在非 test / 非 `voice-skeleton` 构建中明确为 false，理由为 `aec_unavailable`。这表示真实播放时语音打断尚未启用，并非仅缺一次人工验收；本轮不把它当成新引入回归。
- 原生透明桌宠首帧/DPI、真实麦克风与扬声器体验，本轮未测试。

下一步仅修复上面的快照/绑定准备缺口，补入正式入口级反例，再重跑该组和已有清理、跨会话与启动恢复回归；不需要因此引入新消息 store、权限模式或 ASR 模型。
