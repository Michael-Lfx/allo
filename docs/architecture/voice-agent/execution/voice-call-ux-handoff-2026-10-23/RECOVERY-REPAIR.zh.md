# 启动与停止恢复补充修复（2026-10-23）

本轮按用户“全部修复”实施 [再次复审 F1–F3](SECOND-REVIEW.zh.md)。分支仍为 `feat/voice-next`、HEAD `93e0ecac8`；保留接管的未提交工作，没有提交、推送、切分支或新增依赖/迁移。

## 1. 修复后的行为

| 项目 | 生产实现 | 正式验证 |
| --- | --- | --- |
| F1 明确拒绝可恢复 | daemon 明确拒绝使用统一 `daemon_wake_rejected` 结构落入 Rejected，首次与同 ID 重放一致。adapter 释放这个已确定拒绝的 intent，下一次显式操作使用新 ID；网络错误/`daemon_wake_timeout` 等未知仍保留原 ID | HTTP 边界先明确 503、资源恢复后新 ID；未知 503 不允许换目标、重试沿用原 ID；宿主首次/重放拒绝完全一致 |
| F2 未绑定终止 | adapter 的成功停止不再依赖 binding；订阅收到同 request/session 的权威 ended_reason 也可解除 pending。仅有 idle 或绑定缺失不能推断启动成功/已结束。宿主按 request + session 支持 Starting→Stopped；迟到绑定不能把 Stopped/Rejected 改回 Bound | 未绑定停止失败、旧 ID 停止均不解锁；对应成功停止才解锁；无 binding 自然结束解锁，idle 对照不解锁；宿主未绑定停止与自然结束回归 |
| F3 旧停止隔离 | 活动停止要求有效非 idle、未结束的 session；若存在预约，还须与该快照的 call_request_id 一致。所有预约清理调用改为在锁内按预期 request ID 取走；停止失败不清预约，成功停止/权威终态只清对应预约 | 同 host 的旧空 session、旧 ended session、旧 active 投影与新预约交错，均保留新预约；拒绝停止后预约仍在；实际 recorder 的旧 ID 清理无效、正确 ID 消费成功 |

继续沿用现有 session/turn/command/runtime turn 分工及 Conversation/FIFO，不新增权限模式、调度器或另一套停止接口。声学 session 尚未产生时，活动停止接口不会用空身份猜测取消某个 pending 启动。

## 2. 一致性约束

- `take_call_reservation(expected_request_id)` 是内部 recorder 合同变化；生产实现、全部调用点及测试实现一起更新。冻结绑定本身仍在同一预约身份锁下保存并消费。
- 终态协调先释放回执锁，再清匹配预约；避免与 freeze 的“预约锁→回执锁”顺序互锁。
- `mark_request_bound` 只允许对应 Conversation 的 Starting→Bound，不复活 Stopped/Rejected，也不覆盖已经绑定的其他 session。
- 没有因未知结果解除 intent 或换 ID 自动重发；没有因挂断而清掉其他排队项/取消后台任务。

## 3. 失败复现与自动回归

- 三个新增 adapter 正式用例修复前各 0 pass / 1 fail，修复后单文件 21 pass / 0 fail。它们覆盖明确拒绝、未绑定成功停止及未绑定自然结束，并包含未知/旧身份/失败停止的反向断言。
- 宿主生命周期测试在统一拒绝回执之前失败，原因是重放结构与首次不同；修复后 manager 定向 33 pass / 0 fail。新增自然终态测试验证 Starting 退休及晚绑定隔离；同一生命周期测试使用本地 HTTP daemon 边界验证旧停止和未绑定停止成功/失败。
- 前端正式 16 文件组：104 passed / 0 failed / 719 assertions，3.04 秒。
- 宿主 voice 全组：87 passed / 0 failed / 3 ignored，64.11 秒，包含真实 runtime、生产 worker、读随机文件、审批、取消及 FIFO。
- 真实 Conversation 集成、桌面编译、UI 构建和全仓门禁的最终结果见文末补记。本地日志在忽略目录 `build.noindex/voice-call-ux-final-repair/`，不随 Git 交付；测试已纳入正式源码，接手不依赖忽略目录探针。

## 4. 仍保留的项目边界

本轮只修 F1–F3；不把它扩写成全方案已验收。非 Nomi 工作区借用、伙伴音色连接级冻结、原生麦克风/AEC/扬声器与透明首帧/DPI 仍按 [原修复记录](REPAIR-RESULTS.zh.md) 待办。没有新增模型、依赖、数据库迁移或 agent harness 改动。

全仓既有 TypeScript/dead-css 问题不在本轮修复范围；不改门禁、规则或断言以宣称全绿。未运行安装包/跨仓发版/真实硬件签核。

## 5. 最终联合验证补记

| 检查 | 本轮结果 |
| --- | --- |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | 87 passed / 0 failed / 3 ignored |
| `cargo test -p nomifun-app --no-default-features --test voice_dialogue_integration` | 26 passed / 0 failed，1.70 秒 |
| 前端 16 文件组 | 104 passed / 0 failed；最后断言类型兼容调整后的结果另附 |
| `cargo check -p Flowy` | exit 0，25.53 秒；既有 warning 保留 |
| `bun run build:ui` | exit 0，1 分 54 秒；既有 chunk/eval warning 保留 |
| i18n/theme/motion/button-layout-contract/icons/css-scope | 各 exit 0 |

聚合 `bun run check` 在 typecheck 阶段阻断，不能称全仓门禁通过。中途 70 条诊断包含新增测试 `.rejects` 与仓库本地 Matchers 声明不兼容的 9 条，已改用现有 `.then` 错误断言，保持失败语义不变，没有改类型声明或忽略测试；最终 TypeScript 对照见后续补记。全仓 Cargo/安装包/跨仓发版/硬件检查没有本轮证据。

最终断言调整后再次实跑：前端仍为 **104 passed / 0 failed / 719 assertions**（4.85 秒）；正式 `bun run typecheck` exit 2，**61 条既有诊断**。使用与正式检查一致的依赖读取权限运行 compiler 基线覆盖层，按 file/code/message 比较为 **提交基线 62 → 当前 61，新增 0**。`git diff --check` exit 0。聚合门禁此前停在 typecheck，后续串联项不算通过；独立 UI gates 与生产构建结果见上表，dead-css 的既有失败沿用前轮记录，不在本轮重复签核。

F1–F3 已完成实现及上述自动验证。当前工作区仍含原有与本轮未提交改动；未自动提交、推送或发布，不要求用户先说话才能完成这些代码验证。
