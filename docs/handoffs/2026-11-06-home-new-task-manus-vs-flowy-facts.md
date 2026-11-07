# 交接：首页「新建任务」现状事实对照（Manus live vs Flowy v1.7.0 / main）

- 性质：**只读事实采集**。本次未改任何代码，也未执行此前的「方案 A」重排。
- 基线：`feat/redesign-home-new-task` @ `32c2fa01f`（= `origin/main`，工作树干净）。
- 采集时间：2026-11-06 16:33–16:55（Asia/Hong_Kong）。
- 采集工具：bsk 0.4.2（Chrome 156，扩展 0.4.2，daemon protocol 1.8）；视口 **1280×719 CSS，DPR 2.25**（截图 2880×1618）；数据来源为 `getComputedStyle` / `getBoundingClientRect` + 截图像素测量（Pillow）。
- 证据目录：[`docs/handoffs/assets/2026-11-06-home-new-task/`](assets/2026-11-06-home-new-task/)（截图 + 原始测量 JSON）。

## 0. 口径与前提（读结论前先看）

1. **Manus 无源码**：数值全部来自由登录态页面 `https://manus.im/app` 的 DOM computed style 与截图；线上存在运营位轮换/A-B（两次采集期间右上角运营卡从「创建你的视频」变成「游戏开发」），运营位结论只代表采集时刻。
2. **Flowy 采集的是本仓库 dev 页面**：本地 `dev:ui`（vite，端口 5173）渲染 `#/guid`，en-US 文案（dev 默认语言），暗色为主、亮色通过 `data-theme` 切换实测。桌面版参考截图由用户提供（v1.7.0，zh-CN）。
3. **Flowy 桌面与 dev web 外壳不同**：侧栏宽度（参考截图折算 ≈213 CSS vs dev web 184.9）、语言、登录态、会员卡内容不同；首页正文区（`pages/guid`）组件与样式同源，因此正文区几何以 dev 实测为准，外壳差异单独标注。
4. **Flowy 参考截图早于当前 main 的一次全局按钮样式改动**：截图里 Create/Studio 卡圆角约 20px，当前 main 实测为 6px（原因与提交见 §5.4）。看截图对不上代码时以此为准。
5. **未实测窄视口**：Agent Window 被窗口管理器约束，无法缩到 760px；响应式事实来自 Flowy 源码与 Manus DOM 类名（§8），非实测。

## 1. 页面骨架（首屏从上到下）

| 区块 | Manus（live） | Flowy（dev，1280×719，暗色） |
| --- | --- | --- |
| 应用内顶栏 | 有：左「Manus 2.0 Lite」（16px/500，32px 高）+ 右「免费计划 / 升级 / 1,020 积分」 | 无内容区顶栏；桌面版只有窗口标题栏，dev web 有 36px 浏览器式标题栏（属外壳） |
| Hero 主视觉 | 「manus」大 SVG 字标 **376×64**，内容列左对齐；右上运营卡 **252×56**（圆角 12，黑底） | 会员翻转卡 **416×192**（点阵 + `Flowy:` 字标 + 日期 + 环形印章/邮戳）；右侧视频入口 chip **185×54**（英文文案时；中文约为 150×54） |
| 入口卡片 | 3 块：构建 **486×130**、创建 **486×130**、从本地文件开始 **270×272** | 3 块：构建 **538.4×156**、创建 **538.4×156**、从本地文件开始 **289.6×324** |
| 案例画廊 | **无** | 有：3 张 **272×196** 封面卡 + 卡下标题行（行高合计 224.2） |
| 吸底输入框 | 宽度 **768**（与内容列同宽），外框高 151.8，顶部工具条 64 | 宽度 **840**（版心内宽），高 141.8 |
| 页面滚动 | 文档高度 = 视口高度（1280×574 实测 574=574），无页面滚动 | 内容高 **800.2** > 视口 **683.1**，页面滚动；输入框 sticky 覆盖内容底部 |

