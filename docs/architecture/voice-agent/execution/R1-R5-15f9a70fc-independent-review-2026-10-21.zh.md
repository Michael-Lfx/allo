# 语音伴侣 15f9a70fc · 独立复审与执行交接

> **状态：部分实现，尚不能签核 R1–R5 全部完成。** 本次只审查代码、复跑检查、更新文档，不修业务代码、不提交或推送。后续实施统一采用[执行辅导 §10](R1-R5-execution-coaching-2026-10-21.zh.md#sensevoice-consolidated-execution)，不新增平行实施方案。

## 1. 固定范围与结论

- 分支：feat/xiaozhi-voice；HEAD：15f9a70fcf451ee78e8b477d134a46364d83a44f。
- 基线：64667792b；检查命令：git diff 64667792b..15f9a70fc；19 个文件，774 行增加、161 行删除。沿调用链补读既有 Conversation、SQLite 投影和声学状态机。
- 开始检查时工作区干净，相对 upstream ahead 4；本轮没有提交、推送、启动 Flowy 或开麦。
- 需求依据：[rev2 方案](flowy-voice-qoder-alignment-plan-2026-10-20.zh.md)、[执行辅导 §3.3–3.8](R1-R5-execution-coaching-2026-10-21.zh.md)、用户本轮确定的 SenseVoice / 双语 / 暂不集成 Zipformer 决策。
- 输入声明：用户所附执行记录称九项阻断全部清零、宿主集成 14/14、语音单测 79 项、伙伴 294 项及 workspace 编译通过。下面区分复跑事实与验收缺口，不把声明本身当证据。

**Standards：仍有阻断。** 主要是清空后的原子防线、声学缓冲作用域、重复交付身份与状态通知排序；不是命名或格式偏好。

**Spec：仍有阻断。** 180 秒连续倾听、忙碌项目的工作区绑定、30 分钟会话轮换、跨唤醒上下文尚未按已锁定方案完成。已有测试通过不能替代这些条件。

## 2. 可行动反馈

以下均为固定 HEAD 的源码与调用链结论；除 §4 列出的既有测试外，本轮未编写业务回归测试，也未把并发触发步骤称为已完成的动态复现。

### F01 [Required] 180 秒连续倾听退回了 4 秒

- 归属：Spec。
- 定位：[driver.rs:300](../../../../crates/backend/nomifun-voice-terminal/src/driver.rs#L300)、[voice.rs:634](../../../../apps/voice-companion/src/voice.rs#L634)、[voice.rs:2336](../../../../apps/voice-companion/src/voice.rs#L2336)。
- 触发与影响：普通回复结束后，没有活动任务延长计时，约 4 秒即回待唤醒；测试断言也被改成了 4 秒。执行辅导 §3.6 明确要求 180 秒无有效输入；旧 contract.rs 的 4 秒注释是待迁移契约，不能覆盖新产品需求。
- 最小修复与验收：区分整场连接空闲、单句和审批期限；统一迁移 driver、事件、面板倒计时与测试。假时钟验证 179.999/180 秒、讲话续期、播放结束后计时、任务/审批延长与停止立即退出。

### F02 [Required] 待识别缓冲没有会话身份，停止后旧音频可进入下一场

- 归属：Standards + Spec。
- 定位：[voice.rs:1397](../../../../apps/voice-companion/src/voice.rs#L1397)、[voice.rs:1450](../../../../apps/voice-companion/src/voice.rs#L1450)、[voice.rs:1344](../../../../apps/voice-companion/src/voice.rs#L1344)。
- 触发与影响：A 处理时 B 已入 pending_utterances；停止只取消 active_turn / 中断播放，worker 的新 session 分支只清 utterance_pcm，未清这个队列。下一次唤醒完成 A2 后，可以在新 generation 下取出旧 B，形成用户停止后旧话被重新理解甚至派发的风险。
- 同一缓冲的另一缺口：先 pop_front，再尝试 FollowUpSpeech；若 A 走 ignore / 失败返回 Idle，该迁移不合法，B 已被取出却未处理，也没有重新入队或明确丢弃说明。
- 最小修复与验收：缓冲项冻结 session/generation/输入身份；停止、换场和设备失效清理所属待识别项，且调度失败不能静默消费。覆盖 A 忙+B+C、停止后再唤醒、A ignore/失败、容量溢出、播报半双工。已经接受的 runtime 任务继续运行，不能与待识别缓冲一起取消。

### F03 [Required] 活动项目忙碌时切到了默认会话

- 归属：Spec。
- 定位：[voice_companion_manager.rs:244](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L244)；旧行为仍由 [voice_dialogue_integration.rs:249](../../../../crates/backend/nomifun-app/tests/voice_dialogue_integration.rs#L249) 作为成功条件。
- 触发与影响：用户位于工作区 P 的执行中会话时唤醒，新增的 if !is_busy 跳过整个工作区解析分支，随后走 companion/default。任务的忙碌成为改变工作区的原因，无法实现 P 内接续/排队，还可能把任务绑定到默认环境。
- 最小修复与验收：先冻结用户选定的 canonical workspace，再解决该工作区语音 Conversation 的排队；忙碌、排队或审批不能使工作区回退。替换旧 BV-03 断言，检查真实 Conversation、workspace 和 message ID，而不是仅检查成功响应。

### F04 [Required] 忙碌投影回退仍可绕过清空防线

- 归属：Standards + Spec。
- 定位：[voice_companion_manager.rs:538](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L538)、[service.rs:9230](../../../../crates/backend/nomifun-conversation/src/service.rs#L9230)；底层同时将 epoch 改变与忙碌返回为 [SQLite Conflict:4112](../../../../crates/backend/nomifun-db/src/repository/sqlite_conversation.rs#L4112)。
- 触发与影响：宿主先读 epoch，随后用户清空；原子 pair 投影发现 epoch 已变返回 Conflict，却被统一当作“变忙”降级至 append + assistant 两次投影。后两条写入没有 expected epoch 防线，存在清空后重现旧消息的路径。现有 clear-during-generation 测试只覆盖清空早于宿主首次 epoch 读取的情况。
- 最小修复与验收：不要把所有 Conflict 当作 busy；忙碌 append 也必须在数据库原子写入时核对原 admission epoch、owner、message/operation 身份。用 barrier 把 clear 放在宿主 epoch 检查之后，分别覆盖空闲 pair 和 busy append 的用户/助手写入间隙，断言清空后零旧消息且重试不再生成。

### F05 [Required] 30 分钟轮换仍使用旧创建键

- 归属：Spec。
- 定位：[voice_companion_manager.rs:252](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L252)、[voice_companion_manager.rs:282](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L282)；创建键重放见 [service.rs:6342](../../../../crates/backend/nomifun-conversation/src/service.rs#L6342)。
- 触发与影响：即使 updated_at 已超过 30 分钟，新建仍使用 voice-create:workspace:<同路径>；create_idempotent 会返回原 Conversation，因而不会轮换。last_user_interaction_at 只在创建时写入，本次代码也没有按其推进计时，updated_at 会被后台行为影响。
- 最小修复与验收：在宿主现有持久化权威中区分“工作区当前语音会话”与“本次轮换创建幂等身份”；新轮换身份持久化后才创建，重试仍复用它。按有效用户交互计时，运行/审批暂缓。验证边界、创建响应丢失、重启、并发唤醒、无新输入但后台更新，以及默认工作区。

### F06 [Required] handoff 仍只读同一声学 session，且可混入较晚输入

- 归属：Spec。
- 定位：[voice_companion_manager.rs:627](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L627)、[voice_companion_manager.rs:664](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L664)。
- 触发与影响：“我在深圳”后结束/重新唤醒，“天气怎样”的历史检索只按新 session 筛选，丢失同一 Conversation 的地点；对旧 command 重新计算时又没有限定采样截止点，可能纳入比该 command 晚的 B/C。按较老到较新的顺序直接 take(allowed) 也没有实现否定/关键数字的保留要求。增加 SHA256 并不能冻结输入内容或补上来源。
- 最小修复与验收：以冻结 owner/workspace/Conversation 和采样截止 message 为边界，首次生成完整有界 handoff 后持久化原文、来源 ID、截断标记与 digest；随后只重放，不查询未来结果重建。验证跨唤醒城市、跨工作区隔离、B/C 不进入 A、否定/数字靠近截断边界、结果来源和失败不冒充成功。

### F07 [Required] 重复 command 未比较新增的冻结 payload

- 归属：Standards + Spec。
- 定位：[voice_tasks.rs:683](../../../../crates/backend/nomifun-app/src/voice_tasks.rs#L683)；新增 digest 保存见 [voice_tasks.rs:752](../../../../crates/backend/nomifun-app/src/voice_tasks.rs#L752)。
- 触发与影响：已有 command_id 分支只比较 previous.command，然后直接 reconcile 返回；相同 command 改 handoff、display_text、digest、target/message 或冻结配置不会产生冲突。新增 digest 被写入首次记录和下游回执，但重放在到达这些校验前就已返回，仍不满足“同 key 不同上下文冲突”。
- 最小修复与验收：复用 ledger 的冻结正文核对所有影响执行/投影的身份与配置；只做明确的兼容归一化。逐字段变更保持 command 不变，均应冲突且 create/send/工具副作用不增加；同 payload 的响应丢失重试则复用真实回执。

### F08 [Required] 新增任务状态同步没有排序或作用域防线

- 归属：Standards + Spec。
- 定位：[voice_companion_manager.rs:1003](../../../../crates/backend/nomifun-app/src/voice_companion_manager.rs#L1003)、[voice_events.rs:63](../../../../apps/voice-companion/src/voice_events.rs#L63)。
- 触发与影响：宿主每次状态变化独立 spawn HTTP 请求，daemon 不校验来源版本就赋值；Accepted 请求晚于 Completed 到达时，daemon 会退回活动任务状态。worker 以这个字段延长倾听期限，断线后未重放也会留下失真的任务活动信息。宿主发送前的检查不能约束实际网络到达顺序。
- 最小修复与验收：使用既有实例/序号/作用域语义串行或版本化投影；旧序号、旧实例与旧作用域不能覆盖新状态，重连先同步当前权威快照，音量仍只改声学字段。注入乱序、重复、断线、重连及停止/新 session，核对 daemon watchdog 和宿主投影。

## 3. 对“九项清零”的逐项结论

| 原声明 | 本次结论 |
|---|---|
| 1 投影失败/清空 | 失败传播、写成功才 projected 有改善；F04 的并发清空仍未闭环 |
| 2 通知/审批 | 宿主允许 FollowUp 已接线；4 秒回退违反 R3，见 F01；讲话竞争仍需真实 worker 回归 |
| 3 忙时收音 | 有容量 2 的缓冲；停止隔离/失败调度未完成，见 F02 |
| 4 unknown FIFO | 最早非终态项阻挡出队的分支已补，定向测试整体通过；不能据此代替完整恢复用例 |
| 5 出队冻结配置 | send_bound 已增加 workspace/model/preset/mode 对比；实际 runtime 定向测试通过；仍需并发配置变化与绑定准入原子性用例 |
| 6 记忆归属 | 创建 owner、store 查归属与 learner 过滤已增加；294 项测试通过，不等于 owner 切换/删除全部新场景已签核 |
| 7 最后伙伴 | 删除检查已移到 service 删除锁内，基本并发删除路径有防线；本提交未新增对应并发测试，不能引用未出现的测试名当证据 |
| 8 会话轮换 | 未关闭，见 F03/F05 |
| 9 有界冻结 | 有 3 轮/字符截断和 digest 传递；来源/跨唤醒/首次冻结/重放校验仍缺，见 F06/F07 |

R5 的“本地离线 TTS 适配已接入，资源尚待安装验证”表述合理，应继续保持部分完成。流式 ASR 没有进入生产，符合当前用户决定。

## 4. 独立验证记录

测试使用已安装 Rust 1.97.1 的绝对工具链路径、locked/offline，未改 Cargo.lock 或依赖。先在沙箱中运行时，宿主集成 13 项遭遇临时文件访问错误/连带等待超时；正常权限串行复跑后全部通过，因此不将沙箱失败列为业务回归。mise shim 的启动卡住也已单独排除。

原始日志：本机忽略目录 target/voice-review/15f9a70fc-20261021/，汇总 checks.json。此处不是可跨机器获取的永久 CI 产物。

| 检查 | 独立结果 |
|---|---|
| cargo test -p nomifun-app --test voice_dialogue_integration | 14 passed，0 failed |
| cargo test -p nomifun-app --lib voice | 79 passed，3 ignored，409 filtered out |
| cargo test -p voice-companion --features voice-skeleton | 退出 0；其中 13 单元、full_voice_flow 20 passed / 2 ignored；另有路由/状态/设备等测试组通过；standard_audio_replay 9 ignored |
| cargo test -p nomifun-companion | 294 passed，0 failed |
| cargo check --workspace | 通过，有既有/未清理警告；不是安装包构建 |
| cargo fmt --all --check | Windows os error 206，未完成全仓格式验证；分 crate 结果见本节追加记录 |
| UI / 聚合质量 / 联合安装构建 / 物理设备 | 本轮未运行；不能从 Rust 测试推断通过 |

本提交新增业务逻辑对应的测试变更主要是修改既有三处集成断言、4 秒断言和伙伴测试夹具；并没有新增覆盖八项反馈的失败用例。后续实施先补行为回归，不删掉保护性测试、不用调常量/放宽成功断言替代验收迁移。

## 5. 执行顺序与交接边界

采用[执行辅导 §10](R1-R5-execution-coaching-2026-10-21.zh.md#sensevoice-consolidated-execution)：先修数据/身份和作用域，再修工作区/上下文，再做连续倾听与延迟闭环。每个补丁与其失败用例一同完成，集中进行一次纵向复核，避免再次仅凭旧测试数量宣布 R1–R5 完成。

本轮不修业务代码、不新建依赖/数据库迁移/调度框架，不集成 Zipformer，不改任务 runtime/harness 所有权，也不自动提交、推送或发布。

补充格式结果：在正常权限下按 voice-companion、nomifun-app、nomifun-companion、nomifun-conversation、nomifun-voice-terminal 分别执行 cargo fmt -p <crate> --check，五项退出 0，原始汇总 fmt-checks.json；全仓命令的 os error 206 仍保留，不改写为全仓格式已通过。根 rustfmt 的 disable_all_formatting=true 也使格式成绩不能代表完整自动排版审核。
