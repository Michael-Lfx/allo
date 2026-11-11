# 四项修复后的再次独立复审（2026-10-23）

> **后续实施记录**：用户授权后已修复本文 F1–F3，并纳入正式回归，见 [RECOVERY-REPAIR](RECOVERY-REPAIR.zh.md)。本文保留修复前的发现与失败证据，不作为最新未修复清单。

结论：**仍有三个启动/停止恢复边界遗漏，不能认定已完全闭环。** 上轮修复的精确任务取消、P 排队/Q 执行的展示、绑定成功后的重开路径保留有效；下述发现补充上轮未覆盖的分支，不撤销这些正向证据。

## 审查范围与判断

- 分支 `feat/voice-next`，HEAD `93e0ecac8ca817c47b9cf37ad354456977f90c65`；暂存区为空，审查对象是完整未暂存语音改动及相关新文件。没有切分支、提交、推送或修改业务源码。
- Standards：模块/依赖方向没有发现本轮新增违规；失败恢复和操作身份仍有 Required。
- Spec：按主方案 §9.5(2)(5) 检查“未知不重发、明确结果可恢复、旧停止不销毁较新预约、只取消绑定 command”。启动/停止恢复暂不通过；UX4、非 Nomi、原生验收仍按原修复报告单列，不重复当作本轮新发现。
- 本轮仅增加本审查报告和忽略目录中的只读探针。探针调用生产 adapter，仅替换 HTTP 边界；没有以改写业务代码或降低正式断言制造失败/通过。

## Required

### F1：明确收到 daemon 503 拒绝后，前端仍把启动当作未知，恢复后无法正常重试

- 两条审查线：Standards（错误分类/恢复）+ Spec §9.5(2)。
- 定位：`ui/src/common/adapter/voiceCompanion.ts:339–344`；宿主 `voice_companion_manager.rs:2487–2509` 及缓存 `Rejected` 分支。
- 触发：模型尚未就绪、麦克风暂不可用等，daemon 明确返回 503；宿主记录 `Rejected`、清预约，返回 `code=daemon_wake_rejected`。之后资源恢复，用户从正式语音页重试。
- 当前行为：adapter 只按 `<500` 等条件清 pending，把这个确定拒绝保留为未知。入口 target 从 current_selection/workspace 变为 voice_conversation 时，在发 HTTP 前就报 `voice_call_starting`；同 target 重试则复用旧 request，宿主一直重放同一个 Rejected，仍无法恢复。
- 实跑：`rejected-start.test.ts` 先注入真实格式的明确 503，再模拟就绪并从正式页开启，**0 pass / 1 fail**，实际错误 `voice_call_starting`，第二次 HTTP 未送出。宿主的永久重放链为静态确认，不冒充完整 daemon 实测。
- 最小修复：在协议上区分“确定拒绝”与“发送结果未知”，确定拒绝允许下一次显式操作生成新 request；任意网络/5xx 未知不能一律清除。Rejected 首次/重播的结构化错误应一致，保留同 ID 不同参数冲突规则。覆盖模型/麦克风恢复与 timeout 对照。

### F2：同一请求在绑定完成前被成功停止，pending 仍不能解除