首屏截图：

- Manus：[manus-home-live-1280x719.png](assets/2026-11-06-home-new-task/manus-home-live-1280x719.png)
- Flowy 暗色：[flowy-home-live-dark-1280x719.png](assets/2026-11-06-home-new-task/flowy-home-live-dark-1280x719.png)
- Flowy 亮色：[flowy-home-live-light-1280x719.png](assets/2026-11-06-home-new-task/flowy-home-live-light-1280x719.png)
- 用户提供的桌面参考（v1.7.0，暗色）：[flowy-home-desktop-reference-dark.jpg](assets/2026-11-06-home-new-task/flowy-home-desktop-reference-dark.jpg)
- 用户提供的 Manus 参考（暗色）：[manus-home-reference-dark.png](assets/2026-11-06-home-new-task/manus-home-reference-dark.png)

> 1280×719 首屏可见性：Manus 的顶栏、字标、运营卡、3 张入口卡、输入框全部可见且不需滚动；Flowy 首屏只能看到 masthead + 入口卡，案例画廊完全在首屏之下（被 sticky 输入框和视口裁掉），入口卡底部约 51px 被输入框渐变覆盖。

## 2. 版心与网格（computed 实测）

| 指标 | Manus | Flowy（dev web，本仓库 main） |
| --- | --- | --- |
| 左导航宽度 | **300** | **184.9**（dev web）；参考截图折算 ≈213（桌面外壳） |
| 主内容区宽度 | **980**（=1280−300） | **1095.1**（=1280−184.9） |
| 内容列宽 | **768**（`max-w-[768px]`），居中，左右各 **106** | 外层 `max-width:920`，`padding:0 40px` → 内宽 **840**；外层居中，主区两侧各 87.55 |
| 内容列起点 x | **406** | 外层 x=267.3；内宽 board x=**307.3** |
| 卡片网格 | `grid-template-columns: 486px 270px`；`grid-template-rows: 130px 130px`；`gap:12px`；总宽 768 | `grid-template-columns: 538.382px 289.618px`（≈1.45fr / 0.78fr）；`grid-template-rows: 156px 156px`；`gap:12px`；总宽 840 |
| 网格断点 | 容器查询 `@[700px]:grid-cols-[minmax(0,1fr)_270px]`（容器 <700 变单列） | `@media (max-width:860px)` 变单列（源码） |
| 案例画廊 | 不存在 | 3 列 × `272px`，`gap:12px`，总宽 840；卡高 196 + 标题 28.2 |

关键几何（Flowy）：

- masthead 复用 board 的网格列宽（538.4 / 289.6，gap 12），但会员卡固定 416 宽 → **左列空 122.4**；视频 chip 185 宽右对齐 → **右列空 104.6**；**会员卡右缘（x=723.3）到视频 chip 左缘（x=962.3）之间空 239**。
- board 三条左卡/右卡总宽 840，与 gallery 的三列 272×3+24=840 对齐；但入口卡左右分栏（538/290）与画廊三列不等宽。

关键几何（Manus）：

- 字标 x=406 左对齐内容列；运营卡 x=922、右缘 1174=内容列右缘 → 运营卡贴内容列右侧。
- 字标底 143.7 → 卡片顶 171.7，间距 **28**；卡片底 443.7 → 输入区渐变起点 475.3，间距 **31.6**。

## 3. 纵向节奏（1280×719，CSS px）

| 元素 | Manus | Flowy |
| --- | --- | --- |
| 顶栏 | 模型选择 y=7.6、高 32 | 无（dev web 标题栏 36 属外壳） |
| 主视觉 | 字标 y=79.7、高 64；运营卡 y=83.7、高 56 | 会员卡 y=40、高 192；chip y=178、高 54（底对齐 232） |
| 主视觉 → 卡片 | 28 | 16 |
| 卡片区 | y=171.7 → 443.7（两行 130 + gap 12） | y=248 → 572（两行 156 + gap 12） |
| 卡片 → 画廊 | — | 0（画廊容器 y=572；内部 `margin-top:12` 后首行 y=584） |
| 输入区容器 | y=475.3 起，含 48px 顶部渐变；composer 卡 y=567.3，高 119.8，底 687.1 | sticky 容器 y=521.3，`padding:36px 10px 20px`，高 197.8；composer 卡 y=557.3，高 141.8，底 699.1（视口底 719 → 余 20） |
| 底部留白 | 视口 719 − 687.1 = 31.9（外层渐变内） | 20 |

