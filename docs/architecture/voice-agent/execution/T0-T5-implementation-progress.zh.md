# T0–T5 串行实施记录

> **状态**：进行中，2026-10-15；依据 [纵向闭环与接口方案](runtime-vertical-contract-plan.zh.md)。
> **固定目标**：A、打通真实语音任务、控制与确认；B、保持模块解耦并落实接口；C、自动验证、联合产物及 Owner 实测交接。

## 当前门禁

| 切片 | 自动验证 | 真机与 Owner | 下一入口 |
| --- | --- | --- | --- |
| T0 基线 | 自动测试与受影响生产编译通过；全仓基线限制见下文 | 不以此代替真机验收 | T1：真实只读任务生产接线 |
| T1 真实只读任务 | 自动门禁通过，详见最终宿主、真实声学及生产编译记录 | 未验收 | 进入 T2；联合默认产物在 T5 交接 |
| T2 控制与恢复 | 自动门禁通过；记录见本页 T2 出口 | 未验收 | T3：实际工具确认 |
| T3 实际确认 | 自动门禁通过，见 T3 出口 | 未验收 | T4 离屏体验 |
| T4 离屏体验 | 自动门禁通过，见 T4 出口 | 未验收，设备拔插与听感由 Owner 执行 | 进入 T5 |
| T5 构建与交接 | 自动门禁与产物核对通过；默认开发程序已更新 | 未验收，待 Owner 实机验证 | 交付实测与验收表 |

## T0：启动与环境证据

- 当前分支 `feat/xiaozhi-voice`，领先 `origin/main` 40 个提交。全部已有未提交工作保留，不创建并行 Agent、不提交或推送。
- 原命令遇到 SQLx 宏读取既有迁移目录的系统权限错误，记录为环境阻断：[Rust 首次记录](T0-conversation-red.log)。同命令使用环境权限重试，不修改迁移、权限策略或依赖配置：[重试记录](T0-conversation-red-permission-retry.log)。
- 首次 Bun 测试遇到父目录 EPERM，已有依赖解析失败：[首次记录](T0-ui-baseline.log)。同命令环境权限重试后，34 项语音/审批前端测试通过、152 个断言：[通过记录](T0-ui-baseline-permission-retry.log)。没有安装或新增依赖。
- 已确认新增 Conversation 测试的 `update` 调用缺少 runtime registry 参数；先取得编译失败记录，再修正调用。基线类型检查与 Rust 编译结果结束后追加。

本记录不表示 T0 或完整产品已经通过。尚未替换运行产物、启动麦克风/扬声器或运行 Owner 安装验收。

## T0：Conversation 与审批基线

- 已修正测试的 registry 参数，并在临时 SQLite 夹具中创建其引用的 provider/model；保留生产模型存在性校验，没有删除断言。原参数编译失败与夹具缺失行为失败分别见 [编译失败](T0-conversation-red-permission-retry.log)、[夹具失败](T0-conversation-final.log)。[最终定向测试](T0-conversation-fixture-green.log) 1 项通过，证明创建幂等、冻结工作区、关闭委派及公开字段防篡改。
- [类型检查基线](T0-typecheck-baseline.log) 仍有 60 项既有非语音错误，无本次语音/审批文件命中；不将聚合检查标为通过。
- [Rust 回归首轮](T0-rust-regression.log) 中 `nomi-protocol` 40 项通过，随后在实际工具拦截测试的编译阶段遇到磁盘空间不足，尚未执行后续测试。
- D 盘当时剩余约 356 KB。验证 `build.noindex`、`target` 绝对位置属于本工作区后，使用现有 `bun run build:gc` 回收约 64.3 GB，恢复约 62 GB 可用空间。[清理记录](T0-build-cache-gc.log) 同时保留 cargo-sweep 的超时警告；没有使用 `build:clean`。清理后先检查不存在残留 cargo/cargo-sweep/rustc，再启动[串行重试](T0-rust-regression-after-gc.log)。
- worker 的受控输入替身仅由测试命令显式开启 `voice-skeleton`；后续生产编译仍须关闭该 feature，并单独检查完整桌面依赖图。

