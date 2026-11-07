# 首页「新建任务」设计进度

## 当前分层交互方案

- 已确认移除本地文件大卡，保留输入框「在项目中工作」。网站以可在浏览器操作的本地 HTML 验收。发送联动分为开发前核对现有路径、接线后实跑新首页两步。

- 已按最新边界修订。独立视频入口保留并继续进入现有视频页。实现仅修改「新建任务」首页，任务页与侧栏复用现有机制。顶部快捷入口计入小窗预算，紧凑档更新为 518px，并补了独立入口和首页视频模式的分流验收。

- 布局方向已确认。左侧「开发／创作」上下叠放，右侧会员翻转卡替代本地文件卡，下方保留三张封面和贴底输入框。此前并排双入口、无封面的设计已被替换。
- 已定稿待评审的 SVG 规格。以正视浏览器、正视播放框和环形 `F` 印章替代旧抽象等距物件，记录几何、亮暗描边、悬浮位移和减少动效。补全小窗预算、示例草稿保护和从首页到任务页的验收。本轮交付方案，SVG 与布局等待实现和实测。
- 已核对首页 workspace、任务模式、幻灯片模板与视频创建接线。办公／开发走会话，幻灯片复用现有参数，视频首轮复用短片链路。
- 已整理视觉状态、紧凑尺寸、开发拆分和验收矩阵。两张大入口打开二级目录，选择能力后由同一上方区域展示专属准备内容，输入框保持统一位置。
- 已记录执行范围与原冻结边界的冲突，以及网站／小游戏的产物验证门槛。下一步是评审方案。原展示层方案作为提案附录留档。

## 原展示层方案记录

- 目标是交一份可按尺寸评审、可按类名实现的首页展示方案，保留会员翻转卡、视频入口、三个任务入口、三张案例。
- 顺序是冻结证据和测试核对 → 差距 → 主方案七项 → 备选与死路 → 抽查与 Git 状态对账。
- 最大风险是 sticky 输入区和窄宽换行影响预测的首屏几何，以及两个已失败的冻结断言被误当作方案回归。
- 任务 0 完成。数字与主要源码已核对，来源为事实文档 §1–§11、原始 JSON、Manus 及 Flowy 亮暗参考截图和当前源码。初始未跟踪证据与冻结测试基线 6 pass / 2 fail 已进入待裁决。
- 采用产品界面的层级、可达性和主题检查要点。以任务文档限定的证据为准，交付用 Markdown 和 ASCII。
- 任务 1 完成。提案已记录 8 条差距。下一步写主方案七项。
- 任务 2 完成。确定 768 版心、112 行高、152 高会员卡、主题线稿和按钮局部圆角变量。响应式表、实现白名单及冻结断言已对齐。下一步收敛备选和已证实的死路。
- 任务 3 完成。仅保留横排三个入口的备选，作为首屏完整展示画廊时的权衡。
- 任务 4 完成。标出替换会员卡、变更被锁定的 JSX 顺序、关闭竖向滚动三条死路。下一步对账 Markdown、抽查数字与 Git 状态。
- 验收对账完成。已检查 8 张原图和 5 份测量 JSON。桌面参考图历史圆角与事实文档的文字描述有视觉分歧，记入待裁决首项，仅沿用当前 main 的证据。抽查复现两套网格、239 横向间距、50.7 遮挡和圆角规则。三个 Markdown 空白检查通过，Git 已跟踪文件无差异，本次状态差量仅三个白名单文档。冻结用例维持任务 0 的 6 pass / 2 fail。设计交领导验收，方案尺寸为推算，首屏、主题与窄窗留待实现后的浏览器验证。

## 执行轮：首页实现（2026-11-06 19:18 开工）

### 开工快照（任务 0）

- 分支 `feat/redesign-home-new-task`，HEAD `6b1b4e0fd`（比 origin/main 多 2 个用户提交，均未触及 `ui/src/renderer/pages/guid/`）。`git diff --name-only`、`git diff --check` 均为空；`git status --porcelain` 共 6 项，全部为未跟踪交接文档/证据（本文件、设计提案、事实文档、error-boundary 计划与 `docs/handoffs/assets/`）。用户既有改动已全部在 HEAD 中。
- 基线复跑（2026-11-06 19:3x）：`bun test ui/src/renderer/pages/guid` = **138 pass / 2 fail / 140 用例**，失败为 `GuidResourceCards.structure.test.ts:19`（缺 `<TaskProfileSelector`）与 `:47`（`.guidContainer` 期望 `overflow-y: auto`，现值 `scroll`），与规格一致；`ui/` 下 `bun node_modules/typescript/bin/tsc --noEmit` = **71 error / 29 文件**，与规格一致；`check:i18n`、`check:theme`、`check:dead-css`、`check:button-layout-contract`、`check:css-scope`、`check:icons` 通过；`check:motion` = 1 项既有失败（MarketCardShell hover 边框、transition-all 82/82 顶格），与规格一致。
- 结论：前提与规格一致，按白名单开工；不改基线、不改冻结文件、不清理用户文件、不提交/推送。

