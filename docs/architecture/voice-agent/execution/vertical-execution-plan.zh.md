# 语音伴侣完整纵向执行 Plan

> **状态**：🔧 执行中，2026-10-14 用户已授权实施。  
> **固定基线**：`feat/xiaozhi-voice`，`50f8ecb86070a9807ed57704ace070cc6748422e`；开始时已有 README 修改和三份未提交方案文档，全部保留。  
> **上游**：[详细 PRD](../PRD-voice-companion.zh.md)、[技术方案](../voice-companion-integration.zh.md)。  
> **核心原则**：Windows 完整闭环、逐片串行验收；保持独立语音进程，任务执行权属于既有 runtime。

## 1. 执行目标与固定决策

交付“唤醒 → 对话 → 追问 → 真实任务 → 确认与取消 → 安装运行”。Mac/Linux 保持兼容，分别实测后才宣称支持。不派发并行 Agent。

- daemon 拥有声学和短对话，桌面宿主拥有窗口和产品状态，runtime 拥有实际执行。
- 首期支持 Flowy Cloud 与 OpenAI-compatible 模型；配置页过滤不支持协议，失效引用明确显示不可用。
- 快轨聊天不初始化任务 runtime；任务不可用不能阻断聊天。
- 6 秒静默退出、单句最多 30 秒、实际播放结束后 4 秒追问、最多三轮短上下文；确认最多 8 秒并受执行侧授权有效期约束。
- 停止本轮、关闭会话窗和关闭语音功能不自动取消已接受任务。
- 一个活动语音任务，每任务专属 Conversation；不借用任意聊天或桌宠会话。
- 本期不增加流式 ASR、全双工 AEC、自定义唤醒词、远程音频、新 runtime 或数据库迁移。
- 保护当前分支所有已有工作；不重置领先提交，不在 main 修改或提交。

## 2. 接口、状态与测试基础

保留现有 companion 路由与公开字段；新增配置、分层可用性、完整产品快照和带作用域的停止/取消/确认。事件首帧返回快照，宿主 `instance_id/seq` 负责公开状态排序；窗口注册监听后读取最新快照。

daemon 私有控制限定回环地址，使用 supervisor 临时控制凭据；凭据不进入命令行、日志、前端或 SSE。

任务适配调用既有 `create_idempotent`、`send_message_with_idempotency_key`、`public_turn_delivery_state`、会话事件、`cancel`、`list_confirmations`、`confirm`。声学 turn、交付 command 和 runtime turn 分开；创建和发送使用不同的稳定幂等身份。

测试入口为生产 worker 回放、HTTP/快照接口、真实 Conversation 服务和原生宿主窗口。替身只存在于测试，证明调用次数、状态序列、身份及取消结果；结构检查不代替行为验证。

## 3. 纵向切片

| 切片 | 场景与实现 | 出口门禁 | 当前状态 |
|---|---|---|---|
| V0 | 固化基线、失败测试、证据目录；移除无真实任务依据的接单反馈 | 复现消费者、超时和虚假接单断点；无新增假成功 | 自动测试通过 |
| V1 | 模型目录/Ready 装配、统一 wake、启用意愿、常驻协调器、单例窗、首次快照、静默退出 | AC-01/03/19；无 PCM 也能超时，无设备显式不可用 | 进行中，自动检查与真机分开签核 |
| V2 | 唯一采集、预滚/连说、ASR 复用、VAD 校准、预筛、真实模型、无生产 skeleton、阶段反馈 | AC-04/05/09；完整指令一次消费，无模型不假回答 | 未开始 |
| V3 | 物理播放完成、追问、三轮历史、可取消工作、真实 flush 和文字降级 | AC-06/07；停止后无旧声音/迟到结果，追问不截断已开始发言 | 未开始 |
| V4 | 设置页读共享状态、删除重复录音、离页/托盘、重连/实例、设备恢复、双语主题与窗口定位 | AC-02/08/10/18；一个唤醒一个窗口，状态可恢复 | 未开始 |
| V5 | 明确工作区/执行配置、交付落盘、幂等创建/发送、真实接受后 ACK、完整任务链接 | AC-11/12；至多一次执行，拒绝/写入失败不显示运行 | 未开始 |
| V6 | 长任务期间聊天、查询/取消、摘要通知、只读恢复与身份隔离 | AC-13/16/17；取消终态才成功，重启不新建身份 | 未开始 |
| V7 | 实际工具前结构化策略、共享审批权威、物理提示完成后听取、单次确认/拒绝/超时 | AC-14/15；无法由 LLM/自动批准绕过，危险操作不先执行 | 未开始 |
| V8 | 桌面打包 ai-mate/native 库、资源定位、干净机器安装、退出监督与全量性能签核 | AC-20 和 AC-01–19 回归；无需手工编译，有真实证据 | 未开始 |

每片执行：失败测试 → 最小完整实现 → 定向检查 → 运行证据 → 状态更新。上一片门禁未满足，不把后续切片标成完成。

V7 只保留执行侧一个挂起与确认权威：实际派发前采用语音结构化策略，结果交给既有 ToolApprovalManager；复用工具策略分类，不再接入第二套 token/suspended-call 库。权限取交集，不可识别、越界或不支持的工具拒绝；真实危险工具测试通过前不开放该路径。

## 4. 验证和发布