## T0：真实工具拦截与未知回执修复

- [清理后回归](T0-rust-regression-after-gc.log) 中，agent 宿主约束 11 项通过，语音策略 3 项通过；后者实际执行测试临时目录中的 ApplyPatch 删除，证明同意/拒绝及工作区外拒绝不是只验证枚举或提示文本。
- 同一轮在宿主生产交付模块复现：`completed=true` 且 `result_ok=None` 的回执被报为 `Completed`。新增严格回归要求 `Unknown` 和 `task_result_unverified`，保留一次发送断言。
- 修复只在明确 `Some(true)` 时报告完成、`Some(false)` 时报告失败，结果缺失保留未知并继续只读观察。[修复后宿主回归](T0-task-outcome-green-and-worker.log) 19 项通过、1 项显式真实云探针忽略；worker/播放回归仍待该日志的后续结果。
- 仓库现有 `rustfmt.toml` 明确设置 `disable_all_formatting=true`；不擅自改写该策略或执行全仓格式重写。后续检查须说明这一配置限制。

## T0：接单呈现修复与出口

- 补强既有生产 worker 的未接单测试，同时提供 `spoken_ack` 和 `reply_tts`；原行为没有播音，却在助手正文里显示“已接单”，见[失败记录](T0-unaccepted-text-red.log)。只有真实聊天决策才允许直接发布 `reply_tts`；任务与控制决策留待真实回执。完整模型原始输出没有删除。
- [最终 worker 回归](T0-unaccepted-text-green.log)：库 6、完整流程 20、会话 10 项通过；另有 2 项显式在线测试忽略。包含任务未接通不报接单、停止/追问/丢帧/输出消费等既有场景。[语音库回归](T0-task-outcome-green-and-worker.log) 15 项通过，1 项在线测试忽略。
- [受影响生产编译](T0-production-check.log)通过，4m10s；最终 daemon 呈现修改后的[生产复查](T0-final-daemon-check.log)通过，1m39s。[生产 feature 图](T0-production-feature-graph.log)不含 `voice-skeleton`。
- 全仓 `cargo fmt --all -- --check` 因 Windows 命令行长度限制失败（OS 206），见[完整记录](T0-format-check.log)；[分包检查](T0-focused-format-check.log)退出 0。由于仓库禁用格式化，不能将其解释成完整格式覆盖。`git diff --check` 对本轮跟踪修改通过。
- 当前可进入 T1 的自动基线：Conversation 1、协议 40、宿主约束 11、语音策略 3、宿主语音 19、worker/播放 51 项通过；前端 34 项通过。类型检查既有 60 项非语音失败仍未修复，未宣称聚合门禁全绿。
- T0 仅修复执行基线和真实反馈；生产任务入口仍明确返回 `runtime_unavailable`，T1–T5 未完成。没有更新默认程序、启动设备、运行真实用户云模型、提交或推送。

## T1：执行配置读取入口

- 首个生产 API 测试复现 `/api/voice/companion/execution` 返回 404，见[失败记录](T1-execution-settings-red.log)。
- 新增读取契约 `VoiceExecutionSettings`：请求配置、已验证配置、实际可用性、原因码。只读现有 JSON 的 `execution` 字段，损坏或读取失败保留明确原因，不初始化设备或任务 runtime，不写默认配置。
- 未配置时返回 `execution_unconfigured`；已有配置但没有装配任务适配层时返回 `runtime_unavailable`。不能因为已有模型引用就报告任务已接通。
- 该入口的[修复验证](T1-execution-settings-green.log)结束后追加结果；提交接口、真实 runtime、任务投影和 UI 尚未接线，T1 不标完成。

## T1：宿主、任务交付和真实读取

