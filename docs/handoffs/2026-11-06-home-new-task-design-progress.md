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