### 开工回执（≤10 行）

1. 目标：按提案 §1–§3 把首页改为「左开发/创作双入口 + 右跨两行会员卡 + 上方独立视频入口 + 三封面 + 贴底输入框」，并接通办公/开发/幻灯片/视频的草稿与发送。
2. 顺序：任务 0 快照 → D1 舞台/目录/SVG/会员紧凑化 → D2 办公/开发/幻灯片 → D3 视频准备与统一发送 → 测试/门禁/构建 → 真实桌面发送与 V1–V9 证据。
3. 最大风险：真实发送依赖运行中的桌面 debug 构建（`target/debug/Flowy.exe` 在跑）与账号/模型；`useGuidSend` 与新增舞台状态耦合面大，可能触碰冻结断言；视频链路复用面广，可能需写待裁决。
4. 已知验收例外：移除本地文件大卡会让 `GuidPhaseOneStyle.test.ts:12` 的 `onStartFromLocal` 断言失败，按拍板列为单条例外并保留测试原样。
5. i18n 复用（i18n 文件只读）：开发入口 `guid.home.buildTitle`（构建/Build）、创作入口 `guid.home.createTitle`（创建/Create）、新建项目 `common.sider.newTask`、修改现有项目 `conversation.welcome.openFolder`、幻灯片 `presentation.label`、视频 `videoGeneration.create.preferences.video`、返回 `workshop.back`、关闭 `common.close`、视频占位 `videoGeneration.create.composer.generatePlaceholder`。缺「开发/创作/新建项目」专属词条，按拍板记入待裁决。

### 实现与验证回执（2026-11-06 20:35）

1. D1：首页舞台改为 62:38 两列（左双入口 104 行、右会员卡跨两行 220）+ 顶部 44 视频入口 + 三封面 116 + 贴底输入区（768，与舞台同宽）。宽窗实测 768/104/220/44/116，网格行 44/220/148，1280×719 无滚动；紧凑档由 ResizeObserver 实测（可用宽 ≤560 或高 ≤580），880×600 行高 84/178/76，输入区 padding 收紧到 12/8 后无滚动。
2. 目录层与能力层接通：开发目录（新建任务/打开文件夹）、创作目录（幻灯片/视频，能力门槛关闭幻灯片项）；视频面板含 4–15s 时长、480p/720p/1080p；幻灯片复用既有 SlideCreatePanel；ESC/返回/关闭只改舞台与焦点，不清草稿；输入框显示能力 chip。
3. 草稿隔离：文本与工作目录按能力分桶，切换时保存/恢复；任务模式用局部 `homeTaskProfile` 覆盖，不写全局 store；图片入口保持办公默认模式。
4. 发送：办公/开发/幻灯片沿用原 `sendMessageHandler`（幻灯片 brief 仅在 slides 能力下拼接），视频走新增 `submitHomeVideo`（createGenerationTask → rememberVideoGenerationTask → `/video-generation/clip/{id}`；提交中守卫；失败保留首页草稿）。删除旧本地文件大卡与 `setTaskProfile('coding')` 副作用。
5. 实跑：普通任务从新首页发出，会话 `01a1b0ca-a8a2-7901-bb60-8257d91d728d`（DB id 96，标题「Flowy 本地优先介绍」，AIPC-qwen3.8-flash，finished），路由 `/conversation/:id`，侧栏选中同名条目；视频任务 `01a1b0cd-e156-7483-92b5-ee6eeadc3b8c` 打开 `/video-generation/clip/{id}`，侧栏「最近创作」出现同名条目，但云端返回「生成服务鉴权失败，请检查渠道配置」。
6. 门禁：`bun test ui/src/renderer/pages/guid` = 137 pass / 3 fail（2 条基线 + `GuidPhaseOneStyle.test.ts:12` 已批例外）；`tsc` 71 error/29 文件（与基线一致）；i18n/theme/dead-css/button-layout/css-scope/icons 通过；motion 1 项基线失败；`git diff --check` 清洁。
7. 证据：11 张截图在 `docs/handoffs/assets/2026-11-06-home-new-task/impl/`（宽/紧凑、亮/暗、中/英、目录/能力/幻灯片/视频、真实会话与视频结果页），含账号邮箱与本机画面，公开前需按需裁剪。
8. 未闭环：视频参考图显式上传与 `@` 引用未实现；幻灯片未真实发起；视频生成因渠道鉴权失败无产物；`GuidPhaseOneStyle.test.ts` 在 :12 中断后其后的发送/工作区/焦点断言与新版发送入口口径有变，已独立手测并记入待裁决。
- 补充（20:52）：开发能力的真实 coding 会话未实发（首页→构建→目录→能力面板的 UI 已验收，发送链路沿用办公会话，本轮时间盒内未再跑第二条 coding 任务）；工作区生成 index.html 与二次修改验证同样未闭环，列为 V5 未闭环项。