- 配置 GET/POST、任务 POST、按 `command_id` 读取任务已进入宿主生产路由。沿用显式工作区和 provider/model 引用，可选择既有 Conversation 预设；无效预设不再显示可用，见[失败](T1-unresolved-preset-red.log)和[修复](T1-unresolved-preset-green.log)。配置保存保留其他配置字段，不复制 API key。
- 生产组装复用现有 Conversation service、runtime registry 和 preset service；独立补全接口与任务适配接口分开。快轨聊天不依赖任务适配器初始化；配置和空任务状态读取不启动任务推理。
- 生产执行策略目前只允许既有工作区只读工具。写入、删除、命令和未知工具在审批尚未接通时直接拒绝；[实际策略回归](T1-read-only-policy-green.log)通过。现有真实 ApplyPatch 审批测试显式使用测试的已接通审批模式，未据此开放生产权限。
- worker 的[失败回放](T1-worker-delivery-red.log)实际观察到交付次数为 0；修复后每轮稳定命令身份交付一次，只有新且匹配的真实接受回执才能播报固定接单提示。未知回执、旧回执及模型接单文案均不能冒充接单。[worker 回归](T1-worker-delivery-green.log)：库 6、完整流程 20、会话 11 项通过，另有 2 项在线测试忽略。
- daemon 与宿主复用同一个临时凭据保护的回环桥。提交前读取 daemon 当前快照，校验进程、会话及声学轮次后转换成宿主作用域；不依赖固定延迟或重新发送唤醒事件。凭据通过环境传递，不进入命令行或前端状态。
- 真实读取场景先创建随机文件内容，使用生产宿主路由、真实 Conversation、真实 Nomi runtime 和实际 `Read` 工具，再核对原回执及返回文本。只控制外部模型服务，不替换 runtime 或工具。
- 联合回放首次在“真实读取已完成，但结果没有返回 daemon”处[超时失败](T1-joint-worker-fixture-red.log)。现已接通宿主结果通知、daemon 播报和播放完成后的追问。通知发送前持久化占位，明确忙碌拒绝才释放；网络结果不明时保留占位与文字结果，不自动重复播报。
- [生产装配回归](T1-production-wiring-regression.log)：24 项通过、1 项显式云端探针未运行。其中联合场景固定 PCM 进入生产 worker，使用受控 ASR、受控云端模型和合成 WAV 输出；真实任务读取随机内容，该内容同时进入结果文字和 TTS 输入，输出帧验证为有声 WAV。没有打开麦克风或物理扬声器，不能替代真人/安装验收。
- 任务结果在宿主单独投影；声学快照不会清除它。面板和独立窗口均呈现任务状态、结果及完整任务入口；关闭语音后设置页仍保留任务投影。过时任务投影竞态正在增加回归，T1 出口尚未签核。

## T1：界面与验证边界

- 执行设置复用现有设置行、模型目录、预设列表和目录选择桥。无效保存保留草稿；只保存工作区、模型及预设引用。新增文字提供中英文，使用现有主题 token。
- [前端回归](T1-ui-green.log)：35 项通过、167 个断言，包含真实任务结果在声学结束后仍可呈现、未知/失败状态不冒充完成。后续错误提示调整仍需最终复查。
- i18n、主题及按钮布局检查通过，见 `T1-i18n-check.log`、`T1-theme-check.log`、`T1-button-layout-check.log`。类型检查仍有基线中的 60 项非语音错误，新增语音文件无类型错误，见 `T1-typecheck-direct.log`。
- `check:dead-css` 仍被其他页面及旧基线清理要求阻断，见 `T1-dead-css-check.log`；没有报告新增语音组件问题。未修改这些无关页面或降低门禁。
- 用户明确允许仅在 `nomifun-app` 测试依赖引用仓库现有 `ai-mate`；该引用用于联合回放，不进入生产依赖。生产 feature 图与编译仍需最终核对。
- 曾遇自动审批服务额度导致命令未执行，随后正常权限重试恢复；沙箱内 `include_dir!` 读取既有内置 skill 的 LICENSE 仍会出现 OS 5。未调整安全策略、迁移或内置资源来规避。
- 默认开发产物尚未替换，未启动应用、未调用用户真实云模型、未实机播音；T2–T5 尚未进入实施。

## T1：自动出口与 T2 入口

