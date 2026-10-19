# 语音快前台审查修复与交接（2026-10-16）

> 最终更新 2026-10-17：9 项审查问题的代码修复、受影响自动回归和 Windows 开发联合构建完成。最终证据见第 9 节及机器清单；Owner、干净安装和多平台仍未签核。

> 范围：本次 code-review 的 9 项 Required findings。沿 feat/xiaozhi-voice 继续，保护原有 WIP；不提交、推送、发布或修改数据库结构。本记录不替代原 PRD / S0–S9 Plan。此前实施记录中的“已验证”不能据此解释为真人签核。

## 1. 修复边界与数据流

生产 worker 以同一声学 session / turn 构造 scoped request，经过 HostDialogueClient 到宿主补全接口。宿主先冻结归属、保存原始 ASR 与生成占位，再调用一次快模型；先保存模型输出，再通过 Conversation 的事务投影原文、理解和回答。任务提交必须引用宿主保存的本轮 agent_task 决策，目标完全一致，且冻结配置未失效。执行仍使用专属 Conversation 和原有语音工具策略。

停止语音、关闭窗口、执行取消、模型生成、任务回执和音频播放维持独立身份与状态。用户云账号 token / key 不会创建云端工作区；执行路径始终是本地目录。

## 2. 九项修复对应

| 编号 | 修复 | 关键验证入口 |
| --- | --- | --- |
| F1 | worker 传递原始 ASR、session_id、turn_id、voice_record_id 和 binding_id；正式历史显示人类可读的理解与回答，保留原始决策供幂等回放 | voice_session::real_model_message_survives_tts_failure_as_a_text_reply；voice_dialogue_integration 的 production_router 场景；真实 Read 联合回放 |
| F2 | 本地原子记录生成占位；并发同 ID 不重复生成；未核实的生成重启后不自动重试；已生成但投影失败时返回文字和独立保存警告，重投影不重问模型 | production_router / a_claim_survives_restart / a_post_generation_projection_failure；模型回调检查调用前实际持久记录 |
| F3 | Conversation 使用既有 preparation gate；数据库事务在写消息和回执前检查 owner、admission_epoch、active operation 和 running 状态；幂等消息事件使用原稳定 msg_id 重播 | busy target / clear target 集成；conversation_repository 的 dialogue pair 事务回归 |
| F4 | 默认独立语音会话，无需宠物；设置页显式选择一个已有宠物；原生主窗口校验焦点、可见性、最小化和当前 URL；HTTP 无法冒充焦点；唤醒时冻结会话与执行配置，提交前复核路径和预设快照 | target API；navigation freeze/workspace 回归；原生焦点操作留给 Owner |
| F5 | 通知与任务状态分开；daemon 缓存同实例 attempt 的实际播放回执；drain 前不记 played；失败保留文字；精确重试仅读缓存，正文冲突拒绝；宿主响应丢失只读查询，离线/重启转 unknown；重听绑定当前作用域和稳定 replay_id | failed TTS receipt/query；blocked drain；notification replay/restart；旧宿主和离线重听 API |
| F6 | 明确的查询任务、任务进度、取消任务和重听任务结果走本地窄控制，早于模型可用性检查；不把模糊“取消”当成任意后台任务授权 | 真实 worker 连续聊天→查询→取消：4 次 ASR、仅 2 次对话模型调用，控制后模型入口拒绝任何额外调用 |
| F7 | 截止与取消意图先落盘，再进行有界观察；观察错误不跳过截止；重启继续取消原任务；创建/发送也受原持久截止约束；真实终态优先于取消投影 | unreadable runtime + restart；stalled send；deadline/clock rollback；真实取消与完成竞态 |
| F8 | 阶段来自真实 runtime 事件；分别记录事件、业务进度和回执新鲜度；30 秒无法核对标 unverified，90 秒无业务进展标 no_recent_progress；15/45 秒反馈讲事实而非重复 earcon；声音 job 不阻塞截止观察；面板显示阶段、健康及通知 | actual event progress projection；tracker 不将 runtime_turn_id 当进度；高频音量保留宿主字段；离屏听感留给 Owner |
| F9 | 模型 8 秒超时、绑定失效/忙、原文存储失败、生成未核实和后续投影失败均有独立错误；双语面板显示具体阶段，不再都映射 model_response_invalid | Host timeout/bridge 回归；VoiceTaskDiagnostics / VoiceErrorStatus 双语行为断言 |

## 3. 接口与持久化约定