## 二轮修订 · 底部工作区与专属输入（2026-11-06 21:20 开工）

### 开工回执（≤10 行）

1. 目标：按提案「二轮修订」把构建／创建目录收到 `guidFixedInputArea` 贴输入框上沿，幻灯片恢复背景墙且模板面板只挂底部一次，视频改为复用视频模块短片输入并接参考图上传，清理额外能力标签，补示例草稿保护。
2. 顺序：任务 0 快照 → D1 抽屉归位＋幻灯片背景 → D3 视频专用输入与素材提交 → D2 SVG／会员动效 → 示例草稿保护 → 四态量测与真实发送 → 门禁与记录。
3. 最大风险：视频专用输入要复用 `VideoHomeComposer`/`PromptComposer` 的接缝，若需要改共享视频代码则只阻塞该部分；真实视频生成仍可能因云端渠道鉴权失败只证明接单。
4. 已知验收例外不变：`GuidPhaseOneStyle.test.ts:12` 旧大卡断言；`:16` 旧发送字符串断言被 :12 提前中断遮住，工作区、粘贴、发送、焦点需独立核对。
5. 白名单：仅 `ui/src/renderer/pages/guid/` 首页专用代码与本进度、待裁决记录；i18n、测试、脚本、依赖、共享组件只读，不提交、不推送。

### 二轮实现与验证回执（2026-11-06 22:20）

1. D1 抽屉归位：目录/开发准备面板从上方舞台移入 `guidFixedInputArea`，与输入框同宽、间距 8px；头部 32、两项等宽各 56、面板高 122（紧凑档），160ms 淡入 + 8px 上移；打开时上方卡片与画廊隐藏；只留一个关闭按钮（子层用返回）；打开目录不改能力/草稿，首个可用项自动聚焦；Escape 先关模板面板，再退出能力层。额外能力标签 `entryStrip` 已移除。
2. 幻灯片：`slideStudioOpen` 与模板面板状态分离，进入即恢复 `SlideCardWall`；`SlideCreatePanel` 只在底部挂一次、不再包重复标题/关闭的外框；宽窗限高 232、紧凑 168，内容内部滚动；关闭模板保留背景与模式，退出幻灯片才回默认；底部幻灯片模式 chip 与模板/页数控件仍可用。
3. 视频：首页「创作→视频」与视频示例改挂真实 `PromptComposer`（视频模块短片输入：图片 @ 引用、附件预览/删除、上传入口）+ 实际生效的 4–15s 时长与 480p/720p/1080p 清晰度 + 返回/发送；独立「创建你的视频」仍跳原视频页。提交时才上传显式选中的图片并传 `reference_media_ids`，提交中防重复，失败清理已上传素材并保留草稿。
4. 实测（浏览器 1108×569 紧凑档）：默认/目录/幻灯片/视频四态输入框底边 561.4/561.4/561.4/561.3；目录面板 122 高、与输入 8px、选项 56、首项聚焦、页面无滚动（533/533）；准备面板与输入同宽；幻灯片面板限高 168、内容 370 内部滚动、关闭后背景墙仍在；视频输入替换普通输入框且无横向溢出。
5. 真实发送：普通任务从新首页发出 → 会话 `01a1b12a-c7da-7ec2-bd12-e2a00aefa7d1`（DB id 97，标题「新版首页入口说明」，status running），路由 `/conversation/:id`，侧栏选中同名条目。视频任务 `01a1b126-c687-7170-ae45-c2a4c4857caf` → `/video-generation/clip/:taskId`，侧栏「最近创作」出现同名条目；nomicore 日志证据：`POST /api/video-canvas/media/upload` 201，`video_generate API params` 中 `image_count:1`、`duration:6`、`resolution:720p`，证明参考图随任务发出；生成失败为 `API error 403: Your current plan cannot use this model`（AIPC-Doubao-Seedance-2.0），非界面链路问题。
6. D2 图案：构建改为「左下两条代码轨迹→右上浏览器布局」，创作改为「斜向胶片+片孔+播放键+演示页一角」；主轮廓 1.75、辅助 1，插画 144×86 / 紧凑 104×62；hover 只动页块上移 4px、轨迹终点点亮、胶片沿自身方向 5px、后页转 4°，220ms；减少动效只保留明暗。会员 hover 100ms 预览背面、移出回正、点击/键盘固定，减少动效关闭自动预览。
7. 门禁：`bun test ui/src/renderer/pages/guid` 137 pass / 3 fail（2 基线 + 已批 `GuidPhaseOneStyle.test.ts:12`）；`tsc` 71 基线诊断；i18n/theme/dead-css/button-layout/css-scope/icons 全通过；motion 仍为既有 MarketCardShell + 82/82；`build:ui` 成功；`git diff --check` 清洁。证据图在 `docs/handoffs/assets/2026-11-06-home-new-task/r2/`。
8. 未闭环/待裁决：①幻灯片真实发送的模板/页数载荷未实跑（`slideOptions.test.ts` 覆盖拼装，面板控件已接 `slideOptions`）；②示例应用后的取消/确认端到端点击验证未完成（实现为 `pendingExample` 确认条，图像→办公换正文、应用→开发、视频保护视频草稿；代码已接，缺手测证据）；③SVG hover 中间帧与会员键盘/点击固定未单独截图；④桌面窗口中途被应用重启与系统缩放打断，四态桌面截图在 880×600 附近但不完全一致，精确量测以浏览器紧凑档数据为准；⑤视频生成受套餐模型权限限制无产物。