## 4. 卡片内部排版与字体

| 项目 | Manus 构建/创建卡 | Flowy 构建/创建卡 |
| --- | --- | --- |
| 卡片 padding | `0 0 0 20` | `16px 22px` |
| 图标 | 24×24 SVG（`buildMarkDark.svg` 等） | 28×28 内联 SVG（蓝纸飞机 / 三个橙点） |
| 标题 | 16px / 500 / `#e6e6e6`，距卡顶 61 | 18px / 500 / `#f5f5f5`（暗） |
| 描述 | 14px / 400 / `#8c8c8c` | 13px / 400 / `#969696`（暗），`max-width:46%` |
| 插画 | 卡片内置背景线稿（点阵/圆环/纸片，低对比融入底色），无独立 DOM 插画 | 绝对定位 SVG，`right:16px`，宽 `min(44%,196px)`，垂直居中（实测 196×122.5） |
| 从本地文件开始 | 270×272，padding 12；标题 16/500 居中；副标题 14/400 `#8c8c8c`；右上 badge「仅限 Studio」12/400 `#b3b3b3`；图标为**描边**文件夹（无填充） | 289.6×324，padding `28px 20px`；标题 18/500 居中；**无副标题、无 badge**；插画为 112px 宽**白色填充**文件夹 |
| 圆角 | 12 | 设计意图 20；当前 main 实测 构建=20、创建=6、本地文件=6（见 §5.4） |

字体：两者 computed `font-family` 首项都是 `-apple-system`（系统字体栈），无自定义字体。

## 5. 视觉与图案

### 5.1 Manus（暗色）

- 页面背景：侧栏 `#1f1f1f`、主区 `#1a1a1a`、body `#242424`、输入卡 `#242424`、输入卡顶部工具条 `#1c1c1c`。
- 卡片：`background: rgba(255,255,255,0.04)`（在 #1a1a1a 上合成为 ≈`#232323`）；**无边框、无阴影**；圆角 12。
- 文字色：主 `#e6e6e6`、次 `#8c8c8c`、弱 `#b3b3b3`；强调色 `rgb(26,147,254)`；白色。
- 输入卡：border `0.888889px rgba(255,255,255,0.12)`、radius 22、顶部工具条 radius `22px 22px 0 0` + 内描边 `inset 0 0 0 1px rgba(255,255,255,0.14)`；外阴影几乎为 0（`rgba(0,0,0,0.02) 0 12px 32px`）。
- 图标/装饰：单色 24px 图标 + 蓝图风线稿/点阵/圆环背景（低对比），运营卡为纯黑底图片 + 13px/700 标题 + 12px 60% 白副标题。
- 案例卡：无。

### 5.2 Flowy（暗色，当前 main 实测）