- [最终宿主回归](T1-final-host-regression.log)：24 项通过、2 项显式探针默认忽略；随后显式运行已安装声学模型的联合场景，1 项通过。回放没有调用外部付费模型或物理音频设备。
- 真实声学场景复用缓存 `wake-and-command-16k.wav`，验证已安装 KWS、VAD、SenseVoice 均加载、实际 ASR 含“天气”、识别原文保留。受控模型把该输入交付到临时工作区读取场景，以验证接口链路，不据此声称真实模型意图准确率。
- 各联合场景断言 ASR 1 次、对话路由 1 次、创建身份 1 条、实际交付回执 1 条、真实工具读取 1 次；执行模型对工具请求及结果处理共 2 次。真实读取结果进入 TTS 输入，输出帧是有效有声 WAV。
- 延迟接单投影确实能覆盖已完成结果，见[失败](T1-stale-task-projection-red.log)；现发布前核对当前持久化记录，见[修复回归](T1-stale-task-projection-green.log)。配置读取失败也撤销已缓存的就绪状态。
- [最终 worker/播放回归](T1-final-worker-regression.log)：库 6、完整流程 20、会话 12、播放/路由库 15 项通过，另有 3 项在线探针未运行。新增结果通知验证本机凭据、旧进程/旧轮次拒绝、TTS 失败保留文字、重复通知只合成一次。
- [生产编译](T1-production-check.log)通过，2m59s；[普通依赖图](T1-production-feature-graph.log)没有 `voice-skeleton`。[最终前端回归](T1-ui-final.log)35 项通过；既有类型检查和 dead-css 阻断保持单独记录，不降低断言。
- T1 自动门禁完成，2026-10-15 进入 T2。接下来处理历史命令保留、非阻塞查询、实际取消、只读恢复及控制作用域；不提前开放危险工具，不提前更新默认开发产物。

## T2：交付历史、恢复与实际取消

- 命令记录改为同一文件中的历史账本，兼容读取先前单条格式；保存新任务不会抹掉旧命令的目标校验。重复命令先核对冻结目标，再只读查询；改变目标返回冲突，停止语音或改动默认模型不会触发重建或重发。[历史与非阻塞查询失败](T2-delivery-history-red.log)、[修复](T2-delivery-history-green.log)。
- 创建响应丢失后按原创建幂等身份只读查找 Conversation；不据此推断已发送，不自动补发。确定拒绝才报告失败，响应不明保持未知。[恢复失败](T2-recovery-outcome-red.log)、[修复](T2-recovery-outcome-green.log)：11 项通过。写入失败仍禁止创建和发送。
- 查询、取消已接入宿主公开 API 和凭据保护的 daemon 桥。取消请求绑定当前宿主实例、命令、专属 Conversation 和持久化消息身份；旧实例、错误任务引用返回冲突。取消请求成功不等于完成，投影保持“取消中”直到真实终态。[API 回归](T2-control-api-green.log)：28 项通过、2 项显式探针未运行（阶段记录，后续改动还需最终复查）。
- 实际运行中取消最初返回 409：[失败](T2-real-running-cancel.log)。原因是持久化用户消息 ID 与 runtime 根轮次 ID 混用；现在先核对原消息回执，再通过既有 operation 身份在同一运行状态锁中停止，复用原清理流程。[真实取消通过](T2-real-running-cancel-green.log)：任务进入真实非成功终态，随后释放模型响应也没有发生文件读取。
- [完整语音控制联合回放](T2-voice-controls.log)通过：固定音频进入生产 worker；接受真实本地任务后停止语音不取消任务，随后重新唤醒聊天、查询、语音取消。断言识别和短对话各 4 次、创建与发送各 1 次、取消前未发生 Read，模型自称接单或取消的文案不能代替真实回执。外部模型与声学输入受控，实际 Conversation、runtime、工具及取消入口没有替换。
- [面板回归](T2-ui-regression.log)：36 项通过、173 个断言。任务刷新和取消按钮使用实际 API，停止语音与取消任务分开；不乐观地伪造终态，旧请求失败不能污染新任务界面。
- 2026-10-15 用户澄清：云账号提供登录 token 和模型 key，所有文件、工作区与执行环境仍在本机。本实现不引入云端电脑或云端工作区，切换模型凭据不迁移、删除或取消已接受的本机任务；宿主用户隔离沿用既有所有者检查。云凭据刷新后的模型可用性须按实际配置报告。
- T2 尚未签核：继续修复保存后预设再次解析、历史结果通知以及最终回归。T3–T5 未开始，默认程序尚未替换，真人设备与安装验收仍交由 Owner。