### 二轮补测记录（2026-11-06 22:5x，round 2）

- 桌面环境变化：本轮中 Flowy debug 进程被重启后 WebView 首次加载遇到 dev server 拒绝连接；已用受管后台任务重启 `bun run dev:ui`（127.0.0.1:5173 恢复监听），再通过 Ctrl+R 让桌面窗口恢复首页。新窗口为 1280×800 逻辑级，截图/坐标映射变为 `physical = logical × 2.25`（已用点击「构建」打开抽屉验证校准）。
- 幻灯片真实发送补测：已从新首页实际打开「创建→幻灯片」（背景墙 + 底部单份 SlideCreatePanel + 幻灯片输入与模板/画布/页数控件均出现），但自动化点击未能把焦点落到幻灯片输入框，粘贴未生效，本轮仍未发出幻灯片任务；模板/页数真实请求载荷保持未闭环。
- 应用/图片示例取消/确认、SVG hover 中间帧、会员键盘/点击固定、精确 880×600 四态测量，本轮未取得新证据：浏览器自动化会话因标签页/空闲超时不可用，桌面窗口坐标为 DPI 复合缩放，剩余时间不足以稳定复测。
- 未提交、未推送；桌面窗口已用 ESC 退回默认首页；dev server 仍在后台运行（管理任务 pwsh-ee4f960814bc4119）。

### 二轮补测记录（round 3，2026-11-06 23:0x）

- 继续尝试从桌面首页补发幻灯片任务：坐标校准后「构建」卡点击可打开抽屉，但输入框点击始终不落焦；进一步诊断发现**前台窗口被企业微信抢占**（`GetForegroundWindow` 返回 `企业微信`，`SetForegroundWindow` 失败），期间 SendInput 键入未进入 Flowy。已用 Alt 轻敲解除前台锁并把 Flowy 拉回前台，但输入框仍未被命中；为避免继续向错误前台窗口注入文本，停止桌面自动输入。
- 因此本轮仍未补到：幻灯片模板/页数真实请求日志、示例取消/确认点击证据、会员键盘/点击证据、精确 880×600 四态测量。上述项保持未闭环并已在此前待裁决中登记。
- 环境状态：dev server 仍在后台运行（管理任务 pwsh-ee4f960814bc4119，127.0.0.1:5173）；Flowy 桌面窗口处于默认首页；未提交、未推送、未改动白名单外文件。

### 收尾记录（round 4，2026-11-06）

- 再尝试浏览器补证：新 bsk 会话可启动并完成 `navigate`，但 `cooperate sync` 持续返回 `permission_denied`（页面帧不可观测），无法执行 DOM 点击/截图；桌面侧前台仍可能被企业微信抢占。该环境阻塞已连续出现在 round 2、3、4。
- 结论：剩余五项（幻灯片模板/页数真实请求日志、示例取消/确认点击证据、SVG hover 中间帧、会员键盘/点击截图、精确 880×600 四态复测）在当前环境无法继续补测，按任务书「缺真实链路或视觉验收证据时明确标为未闭环」处理，保留在待裁决清单。
- 交付状态：代码与门禁维持绿色（137 pass/3 fail 已批、tsc 71 基线、i18n/theme/dead-css/button-layout/css-scope/icons 通过、build:ui 成功、diff check 清洁）；未提交、未推送；dev server 仍在后台运行；Flowy 桌面窗口处于默认首页。

