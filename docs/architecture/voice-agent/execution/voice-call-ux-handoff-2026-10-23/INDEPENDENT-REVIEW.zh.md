# 通话交互收口独立复审（2026-10-23）

> **后续修复记录**：用户授权后已实施下述四项 Required 的代码修复并补正式反向回归，详见 [REPAIR-RESULTS](REPAIR-RESULTS.zh.md)。本文保留审查时发现与红色探针，作为历史证据；整体方案的未实现/实机项仍单列。

结论：**报告中的主要测试与构建结果可以复现，但不能认定恢复边界全部闭环；四项 Required 仍需修复。** 其中包含上一轮实现遗留路径，本次审查覆盖完整待提交改动，不只审查接续者新增的两处断言。

## 范围与事实

- 分支 `feat/voice-next`，HEAD `93e0ecac8ca817c47b9cf37ad354456977f90c65`，比本地 `origin/feat/voice-next` 多一个提交；没有 fetch/push/切分支。
- 范围为 `git diff HEAD` 的未暂存业务/文档变化，以及相关未跟踪的新组件、交付 helper 和测试；暂存区为空。工作区不是 Git clean，报告末尾“保留未暂存改动”才准确。
- 规范：AGENTS.md、CONTRIBUTING.md；需求：[主方案 §0/§8/§9](../voice-call-entry-pet-presence-plan-2026-10-23.zh.md)、[原交接](HANDOFF.zh.md)。本轮只读业务代码，新增独立审查报告和忽略目录测试探针，没有提交或修复生产代码。
- Standards：模式字段类型拒绝、无新依赖/迁移/agent loop 改动可以确认；下述恢复与操作身份问题阻断通过。
- Spec：UX1 的迟到绑定/未知启动恢复、UX3/CLOSE-04 的精确任务取消未完成；UX2 的队列呈现也有反向场景缺口。UX4 音色冻结仍是已声明的未实现项，不应与 TTS 诊断混为一谈。

## Required 发现

### R1：过期启动 reservation 会重新采用当前项目，迟到唤醒可能错绑工作区

- 审查线：Spec + Standards（作用域正确性）。
- 定位：`crates/backend/nomifun-app/src/voice_companion_manager.rs:589–596`，`freeze_session`。
- 触发：用户从 P 开启，wake 已发出但 SSE/冻结绑定延迟超过 10 秒，期间主窗口选择 Q。
- 当前行为：过期分支忽略已冻结的 `res.conversation_id`，重新读取 `active_window_candidate` 并调用有创建副作用的 `resolve_target_conversation`。同一唤醒因此可能绑定/创建 Q 的语音 Conversation。daemon 的 wake handler 只消费 keyword，没有另一份 conversation_id 绑定可以兜底。
- 影响：后续语音目标在用户未重新授权选择的情况下变为 Q。与方案 §9.5(2)(3) 的目标冻结、迟到 ready 身份约束冲突。
- 证据级别：生产调用链静态确认；未声称在真人设备上复现。新增容量测试仅检查 128 条和最旧记录删除，并不验证本场景；现有 11 秒用例老化的是 handled cache，不是实际 reservation 的迟到绑定。
- 最小修复：迟到绑定只能兑现仍有效且匹配的原 reservation，或报告未知/拒绝；不得重新采用当前候选。补测试：P reservation 老化、当前候选 Q，再 freeze；断言零 Q 创建/绑定，并覆盖已有更新 reservation。

### R2：启动响应丢失后，本地 pending 无法随真实挂断解除，阻挡再次通话

- 审查线：Spec + Standards（恢复）。
- 定位：`ui/src/common/adapter/voiceCompanion.ts:326–340`；关联 `:264` 及 `subscribeVoiceSession` 的 pending 清理。
- 触发：开启 P 的 POST 已送达但响应丢失；用户之后成功停止真实 P 通话，再从正式语音页开启。
- 当前行为：网络失败保留 pending，但 `conversationId` 只有 `.then` 成功响应才赋值。快照和成功 stop 都只按这个未赋值字段清理；正式页使用 voice_conversation target，与旧 workspace target 不同，327 行在发请求之前直接抛 `voice_call_starting`。
- 影响：用户已成功挂断仍无法从正常入口重开；改变模式/入口也会被旧 pending 阻挡。保持未知幂等身份是正确的，但缺少按真实回执解除的路径。
- 实跑：忽略目录 `lost-start.test.ts` 直接调用生产 adapter，只有 HTTP 边界注入；第一次模拟响应丢失，第二次 stop 成功，第三次期望 ready，实际 `voice_call_starting`。0 pass / 1 fail。最初 tsconfig 参数噪声已通过从 ui/ 无 override 重跑排除，结果一致。
- 最小修复：通过稳定 request_id 查询/绑定真实启动结果，按相同请求/会话回执协调 pending 的解除与取消；不能简单对任意超时清 pending 然后换 ID 重发。将该探针补为正式回归。

### R3：语音页主停止按钮仍走普通 Conversation 停止，未冻结到 command