1. 每片先跑生产入口的回放/接口/取消/失败测试，再检查受影响 crate。
2. `cargo test -p <crate>`、`cargo check -p <crate>`；检查生产 feature 图，无默认 skeleton。
3. UI 行为和双语测试、typecheck、i18n/theme/相关门禁；必要时生成 i18n 类型。
4. 集成后 `cargo fmt --all -- --check`、`cargo check --workspace`、`bun run check`、`bun run build:ui` 和桌面构建。
5. 实机录音、真实模型、扬声器、原生窗和任务单独留证；替身和 healthz 不作实机签核。

R0 = V0–V1 内部验收；R1 = V2–V4 加聊天安装包验收，可发布对话；R2 = V5–V7 加 V8 全量回归，可发布任务与确认。

窗口热/冷启动分别采样，P95 目标 500/1500ms；保留已有四分类、JSON 解析和首音性能门禁，记录模型、设备、网络、样本数和计时点。没有数据不能改成“体验正常”。Windows 完成不代替其他平台、无屏硬件或 72 小时签核。

## 5. 执行与证据维护

验收状态分别记录未开始、进行中、自动测试通过、真机通过、Owner 验收。每片留下变更、命令/结果、证据、剩余问题和下一入口。

不新增依赖、数据库表、通用调度框架、GitHub Actions；打包和审批接线分开审查。不自动发布、推送或合并。既有 PRD/方案/原始 Handoff 保留全文，只追加记录。

### 初始验收矩阵

| 验收 | 切片 | 自动测试 | 真机 | Owner |
|---|---|---|---|---|
| AC-01/03/19 | V1 | 部分回放/接口/宿主测试通过，完整 AC 未签核 | 未运行 | 未签核 |
| AC-04/05/09 | V2 | 未运行 | 未运行 | 未签核 |
| AC-06/07 | V3 | 未运行 | 未运行 | 未签核 |
| AC-02/08/10/18 | V4 | 未运行 | 未运行 | 未签核 |
| AC-11/12 | V5 | 未运行 | 未运行 | 未签核 |
| AC-13/16/17 | V6 | 未运行 | 未运行 | 未签核 |
| AC-14/15 | V7 | 未运行 | 未运行 | 未签核 |
| AC-20/完整回归 | V8 | 未运行 | 未运行 | 未签核 |

---

## 附录：授权与历史

[用户授权 Plan 完整原文](vertical-execution-plan.original.zh.md)逐字保留讨论时的状态和门禁；其中“尚处于 Plan/等待实施”与末尾黄灯是历史文本，当前执行状态以上方实施记录为准。

2026-10-14 用户在完成 Plan 讨论后明确要求 IMPLEMENT THIS PLAN，授权以上 V0–V8 实施；原 PRD 文档阶段的“只交付文档”限制被本轮实施授权更新。原文仍保留，避免混淆历史阶段。

### V0 执行记录

- `cargo test --locked -p nomifun-voice-terminal --lib`：原始基线 5/5 通过。
- 新增生产 worker 回放与 HTTP 迟订阅用例。模型管理实际读取磁盘，单纯 set_state(Ready) 不构成有效 fixture；改为显式 voice-skeleton 回放入口，生产默认不启用该入口。没有打开真实麦克风。
- 修复前回放确实到达 IntentDecided，再观察到不具有接受回执的 TtsSpeaking("已接单")，测试失败；外部唤醒无 PCM 超时和迟订阅快照测试同样失败。
- 修复：移除虚假任务接单/取消反馈，输出 runtime_unavailable；Listening 静默 deadline 由共享 driver 的 WakeHit 初始化并在 watchdog 中检查；事件订阅首帧交付当前声学状态。指定模型目录不再借用其他安装。
- `cargo test --locked -p ai-mate --features voice-skeleton --test voice_session`：3/3 通过（6.05 秒）。这是回放/HTTP 代码证据，不是原生窗口或真机音频签核。

### V1 实施记录（尚未满足全部出口门禁）

- 宿主统一状态投影：保存用户启用意愿，分别显示启动/连接/声学/对话/播放可用性；daemon 快照转换为宿主 instance_id/seq，首次订阅返回完整快照，Lagged 回到当前快照。宿主退出保留 autostart，不让投影循环在退出时再次拉起子进程。
- supervisor 显式传递模型目录和临时私有控制凭据（环境，不进入 argv）；受管 daemon 限定回环绑定，私有语音接口需要凭据；调试与 TOML 输出均不含凭据。
- 内外唤醒复用同一接受入口；worker 在新会话清空旧缓冲；真实 KWS/STT 引擎装配后才公布 acoustic_ready。无设备/未配对/未就绪不返回虚假唤醒成功。
- 原生常驻协调器订阅宿主，不依赖设置页或主窗口 renderer，按 Listening 创建/展示单例 voice-session 窗口。Windows 使用 SW_SHOWNOACTIVATE；页面先监听再读快照；旧实例/旧会话停止请求返回冲突。无发言 Idle 自动隐藏。
- 设置页常驻浏览器采集和重复声学状态机已删除，保留显式诊断；改读同一快照，并增加“试用唤醒”。移除没有真实采集依据的浏览器音量显示。
- 自动验证：daemon 会话回归 6/6、配置 golden 9/9；前端生产订阅/作用域控制 4/4；既有设置结构 17/17（仅结构证据）。i18n、主题、进程边界通过；SPA 构建通过。
- `cargo check --locked -p Flowy --bin Flowy` 使用固定 Cargo/rustc 1.97.1 通过；主机测试仍在执行。此前 mise/rustup 混用引起 E0514，统一工具链后重新检查，不清理用户构建目录。
- 全量前端 typecheck 失败，当前输出含 60 个错误，位于本轮未修改的 credits、会话、视频画布等路径；未发现新增 voiceSession/voiceCompanion/VoiceSettings 类型错误。完整输出：[typecheck-current.log](typecheck-current.log)。这不是独立 checkout 基线复跑，不能把所有失败直接签核为基线。
- 真机录音、扬声器、窗口焦点/托盘、首次安装与 Owner 签核仍未进行。生产 router 默认 skeleton、物理 flush/播放完成、任务接线等仍待后续切片，不宣称聊天或任务产品已完成。