- 页面背景 `#2c2c2e`；卡片 `#3a3a3c`；卡片 hairline 通过 `box-shadow: 0 0 0 1px #303030` 绘制，另加 `0 2px 12px 5% 文本色` 软阴影。
- 文字色：主 `#f5f5f5`、次 `#c7c7c7`、弱 `#969696`。
- 插画为**内联 SVG 且颜色硬编码**：填充 `#ffffff / #f4f6f8 / #e7ebf0 / #fbfcfd`，描边 `#b7bec7 / #c5cbd3 / #d7dbe2`；强调色 `#4c8dff`（构建）/ `#ff9a3c`（创建）。源码：`ui/src/renderer/pages/guid/components/FeatureCards.tsx:87-153`。
- 暗色主题下的结果：白填充在 `#3a3a3c` 卡底上成为大面积高亮块；「从本地文件开始」的文件夹插画几乎是纯白团（见 [flowy-studio-folder-dark.png](assets/2026-11-06-home-new-task/flowy-studio-folder-dark.png)）。
- 亮色主题下的结果：卡底 `#ffffff` 与插画白填充同色，填充视觉消失、只剩描边（实测 card bg `rgb(255,255,255)`，path fill `rgb(255,255,255)`；见 [flowy-home-live-light-1280x719.png](assets/2026-11-06-home-new-task/flowy-home-live-light-1280x719.png)）。
- 会员卡：416×192，暗色渐变（`#1a2333 → #242233 → #2c2830 → #3a2c28`）+ plus 点阵（24×10）+ 日期 + 环形渐变印章 + 邮戳；入参来自 `GuidHomeCard.module.css`，含正反面翻转。
- 视频 chip：`#121212` + 星点，radius 16，38×38 缩略图 + 16×16 播放圆点；标题 13px/700、副标题 11px/400/58% 白。
- 画廊卡：照片/截图封面 196 高，radius 16；左上 badge 26 高、radius 8、`rgba(20,20,20,0.72)`；标题在卡**外**下方 10px，13px/400 居中。
- 输入卡：`#3a3a3c`、radius 24、border `0.888889px rgba(229,229,229,0.28)`、shadow `0 18px 52px 11%`；内部 panel padding 12；「在项目中工作」chip 123.6×24/12px；输入行 40 高；底排按钮 28 高（+ 圆形 28、YOLO 模式选择 84.6×28 r14、日常办公 136×28 pill、模型选择 176×28、发送圆按钮 26，teal `#2dd4bf`）。

### 5.3 两套图案体系对照

| 项 | Manus | Flowy |
| --- | --- | --- |
| 图案形态 | 低对比蓝图线稿/点阵，作为卡片背景；入口图案单色 | 具象等距纸片/文件夹，独立 SVG 元素 |
| 颜色来源 | 语义化（低透明白/灰） | 硬编码十六进制（浅色纸片） |
| 暗色表现 | 与卡底融合 | 高亮白色块 |
| 亮色表现 | 与卡底融合 | 填充与白卡同色 → 视觉消失 |
| 尺寸 | 装饰随卡走，不占布局 | 插画 196×122.5，绝对定位占卡片右 44% |

### 5.4 圆角规则冲突（当前 main 的实测事实）

- 源码意图：`FeatureCards.module.css:13` `.entry, .studio { border-radius: 20px; }`。
- 全局覆盖：`ui/src/renderer/styles/arco-override.css:44-48` 对普通 `<button>` 施加
  `border-radius: var(--flowy-radius-control, 6px) !important;`（`--flowy-radius-control: 6px`，见 `flowy-visual-system.css:51`）。
- 实测结果（当前 main，暗色/亮色一致）：构建卡 = `<article>` → **20px**；创建卡、从本地文件开始卡 = `<button>` → **6px**。
  证据：[flowy-build-card-corner.png](assets/2026-11-06-home-new-task/flowy-build-card-corner.png)、[flowy-create-card-corner.png](assets/2026-11-06-home-new-task/flowy-create-card-corner.png)。
- 用户提供的 v1.7.0 参考截图里，创建卡圆角像素折算 ≈17–20，且与构建卡视觉一致（非 6px 的锐角）；引入该 `!important` 规则的提交是 `18fa9901c`（2026-10-30 16:32），晚于 `v1.7.0` tag `c3cfeb0d2`（2026-10-30 14:33）。即：**该圆角不一致在当前 main 存在，v1.7.0 截图里不存在**。

## 6. 输入区（composer）