## 三轮 · 视觉深化与真实交互闭环（2026-11-07 00:10 开工）

### 开工快照（任务 0）

- 分支 `feat/redesign-home-new-task`，HEAD `aafed553f`（二轮改动仍在工作区，未提交）。`git status --porcelain` = 5 个 `guid/` 产品文件修改 + 4 个二轮新组件未跟踪 + 6 项既有未跟踪资料/证据；`git diff --check` 清洁；未暂存 diff 与二轮交付一致，无缺失。
- 基线复跑：`bun test ui/src/renderer/pages/guid` = **137 pass / 3 fail / 140**（`GuidPhaseOneStyle.test.ts:12`、`GuidResourceCards.structure.test.ts:19`、`:47`），与交接一致；类型检查与前端门禁本轮冻结前统一复跑。
- 读图对照：Manus `01/03/05/06/07/11` 与 Flowy `r2-07/08/09` 已逐张查看。三条可见差异：
  1. **首页**：Manus 由视频/应用/网站三张大幅成品封面承担色彩与信息层次，入口线稿只作辅助；Flowy 二轮封面 116 高但入口线稿与会员卡同样抢眼，独立视频入口与封面缺少主弱，封面看不出「点进去会得到什么」。
  2. **目录**：Manus 构建目录是「网站推荐位 + 8 张成果缩略图」，创建目录按文档/媒体分组、每项都有真实内容缩略图；Flowy 二轮抽屉仍是「新建任务/打开文件夹」「幻灯片/视频」两枚灰框文字，选择前看不到成果。
  3. **专属准备**：Manus 幻灯片准备在背景墙上是「3 张主题建议 + 模板横排（真实模板封面、选中描边）+ 底部真实参数」；Flowy 二轮紧凑截图里主题建议占满面板、模板被挤到面板下，参数与右侧操作重叠；视频缺模型选择，开发准备区只有一段文字示例。

### 开工回执（≤10 行）

1. 目标：先修已审查的交互缺口（附件/模板跨能力隔离、示例统一确认、视频确认替换、开发准备区覆盖、会员悬浮点击冲突、Escape 优先级），再交「默认→创建目录→幻灯片准备→收起模板」四态视觉切片（1280×832 与 880×600），最后扩到构建/视频并走通普通、幻灯片、带参考图视频的真实发送。
2. 顺序：任务 0 读图/基线 → D1 状态与草稿隔离 → D2 创建目录/幻灯片样板 → D3 构建/视频与模型校验 → D4 真实发送与四态证据 → D5 门禁与记录。
3. 最大风险：附件与模板按能力隔离要同时改保存/恢复与发送输入，容易影响 `useGuidSend` 与幻灯片 brief；视频模型选择若必须改共享组件则按范围裁决只阻塞该项；真实发送仍受桌面自动化/套餐限制。
4. 硬约束：只改 `ui/src/renderer/pages/guid/` 首页产品代码与两份执行记录；冻结测试、i18n、脚本、依赖、共享组件只读；未授权不提交、不推送。

### 三轮 · 第一片视觉样板与交互缺口（2026-11-07 01:0x update）

**已实现（仅 `ui/src/renderer/pages/guid/` 白名单 + 两份记录）**

1. 草稿按能力隔离：`homeDraftsRef` 现在保存 `{ text, dir, files, slideOptions }`；切换能力时恢复正文、目录、附件和幻灯片模板/参数。离开幻灯片会清空可见模板状态，切回恢复原模板与已选主题；办公/构建不会看到或发送幻灯片导入的附件与模板。
2. 示例统一入口：封面、图片示例、开发准备区按钮与视频主预览都走 `requestHomeExample → applyHomeExample`；目标能力已有内容（含附件/导入模板）先出确认条，取消不动草稿，确认才替换。修掉视频确认后保留旧正文和开发准备区直接覆盖两条路径。
3. 视频专用输入补齐模型：`HomeVideoComposer` 增加模型选择（`useMediaModels` + `pickDefaultVideoModel` 默认选中、`noModelSelected` 校验与提示），时长按所选模型钳制；提交时 `model`、`resolution`、`duration_secs`、`reference_media_ids` 进入同一 `createGenerationTask` 请求，无模型时禁止发送并保留草稿。
4. 会员：悬浮 100ms 预览背面；悬浮中点击固定为背面不再翻回；移出保持；再次点击解除并抑制指针停留导致的立即重新预览；键盘 Enter 可主动翻面。
5. Escape：先看 `event.defaultPrevented` 与可见的 Arco/mention 浮层，再关模板面板 → 退能力层 → 回首页；不再抢已打开下拉/引用菜单的 Escape。
6. 视觉样板：创建目录改为真实预览卡（幻灯片用真实主题封面 `SlideThemeThumbnail`，视频用已有短片封面）；构建目录改为主项阅读应用封面 + 低对比项目树轮廓 + `index.html` 说明；幻灯片面板顺序改为「头部 → 模板横排（真实封面）→ 一行主题建议」，紧凑档模板先露 6 个可点封面；开发/视频准备态新增上方封面 hero（视频 hero 可直接套用示例）；紧凑档参数把画布/出稿收成 `16:9 · Quick draft` 摘要，避免与模型/发送重叠。

