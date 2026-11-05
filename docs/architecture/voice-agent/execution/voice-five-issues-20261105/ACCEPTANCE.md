# 语音五组问题修复 · 验收报告

- 日期：2026-11-05（UTC+8）
- 分支：`fix/voice-missing-service-component`；基线提交 `ded6a29dc`，复审修复已作为后续提交落在当前分支；未 push
- 基线：`bdcbe49f4`
- 详细方案：`C:\Users\38788\.codex\visualizations\2026\11\05\01a1a965-aa1c-72b2-b7fd-e195b68e18f4\voice-five-issues-repair-plan.zh.md`
- 任务账本：`PROGRESS.md` / `BLOCKED.md`（本目录）

## A. 需求 → 修复 → 证据

| 原始现象 | 修复 | 证据 |
| --- | --- | --- |
| 首轮已播放但 chip 仍在“正在合成” | host reader 不再 await cleanup/audio/admission 维护；单实例 maintenance worker 受 generation/shutdown 约束；voice view warmup 直接跳过 | `voice_view_warmup_never_builds_an_agent_runtime`；host voice 142；UI hook 探针未发现 chip 漏更新（未改 chip） |
| 首轮历史缺失、第二轮才标题 | `record_pair` 锁内只保存 output/短校验/epoch，离锁投影；回写重读最新 journal 仅合并本轮回执；workspace update 先于 `projected=true`（R1）；recording transient busy 有界重试 | 原 R1 失败用例绿；integration 53；conversation 658 |
| 01/01 08:00 | `LiveVoiceHistory.observedAt` 首次观测时间；空历史不再用 0；持久化行接管 DB 时间；NomiChat 传 `live.state` | 实现前 14 失败用例→0；UI 相关 72 pass |
| AgentTask 未执行 / 15s 断话 | voice warmup 跳过 63s gate；TaskControl 锁外等 gate；`task_configuration` 1s 短锁等待 projected；pending 同 command 后台重试，提交前重跑 scope guard；daemon unknown 不 SessionEnded，3s watchdog 给未确认 text follow-up，真实回执优先 | integration `task_configuration_waits...`；daemon unknown watchdog test；daemon 109 |
| 打字无反馈 | VoicePetInput 发送中 `aria-busy`、按原 command_id 轮询真实状态、失败保草稿、卸载/切会话失效 | VoicePetInput 12 tests；UI 72 |

## B. 安全 / 顺序 / 幂等 / 取消

- journal 锁不再跨 projection / preparation gate；TaskControl、placeholder finish/cleanup 已缩短锁范围。
- 回写不覆盖离锁期间的新 journal 记录；同 record/request/output 精确匹配后才置 projected。
- 任务只以原 `command_id` 提交；replay 先于 session/turn/phase 检查；pending 后台重试不换身份；真正 submit 前重跑 instance+session guard（instance/session/turn/phase/enabled/connection/revoked/shutdown）。
- 真实 Accepted/Queued 回执到达时 watchdog 不覆盖；真实回执优先并释放 DispatchingTask，成功结果可走摘要播报。
- 既有 epoch、冻结目标、reset/delete、task replay、workspace update 顺序保留；R1 已修复。
- 未新增持久化格式、公共协议、依赖、鉴权/进程边界改动。

## C. 分层验证（原始汇总）

| 层 | 命令 | 结果 |
| --- | --- | --- |
| conversation unit | `cargo test -p nomifun-conversation --lib` | **658 passed / 0 failed / 0 ignored** |
| host voice | `cargo test -p nomifun-app --lib voice` | **142 passed / 0 failed / 11 ignored** |
| voice integration | `cargo test -p nomifun-app --test voice_dialogue_integration` | **53 passed / 0 failed** |
| voice dialogue unit | `cargo test -p nomifun-app --lib voice_dialogue` | **19 passed / 2 ignored** |
| daemon | `cargo test -p voice-companion` | **109 passed / 2 ignored / 0 failed**（lib 78 + integration 31） |
| voice terminal | `cargo test -p nomifun-voice-terminal` | **60 passed / 0 failed / 3 ignored** |
| UI related | `bun test` 11 files | **72 pass / 1 skip / 0 fail** |
| UI joint trace | `bun run test:voice-joint-trace` | **6 pass / 0 fail；SUCCESS**（首次并发构建超时，空闲重跑通过） |
| workspace compile | `cargo check --workspace` | **exit 0** |
| UI build | `bun run build:ui` | **exit 0** |
| i18n | `bun run check:i18n` | **exit 0** |
| formatting | `rustfmt --edition 2024 --check` changed files | **exit 0** |
| diff hygiene | `git diff --check` | **exit 0** |
| daemon build | `cargo build -p voice-companion --bin voice-companion` | exit 0 |
| desktop build | `cargo build -p Flowy` | exit 0 |
| typecheck baseline | `bun run typecheck` | **exit 2，基线既有 71 errors；改动文件 0 error** |
| aggregate gate | `bun run check` | **exit 2，同一批基线 typecheck 阻断** |

## D. 独立审查与修复

| 轮次 | 发现 | 处理 |
| --- | --- | --- |
| 初审 R1 | workspace update 失败后 `projected=true` 破坏可重试 | update 先于 merge；原失败 integration 绿 |
| 初审 R2/R3 | TaskControl / placeholder cleanup 持 journal 等 gate | 短锁快照 + 锁外 service 调用 |
| 初审 R4 | task_configuration 未投影直接冲突 | 1s 短锁等待 + `task_delivery_pending`；宿主 delayed retry |
| 初审 R5 | watchdog 覆盖真实回执 / TextFollowUp 无 deadline | 真实回执优先并释放 Dispatching；补 TextFollowUpWindow；host gate 纳入 TextFollowUp |
| 复审 R-1 | delayed retry 绕过 scope 校验 | 抽 `submit_scope_guard`，真正 submit 前重跑；失败不提交 |
| 复审 R-2 | 真实回执跳过 watchdog 时未释放 Dispatching | 静默 TaskAccepted + Listening 恢复；`announce_task_result` 测试成功；最终复审无 Required |
| 提交后 F1 | replay 排在 session/turn/phase guard 之后；resource-write 错误码退化 | 拆 instance/session guard，replay 先于 session guard；新增 Idle 后 replay 测试；恢复 `voice_resource_write_in_progress` |
| 提交后 F2 | `record_pair` phase-1、`refresh_projection_epoch`、`task_configuration` 仍持 journal 跨 DB/磁盘 | 前者锁内只 journal；epoch 校验锁外；task config 短锁快照/锁外计算/短锁回写 |
| 提交后 F4/F5/F7/F8 | UI marker 不一致、registry RMW 半覆盖、finish TOCTOU、tracker 未校验 command_id | UI 补 marker；`save_workspace_entry` 合并写回；finish finalize 前复核；tracker 校验 command_id |
| F6 | 首次 transient failure 已按既有 Complete-frame 契约上报 | 保留现有行为；不改为 retry 成功后才回报，避免破坏既有测试/契约 |

## E. 未完成 / 未验证

- TC12 真实两轮天气（真实麦克风、上游 runtime、物理扬声器）未执行；见 `BLOCKED.md`。
- R2/R4/R5 部分专项时序缺专门 integration 红灯；R4-1 >3s 投影为安全 fail-closed。
- `bun run typecheck` / `bun run check` 的 71 个基线错误非本次引入；未修无关 UI 类型错误。
- `ded6a29dc` 为基线提交；本次复审修复已提交在当前分支；未 push。