### V1 补充验证记录

- `cargo test --locked -p nomifun-app --lib voice_companion_manager::tests`（固定 Cargo/rustc 1.97.1）：5/5 通过，覆盖公开状态序列、断线后相同 daemon 快照恢复、旧进程 generation 隔离、应用退出不清除启用意愿。
- 前端 CSS scope、按钮布局、图标、浏览器边界、Windows 无控制台契约通过。dead-css 检查发现本次修改的语音状态区存在旧无效类名，已改用真实语义变量和 fill token；其他路径的 dead-css 失败独立记录，未调整基线。
- 聚合 `bun run check` 因 typecheck 中已有错误和新增测试的 Bun matcher 类型差异失败；新增测试已改为显式捕获 rejection，4/4 行为测试重新通过。随后重新运行全量 typecheck 的输出覆盖在 typecheck-current.log，check-current.log 保留原失败快照，不视为最终通过。
- `cargo fmt --all -- --check` 在 Windows 返回 OS error 206（命令行长度），不能算通过；对本轮 Rust 修改逐文件格式化，并保留完整格式门禁未运行成功的限制。
- Windows 真机验证条件已通过会话问题请求用户提供；未收到硬件/模型可用性答复前不打开录音或声称真机通过。按照逐片验收约束，V1 仍为进行中；V2–V8 未签核。
- 当前真机入口：[V1 Windows 唤醒承载验收](V1-desktop-smoke.zh.md)。它是待执行步骤，不是完成证据。
- 修复 Bun 测试声明差异后的全量 typecheck 仍为 60 个其他路径错误，新增语音模块无报错；最终 SPA 构建通过，完整日志见 [build-ui-current.log](build-ui-current.log)。四项生产订阅行为和十七项既有结构测试再次通过。双语状态/控制键对称检查通过。
- 本轮 Rust 文件显式开启格式化的定向 `rustfmt --check` 通过，不能据此宣称 OS error 206 的全 workspace cargo fmt 已通过。相关脚本门禁中，剩余 dead-css 失败不含 VoiceSettings/voiceSession/voiceCompanion，详情见 [dead-css-current.log](dead-css-current.log)。
- `cargo check --locked --workspace`（固定工具链）：通过，完整输出见 [cargo-check-workspace-current.log](cargo-check-workspace-current.log)。`vt_0/vt_2/vt_3` 既有 21 项回归通过；其中延迟/分类/任务测试仍为 fixture 证据，不能当作真实模型、播放或 runtime 签核。
- 为避免新窗口暴露旧 skeleton 问候，worker 在 router 未配置时直接返回 model_unavailable；仅唤醒词的空内容不再替用户合成“你好”，路由失败返回明确错误。这是 V1 消费者的失败路径保护，不等于 V2 的真实模型装配已完成。
- 增加不可用模型回放：确实走过生产 PCM/ASR 消费入口，模型路由调用计数为零，不发出任何 TtsSpeaking；新增计数/不可用 fixture 只编译于测试或显式 skeleton feature。daemon 会话回归最终 7/7、配置 golden 9/9 通过（6.06 秒）。删除误导性的 set_state(Ready) fixture，不用它假装模型文件已验证。

### G：不可用提示与启用意愿诊断（2026-10-14）

- 用户接管实机验证；本轮未启动录音、播报或操作运行中的语音开关。当前 `Flowy.exe` 与其 `ai-mate.exe` 子进程存在，daemon `/healthz` 返回 200。这仅证明进程存活，不证明 KWS、ASR 或声卡已就绪。
- 确认页面反馈缺陷：模型安装 Ready 被显示为产品“已就绪”；未收到产品快照时又显示“语音服务不可用”。启用开关读取保存的用户意愿，不能作为引擎就绪证据。截图的“正在连接”不足以判定真实声学故障。
- 回放先复现两个订阅问题：首次 GET 失败导致已建立的订阅被销毁；旧宿主的迟到 GET 覆盖新宿主事件后，后续新事件被永久忽略。修复首次读取失败的重试与订阅保留，初始 GET 仅在尚无事件快照时作为种子。
- 设置页分别呈现状态同步中、同步失败、未启用、启动中、进程离线、声学未就绪和唤醒就绪；同步失败提供重试。尚未收到可信状态时禁止新启用，已经启用时仍允许关闭。试用唤醒依赖实际声学就绪。
- 桌面 WebView 的本机信任凭据与宿主到 daemon 的临时控制凭据均由软件自动传递，不是 Flowy Cloud 的用户 token，也不是自定义模型 provider 的 API key。终端直接请求受保护 API 得到 403，不能用来证明应用内请求被拒绝；没有读取或绕过凭据。
- 前端行为测试 9/9 通过，覆盖首次读取失败保留订阅、实例竞态、后续事件恢复同步状态和意愿/就绪区分；双语键生成及检查通过（11466 keys），SPA 构建通过，见 [G-build-ui-current.log](G-build-ui-current.log)。主题和按钮布局检查通过。
- 当前实例首次状态同步失败的具体原因尚未确认；实际模型/设备就绪、应用内校验结果仍需实例快照或初始化日志。代码测试不能替代用户实机签核，V1 不因此标记完成，V2–V8 保持原门禁。
- 最终全量 TypeScript 检查返回 2，输出仍有 60 项其他路径错误，没有 VoiceSettings/voiceCompanion/voiceSession 报错，见 [G-typecheck-current.log](G-typecheck-current.log)。`git diff --check` 通过。聚合质量门禁未签核。