- 私有 POST /complete：保留 prompt，生产请求另带 voice_record_id=voice-{turn_id}、binding_id=voice-binding-{session_id}、session_id、turn_id、raw_user_text。binding_id 是宿主作用域引用，不能直接提供任意 Conversation ID。8 秒模型截止、10 秒调用端总超时。
- 补全响应保留 content，增加可选 record_error。首次原文写失败返回 voice_record_write_failed，模型调用次数必须为 0；模型已返回后投影失败返回原 content + record_error。voice_projection_write_failed 明确说明本地保存未完成，不承诺重启后仍可恢复一次磁盘写入失败的结果。
- 正式历史由 Conversation 承载；data_dir/voice/dialogue-{本机所有者}.json 仅承担生成占位、冻结配置、模型输出与幂等恢复。output 为空的记录不能自动重新生成。历史投影失败不能改绑定或新建任务绕过。
- GET/POST /api/voice/companion/target：独立语音会话或一个显式已有宠物；已有 /candidate HTTP 入口返回 native_context_required，桌面改走 report_voice_context 原生 IPC。失焦立即清候选，原生 URL 不匹配的过时报告拒绝。
- 生产 task command 带 record_id / binding_id；同 ID 改目标冲突。首次新交付使用冻结的 canonical workspace / model / preset；预设版本或路径解析变化拒绝本轮，不默默扩大权限。已交付记录仍先只读核对，恢复不重发。
- POST /api/voice/task-result 与 /task-progress 使用同一受保护的音频播放入口；progress 不注入“任务结果”上下文。GET /api/voice/notifications/{attempt_id} 只读查询同实例的 dispatched / played / failed / unknown / suppressed。
- GET /api/voice/companion/tasks/{command_id}/notification 返回通知状态；任务 completed 不代表 played。POST .../replay 要求 instance_id / session_id / turn_id / replay_id；语音离线、用户已停止或作用域过时明确拒绝。重复 replay_id 不新建播放；只有明确的新重听意图才产生新 attempt。
- snapshot 新增可选 binding、task_progress、task_notification，均由宿主合并；声学快照不得覆盖宿主任务字段。新增字段保持 serde 缺省兼容。

## 4. 自动验证与发布门禁

验证仍在进行。完成后在本节追加精确命令、结果与证据文件；未完成项不预先签核。

已取得的阶段证据：双语 UI 48 项通过；生产补全/Conversation 集成 7 项通过；串行宿主 voice_ 回归 64 项通过、2 项显式手动探针忽略；UI 生产构建通过。新增导航冻结和投影失败端到端场景及剩余 crate / 生产 feature / 桌面检查继续核验。

全仓 bun run check 在 typecheck 阶段被 60 个非语音文件类型错误阻断，完整输出见 target/voice-review/quality-check.log；本次该输出没有 Voice / voiceCompanion / useVoiceContext 错误。check:i18n、check:theme、check:icons 已通过。check:dead-css 的非语音基线问题单列，不以跳过或降低规则使门禁变绿。

所有测试最终产物使用 target/voice-review；生产检查使用 target/voice-staging。仓库现有 build.noindex 中间缓存仍由 Cargo 配置复用。不得用 voice-skeleton 测试二进制覆盖默认开发 daemon。

## 5. Owner 真机交接（未签核）

| 场景 | 操作及验收 | 状态 |
| --- | --- | --- |
| 无宠物首次使用 | 默认独立语音会话；原文、理解、回复可见；打开会话检查真实历史 | 待 Owner |
| 原生当前项目 | 主窗口聚焦项目 A 唤醒；说话前切项目 B；本轮仍固定 A，下一次唤醒才重新判断；切到浏览器或最小化后回默认目标 | 待 Owner |
| 宠物默认目标 | 零宠物不要求创建；多个宠物只绑定所选一个；删除绑定宠物明确降级，不借其他宠物会话 | 待 Owner |
| 实际任务与并发聊天 | 指定工作区随机文件读取；实际接单才 ACK；运行中再聊天、查询、取消；任务详情指向独立执行 Conversation | 待 Owner |
| 离屏反馈 | 首次 15 秒、后续至少 45 秒；讲话/回答/审批期间不抢播；无新进展或无法核实须讲清楚 | 待 Owner |
| 真实播放 | 验证扬声器能听见；播完后开始 4 秒追问；停止无残音；设备断开时文字保留且通知不能显示已播完 | 待 Owner |
| 重听与重启 | 失败后显式重听；快速重复点击、旧窗口请求不重复播放；宿主或 daemon 重启只读核对旧通知和旧任务 | 待 Owner |
| 实际危险工具 | 挂起的真实调用确认/拒绝/静默/过期；验证文件副作用，非只看提示；始终一次批准，越界拒绝 | 待 Owner |
| 干净安装与多平台 | 重新构建后再签默认产物、安装资源及退出；Mac/Linux、性能指标和 72 小时挂机仍需分别取证 | 待 Owner |