**实测（CDP 1280×832 / 880×600，`evidence-report.json` 与 e1–e18 截图）**

- 宽窗：默认首屏无滚动（796/796），输入框底边 755 恒定；创建目录宽 768、与输入卡间距 8、封面 352×120；幻灯片面板宿主 232，面板 225，内部溢出仅 3px；关闭面板后背景墙仍在、模式 chip 与正文保留。
- 紧凑：默认无滚动（552/552），输入底边 535；目录宽 784、间距 8、封面 362×84；幻灯片宿主 167/167、可点模板首屏 6 个、摘要与模型选择间距 25.8px 无重叠、关闭后墙/模式/正文仍在。
- 示例：图片示例切办公后正文替换且不含幻灯片正文/控件；幻灯片草稿正文与主题恢复；确认条出现后取消保留原文、确认替换为阅读应用提示并进入开发准备态。
- 会员：hover `aria-pressed=true`，悬浮中点击仍为背面，移出保持，键盘 Enter 可翻面（焦点在卡片）。
- 亮暗主题：880×600 浅色截图 e17；减少动效 `prefers-reduced-motion` 下过渡 0s（e18）。
- 门禁：见本轮尾部门禁输出。

**未闭环**

- 幻灯片模板/页数真实发送、带参考图视频的真实发送：桌面自动化在本轮被前台窗口抢占且窗口句柄失效，浏览器 dev 无后端（视频模型列表为空、上传 404），未取得新的真实请求日志；二轮已证普通任务与视频（含 `image_count:1` 参考图）真实接单，本轮新增 `model` 字段仍缺一次真实请求证据。
- 附件隔离：dev 浏览器不支持原生文件选择/合成拖放（paste/drop 均未产生 chip），仅验证「切换能力后办公无幻灯片正文/模板/控件」与代码级 files 隔离；附件实物跨能力隔离待桌面手测。
- 会员「再次点击解除」自动化时序受 hover 重入影响，未取得干净 aria 证据；触屏翻面未单独验证。
- Escape 浮层优先：点击参数下拉未在 DOM 观察到可见 popup（可能测试点未命中），仅代码级保证。
- 宽窗幻灯片面板仍有 3px 内部滚动；紧凑目录封面 84px 高，如需 Manus 式横向小图可继续调。

**门禁（本轮冻结后实跑）**：`bun test ui/src/renderer/pages/guid` = 137 pass / 3 fail（2 基线 + 已批 `GuidPhaseOneStyle.test.ts:12`）；`tsc --noEmit` = 71 条既有诊断（guid 仅 `entryFlyoutPlacement`）；i18n / theme / dead-css / button-layout-contract / css-scope / icons 全 0；`bun run build:ui` 成功；`git diff --check` 清洁。改动仅 `ui/src/renderer/pages/guid/` 与两份执行记录，未提交、未推送。

### 三轮 · 第二次补测（round 2，2026-11-07 01:2x）

**桌面点击校准已解决**：窗口需先 `SetWindowPos(HWND_TOPMOST)` 再 `SetForegroundWindow`，用 `MOUSEEVENTF_ABSOLUTE` 归一化到真实屏幕（本机 1920×1200，脚本按 2880×1800 归一化恰好得到 `physical≈css×1.5`），点击立即生效。验证：桌面创建抽屉打开（真实幻灯片/短片封面）、幻灯片准备态打开并可选中「极简」模板（pill 显示「极简」、卡片描边）。