### G 补充：定位跨域凭据冲突（2026-10-14）

- 新截图确认页面处于状态同步失败。读取当前 Flowy（PID 152596）的本机监听端口 53966，对 `/api/voice/companion/session` 发起不带凭据的 OPTIONS 预检，返回 200、`Access-Control-Allow-Origin: *`、`Access-Control-Allow-Headers: *`，没有 `Access-Control-Allow-Credentials`。预检没有读取私有状态或绕过鉴权。
- 根因：新语音适配器的快照 GET、SSE GET、停止 POST、唤醒 POST 均指定 `credentials: include`。桌面页面跨源访问 loopback 后端时，该模式与实际 wildcard CORS 响应不兼容，浏览器拒绝向脚本提供响应，状态同步持续失败。既有 httpBridge 使用本机信任头，与 cookie 跨域凭据无关；前一轮提示/重试修复没有消除此冲突。
- 新增跨四个生产请求入口的凭据契约回归，修复前 1 fail / 9 pass，失败实际捕获四个 include；最小修复将四处改为 same-origin，保留 buildBackendAuthHeaders。桌面仍使用自动注入的本机信任头；WebUI 同源请求仍携带登录 cookie。不修改后端 CORS/认证策略，不要求用户 token。
- 修复后订阅/控制行为、设置结构及双语测试共 35/35 通过（其中结构测试不是浏览器实测）；`git diff --check` 通过。用户运行页面恢复与声学实际就绪仍须用户核对，未代替实机签核。
- 最终 SPA 构建退出 0，见 [G-cors-build-ui.log](G-cors-build-ui.log)。全量类型检查退出 2，仍为 60 项其他路径错误，没有 voiceCompanion/VoiceSettings/voiceSession 错误，见 [G-cors-typecheck.log](G-cors-typecheck.log)。本次仅改前端请求模式，无需改动或重建 Rust 服务；开发页面可加载更新，安装包仍须重新打包后分发。

### H：语音窗口主题及试用唤醒说明（2026-10-14）

- 用户实测提供窗口白底、文字/按钮不可读截图。独立 `/voice-session` 不挂载 Layout，漏掉主页面的 customCss 注入、控制主题契约和跨窗口主题广播；容器使用 Arco 填充色而非应用主背景。
- 复用既有 `injectCompanionCustomCss`、主题广播与壁纸颜色变量同步，不新建主题体系。ThemeProvider 继续负责初始明暗偏好，语音窗口额外加载实际预设并监听主窗口切换；更新 html data-theme 和 body arco-theme，保持文字与 Arco 控件一致。容器使用 bg-base 和 box-border，停止按钮使用现有 primary 控件样式。
- 双语试用唤醒说明明确：手动触发、无需说唤醒词、六秒内开始说话、静默退出；该入口不验证 KWS 对唤醒词的识别准确率。截图中的 Idle/等待唤醒是空闲状态，不能据此签核完成对话。
- 主题契约与 i18n 检查通过，语音订阅及双语测试 18/18 通过。用户继续负责原生窗口明暗主题、实际发言、播放和 Owner 验收；本轮未操作运行中的录音/唤醒，不把代码门禁当作实机证据。
- SPA 构建通过，见 [H-theme-build-ui.log](H-theme-build-ui.log)；类型检查仍为 60 项其他路径错误，语音模块无报错，见 [H-theme-typecheck.log](H-theme-typecheck.log)。dead-css 未通过，输出没有语音模块，见 [H-theme-dead-css.log](H-theme-dead-css.log)。全量按钮布局矩阵耗时过长，本轮中止，未签核；不把该通用探针当作原生语音窗口视觉验证。`git diff --check` 通过。

### I：普通讲话无响应与真实音量诊断（2026-10-14）