## T2：冻结与历史通知修复

- [修复前行为回放](T2-freeze-and-notification-red.log)实际复现两项失败：创建时得到预设编辑后的指令，旧命令未播报标记被新任务遮掉。未通过更改预期或放宽断言规避。
- 命令首次副作用前保存既有 `ResolvedPresetSnapshot`；Conversation 的可信语音创建入口沿用同一个冻结快照，不再次查询预设目录，也不复制 provider key。直接修改真实 PresetService 的目录后创建，仍保留原指令和 revision。
- 结果通知从已有历史账本读取最早未认领终态，按 `command_id` 持久化去重；新任务不覆盖旧通知。播报前附上冻结目标，以区分历史结果与当前任务。查询和任务投影仍指向各自身份，不为此新增调度框架或数据库。
- 发送失败后先核对原交付回执：已有回执或回执查询失败时按不确定交付处理，避免把已接受任务误判为确定拒绝。取消消息身份缺失时不预先写入“取消中”。本地 wake 控制客户端也统一使用既有回环地址解析、无代理和禁止重定向。
- [最终宿主回归](T2-host-final-regression.log)：32 项通过、2 项显式探针未运行。[共享停止回归](T2-conversation-stop-regression.log)：18 项通过，包括正确 operation 停止、旧消息/用户拒绝、停止栅栏及终态回执修复。[可信创建回归](T2-conversation-creation-regression.log)：1 项通过。
- 新增面板测试最初使用当前类型声明未提供的 Bun matcher，类型检查产生两项语音错误；改用等价的标准字符串断言后，[最终类型检查](T2-typecheck-final.log)恢复为 60 项既有非语音错误，语音文件无错误。i18n 生成、i18n 和主题检查通过（`T2-i18n-generate.log`、`T2-i18n-check.log`、`T2-theme-check.log`）。
- 生产 worker/播放与受影响生产编译仍在最终检查；T2 不提前标记完成，默认开发产物尚未更新。

## T2：自动出口

- [worker/播放最终回归](T2-worker-final-regression.log)：库 6、完整流程 20、会话 12、播放/路由 15 项通过；3 项显式在线探针未运行。[最终 UI 回归](T2-ui-final.log)：36 项通过、173 个断言。
- [生产编译](T2-production-check.log)通过，3m36s；[普通/构建依赖图](T2-production-feature-graph.log)不含 `voice-skeleton`。受影响跟踪文件的 `git diff --check` 通过。全仓既有类型错误、格式配置及真机限制沿用前述记录，不据此宣称全仓检查全绿。
- 本轮一条测试命令遗漏隔离 target 参数，Cargo 将测试 daemon 写入默认输出。发现后核对应用已退出，使用既有 Q 生产回执核对缓存副本 SHA256，再恢复原生产 daemon，见 [恢复记录](T2-default-daemon-restoration.json)。宿主没有被替换；后续所有测试显式使用 `--target-dir target/voice-review`，联合生产产物仍留待 T5。
- T2 自动门禁通过，继续 T3；真人设备、安装与 Owner 验收保持未通过状态。

## T2：构建空间与默认产物保护

- [缓存回收](T2-build-cache-gc.log)使用原有 `build:gc`，已核对 `build.noindex`、`target` 绝对路径均在仓库内且不是重解析根，没有活动构建或应用。回收约 40.0 GB（111.7 → 71.6 GB），保留日志中的软预算警告，没有执行 `build:clean`。
- 回收前备份默认生产 exe/dll，回收后逐一核对原 SHA256；没有启动应用，默认宿主和恢复后的生产 daemon 保持原身份。后续验证只使用隔离 target。

## T3：实际工具审批（2026-10-16）