**真实幻灯片发送成功（新版首页）**：桌面从新首页「创建 → 幻灯片」选择了模板、输入提示词后发送，DB 新增会话 98：`conversation_id=01a1b1d1-7cb3-7ca1-bd90-614445618ca6`，名称 `Create PPT Deck via`，状态由 running → finished，路由进入现有会话页、侧栏选中同名条目。该会话首条 user message 为：
`[ppt-master] canvas=ppt169; pages=auto; route=Quick, decide confirmations yourself. Make this deck with the ppt-master Skill; call nomi_presentation_status first.` + 提示词。
→ 证明「幻灯片头部（画布/页数/出稿方式）随真实请求进入会话」，且 `pages=auto` 为实际生效值。

**本轮仍未取得**：
- 模板指令（`look=…`）进入真实请求：该次发送时选中项回落为「自由设计」（自动化点击把模板切回自由设计），首条消息无 template/look 段；模板选择状态与 pill 已在桌面截图 d2 证明，`buildSlideHeader` 的 theme directive 由既有 `slideOptions.test.ts` 覆盖，但缺一次带模板的真实请求。
- 非默认页数：页数 pill 点击未命中下拉（多次尝试未出现菜单），只验证 `pages=auto`。
- 视频模型真实请求、附件实体隔离、会员解除/触屏、Escape 浮层实测、开发 HTML 双指令仍按待裁决保留。

**证据**：`d1-desktop-create-dir.png`、`d2-desktop-slides-template-selected.png`、`d3-desktop-slides-task.png`（r3 目录）。

### 三轮 · 第三次补测（round 3，2026-11-07 01:5x）

- 桌面视频准备态打通：新版首页「创建 → 视频」在桌面打开，上方出现真实短片封面 hero，底部复用短片编辑器与真实参数：**模型自动选中 "Wan 3.0"**（`useMediaModels` + `pickDefaultVideoModel` 生效）、单段时长 chip 变为该模型的 2–30s 集合、清晰度 480p/720p/1080p、返回与发送按钮齐全。证据：`d4-desktop-video-prep-model.png`。
- 视频真实发送未完成：桌面自动化鼠标点击未能把焦点落到 `PromptComposer` 文本域（点击后 unicode 注入与剪贴板粘贴均未进入文本），未发出视频任务，因此 `model` 字段真实请求日志仍缺；模型选择与校验的界面/默认值已在桌面验证。
- 本轮无代码改动；代码与门禁沿用第一轮冻结结果（137 pass/3 fail、tsc 71 基线、前端门禁全绿、build:ui 成功、diff check 清洁）。未提交、未推送。
- 台面状态：Flowy 桌面窗口停在视频准备态（未发送），dev server 运行中。

### 三轮收尾（2026-11-07 02:0x，时间盒用尽）

**已通过项**：草稿/模板按能力隔离（代码 + 浏览器四态验证）；示例统一确认流程（取消保留、确认替换、图片示例切办公、视频确认替换）；视频模型选择/默认/校验（桌面实机 Wan 3.0 自动选中、模型时限 chip、无模型禁用）；会员悬浮预览/悬浮点击固定/移出保持/键盘翻面（aria-pressed 证据）；Escape 浮层优先代码路径；创建目录真实封面预览（模板封面/短片封面/阅读应用封面/项目树轮廓）与 8px 贴底；幻灯片模板第一 + 一行主题建议 + 紧凑摘要不重叠、关闭保留墙/模式/正文；开发/视频准备态 hero；暗/亮主题；减少动效；四态同尺寸 1280×832 与 880×600（e1–e18 + evidence-report.json）；桌面幻灯片真实发送（会话 98，header 含 canvas/pages/route）；普通任务与带参考图视频真实发送（二轮证据，ID/路由/侧栏一致）。

**未闭环**：视频 `model` 真实请求日志（桌面文本域焦点自动化未攻克）；带非默认模板/页数的真实幻灯片请求；附件实体跨能力隔离手测；会员再次点击解除干净证据与触屏；Escape 浮层实测；构建交互 HTML 双指令；宽窗幻灯片面板 3px 内滚。以上均写入待裁决，按任务书「缺项明确标为未闭环」处理。

**门禁（最终）**：137 pass / 3 fail（2 基线 + 已批 `GuidPhaseOneStyle.test.ts:12`）；tsc 71 条既有诊断；i18n/theme/dead-css/button-layout-contract/css-scope/icons 全 0；`bun run build:ui` 成功；`git diff --check` 清洁；改动仅 `ui/src/renderer/pages/guid/` + 两份执行记录；未提交、未推送。

### 审查修复与最终验收（2026-11-07 12:0x）