- 截图显示 Listening 在六秒后回到 Idle，未出现 Transcribing；它证明未进入识别阶段，不能单凭截图认定 ASR 未加载。代码确实把默认 Silero 概率阈值 0.5 用到了 EnergyVad 的归一化 RMS 比较。峰值 0.03 的 200Hz 回放（RMS 约 0.021，约 -33.5dBFS）无法触发，远低于旧门槛的约 -6dBFS。
- 先将既有生产 worker 回放改为普通音量，确实失败：[I-vad-red.log](I-vad-red.log)。语音伴侣局部改用 RMS 0.01（约 -40dBFS），保持三帧进入和 800ms 结束，不修改其他 VAD/Silero 使用方。持续单句最多收集 30 秒后进入识别；先发 Transcribing，识别结果才发 Deciding；识别空文本明确显示原因。
- ASR 使用已验证目录装配的缓存 transcriber，待机的级联唤醒也复用它，不再在声学 worker 里调用全局诊断转写函数。此回放使用测试范围的 STT，不等于真实 SenseVoice 准确率签核；真实声卡是否送达普通讲话仍须新版实机验证。
- 增加共享快照的可选 audio 遥测：实际 PCM RMS/峰值、帧计数、最后一帧时间、采集状态、VAD 活动、ASR/KWS 装配状态。daemon 最多每 200ms 推送，未启用其他麦克风消费者；宿主断线清空音频投影。设置页和窗口复用显示组件，以 dBFS 显示实际幅值，超过两秒无新帧显示过期，状态流水不重复写入同一阶段。
- 新增设备失败测试，先复现“已关闭采集但仍声学就绪”：[I-device-red.log](I-device-red.log)；修复为先撤销可用性再发布关闭状态。普通音量回放同时验证真实遥测、VAD 命中、单轮一次 ASR 和一次 Transcribing。最终八项会话回归通过：[I-vad-green.log](I-vad-green.log)。
- 日志记录实际设备/输入格式、VAD 参数、五秒采集心跳、语音起止、六秒退出、ASR 加载错误与运行耗时/字数，不输出 PCM、识别原文或控制凭据。此前 daemon stdout 被宿主丢弃；现以宿主实际 NOMIFUN_LOG_DIR（缺省数据目录 logs）下 `voice/ai-mate-<启动毫秒时间戳>.log` 保存 stdout/stderr，并在宿主日志记录路径。未增加日志依赖或新配置。
- 窗口改用 fixed inset-0 覆盖完整 WebView，修复截图下半部未涂背景的问题。具体 ASR 空结果和对话模型未接通原因在两处诊断区域显示。
- 开发 cargo run 占用共享构建目录，首次测试还遭遇运行中 ai-mate.exe 文件锁。本轮不结束用户实例，使用独立 target/voice-diagnostics 与 build.voice-diagnostics.noindex 验证。用户测试前须重建/更新宿主和 ai-mate 并重启；仅刷新前端不能使旧 daemon 获得遥测或 VAD 修复。测试 feature 的二进制不作为生产交付。
- i18n/主题、进程边界、Windows 无控制台及 CSS scope 检查通过；前端订阅/双语回归 18/18。真机麦克风、音量适应性、窗口铺满、ASR 内容和完整聊天仍由 Owner 验收，V1–V8 状态不因此全部签核。
- 最终宿主/daemon `cargo check -p ai-mate -p nomifun-app` 通过，见 [I-cargo-check.log](I-cargo-check.log)；最终八项回放通过。SPA 构建通过：[I-audio-build-ui.log](I-audio-build-ui.log)；typecheck 仍有 60 项其他路径错误、无语音模块错误：[I-audio-typecheck.log](I-audio-typecheck.log)。定向 Rust 格式检查通过。当前实机入口：[音量与识别闭环核对](I-audio-debug-smoke.zh.md)。
- 独立目录 `cargo build --no-default-features -p ai-mate` 成功：[I-ai-mate-build.log](I-ai-mate-build.log)。该产物是当前代码的非测试 feature 构建，尚未替换用户运行的默认路径 daemon；替换和启动后的日志落盘仍需实机核对。

### I 补充：默认开发路径的旧 daemon（2026-10-14）

- 用户继续报告音量不动。检查时 Flowy/ai-mate 均未运行，不能据此判断当前声卡无输入。默认 `target/debug/ai-mate.exe` 是旧产物，不包含 AudioDiagnostics、voice_capture_heartbeat 或 voice_vad_configured；独立验证目录的新版包含这些标记。宿主默认先寻找同目录 daemon，两个产物的错配足以让新版页面拿不到音量数据。
- 之前未替换默认 daemon 是为了保护用户运行实例；本轮实例已退出，已在默认目录执行 `cargo build --locked --no-default-features -p ai-mate`，退出 0，见 [I-default-daemon-build.log](I-default-daemon-build.log)。重新检查默认产物包含全部遥测标记。默认 Flowy 包含 AudioDiagnostics 与 Voice companion diagnostics log 标记；没有改设备配置或启动录音。
- 修正界面：没有 audio 投影时明确显示后台尚未提供音频诊断，不能将版本错配/诊断未送达当作实际无 PCM。双语生成/检查与 18 项既有行为/双语回归通过。旧独立目录构建记录保留，不冒充当时已部署到默认路径。
- 仍需 Owner 启动新版后区分三个观察值：尚无诊断、采集无帧/关闭、帧送达但幅值为零。第一类查进程版本和投影；第二类查流启动/设备错误；第三类查实际设备、静音和通道。不能用 VAD 回放通过代替麦克风硬件输入证明。
- 默认路径生产 daemon `--help` 退出 0，在麦克风装配前退出，见 [I-default-daemon-help.log](I-default-daemon-help.log)。最终 SPA 构建通过：[I-default-ui-build.log](I-default-ui-build.log)；类型检查仍为 60 个非语音路径错误，无语音模块报错：[I-default-typecheck.log](I-default-typecheck.log)。`git diff --check` 通过。

### J/K：完整回复与跨平台 Edge 播报（2026-10-14）