- [真实任务入口失败](T3-actual-delete-red.log)复现审批查询接口 404；[客户端失败](T3-approval-adapter-red.log)、[双语面板失败](T3-approval-panel-red.log)分别复现缺少审批请求和实际参数展示。现在提供计划中的确认查询和一次确认接口，面板展示实际工具、完整参数、仅本次同意和拒绝。
- 执行前结构化策略仍是既有 `ToolApprovalManager` 的约束，删除 T1 临时只读模式，保留路径边界和不支持工具拒绝。运行时自动允许、工具允许表以及 LLM 的 `requires_confirmation=false` 都不能跳过强制审批。执行侧最多保留 45 秒授权；口头听取仅在提示实际消费后开始，最多 8 秒并取剩余授权期限的较小值，不能延长授权。
- 确认 API 固定 `always_allow=false`，额外永久允许字段被请求契约拒绝。核对宿主、命令、专属 Conversation、持久化消息、实际调用消息、call 和 approval 身份；最终放行在现有运行状态线性化锁中再次核对原交付 operation，不能授权同一 Conversation 的新轮次。
- 第一次“审批成功但文件没删除”实际返回了工具的先读保护：[诊断](T3-approved-tool-diagnostic.log)。补真实 Read 后仍被拦截，进一步确认语音入口保存了 Windows `\\?\` 路径，而 Read 使用普通路径。现在复用项目已有 `canonicalize_simplified` 保存/传递语音工作区；保留先读与文件陈旧检查，没有改通用文件缓存或绕过工具保护。
- 写操作后的额外模型调用来自既有 office evidence nudge，测试对其请求内容作明确断言；不把它算成第二次语音意图路由。实际 Read、修改及任务交付身份分别计数，保留随机输入和副作用断言。
- [宿主阶段回归](T3-host-approval-regression.log)中 35 项通过，口头联合场景首次失败。实际快照显示 daemon 已 Idle，宿主缓存仍为 Dispatching；原通知代码在刷新前就依赖缓存 Idle，导致永远不询问新状态。[诊断](T3-oral-tool-diagnostic.log)留存了两侧状态。现以新读取的 daemon 快照判断声学是否空闲，不靠固定延迟补发。
- [修复后的真实口头工具联合场景](T3-oral-tool-integration-green.log)通过，含实际读取后挂起、口头同意、拒绝、静默三个子场景。确认答复没有调用 LLM；同意才删除，拒绝/静默保留文件，任务创建和交付仍各一次。
- [daemon 听取回归](T3-oral-confirmation-green.log)先通过明确/否定/含糊答复及授权期限场景；新增长提示、停止播报等场景仍在最终回归。只使用受控 PCM/ASR/TTS 与消费边界，不宣称真人麦克风和扬声器已通过。
- [前端阶段回归](T3-ui-regression.log)：39 项通过、189 个断言。审批失败使用独立提示；不能完整口述的目标要求在面板核对完整范围，不截断授权目标、不增加第二套 token 或审批存储。最终双语生成、质量门禁与生产编译仍待完成。
- T3 未签核，T4–T5 尚未进入；默认生产产物未更新到本轮代码，Owner 实测仍未运行。

## T3：自动出口

- [策略最终回归](T3-policy-final.log)：6 项通过，包括实际删除、越界拒绝及过期后迟到授权不能恢复删除。[Conversation 停止回归](T3-conversation-stop-final.log)18 项、[可信创建回归](T3-conversation-create-final.log)1 项通过。
- [worker 最终回归](T3-worker-final.log)：库 6、完整流程 20、语音确认 4、会话 12 项通过，2 项在线探针未运行。新增 9 秒长提示证明 AwaitingConfirmation 不消耗听取窗口；停止提示实际 flush 后不能授权，期限始终不超过执行侧期限。
- [宿主最终回归](T3-host-final.log)：36 项通过、2 项显式探针未运行。包含真实只读任务、实际删除、实际危险命令、口头同意/拒绝/静默、停止语音后聊天/查询/取消、历史恢复及错作用域。日志 `voice_acceptance_evidence` 逐场景记录原始身份、调用次数、回执数量与实际文件状态；物理设备字段明确为 false。
- [语音库及状态机](T3-terminal-final.log)：库 15、既有 VT-3 6 项通过。旧纯状态机/token 用例仅为回归，不能代替本轮真实工具审批证据。
- [生产编译](T3-production-check.log)通过，4m17s；[生产依赖图](T3-production-feature-graph.log)不含 `voice-skeleton`。[定向格式命令](T3-focused-format.log)退出 0，但仓库禁用格式化的限制继续保留。受影响跟踪文件 `git diff --check` 通过。
- [UI 回归](T3-ui-regression.log)39 项通过、189 个断言；[i18n](T3-i18n-final.log)、[主题](T3-theme-final.log)通过。[最终类型检查](T3-typecheck-final.log)仍为 60 项既有非语音错误，语音文件无命中。
- 当前[本机 runtime API 契约](voice-runtime-api-contract.zh.md)已落盘，保留旧 PRD/方案/计划原文。T3 自动门禁完成，进入 T4；安装、原生窗口、真实麦克风/扬声器和 Owner 签核仍未运行，默认生产程序保持原版本。

## T4：恢复工作区与讲话保护

- 2026-10-16 10:10，工作区被误存入 `stash@{0}` 后切换至 main，正在运行的追问测试因源码目录消失而中断。这次失败是工作区变化，不是追问业务失败。收到 Owner 明确恢复指令后，切回原 `feat/xiaozhi-voice`（`50f8ecb86070a9807ed57704ace070cc6748422e`），执行 `git stash apply`，保留原 stash 备份；没有合并 main、提交或推送。
- [讲话通知失败回放](T4-speaking-notification-red.log)复现 Idle 期间检测到讲话仍立即播报任务结果。宿主与 daemon 现在同时检查真实采集的讲话状态；忙碌拒绝不消耗结果通知身份，输入静默后仍可播报。[通过回放](T4-speaking-notification-green.log) 1 项通过。
- 任务结果追问、设备故障恢复和最终联合构建仍未通过；当前默认生产程序保持原版本，真人设备验收仍由 Owner 执行。

## T4：任务结果追问、设备恢复与自动出口

- [追问失败](T4-task-result-context-red.log)证明“解释刚才的结果”没有携带实际 runtime 结果；修复后，宿主通过私有结果通知携带冻结目标，daemon 将目标与真实结果组成一对短上下文，仍限制最多三轮。没有额外模型总结调用，也不重新执行任务。[联合回放](T4-task-result-context-green.log)通过：追问识别/对话各增一次，任务创建/交付/实际 Read 仍各一次。
- [重连文字失败](T4-device-reading-red.log)复现新进程空快照清掉既有回答。[修复](T4-device-reading-green.log)保留阅读文本和三轮阅读记录，但会话/轮次身份来自新 daemon；旧控制不能恢复权限，新唤醒开始后使用新会话内容。
- [设备恢复入口失败](T4-device-recovery-red.log)先记录缺少自动恢复和受控启动入口；[设备恢复回归](T4-device-recovery-green.log)三项通过。现复用宿主 SSE 循环与既有进程管理器，在采集关闭、持续无 PCM、扬声器不可用且声学空闲时按 2/4/8/16/30 秒退避重建 daemon；不重建任务 runtime，不重新交付任务。模型/TTS 网络错误不触发设备重启，讲话和播报期间暂缓；用户停止语音、禁用或宿主退出不能被恢复逻辑撤销。
- [宿主最终回归](T4-host-final.log)40 项通过、2 项显式探针未运行，包含新的结果追问、恢复保护及 T1–T3 真实工具/取消/审批回归。[worker 最终回归](T4-worker-final.log)：库 6、完整流程 20、确认 4、会话 13 项通过，2 项在线探针未运行。实际播放消费、停止 flush、长提示后才开始确认听取、TTS 失败文字保留和输入讲话期间通知推迟均有自动断言。
- 前端沙箱重现既有 EPERM 后同命令重试，[最终 UI 回归](T4-ui-final.log)43 项通过、200 个断言。故障文案明确文字保留及空闲重试；[双语](T4-i18n.log)、[主题](T4-theme.log)通过，[类型检查](T4-typecheck.log)仍为 60 项既有非语音错误，没有语音文件命中。没有降低断言。
- [生产编译](T4-production-check.log)通过，1m07s；本轮相关跟踪文件 `git diff --check` 通过。自动恢复的退避/栅栏和宿主投影已验证，但没有实际拔插硬件、播放扬声器或启动桌面应用；T4 真实设备/Owner 状态仍未验收。继续 T5 联合构建和交接。

## T5：联合构建、资源与真实探针

- [资源收集失败](T5-resource-red.log)记录缺少构建钩子；[资源测试](T5-resource-final.log)3 项、[安装目录解析](T5-resource-resolution-green.log)1 项通过。钩子按目标架构与 debug/release 构建无 skeleton 的 daemon，读取 Cargo 实际 executable 路径并收集既有 LICENSE；宿主复用桌面已有资源目录环境接口，不从安装包猜测开发源码位置。没有引入新依赖或替换进程监督边界。
- [UI 构建](T5-build-ui.log)通过；[联合生产构建](T5-joint-production-build.log)通过，11m19s，宿主启用 `tauri/custom-protocol` 嵌入 UI。[生产依赖图](T5-production-feature-graph.log)不含 `voice-skeleton`。[daemon --help](T5-daemon-help.log)退出成功，仅是加载探针，未启动采集或服务；[PE 导入](T5-daemon-native-imports.log)单独保留，不能代替干净机器安装。
- [workspace 编译](T5-workspace-check.log)通过，5m25s。`git diff --check` 通过；[定向格式命令](T5-format-focused.log)退出 0，但仓库 `disable_all_formatting=true`，不宣称已实际格式化。[全仓格式](T5-format-all.log)仍遇 Windows OS5/OS206，属于前述环境/配置限制。
- [聚合质量门禁](T5-quality-aggregate.log)仍被 60 项既有非语音类型错误阻断；[单独质量结果](T5-quality-details.json)记录动效、错误/支持面、按钮、图标、CSS 作用域、CodeMirror、进程/浏览器边界及 Windows 控制台检查通过。dead CSS 仍为五个既有非语音文件失败，没有扩大修复范围或收紧/放宽基线；脚本目录检查通过。
- 全仓构建后磁盘仅余约 1.4GB。确认缓存根位于仓库内、不是重解析点且无活动构建/应用后，先[备份并校验九份产物](T5-preserved-artifacts.json)，再使用原有 [GC](T5-build-cache-gc.log)回收约 45.8GB（118.2 → 72.4GB）。保留软预算警告，没有运行 `build:clean`；GC 后逐文件恢复/核对哈希，默认程序保持旧版本。
- [完整桌面图的真实探针](T5-real-acoustics-edge.log)2 项通过：真实 Edge 合成 71424 个 24kHz 样本，单次调用 2653ms；真实 KWS/VAD/SenseVoice、一次受控模型调用和真实 Edge 输出经过生产 worker 到达受控 sink，状态为 Idle → Listening → Transcribing → Deciding → Synthesizing → Speaking → FollowUp → Idle。没有麦克风或物理扬声器；单次时延不是 P95。新样本使用 T5 文件名，没有覆盖历史 Q 音频。
- [Owner 交接脚本与 AC-01–20 对照](T5-owner-handoff.zh.md)已落盘。Windows 未签名本地验收安装包 `target/voice-staging/debug/bundle/nsis/Flowy_1.6.1_x64-setup.exe`（SHA256: `47F5237E22BE85D663CDC63FE2AD423E6A264862269ADDAC88E46EB144DA8A32`）已完成 7z 提取与逐项核对：宿主、daemon、原生依赖及双许可证无缺失、无 skeleton。UI 构建身份为 `385e5135-12c4-4699-a955-71a1c1905761`。默认开发程序目录 `target/debug/` 下的 `Flowy.exe` 与 `ai-mate.exe` 已完成安全备份（详见 [T5-default-dev-restoration.json](T5-default-dev-restoration.json)）并更新为本轮最新二进制，资源解析优先级核对无误。自动门禁与产物收尾完成，进入工作包 3 实机验收阶段；保持“未验收”，不自动发布。