- 审查线：Spec（§9.5(5)、CLOSE-04）+ Standards（操作身份）。
- 定位：`ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx:1957–1965`；`SendBox` 继续传入此 handleStop。
- 当前行为：无 isVoiceConversation 分流，调用 `stopConversationAndConfirmRelease(conversation_id)`；底层 payload 只有 conversation_id。旁边 VoiceTaskStatus 的取消则走 `cancelVoiceTask(instanceId, task)`，有绑定 command/message 身份及 durable cancelling，两条按钮语义不同。
- 影响：未写入语音 task 的 cancel intent，无法保证“取消中”跟随真实 task 终态；A 结束/B 出队与旧点击交错时，普通 conversation.stop 可能停止当时的新 turn，而不是用户所见的 A。现有 FIFO 取消回归不能证明这个主按钮路径正确。
- 证据级别：静态追踪生产主按钮→helper→IPC；没有把错取消 B 描述为已实跑的设备事故。
- 最小修复：语音页主停止使用所显示 task 的冻结引用，走同一个 voice cancel 服务；未知状态核对、取消中等待终态，保留其他队列项。普通聊天保留原 stop helper。增加 A→B 交错、旧点击和队列仍保留的按钮级回归。

### R4：Q 正在执行时，P 的排队项会被状态条提前隐藏

- 审查线：Spec（队列可见与可取消）。
- 定位：`ui/src/renderer/pages/voiceSession/VoiceConversationBar.tsx:20–26`。
- 触发：无活动声学通话，当前全局 task 在 Q，用户在已有语音 Conversation P 输入文字 B，B 持久排队。keyboard API 接受绑定 P 的消息，ledger 的当前 task 仍为 Q，而 queue 包含 P。
- 当前行为：items 已筛选出 P 的有效排队项，但 activeTask 只看 snapshot.task 是否属于 P；提前 return 条件没有考虑 items，整个 bar 为空。
- 影响：P 用户看不到等待状态、位置和取消入口，容易误以为没接收而重复提交。
- 实跑：`queue-visibility.test.tsx` 只替换快照观察 hook，实际渲染生产 bar；Q active/P queued 预期出现 B，实际 HTML 为空。相同输入在 active task 属于 P 时另有正向对照。依赖目录 JSX 路径问题已修正；SSR 的 i18n provider 提示不是提前 return 根因。
- 最小修复：只有当前会话既无通话、任务、排队项和错误时才隐藏；保留准确作用域，不能展示其他会话队列充数。

## 已独立实跑

| 检查 | 结果 |
| --- | --- |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | 85 pass / 0 fail / 3 ignored，65.05 秒 |
| `cargo test -p nomifun-conversation voice_ --lib` | 9 pass / 0 fail |
| `cargo test -p nomifun-voice-terminal` | 27 lib + 8 vt_2 + 6 vt_3 = 41 pass / 0 fail / 1 ignored |
| `cargo test -p voice-companion` | 45 pass / 0 fail / 1 ignored |
| 原报告 12 文件 Bun 定向组 | 83 pass / 0 fail / 555 assertions，4.57 秒 |
| `cargo check -p Flowy` | exit 0，2.81 秒；有既有 unused 等 warning，未当错误 |
| `bun run build:ui` | exit 0，Vite 2 分 15 秒；chunk/eval warning 保留 |
| `bun run check` | exit 2，停在 typecheck，61 条 TS 诊断；不能称聚合门禁通过 |
| 独立 TS compiler 基线覆盖层重跑 | 基线 93e0ecac8 为 62，当前 61，按 file/code/message 比较新增 0；不切分支、不改源文件 |
| 独立静态 gates | i18n、theme、motion、button-layout-contract、icons、css-scope、windows-console-hide、process-runtime-boundary、browser-platform-boundary、error-surface-contract、support-surface-contract、codemirror-runtime、market 均 exit 0；dead-css exit 1 |
| `git diff --check` | exit 0；仅现有文档 CRLF 正规化 warning |

补跑 `voice-companion --features voice-skeleton` 的原因：默认命令没有启用 voice_session、voice_confirmation、voice_streaming、standard_audio_replay 中受 cfg 控制的行为用例。最终结果另附本报告末尾，不把默认 45 项误认为这些用例全覆盖。

独立探针为预期失败的审查证据，不应删除或放宽断言来宣布通过。日志与探针在本地忽略目录 `build.noindex/voice-call-ux-independent-review/`，不随 Git 交付；接手需将有效用例纳入正式测试。运行 queue 探针使用只读快照输入，不替换任务业务逻辑。

## 未签核与执行次序

先处理 R1/R2 的启动身份恢复，再处理 R3 精确取消和 R4 队列可见；各自加入反向回归，然后复跑既有套件。之后才能更新 PROGRESS 和 current-implementation-status 中“恢复边界已全部闭环”的描述。

伙伴音色连接级冻结、非 Nomi 作用域借用仍未完成；BLOCKED/HANDOFF 与新 PROGRESS 的部分状态不一致，需要按实际结果同步。真实麦克风/AEC/扬声器、原生透明首帧/DPI 均未验收。未跑跨仓 fingerprint/release-sync、全仓 cargo check，不将桌面 feature 编译等同全仓签核。

本轮没有改业务源码、依赖、测试门禁或已有正式测试，没有提交、推送、发布。

## 补充最终结果

- `cargo test -p voice-companion --features voice-skeleton`：exit 0，全部 target 汇总 **100 pass / 0 fail / 11 ignored**。其中包含默认命令未覆盖的会话、确认、流式和完整语音流；跳过项仍不等于实际设备/模型验证通过。
- 队列反向探针最终 **1 pass / 1 fail**：Q active/P queued 为红，相同队列在 P active 时的正向对照为绿，确认隐藏来自 bar 的提前 return。
- 当前 diff 空白检查 exit 0；审查前后四个关键业务文件 SHA256 保持一致，未发现这些文件在验证中被其他执行者改写。
- Standards 与 Spec 均保持未通过；常规测试全部为绿不覆盖上文四项 Required。