- 用户明确要求显示全部 LLM 文字，并保留跨 Windows/Linux/macOS 的 Edge 播报；明确授权在语音 crate 引用仓库已有 sha2、chrono。这是对原“不新增依赖”限制的本次最小引用例外，没有添加新第三方包或 Windows 系统声音路径。
- 根因及修复、自动测试、网络合成证据与实机步骤集中记录在 [J-K-reply-playback-smoke.zh.md](J-K-reply-playback-smoke.zh.md)。线上固定句合成返回非零 24 kHz 音频；不作为物理播放或完整聊天验收。
- 回归实际发现文字追问被采集 worker 重复发布普通 FollowUpWindow 覆盖；移除重复事件。轮次工作脱离采集等待，保持持续消费；追问开始说话后允许完成整句、无 PCM 追问仍可超时。迟到旧合成不会 flush 新一轮声音。
- 快照新增带 turn_id 的最近三轮正文，模型调用一次返回解析结果和原始 message.content；空 Chat 正文直接报真实格式错误。播报失败保留回复、显示具体阶段原因，后台设备失效不伪装可用。
- V1–V8 仍按原出口门禁验收，不能把当前修复或在线探针等同于已完成真实任务、确认与安装发布。
- 最终宿主和 daemon 默认路径生产构建退出 0：[J-final-default-build.log](J-final-default-build.log)。语音库 31 项、音频库 73 项、worker 9 项、前端 12 项通过；定向格式、i18n、主题及 SPA 构建通过。全量类型检查仍有 60 个非语音路径错误。Windows 实际扬声器、双主题、停止和追问的 Owner 签核待补，Mac/Linux 和安装包仍未验证。

### L：活动面板展示识别与模型正文（2026-10-14）

- 用户明确追加：ASR 文字与思考后的回复必须显示在设置页活动面板。此前完整文字只接入独立语音窗口，活动面板仍只有阶段流水。
- 在活动面板音量诊断之后增加“识别原文与模型回复”，直接复用 VoiceConversation 和共享快照，不新增采集、模型调用或第二份会话状态。显示 ASR 原文、助手回答及完整 message.content；保留当前会话最近三轮，Idle 时仍可阅读。没有回答的 ignore 结果展示原始模型正文，不伪造回复。
- 中英文标题、语义主题和全文滚动阅读；独立窗口继续复用相同正文组件。新增双语 Idle/ignore 行为回归证明退出阶段不隐藏原文与模型结果，见 [L-idle-reply-tests.log](L-idle-reply-tests.log)。其余前端回归见 [L-panel-tests.log](L-panel-tests.log)，其中设置结构检查不是浏览器实测。
- 本次只改变前端展示，无需重建或重启 daemon；开发模式刷新设置页面后核对。Owner 应复现“识别→思考→等待唤醒”，读取面板内 ASR 原文和完整模型输出，才能确认这轮真实返回意图。实际页面视觉验收待补。
- 现有四文件前端回归 37 项通过，最终正文双语行为 4 项通过；i18n、主题、CSS scope、按钮布局与 diff 空白检查通过。全量类型检查仍为 60 个其他路径错误，无语音模块错误：[L-panel-typecheck.log](L-panel-typecheck.log)。dead-css 仍因其他路径失败，无 VoiceSettings/voiceSession 命中：[L-panel-dead-css.log](L-panel-dead-css.log)。前端构建记录见 [L-panel-build-ui.log](L-panel-build-ui.log)。
- 最终 `bun run build:ui` 退出 0；开发页面可刷新使用，安装包尚未重新打包。L 的代码/自动测试通过，实机通过与 Owner 验收待补。

### M：标准音频与真实引擎回放（2026-10-14）

- 用户要求生成标准音频并自行模拟测试。生成固定测试句 WAV，使用真实安装的 KWS 和 SenseVoice；对话与输出只在测试入口替换，未占用麦克风或物理播音。具体音频、失败/成功记录和复现命令见 [M-standard-audio-report.zh.md](M-standard-audio-report.zh.md)。
- 原词表是下载模型的示例词，真实神经 KWS 正样本 0/4；改用既有 keywords_buf 配置固定产品词表，原阈值不变、不修改下载文件，同样样本 4/4。两个非唤醒语音对照均无命中。
- 5 项显式真实音频回放通过，中文直接识别、连说与相邻问题均保留完整指令；生产 worker 单次路由、输入零丢帧有断言。9 项原会话回归及 2 项 KWS 单测通过。当前样本没有复现 ASR 加载失败或丢字，没有凭此改 ASR 权重。真人、设备、噪声与口音验收待补。

### N：原文、理解的需求与回答（2026-10-14）

- 用户明确选择：一次模型调用，直接展示 ASR 原文、模型理解的需求及回答，不增加确认步骤。
- VoiceDecision 增加 understood_request；提示要求忠实整理输入，不能补造未说出的事实，并在同一 JSON 输出中直接回答。保持既有 public JSON 的兼容：字段可缺省，旧响应不伪装为模型已经提供了理解结果。整理不影响执行侧工具授权语义。
- 原有生产 worker 的单次 HTTP 回放先捕获投影丢字段：[N-understanding-red.log](N-understanding-red.log)。实现经 IntentDecided、共享快照及最近三轮会话传递；独立窗口与设置活动面板复用 VoiceConversation，顺序为原文→理解→回答，完整模型正文继续保留。
- 回归验证模型字段与回答在 TTS 失败后仍可见，并断言只有一次模型调用。9 项 worker、31 项语音库与 39 项前端测试通过：[N-understanding-green.log](N-understanding-green.log)、[N-terminal-tests.log](N-terminal-tests.log)、[N-panel-tests.log](N-panel-tests.log)。前端双语测试验证三段顺序及不伪造旧响应理解。
- i18n、主题、CSS scope、定向 Rust 格式与 diff 检查通过；前端构建通过：[N-build-ui.log](N-build-ui.log)。全量类型检查仍有 60 个非语音路径错误：[N-typecheck.log](N-typecheck.log)。
- 用户确认已完全退出应用，检查无 Flowy/ai-mate 运行后，同步重建默认路径宿主与 daemon，生产构建退出 0：[N-default-build.log](N-default-build.log)。未启动麦克风或播音，也未调用用户真实云模型；实际模型字段遵循、需求理解质量与声音体验须 Owner 重启后核对。V1–V8 与跨平台/安装包未因本轮自动签核。

