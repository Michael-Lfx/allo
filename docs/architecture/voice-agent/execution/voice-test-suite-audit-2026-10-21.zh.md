# 语音测试集 · 有效性与退役建议

> **状态**：📋 审计建议，2026-10-21；未删除、修改或跳过任何测试。
> **代码范围**：`feat/xiaozhi-voice` / HEAD `5800811c7e3fb76e43b32afdda9916357f70d900` 加当前未提交改动。R1.2 修复正在工作区演进，本报告不重新签核业务进度。
> **目标**：区分保留、更新、迁移后退役及可直接精简的用例；减少无效维护，保留真实回归保护。不以用例数量或全绿作为覆盖充分的证明。
> **依据**：[v0.4 PRD](../PRD-voice-companion-v0.4.zh.md)、[R1–R5 执行辅导](R1-R5-execution-coaching-2026-10-21.zh.md)、当前代码调用方、测试正文和本次定向运行。

## 1. 结论与主要问题

现有测试仍然有用，尤其是声学输入、播放 drain、停止/取消、真实工具副作用、回执幂等与 UI 状态契约。问题主要集中在三类：

1. **旧路径仍有大量绿测，生产接缝覆盖不足。** `voice_tasks` 旧 submit/非 queued 分支仍被许多测试使用；它们不能直接证明新的 queued/同 Conversation 路径。
2. **指标名称超过实际测试能力。** 固定 JSON 被计算为“模型准确率”，手工延迟样本被计算为“快链路达标”，测试进程 RSS 被称为 daemon 待机资源验收。
3. **部分测试绑定文档或源码写法。** 检查历史 bsk 文件存在、字符串/固定数值出现，不能证明用户操作或异步生命周期。

**Standards 结论：** 有可精简的重复与低价值断言；公共/持久化兼容与仍有调用方的路径不能只因旧而移除。

**Spec 结论：** 新版需求需要迁移旧测试入口并增加纵向断言；删除失真指标的测试不表示删除 PRD 指标，原指标保持“未获得真实测量”状态。

## 2. 保留、更新与删除候选

### 2.1 优先保留

| 测试/入口 | 仍然保护什么 | 维护方式 |
| --- | --- | --- |
| [standard_audio_replay](../../../../apps/voice-companion/tests/standard_audio_replay.rs) | 真实 KWS/ASR、低音量、连续唤醒接命令、首字保留和非唤醒负例 | 保留显式资源门禁；记录 fixture/version/hash。缺 fixture 时目前可能调用 Edge 生成，不能默认当纯离线用例 |
| [voice_session](../../../../apps/voice-companion/tests/voice_session.rs) | 真实 worker 的采集故障、旧窗口隔离、结果文字保留、真实接单依据、播放 drain | 保留；对新增 queued 状态补覆盖，不因使用替身删除 |
| [voice_streaming](../../../../apps/voice-companion/tests/voice_streaming.rs) | 首句先播放、停止后丢弃晚到内容、模型/合成半途失败、静音、最后 drain 后追问 | 保留。它测试的是文本补全流与分句播放，不是本期未做的流式 ASR |
| [voice_confirmation](../../../../apps/voice-companion/tests/voice_confirmation.rs) | 实际提示消费后才听确认、授权期限、静默不延长、停止不能授权 | 保留；与 runtime 实际副作用测试分层互补 |
| [full_voice_flow](../../../../apps/voice-companion/tests/full_voice_flow.rs) | TLS 不 panic、音频解码/播放、模型/TTS 失败保留文字、生产 worker 串联 | 保留；联网用例显式运行，不能把无物理输出写成扬声器通过 |
| [voice_task_runtime](../../../../crates/backend/nomifun-app/src/voice_task_runtime.rs) | 真实 Conversation/runtime 的随机 Read、实际删除/危险命令审批、取消、结果回传 | 保留并增加多任务/同时间线断言；替身模型 + 真实工具的证据边界写清 |
| [Conversation service_test](../../../../crates/backend/nomifun-conversation/src/service_test.rs) 和 SQLite 测试 | owner/消息/准入/回执身份、跨连接竞争、持久化和旧记录兼容 | 保留。643 项包含普通会话回归，不能按“语音不需要”批量删除 |
| [voice_dialogue_integration](../../../../crates/backend/nomifun-app/tests/voice_dialogue_integration.rs) | 原文落盘、生成至多一次、丢投影响应、工作区/代际、上下文 | 保留保护意图；选择目标、时间线相关断言随新版可信绑定更新 |
| [voiceCompanion adapter tests](../../../../ui/src/common/adapter/voiceCompanion.test.ts) | instance/seq 竞态、订阅先于快照、停止/取消分开、旧作用域冲突、接口凭据 | 保留；这些直接对应曾经出现的实测问题 |
| VoiceConversation/VoiceTaskStatus/VoiceFeedbackSettings、locale 测试 | 双语文案、失败后文字、阶段/授权显示 | 保留；SSR 验证显示，交互另用现有行为工具验证，不能称 SSR 已证明按钮效果 |
| [prepare-voice-companion tests](../../../../scripts/prepare-voice-companion.test.mjs) | 使用 Cargo 真正输出、阻止 skeleton 打包、资源与许可文件接线 | 保留；生产/测试 feature 隔离仍然必要 |