- 两条审查线：Standards（生命周期完整性）+ Spec §9.5(2)。
- 定位：`ui/src/common/adapter/voiceCompanion.ts:257–263、324–327`；关联宿主 `mark_active_request_stopped` 仅处理 Bound。
- 触发：启动响应丢失，daemon 已产生 session/request 身份，但宿主绑定因落盘或校验失败等尚未完成；停止已收到真实成功回执，或同一未绑定 session 结束。
- 当前行为：成功 stop 仍调用要求 `binding.session_id` 的同一清理函数。即使 request_id/session 均明确，binding=null 也不能清 pending。正式页重开继续 `voice_call_starting`。宿主未绑定 Starting 回执也没有对应终态协调路径。
- 实跑：`unbound-stop.test.ts` 使用生产 `startVoiceCallIntent` 与 `stopVoiceSession`：响应丢失→同 ID/session、binding=null→stop HTTP 200→正式页重开，**0 pass / 1 fail**，实际错误 `voice_call_starting`。这证明 adapter 边界遗漏；没有声称本轮模拟了原生设备或完整绑定落盘故障。
- 最小修复：成功挂断/同身份权威终态能在没有 binding 的情况下退休原 request；活动快照仍要求可靠绑定才能解释为启动成功。宿主同步处理 Starting→Stopped，不能用旧请求或任意 idle 清理新 pending。补成功/失败停止、无 binding 自然结束和旧请求对照。

### F3：旧 idle/已结束 session 的停止仍可能删除较新的启动预约

- 两条审查线：Standards（并发身份）+ Spec §9.5(2)“不得销毁较新 reservation”。
- 定位：`crates/backend/nomifun-app/src/voice_companion_manager.rs:2189–2195`；`nomifun-api-types/src/voice.rs:648–653`。
- 触发：旧会话已结束，或当前仍是无 session 的 idle 快照。新启动已写 reservation/wake，但新声学快照尚未到达宿主；此时旧停止请求迟到。
- 当前行为：`matches` 只比较 instance/session/turn，不看 active/ended、启动 request 或 reservation。旧快照尚未被替换时它仍能匹配，随后无条件 `take_call_reservation`，而 daemon 的停止是否成功还未核对。若 daemon 已接受新会话，会拒绝旧停止，但宿主已删除新预约；新唤醒的 freeze 随后只能报 `start_request_changed`，不能完成绑定。
- 证据：生产代码的状态比较和副作用顺序静态确认。现有 F3 测试仅发送不同 instance/session，覆盖的是前置拒绝，没有覆盖“旧请求仍匹配当前旧投影、新 reservation 已存在”。本轮未声称该后端交错已动态复现。
- 最小修复：活动停止与取消 pending 启动分别按明确身份核对；旧 idle/ended 停止不得消费新预约。消费 reservation 必须匹配对应 request，而非无条件 take。补相同 host 的旧 ended session/None session、新 reservation 与延迟 SSE，并断言拒绝/失败停止后新预约仍在。

## 验证与交接

- 正式前端定向 16 文件组：**101 passed / 0 failed / 702 assertions**，4.49 秒。
- 两个新增边界探针分别 **0 passed / 1 failed**；失败为生产 adapter 的 `voice_call_starting`，没有别名/权限/编译错误。
- 原始日志/探针：忽略目录 `build.noindex/voice-call-ux-second-review/`。日志不随 Git 交付，修复者应把有效失败用例纳入正式测试。
- 宿主 manager 与实际 Conversation 集成的本轮结果补记于文末；没有重跑全仓构建、声学硬件、UI 视觉或发版检查，不把上轮结果称为本轮新实跑。
- 最小执行顺序：先 F1 明确拒绝恢复，再 F2 未绑定终止，再 F3 预约身份；分别保留 unknown 不重发、旧回执隔离、取消不影响其他队列项的已有断言，完成后复跑正式套件。业务代码本轮未修复，等待用户安排。

### 最终补记

- `cargo test -p nomifun-app --lib voice_companion_manager::tests --no-default-features`：**32 passed / 0 failed**，4.14 秒。
- `cargo test -p nomifun-app --no-default-features --test voice_dialogue_integration`：**26 passed / 0 failed**，1.80 秒。
- `git diff --check`：exit 0；暂存区为空。adapter、manager、NomiSendBox 的审查前后 SHA256 一致，没有在审查中修改这些业务源码。
- Standards 与 Spec 的恢复边界保持未通过；“常规测试为绿”与“两个补充探针为红”同时成立。上轮报告的四项修复不应再被概括为所有恢复分支已完成。