**代码修复（全部在 `ui/src/renderer/pages/guid/` 白名单内）**
1. [R1] 视频清晰度改为按模型能力生成：`homeVideoResolutionOptions = videoModelCapabilities(model).resolutions`，模型切换时 `normalizeVideoResolution` 归一化；提交也做同样归一化。实测 minimax-h3 请求发送 `resolution=768P`（该模型能力表内的值）。
2. [R2] 默认模型按套餐过滤：`isPlanLocked(min_tier_level, effectiveModelTier)` 过滤后再 `pickDefaultVideoModel`；无可用时回退原始目录，选择器不再展示锁定项。桌面实测列表仅 `minimax-h3`（可用项）。
3. [R3] 素材删除守卫：任务创建成功（`taskCreated=true`）后不再因导航失败删除 `uploadedMediaIds`；导航失败单独记录日志，任务与素材保留。
4. [Optional] 删除遗留 `HomeStagePanel.tsx/.module.css`（`HomeStageFrame/HomeStageOptions` 无导入方），类型移到 `components/homeStage.ts`；删除无引用 `.subtitle` 选择器。
5. [Optional] 键盘解除固定不再吞 hover 预览：仅在指针悬停于卡片时的一次 unpin 设置抑制；实测「键盘翻回正面 → 再 hover」`aria-pressed` 恢复 true。
6. [Optional] 紧凑摘要改为可操作控件（Button + Dropdown）：实测点开菜单选择 4:3 后摘要更新为 `4:3 · Quick draft`；`Escape` 在 capture 阶段先关下拉、面板保持打开，再按一次才关面板。
7. [Optional] 示例确认条补 `role="alertdialog"`、焦点落在 Cancel、`Escape` 取消并聚焦回输入框；取消保留原草稿（实测 `text` 不变、`focusReturned=guid-input`）。
8. [Optional] 模型目录失败重试：视频能力打开时自动重试一次，并提供手动 `Refresh` 按钮；[Optional] 模型切换时夹取时长到该模型 ticks。
9. [Review-发现的新问题] 紧凑档 action row 内 `sendbox-strategy-slot-placeholder` 曾盖住摘要按钮（命中测试 `hitTag=SPAN`）；改为 compact 下 `.actionRow` 换行后命中 `hitTag=BUTTON, hitTestId=guid-slide-summary`。

**最终验收证据（r3 目录）**
- 四态短录屏（新增交付物）：`f5-four-states-880.gif`（149 帧）、`f6-four-states-1280.gif`（144 帧）。
- 修复验证：`f1-summary-compact.png`、`f2-example-confirm.png`、`f3-member-hover-after-keyboard.png`、`fixes-report.json`。
- 幻灯片真实发送（模板+非默认页数）：会话 **99** `01a1b41f-e4c8-7c10-8061-ce08647d10e1`，首条 user message = `[ppt-master] canvas=ppt169; pages=5-8; look=swiss: visual_style=swiss-minimal …`（模板与 5-8 页均进入真实请求）；路由与侧栏一致（`d5/d6`）。
- 视频真实发送（模型+参考图）：任务 **01a1b420-1013-7d82-9f24-21f367f2df76**，nomicore `video_generate API params` = `model: AIPC-minimax-h3, duration: Some(6), resolution: Some("768P"), image_count: 1, images:[reference_image=http://download.flowyaipc.cn/...]`；素材上传 `POST /api/video-canvas/media/upload` 201。云端生成因参考图 320×200 小于最小边 256 被拒（结果页可见错误），属云端参数校验，非首页链路问题（`d7/d9`）。
- 失败→恢复成对证据：通过 CDP `Fetch.failRequest` 让 `POST /api/video-canvas/tasks` 失败一次 → 错误可见、无跳转、草稿与参考图保留（`d8`，`videoFailure.hash=#/guid, draftLen=8, composerStill=true`）；关闭拦截后重试成功创建任务并进入现有剪辑结果页（`d9`，route `#/video-generation/clip/01a1b420-…`）。

**门禁（最终）**：本轮修复后统一复跑（见下一条命令输出回执）。改动仍仅 `guid/` + 两份执行记录；未推送。

**门禁回执（最终）**：`bun test ui/src/renderer/pages/guid` = 137 pass / 3 fail（2 基线 + 已批 `GuidPhaseOneStyle.test.ts:12`）；`tsc --noEmit`（ui/）= 71 条既有诊断（guid 仅 `entryFlyoutPlacement.ts` 两条既有）；`check:i18n`、`check:theme`、`check:dead-css`、`check:button-layout-contract`、`check:css-scope`、`check:icons` 全部 0；`bun run build:ui` 成功；`git diff --check` 清洁。改动范围仅 `ui/src/renderer/pages/guid/`、两份执行记录与任务证据目录。