替身适合稳定验证超时、乱序、错误和调用次数；它不能单独证明外部模型准确率、真实音频设备、真实网络或实际工具权限有效。

### 2.2 可以先精简的历史文档断言

[VoiceSettings.structure.test.ts](../../../../ui/src/renderer/pages/settings/VoiceSettings.structure.test.ts) 中以下四项只验证历史验收脚本文本存在及包含关键词，没有执行页面或脚本，可以从功能测试集中删除：

| 当前位置 | 用例 |
| --- | --- |
| [第 80 行](../../../../ui/src/renderer/pages/settings/VoiceSettings.structure.test.ts#L80) | `bsk-0-01 smoke test script is properly authored and archived` |
| [第 117 行](../../../../ui/src/renderer/pages/settings/VoiceSettings.structure.test.ts#L117) | `T1-09: bsk-1-01 verification script is properly authored and archived` |
| [第 151 行](../../../../ui/src/renderer/pages/settings/VoiceSettings.structure.test.ts#L151) | `T2-07: bsk-2-01 verification script is properly authored and archived` |
| [第 170 行](../../../../ui/src/renderer/pages/settings/VoiceSettings.structure.test.ts#L170) | `T2-08: bsk-2-02 verification script is properly authored and archived` |

删除这些测试不删除历史文档。文档链接如需校验，归普通文档链接检查。相同文件中的主题/i18n、布局红线、路由与接口接线检查应逐项评估，不能顺便整文件删除。单纯 `VoiceSettings.tsx exists` 也可在已有构建/导入检查覆盖后合并。

### 2.3 需要删除失真断言，或合并成正确单元测试

| 测试 | 实际内容 | 建议 |
| --- | --- | --- |
| [VT-2-01 schema_parse_rate:22](../../../../crates/backend/nomifun-voice-terminal/tests/vt_2.rs#L22) | 四段固定合法 JSON 重复 200 次，得到解析率 ≥99.5% | 合并成四种意图的参数化解析测试，保留非法/缺字段用例；删除重复计数与“模型解析率达标”结论 |
| [VT-2-02 intent_accuracy:55](../../../../crates/backend/nomifun-voice-terminal/tests/vt_2.rs#L55) | `_text` 根本未输入 router；直接解析预写 mock_json，与预写标签比较 | 删除准确率/混淆矩阵验收断言；有价值的 JSON 样例并入解析用例。真实模型语料评估另列显式任务，不新增生产替身 |
| [VT-2-05 latency_distribution:178](../../../../crates/backend/nomifun-voice-terminal/tests/vt_2.rs#L178) | 人工构造 700–1220ms 样本，验证样本落在 850/1250ms 指标内 | 若保留 tracker，改为空集/排序/边界/确切分位数的数学单测；删除产品延迟达标推论。当前未找到该 tracker 的生产采样调用方，若确认退役则和模块一起删 |
| [VT-4-01 standby_resource:99](../../../../apps/voice-companion/tests/vt_4_01.rs#L99) | 测量测试 harness 自身 RSS，没有启动 ai-mate 声学/模型/服务 | 移除 daemon 待机资源门禁断言；迁移成受控子进程的真实资源测量，独立显式运行 |
| [VT-4-04 memory_leak_check:79](../../../../apps/voice-companion/tests/vt_4_04.rs#L79) | 重复检测网卡，再比较测试进程内存 | 可保留具体枚举函数稳定性单测，移除“语音系统无泄漏/挂机通过”含义；实际 daemon 长时间测试另行留证 |

阈值仍保留在产品/性能要求中。不能为了减少失败而降低阈值，也不能把真实基准数据改成上述合成数据。

### 2.4 先迁移，再随废弃路径退役

| 旧入口或用例 | 事实 | 执行建议 |
| --- | --- | --- |
| [voice_tasks::submit:579](../../../../crates/backend/nomifun-app/src/voice_tasks.rs#L579)、submit_frozen、非 queued 分支 | 当前生产 manager 调 queued；非 queued 包装的引用主要是自身和测试。很多恢复/取消/持久化用例仍走旧入口 | 将幂等、写失败、响应丢失、取消等保护迁到生产 queued 接缝；补 append/原文/每 key receipt 的断言。确认无剩余生产消费者后，同一补丁删除旧非公开分支和专属测试，不留测试专用旧生产行为 |
| [duplicate_identity…second_active…:1070](../../../../crates/backend/nomifun-app/src/voice_tasks.rs#L1070) | 将“同 ID 改目标拒绝”与“第二活动任务必拒绝”绑在一起 | 前者保留；后者更新为 B queued，容量满才拒绝。拆开测试意图，不整条删除幂等保护 |
| [continuous_session…rotates…:1408](../../../../crates/backend/nomifun-app/src/voice_tasks.rs#L1408) | 通过旧 submit 检查 `voice-create:{acoustic_session}:*`，按回执观察时间模拟 31 分钟轮换 | 旧创建键/声学 session 假设与工作区语音目标不一致，属于退役候选。先补同 canonical workspace 跨唤醒复用、不同 workspace 隔离；30 分钟轮换口径尚有文档冲突，锁定后测最后有效用户交互，不按轮询回执续期 |
| [terminal/tests/vt_3.rs](../../../../crates/backend/nomifun-voice-terminal/tests/vt_3.rs) | 6 项使用本地 TaskManager/ConfirmationManager，手动 start/cancel/revoke；未连接生产任务 ledger、runtime 或唯一审批管理器 | 将任务隔离、取消、审批一次性/过期的行为要求迁到真实工具/worker 用例；确认旧模块无生产或公共消费者后连同旧模块退役。声学状态迁移仍可保留为 driver 单测 |
| [ai-agent/tests/vt_3.rs](../../../../crates/backend/nomifun-ai-agent/tests/vt_3.rs) | 包含独立 ToolPolicyEngine 的 token/suspended-call 和 MockQueue；真实执行侧使用 VoiceInvocationPolicy + ToolApprovalManager | 不能整文件删除：生产仍复用 ToolPolicyTable、命令检查与分类规则。只把独立审批/队列模拟的专属测试列为迁移后退役；表快照/危险调用策略覆盖保留，生产实际拒绝测试补齐 |

仓库搜索没有发现 terminal TaskManager/ConfirmationManager/FastPathLatencyTracker 的当前生产装配调用，主要在定义与 VT 中；但它们由 crate 公开导出。删除模块前确认外部 Rust 使用/兼容要求，不把“仓库内没引用”当作已证明无任何外部消费者。

旧消息/单条 ledger/旧回执读取测试继续保留：旧数据兼容与保留旧发送实现是两件事。新任务统一生产路径后，历史记录仍应只读可核对，不应重新执行。

## 3. 不该因为旧或失败而删除的测试

- `voice-skeleton` 当前是显式回放/测试能力开关；`voice_session`、`voice_streaming`、`voice_confirmation`、`standard_audio_replay` 的关键用例受它控制。名字含 skeleton 不构成退役证据。默认 Cargo 命令可能少跑这些文件，测试清单应记录 feature 和 ignored 数。
- VT-2 的 follow-up 用例没有实际推进 4 秒，只是手动发 FollowUpTimeout；应按实际行为命名。R3 改连续 180 秒后更新 worker/时钟测试，不能仅改注释数字，也不能提前删掉现行状态迁移保护。
- VT-0 的 JSON/schema、非法状态转换仍有用。“恰好 20 条 transition”这类实现数量断言可在状态机改造时更新成明确迁移契约，不整套丢弃。
- OOBE 配对、服务描述符、配置 golden、watchdog 与服务器权限仍有独立/headless 入口，不能只因当前先交付 Windows 桌面就删除。三端描述符文本通过不等于三端安装通过。
- UI 双语/theme/adapter 行为、授权和路径越界、取消、幂等、普通 Conversation 回归继续保留。失败应先判定环境/基线/本片回归，再判定需求是否已改变；不能以失败本身作为过时证据。

## 4. 给执行 Agent 的清理顺序

1. **建立覆盖映射**：每条候选记录“当前需求 → 生产入口 → 测试入口 → 断言/替身边界”。先锁定 R1.2 代码快照，避免清理与业务改造互相覆盖。
2. **先做低风险精简**：删除四条历史 bsk 文档自检；合并重复解析循环，移除失真的准确率/延迟门禁措辞。历史文档不动，PRD 指标不改。
3. **迁移旧任务回归**：保留业务断言，改走生产 queued 接缝和真实 SQLite；至少覆盖同时间线原文一次、A/B/C FIFO、容量、冻结、恢复默认暂停、真实工具审批/取消及晚到事件。
4. **证明替代覆盖后退役**：删除无调用方的旧非公开发送分支及其专属假设；终端旧任务/确认模块涉及公开导出，先确认兼容边界。不要新增兼容层来永久保留两套内部发送逻辑。
5. **检查未失去覆盖**：清理前后按场景比对，不以测试总数保持不变为目标。不能通过 `--lib`、改 glob、加 ignore 来隐藏仍有效但失败的 integration 用例。
6. **单独复审与交接**：把低价值测试清理、旧路径退役、真实性能测量分开记录，便于复核。此建议不授权提交/推送，也不包含业务修复实施。

## 5. 分层运行建议与本次验证

建议将运行清单按证据分类，暂不增加新的通用测试框架：

| 层 | 用途 | 运行策略 |
| --- | --- | --- |
| 单元/契约 | JSON、状态、队列规则、摘要、错误分类 | 改动后快速运行，替身确定性，避免重复大矩阵 |
| 持久化/生产集成 | SQLite 竞争、ledger 中断、真实 Read/审批、worker/API/回执 | 当前切片必须；核对实际调用路径与副作用 |
| UI | adapter 乱序/作用域、双语显示与用户操作 | 改 UI/DTO 时运行；SSR 不替代交互 |
| 已安装声学/联网 | KWS/ASR 标准音频、Edge/provider、CPU/RTF | 显式选择，记录依赖与环境；ignored 不计通过 |
| 联合产物/真机 | feature 图、真实 executable、麦克风/扬声器/安装/挂机 | 集成节点与 Owner 分别签核 |

本次实际执行：

| 命令 | 结果 | 证据 |
| --- | --- | --- |
| `cargo test --locked -p nomifun-voice-terminal --features voice-skeleton --test vt_2 --test vt_3` | VT-2 7 通过、VT-3 6 通过；不表示模型准确率或延迟达标 | `target/voice-review/test-audit-vt23.log` |
| `bun test --cwd ui src/renderer/pages/settings/VoiceSettings.structure.test.ts src/renderer/pages/voiceSession/VoiceTaskStatus.test.tsx` | 正常宿主权限下 21 通过；其中 structure 17、双语 SSR 4 | `target/voice-review/test-audit-ui-host.log` |
| 同一 UI 命令首次沙箱运行 | 17 pass + 1 fail/1 error，含 EPERM/EBADFD 和无法解析 react-i18next；正常权限重跑消除，不作为删除依据 | `target/voice-review/test-audit-ui.log` |

没有运行完整 workspace、全部声学/网络/真机测试，也没有在本轮改测试或业务代码。对 retired 模块的结论来自静态引用和测试正文，不等于已完成公开 API 删除审核。

后续迁移时的关键命令示例：

```powershell
cargo test --locked -p nomifun-app --lib voice_tasks
cargo test --locked -p nomifun-app --lib voice_task_runtime
cargo test --locked -p nomifun-conversation
cargo test --locked -p nomifun-companion --lib collector
cargo test --locked -p voice-companion --features voice-skeleton --test voice_session --test voice_streaming --test voice_confirmation
bun test --cwd ui src/common/adapter/voiceCompanion.test.ts src/renderer/pages/voiceSession
bun test scripts/prepare-voice-companion.test.mjs
cargo tree --locked -p voice-companion --edges normal,build,features
git diff --check
```

资源回放要先 `--list` 查看准确模块名，再按需 `--ignored` 单独运行；不全量强跑联网/设备用例。生产 feature 图不得含 skeleton。测试数量减少后仍应保留上述每类必要场景，未验证的产品指标继续列为待验收。

## 6. 第一阶段清理落地记录 (2026-10-21)

按照本方案 §4 清理顺序的步骤 1–2（低风险精简与指标失真重构），本阶段已完成以下用例精简与重构（净减 59 行代码），所有相关门禁已通过：

1. **已删除 4 条历史文档自检断言**（`VoiceSettings.structure.test.ts`）：
   - 移除了 `bsk-0-01`、`T1-09: bsk-1-01`、`T2-07: bsk-2-01`、`T2-08: bsk-2-02` 仅检查 markdown 文件存在的测试，保留其余 13 条真正的 UI / SSE / 动画结构测试。
   - 验证：`bun test --cwd ui src/renderer/pages/settings/VoiceSettings.structure.test.ts src/renderer/pages/voiceSession/VoiceTaskStatus.test.tsx`（17 pass，0 fail）。
2. **重构 VT-2 失真指标为真实单元测试**（`vt_2.rs`）：
   - `test_vt_2_01_schema_parse_variants_and_validation`：覆盖 4 种合法意图结构与 3 种非法/缺字段校验，移除 200 循环与伪解析率断言。
   - `test_vt_2_02_parse_decision_payloads`：校验意图载荷准确反序列化为相应 `IntentType`，移除误导性的模型准确率断言。
   - `test_vt_2_05_fast_path_latency_tracker_percentiles`：覆盖空集、单点及乱序倒序数据下的 P50/P95/P99 计算数学正确性，移除伪造延迟样本与性能达标推论。
   - 验证：`cargo test --locked -p nomifun-voice-terminal --features voice-skeleton --test vt_2 --test vt_3`（VT-2 7 passed, VT-3 6 passed）。
3. **清理 VT-4 测试进程内存验收断言**（`vt_4_01.rs`, `vt_4_04.rs`）：
   - 移除测试 harness 自身 RSS < 250MB 的假待机验收断言（`vt_4_01.rs`）。
   - 将 `vt_4_04.rs` 中的 `test_vt_4_04_ci_sampling_stability_and_memory_leak_check` 重命名为 `test_vt_4_04_detect_ethernet_status_loop_stability`，仅保留网卡循环检测函数稳定性，移除“语音系统无泄漏”断言。
   - 验证：`cargo test --locked -p voice-companion --test vt_4_01 --test vt_4_04`（7 passed, 0 failed）。
4. **解耦 `voice_tasks` 幂等与排队测试意图**（`voice_tasks.rs`）：
   - 拆分出 `duplicate_command_identity_cannot_change_goal`（幂等）与 `second_active_task_is_enqueued_and_queue_capacity_is_enforced`（排队及容量）。
   - 修复测试替身 `Runtime::send_bound` 的回执观察同步：确保 `send_bound` 将带有真实 `message_id` 的回执同步写入 `*self.receipt`，彻底杜绝后续 `observe` 产生 `task_receipt_scope_changed`。
   - 调优 `bv_11_deadline_expiration_and_clock_rollback` 超时基准（设为 1200ms），在并发和慢 I/O 下稳定通过。
   - 验证：`cargo test --locked -p nomifun-app --lib voice_tasks`（28 passed, 0 failed）。