| 项 | Manus | Flowy |
| --- | --- | --- |
| 宽度/高度 | 768 / 外层 151.8（卡 119.8 + 顶部工具条 64 + 渐变） | 840 / 141.8（panel 140） |
| 圆角 | 22（顶部工具条 `22 22 0 0`） | 24（内 panel 12） |
| 背景/边框 | `#242424` / `0.889px rgba(255,255,255,0.12)` | `#3a3a3c` / `0.889px rgba(229,229,229,0.28)` |
| 顶部工具条 | 「云服务」「选择项目」pill，32 高、radius 9999、14px `#b3b3b3` | 一行「在项目中工作」chip（123.6×24、12px `#969696`） |
| 输入区 | contenteditable（tiptap），16px，宽 742.2 | 40 高输入行，宽 821.2，左 padding 7 |
| 底排 | 左：+ 按钮（32 圆）、工具按钮（32 圆）；右：语音/麦克风图标、发送圆按钮 32（`rgba(255,255,255,0.08)`，当前 disabled） | 左：+ 圆按钮 28、YOLO 模式 84.6×28、日常办公 136×28 pill；右：策略槽 108、模型选择 176×28、发送圆按钮 26（teal） |
| 阴影 | 极弱（`rgba(0,0,0,0.02) 0 12px 32px`）+ 工具条内描边 | `0 18px 52px 11%` + 1px 外描边 |

## 7. 交互语义

| 入口 | Manus | Flowy（当前 main） |
| --- | --- | --- |
| 构建卡 | `<button>` | `<article>`，**不可点击**（只有 hover 插画位移动画） |
| 创建卡 | `<button>` | `<button data-testid="guid-home-create">`，打开幻灯片面板；后端不支持 goal command 时退化为静态 `<article>` |
| 从本地文件开始 | `<button>` | `<button data-testid="guid-home-studio">`，打开本地工作区 |
| Hero 区 | 字标（静态）+ 运营卡（button） | 会员卡（可点击翻转、可拖拽的 FlipCard）+ 视频 chip（button → `/video-generation`） |
| 案例卡 | 无 | 3 个 button（视频 → `/video-generation`，应用/图片 → 预填 prompt） |
| 卡片 hover | 未采集 | 卡底提亮 + hairline 提亮 + 软阴影；插画位移（构建 slab ±、创建 sheet ±、文件夹 sheets −14Y）；chip 上浮 2px、封面 scale 1.08 |

## 8. 响应式（源码/类名事实，未实测窄视口）

Manus：

- 内容容器 `max-w-[768px]`；网格类名 `grid grid-cols-1 gap-[12px] @[700px]:grid-cols-[minmax(0,1fr)_270px]` → **容器 <700px 时单列**（卡片纵向堆叠、右卡变成普通行），≥700px 时 1fr + 270。
- 侧栏 300 固定；输入区随内容列宽（768）走。

Flowy：

- `FeatureCards.module.css:199-219`：`@media (max-width:860px)` → board 单列、studio 卡最小高 220。
- `index.module.css:2794-2799`：`@media (max-width:860px)` → masthead 变 `minmax(0,1fr) auto`。
- `ShowcaseCards.module.css:77-85`：`@media (max-width:860px)` → 3 列变 1 列、卡高 160。
- `.guidLayout`：`width: clamp(360px, calc(100% - 64px), 920px)`，`padding: 0 40px`；`.guidPrimaryStage`/输入区都用 920 上限。

## 9. 与用户提供的参考截图差异

- **Manus 参考截图**（用户提供，1280×631 CSS 折算）：与 live 同一版式家族；卡片、圆角、配色一致。差异：live 当前多了「manus」大 SVG 字标（376×64），且没有参考截图里那条横向推广 banner；右上运营卡内容在两次采集间从「创建你的视频」变成「游戏开发」。无法确认是版本差异还是 A/B/运营轮换。
- **Flowy 参考截图**（v1.7.0，zh-CN，1280×719 CSS 折算）：正文区几何与当前 dev 实测一致（版心 840、538/290 分栏、156 行高、masthead 416+chip 右对齐）；差异：① 参考截图三卡圆角都≈20（当前 main 创建/本地文件卡=6，见 §5.4）；② 参考截图是桌面外壳（侧栏≈213、中文、登录态会员卡），dev web 外壳不同；③ 参考截图会员卡显示登录用户信息，dev 为游客态。