### O：主产品 provider 目录与独立语音模型接口（2026-10-14）

- 用户实测确认激活与识别效果改善，追加：查明 Flowy 模型列表和接口，接入语音伴侣，保留后续本地模型替换接口。接口位置、架构、原始需求全文和测试证据见 [O-provider-integration.zh.md](O-provider-integration.zh.md)。
- 当前宿主目录模型 ID 使用 AIPC 前缀；旧 daemon 默认模型名及 5 秒客户端与主产品解析路径不同。宿主现在复用 provider 仓库、模型目录、加密凭据解析与一次性模型客户端，不初始化任务 runtime。daemon 使用 VoiceCompletionClient 和受保护的私有回环接口；provider key 不进入子进程或公开状态。独立运行原公开配置入口保持兼容。
- 设置页复用聊天目录、支持 OpenAI 兼容模型、明确显示失效引用；配置只保存 provider_id/model，更改在下一次请求生效。读写接口沿用既有 owner 保护，配置与启用意愿原子合并保存。没有新增依赖、数据库迁移或通用调度框架。
- 2 项宿主 provider 回归、7 项配置/投影回归、9 项生产 worker、31 项语音库、40 项前端回归通过。worker 同时验证独立 OpenAI 与宿主接口，保持一次请求、原文/理解/回答和失败后文字。
- 对当前真实 Flowy provider 只发一次固定连接测试，AIPC-deepseek-v4-flash 在约 2259 ms 返回可解析 JSON：[O-live-provider-probe.log](O-live-provider-probe.log)。只读 DB，复用已有配置，不使用用户录音、不录音/播音。该结果证明实际模型调用，不替代真人语音及完整首音延迟验收。
- Rust 定向检查与 SPA 构建通过；双语、theme、CSS scope、按钮布局、进程边界和 Windows 控制台门禁通过。类型检查仍为 60 个其他模块错误，无本次语音错误：[O-final-typecheck.log](O-final-typecheck.log)。默认产物更新记录见 [O-default-build.log](O-default-build.log)，最终构建结束后再签核产物状态。
- 最终默认 `Flowy.exe` 与 `ai-mate.exe` 生产构建退出 0，定向 Rust 格式检查退出 0；最终 SPA 构建退出 0：[O-final-build-ui.log](O-final-build-ui.log)。用户重启后实测完整声学与回答闭环，代码和真实模型探针不能替代物理播报、跨平台或安装包签核。

### O 追加：优先使用 Qwen3.8 Flash（2026-10-14）

- 用户追加语音低延迟模型偏好。仅调整语音自动选择为 `AIPC-qwen3.8-flash` → `AIPC-deepseek-v4.1-flash` → 原目录启用聊天模型；手动选择保留，失效手动引用明确拒绝。复用原有能力校验，未修改全局 resolver 或其他任务模型。
- 旧逻辑失败断言、2 项回归及一次真实 Qwen 请求证据见 [O-provider-integration.zh.md 第 7 节](O-provider-integration.zh.md)。本次固定句请求 4018 ms，不作为跨模型性能或成本比较结论。
- 默认宿主构建结果以 [O-default-preference-build.log](O-default-preference-build.log) 的结束状态为准；真实语音、跨平台、任务与安装包门禁继续保持待验收。
- 最终定向格式、宿主检查及默认宿主构建均退出 0，`target/debug/Flowy.exe` 已更新。重启后保持自动模型即可核对新默认 Qwen；未代替 Owner 录音或播音。

### P：唤醒后 ASR 与面板闭环（2026-10-14）

- 用户追加“唤醒顺利但讲话未识别、六秒后休眠，并补齐 ASR 呈现和面板交互”。当前实例阶段记录与标准低音量音频复现了固定 EnergyVad 门槛拦截问题；套件 Silero VAD 现已接入生产 worker，不新增依赖或模型资产。
- 八项真实声学回放、14 项库/worker 回归、21 项前端测试通过；收音期限、具体退出原因、ASR/理解/回复/模型原始输出及再次开始入口已补齐。完整证据、计时修正与 Owner 操作见 [P-post-wake-asr-panel.zh.md](P-post-wake-asr-panel.zh.md)。
- 宿主与 daemon 生产编译检查通过，聚合前端检查仍在 60 个非语音类型错误处失败。运行中的应用锁住默认产物时先构建独立输出；不得把独立构建视为默认文件已更新或实机验收。最终状态以构建及更新记录为准。
- 最终语音库 31 项通过，默认 voice-skeleton 已关闭；生产图及 SPA、宿主/daemon 构建通过。独立产物在 `target/voice-staging/debug`，运行中的默认文件尚未替换，等待 Owner 完全退出后更新与实测。
- Owner 随后确认完全退出。检查无运行实例后，默认 `target/debug/Flowy.exe`、`ai-mate.exe` 及对应调试符号已同步更新，SHA256 与生产构建一致：[更新回执](P-default-artifact-update.json)。默认产物替换已完成，Owner 实机复测仍待签核。

### Q：完整桌面 TLS 与 TTS 回归闭环（2026-10-14）