## 6. 历史记录与当前判定

此前 S0–S8 总览保留为历史，不删除原文。本次 9 项缺陷说明旧的“通过”覆盖不足，尤其不能证明生产 worker 的 scoped 请求、原生焦点、生成占位或真实音频完成。后续交付判断以本记录中的实际证据和 Owner 签核为准。

## 7. 最终收尾补充（2026-10-17）

- 宿主与 daemon 使用 nomifun-api-types 中同一 VoiceDecision / parse_voice_decision，统一处理 JSON 围栏、字段类型和可执行决策；无效输出不得生成任务。保留 voice-terminal 原有公开类型名称的 re-export，避免第二套解析权威。
- 同一已保存 agent_task 决策只允许绑定一个 command_id；重复同身份恢复只读核对，另换 command_id 不能绕开交付幂等。首次绑定前校验目标、配置与作用域。
- 通知状态补充 started：实际音频进入播放阶段后才可报告开始；物理 sink drain 完成前始终不能记 played。查询旧 attempt 不能读取新重听的播放状态。
- 无活动任务的口头查询/取消直接显示具体说明，不落入泛化模型错误；最终 UI 回归覆盖双语文案。

本节验证汇总将在最终命令结束后追加；第 4 节的阶段计数保留为历史。

## 8. 下一轮 Owner 实测计划（不自动启动）

目标：在真人设备上确认本次 9 项修复的交互与声音；以第 5 节逐项记录时间、当前产物身份、实际结果和签核人，失败项保留日志及任务身份，不包含凭据。

1. 沿现有 bun run dev 工作流启动。此轮联合产物是 Windows 开发构建，UI 使用开发入口；不能据此宣称安装包已签核。先确认语音功能、模型及工作区配置，保留 data_dir，不清历史。
2. 无宠物时完成一轮聊天，核对原始 ASR、理解、回答和打开会话后的落盘消息；再选一个已有宠物验证默认归属。
3. 在聚焦的项目 A 唤醒后切换项目 B，本轮仍保持 A；主窗口失焦或最小化时新的唤醒使用默认归属。另开一个正在执行的目标，确认语音不会向其插入快对话。
4. 在指定本地工作区创建带随机标记的只读样本，语音要求读取；检查真实工具结果包含随机内容、专属任务详情入口和进展。任务中继续聊天、查询并显式取消；关闭语音窗口不得变成任务取消。
5. 在测试目录内请求删除专用样本，分别同意、拒绝和静默；必须检查实际文件副作用。普通取消语音不能被当成批准；越界请求始终拒绝。
6. 断开输出设备或触发 TTS 错误，文字仍保留，通知不可标已播完；恢复后明确重听，重复点击和旧窗口请求不得重复执行任务。重启宿主或 daemon 后先只读恢复，不能自动重发任务或未知播报。
7. 离开屏幕听取任务阶段、未核实状态、结果和审批；讲话时不抢播，实际播完后再计追问。记录真实扬声器、焦点/主题与窗口证据。

出口：上述行为及第 5 节真机项分别留证并由 Owner 签核。干净安装、Mac/Linux、性能 P95 和 72 小时挂机按原计划另行验收；非语音全仓质量问题单列，不扩大本次修复。

## 9. 最终验证汇总（2026-10-17）

以下为最终源码身份上的自动验证，优先于第 4 节阶段记录。测试使用 target/voice-review，正式开发构建使用 target/voice-staging；两个目录仅隔离最终产物，Cargo 仍复用仓库配置的 build.noindex 中间缓存。