## 10. 事实性观察清单（不含设计建议）

1. Flowy masthead 在 1280 宽下，会员卡右缘到视频 chip 左缘之间有 **239px** 空白（`538.4` 左列只放 `416` 的卡、`289.6` 右列只靠右放 `185` 的 chip）。
2. Flowy 三种圆角体系并存：卡 20（设计意图，实际只有 article 生效）/ 6（按钮被全局规则覆盖）/ 24（composer、会员卡）、16（chip、画廊卡）、12（composer 内 panel）。
3. Flowy 首页并存三套视觉语言：会员卡（渐变+点阵+邮戳）、功能卡（浅色手绘等距贴纸）、画廊卡（照片封面 + 卡外标题）。
4. Flowy 功能卡插画颜色写死在 TSX 里，`--flowy-raised` 卡底随主题变化而插画不变；暗色下高亮、亮色下填充消失（§5.2）。
5. Flowy 功能卡 156 高、内部分栏 46% 文案 + 44% 插画，两张左卡信息量（图标+标题+描述+插画）与右卡（插画+标题）不对称；右卡没有副标题。
6. Flowy 构建卡不可点，创建/本地文件卡可点，会员卡可翻转可拖拽，三者点击反馈不一致（§7）。
7. Flowy 首屏内容高 800.2 > 视口 683.1，画廊完全在首屏之外；sticky 输入区覆盖入口卡底部约 51px（§3）。
8. Flowy 版心 840（外层 920 + 40 padding）比 Manus 768 宽 9.4%；卡片高 156 比 Manus 130 高 20%；卡片间距同为 12。
9. Manus 内容列宽 768，全部首屏内容（字标+3 卡+输入框）在 1280×719 内完成且无页面滚动；Manus 无案例画廊区块。
10. Manus 入口卡无独立插画元素、无边框无阴影、背景仅 `rgba(255,255,255,0.04)`，装饰以低对比线稿形式内置；Flowy 入口卡有白底 hairline + 软阴影 + 独立插画 DOM。
11. 两者字体都是系统字体栈（`-apple-system`），都使用暗色主色（Manus `#1a1a1a`，Flowy `#2c2c2e`），但卡片底色层级相反：Manus 卡片比主区**亮 9 级**（26→35），Flowy 卡片比主区**亮 14 级**（44→58）。
12. 响应式断点不同源：Flowy 用 `max-width:860px` 媒体查询切单列；Manus 用容器查询 `@700px` 切单列。

## 11. 原始测量数据

| 文件 | 内容 |
| --- | --- |
| [evidence-manus-1280.json](assets/2026-11-06-home-new-task/evidence-manus-1280.json) | Manus 1280 视口：导航/网格/卡片/字标/promo/composer 的 rect + 样式 |
| [evidence-manus-composer.json](assets/2026-11-06-home-new-task/evidence-manus-composer.json) | Manus 输入区 DOM 链（textbox → 工具条 → 卡片）的 rect + 样式 |
| [evidence-flowy-dark-1280.json](assets/2026-11-06-home-new-task/evidence-flowy-dark-1280.json) | Flowy 暗色 1280：全部 data-testid rect、board/masthead/gallery/composer 链、按钮样式 |
| [evidence-flowy-styles-dark-1280.json](assets/2026-11-06-home-new-task/evidence-flowy-styles-dark-1280.json) | Flowy 暗色：卡片内部元素样式、命中圆角规则列表、CSS 变量值 |
| [evidence-flowy-styles-light.json](assets/2026-11-06-home-new-task/evidence-flowy-styles-light.json) | Flowy 亮色：卡底/插画 path 填充与描边、亮色 CSS 变量 |