- 用户追加合成失败、没有声音，要求全流程 mock 与测试后再实测。运行日志显示 ASR 和模型回复成功，Rustls 在完整桌面 feature 图中因双 CryptoProvider 自动选择失败而 panic；独立包在线探针此前没有覆盖该组合。原因、原始需求、修复和分层证据见 [Q-full-flow-tts-closure.zh.md](Q-full-flow-tts-closure.zh.md)。
- 复用频道 WebSocket 的显式 provider 与平台根证书装配，不绕过证书校验；引用已有锁定依赖，没有新增第三方包。增加轮次 panic 恢复和具体错误，保留文字；实际音频写入后才进入播放阶段，合成与播放分别呈现。窗口可调整大小，诊断折叠，正文滚动阅读。
- 完整 worker 20 项、语音库 31 项、宿主 101 项、前端 23 项通过；完整桌面图中的原 TLS panic 失败测试现通过。真实声学引擎与在线 Edge 回放单次模型请求、零丢帧，顺序完整经过倾听、识别、思考、合成、播放、追问及 Idle。
- 在线固定句和整链输出均为实际 Edge 24 kHz 音频，模型与输出消费确认仍由测试控制，没有占用麦克风、物理扬声器或真实 Flowy 模型调用。不能据此签核实际声音、Mac/Linux、任务/确认/恢复或安装包。
- 生产编译检查、最终 SPA 构建、定向格式/质量检查通过，生产 feature 图不含 voice-skeleton。聚合检查仍有 60 个既有非语音类型错误。Owner 已确认完全退出，构建和默认产物替换结果完成后追加，实机播报仍待 Owner 复测。
- 最终联合生产构建退出 0；确认无运行实例后，默认宿主、daemon 及 PDB 已替换，四份源/目标 SHA256 一致：[Q-default-artifact-update.json](Q-default-artifact-update.json)。未启动应用，实机声音与 Owner 验收仍未签核。

### R：Owner基本链路反馈与离屏声音PRD（2026-10-14）

- Owner确认KWS→ASR→LLM→TTS基本链路正常，runtime仍未接通。此反馈补充Q的定性试用记录，不把原V1–V4完整门禁或V8自动标完成。
- 用户追加完整提示音与离屏使用要求，并明确选择轻量状态反馈。当前代码存在六件资产，但ASR成功、静默/追问超时等出口接线不完整，提示音输出完成/失败不可观测；探索与方案见 [v0.2 PRD](../PRD-voice-companion-v0.2.zh.md)及[声音反馈技术方案](../voice-feedback-integration.zh.md)。
- 新增[R0–R6串行计划](R-auditory-feedback-plan.zh.md)细化对话声音与后续runtime声音门禁；保留原AC-01–20并增加AF-AC-01–16，全部新增验收未执行。本轮只落盘文档，下一实施入口R0。

### R0–R1 无人值守执行进展

- 用户已授权按PRD无人值守打通完整链路。生产worker识别音缺失、earcon没有消费回执及停止回复占用输出均有失败测试；已补单次识别/追问/休眠反馈、统一诊断与可取消的输出消费等待。
- 33项库/worker/播放定向回归通过，完整R1时间窗、回声、设置、runtime及安装等门禁仍未签核；详见 [R0–R1实施记录](R0-R1-feedback-progress.zh.md)。全目标继续执行，最终产物和实测交接尚未交付。
- 接续至2026-10-15：六秒听取改为Wake提示消费结束后启动，并验证提示期间讲话不重新起期限、PCM持续消费；最终35项定向回归与8项真实声学回放通过，最终生产编译检查退出0。四秒追问的driver统一权威及其他R1出口继续执行，不提前签核整片。
- 后续四秒追问亦由同一driver管理，正常静默/主动停止有明确原因与短音，公共Idle兼容保留；处理期3/9/15秒最多三次轻音，输出可抢占且不排队。最新41项Rust/27项前端、8项真实声学和提示音负对照通过，证据及尚未接入实际推理/HTTP取消等剩余门禁见 [R1–R3实施记录](R1-R3-feedback-progress.zh.md)。全目标继续active，runtime/确认/安装及Owner交接未完成。

### R3–R4 接续（2026-10-15）

- 倾听中点一次提醒与实际输出时间补偿已接线，停止/采集失败撤销持有的异步轮次；已运行中的阻塞原生推理与宿主端请求是否中断不据此冒称，细节追加在[R1–R3记录](R1-R3-feedback-progress.zh.md)。
- 声音模式、音量、回复播报、六类语义试听与双语回执诊断接入现有配置/控制路径；已保存/实际生效分开，省略字段保留现有偏好。48项worker/播放/会话、8项宿主配置、32项前端测试通过，SPA构建通过；[R4记录](R4-feedback-settings-progress.zh.md)保留门禁和剩余项目。
- 没有更新默认程序、没有启动设备、没有中途请求Owner验收。全目标保持active；设备恢复、实际回声、三轮推理上下文、V5–V7真实任务/取消/恢复/审批、安装性能与最终实测仍须完成，不能将本轮测试签成整体完成。

### V3短上下文接续（2026-10-15）

- 已复现生产请求没有历史：五轮prompt历史计数全为0。修复后为0/1/2/3/3，停止后新会话为0且阅读记录保留，六轮恰好六次模型请求；联合50项回归通过，见 [V3记录](V3-short-context-progress.zh.md)。原V3仍有实际声学、计时与停止门禁，未整体签核。