| 命令/范围 | 当前结果 | 证据 |
| --- | --- | --- |
| cargo test -p nomifun-app --lib voice_ -- --test-threads=1 --nocapture | 64 通过，2 项显式外部条件探针忽略 | target/voice-review/host-regressions-final.log |
| cargo test -p nomifun-app --test voice_dialogue_integration -- --test-threads=1 | 9 通过 | target/voice-review/dialogue-integration-final.log |
| cargo check --workspace | 通过，含 Flowy 与 ai-mate | target/voice-review/workspace-check-final.log |
| cargo build -p Flowy -p ai-mate --no-default-features --message-format=json-render-diagnostics | Windows 开发联合构建通过，两个可执行产物 features=[] | target/voice-review/production-build-final.log、production-artifacts-final.jsonl |
| cargo tree -p Flowy -p ai-mate -e normal,features | 不含 voice-skeleton | target/voice-review/production-features-final.log |
| 语音 UI 五个测试文件 | 49 通过，342 次断言，含双语通知和错误状态 | target/voice-review/ui-regressions-final.log |
| bun test scripts/prepare-voice-companion.test.mjs | 3 通过 | target/voice-review/packaging-regressions-final.log |
| bun run build:ui | 通过，最终构建无重复错误码映射警告 | target/voice-review/ui-build-final.log |
| gen:i18n / check:i18n / check:theme / check:icons | 通过 | 最终命令输出及 browser-boundary-final.log |
| error/support surface contract、process/browser boundary、windows-console-hide | 全部通过 | target/voice-review/*boundaries-final.log、browser-boundary-final.log |
| 受影响 Rust 文件 rustfmt --check、git diff --check | 通过，沿用现有格式配置 | target/voice-review/voice-format-final.log |

全仓 cargo fmt --all -- --check 受 Windows 命令长度限制（OS 206）未通过，已对受影响文件执行同一配置的定向检查；仓库 rustfmt.toml 当前关闭自动格式化，未更改这一设置。全仓 bun run check / typecheck 仍被 60 个非语音文件类型错误阻断，最终复核没有 Voice / voiceCompanion / useVoiceContext 相关错误；完整输出分别见 quality-check.log、typecheck-final.log。dead-css 为非语音文件问题，button-layout 未取得可用 Vite fixture 的通过证据；没有将这些门禁标绿。

真实 runtime 文件副作用及身份计数已保存为 [review-repair-runtime-2026-10-17.json](fast-front-evidence/review-repair-runtime-2026-10-17.json)。口头同意、口头拒绝与静默分别验证了文件删除/保留，每个场景 creation_records=1、delivery_receipts=1；测试使用受控模型和音频替身，physical_devices=false，不是云模型或真人声学证明。

其余受影响 crate 的前一轮定向结果：DB dialogue pair 1 项、Conversation 投影 1 项通过；完整 ai-mate 74 项通过、11 项显式声学/在线探针忽略，api-types 744 项通过。最后的共享解析与播放状态追加回归结果在下方补记，不混算重复套件。

运行身份注意：若本地显式设置 AI_MATE_BIN 或 NOMIFUN_BUNDLED_BIN_DIR，须对照最终产物清单确认实际路径；环境覆盖优先级可能高于默认开发文件。文件哈希一致仅证明交接产物一致，不能替代运行实例或声音签核。

### 9.1 末轮结果与默认开发产物

- ai-mate 关键回归：full_voice_flow 20 通过、2 项显式外部探针忽略；voice_confirmation 4 通过；voice_session 14 通过。包括实际 sink drain 前非 played、started 状态、TTS 失败文字保留及同 attempt 精确重试不重播。证据 daemon-critical-final.log。
- 共享 DTO / 决策解析 6 通过，含 JSON 围栏和无效字段；路由/播放库 20 通过、1 项在线探针忽略。证据 shared-contract-final.log、terminal-contract-final.log。
- 已核对无 Flowy / ai-mate 运行进程，备份旧文件后更新 target/debug/Flowy.exe、target/debug/ai-mate.exe、apps/desktop/resources/bin/ai-mate.exe；三份均匹配本轮 Cargo 实际产物。宿主 SHA256=9dbf20d64b86bca7bd0a49425b143be8a0a556ef143c7a1e0081a69943f1712d；两处 daemon SHA256=b9d0131777bfe7b42c6777d7673f009269606321f22beefe1f67c6946e5a56cf。未启动桌面应用、录音或播报。
- 旧产物保存在 target/voice-review/preserved-2026-10-17/manifest.json 所列位置。未覆盖此前 T5 历史清单，未提交、推送或发布；HEAD 仍为 50f8ecb86070a9807ed57704ace070cc6748422e。

完整命令、日志哈希、20 个关键源码身份、正式 feature 图判定和产物/备份清单见 [review-repair-2026-10-17.json](fast-front-evidence/review-repair-2026-10-17.json)。i18n/主题证据已固定为 i18n-theme-final.log，非语音 CSS 失败详情为 dead-css-final.log。第 8 节是下一轮人工实测入口；自动测试通过不等于 Owner 或完整产品发布签核。
