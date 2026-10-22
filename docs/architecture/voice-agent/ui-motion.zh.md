# AI Mate 交互界面与动效设计规范 (UI Surfaces & Motion Design)

> **状态**：🎨 子规范 · 实施契约级
> **上游基线**：本文档是架构基线 [interaction.zh.md](interaction.zh.md) §3.10 的完整展开；设计体系继承 PRODUCT.md「Quiet Kinetic Workspace」基调与 DESIGN.md 设置壳契约；状态机定义以 [model-management.zh.md](model-management.zh.md) 的 `ModelInstallState` 为唯一事实源。
> **适用范围**：桌面端“语音伴侣”设置详情页（React 19 / Arco，桌面 SPA 内）与无屏配对 Web 页（设备自托管极简页面）。

---

本方案仅存两块人机界面：**桌面端“语音伴侣”设置详情页**与**无屏配对 Web 页**。二者统一继承 Flowy 既有设计体系（PRODUCT.md “Quiet Kinetic Workspace” 基调与 DESIGN.md 设置壳契约），只允许状态驱动的克制动效，严禁任何一次性视觉发明。

## 1. 物理场景与设计基调
- **桌面端场景**：用户坐在明亮办公室的显示器前，第一次为 Flowy 桌面端启用语音能力。他只需要知道三件事：要下载什么、下到哪了、好了没有，然后立刻回到自己的工作流。基调：静时安定，动时即应。
- **无屏配对场景**：用户刚给 AI Mate 通电，单手持手机站在客厅，环境光多变，设备正在语音播报配对码。页面必须在几十秒内支持“听懂、输入、看懂进度”。基调：移动优先、高对比、状态永远可读。

## 2. 动效令牌总线 (Motion Token Bus)
所有界面动效只允许引用以下三档时长与两条曲线，严禁自定义发明新时长或新曲线：

| 令牌 | 时长 | 用途 |
| :--- | :--- | :--- |
| `motion.feedback` | 120ms | 按下 / 悬停 / 聚焦的前景反馈（仅颜色与不透明度） |
| `motion.state` | 180ms | 状态内容交叉淡化、进度轨道淡入淡出 |
| `motion.spatial` | 240ms | 页面级转换与唯一的就绪确认“愉悦时刻”上限 |

- **曲线**：入场 `cubic-bezier(0.16, 1, 0.3, 1)`（ease-out-expo）；退场时长恒为入场的 75%；**严禁 bounce / elastic 曲线**；
- **材质**：仅允许 `opacity` 与 `transform` 两通道；严禁动画化 `width` / `height` / `margin` 等布局属性（进度条一律以 `transform: scaleX()` 驱动）；
- **无障碍降级**：`prefers-reduced-motion` 生效时取消全部位移与淡化，状态即时切换且语义完整保留；动效绝不阻塞输入、导航或网络回调。

## 3. 场景 A：桌面端“语音伴侣”设置详情页
- **落位与容器契约**：设置 → 能力与扩展组 → “语音伴侣”详情页（`form` 型，内容上限 960px），严格遵守设置壳 `Page Header → Section → List → Row` 契约：模型管理是该 Section 下的一行标准设置行（左侧文案区 + 右侧 220~280px 控制槽），**严禁独立卡片容器与嵌套卡片**；
- **几何恒定与行内解剖**：行高在全部状态下保持不变；进度呈现为行底部一条常驻渲染的 2px 发丝轨道（空闲时 `opacity: 0`），填充仅走 `scaleX`；百分比与速率文本使用 `font-variant-numeric: tabular-nums` 消除数字宽度抖动；SSE 进度事件在前端按 200ms / 每帧节流合并，文本与进度条同帧更新；
- **状态视觉映射**（与 [`ModelInstallState`](model-management.zh.md) 一一对应；任何状态都由图标 + 文案 + 语义色三要素共同表达，绝不单靠颜色）：

| `ModelInstallState` | 行内表现 | 控制槽内容 | 动效 |
| :--- | :--- | :--- | :--- |
| `NotInstalled` | “AI Mate 语音交互套件（约 249MB）” + 一行功能说明 | 主按钮【下载并启用】 | 无 |
| `ProbingSource` | 阶段文案“正在选择加速源” | 主按钮原地加载态 | 发丝轨道 180ms 淡入，不定态微光脉冲 |
| `Downloading` | 阶段文案（当前文件 `n/7`） | 百分比 + 实时速率 | 轨道 `scaleX` 随 SSE 推进（180ms 缓出） |
| `VerifyingSha256` | 阶段文案“正在校验完整性” | 校验进度 `n/7` | 不定态脉冲延续 |
| `Ready` | 就绪图标 + “已就绪” | 启用开关 + 【释放空间】文本按钮 | 唯一愉悦时刻：就绪图标 240ms 内 scale 0.9→1，全页无彩带无弹跳 |
| `Failed` | 错误图标 + 一行本地化原因 | 【重试】主按钮 | 错误内容 180ms 交叉淡化入场，绝不抖动布局 |
| `Removing` | 文案“正在释放磁盘空间” | 【释放空间】加载态 | 静默过渡至 `NotInstalled` |

- **删除确认**：【释放空间】采用行内两步确认（点击后原地变为“确认释放？”，3 秒无操作自动还原），严禁 Modal 弹窗；
- **工程门禁**：全部用户可见文案走 i18n（zh-CN / en-US 双字典对称）；颜色只允许语义令牌（`bg-fill-1`、`text-t-primary`、`var(--border-base)`、`var(--flowy-attention)` 及主题 success / warning / danger 令牌）；验收须通过 `bun run check:theme`、`bun run check:i18n` 并人工核查全部内置明暗双主题。

## 4. 场景 B：无屏配对页 (Mobile-First Pairing & Progress Surface)
- **形态**：设备自托管的独立极简页面（不属于桌面 SPA），360px 移动优先基线；自带一组映射 Flowy 语义令牌的 CSS 变量并响应 `prefers-color-scheme`；正文对比度 ≥ 4.5:1，触控目标 ≥ 44px；页面语言跟随浏览器 `Accept-Language` 自动协商（MVP 提供 zh-CN / en-US 双语资源，与桌面端共享同一 i18n 键值集）；
- **自上而下结构**：小型品牌标识 → 设备状态行 → 配对码区 → 模型就绪投影区 → 完成态；
- **配对码输入**：沿用桌面端 `OtpCodeInput` 既有契约语言：单一真实输入框 + 6 格视觉投影（3+3 分组）、`inputmode="numeric"`、输满自动校验一次、错误反馈局部呈现且绝不抖动布局、失败后首格自动聚焦；连续 5 次失败进入 15 分钟锁定并显示本地化倒计时；
- **模型就绪投影区**：与桌面端共用同一状态词表与同一发丝进度语言，状态变化通过 `aria-live="polite"` 向读屏器播报；
- **服务生命周期**：配对 / 进度 Web 服务与配对码 10 分钟有效期**解耦**，持续服务至 `ModelManager` 抵达 `Ready` 或 `Failed` 终态后才按闲置策略关闭，杜绝慢速网络下进度页中途消失；
- **声画一致性**：页面状态与设备听觉反馈（`guide.wav` / `done.wav` / `error.wav` 与就绪播报）由同一个 `ModelInstallState` 驱动，视觉状态绝不依赖音频播报成功与否。

## 5. 界面反模式红线（与全库设计法一致）
渐变文字、彩色侧边条、玻璃拟态、英雄式大数字模板、等大卡片阵列、Modal 优先思维一律禁止；界面文案不使用破折号修辞；动效只传达状态，不承担装饰职能。

## 6. 验收映射 (Acceptance Mapping)

本规范的生效验收锚定在基线文档 §5 的以下条目（测试执行与门禁判定以基线为唯一入口）：

| 条目 | 验收要点 |
| :--- | :--- |
| **TC-AM-017** 设置面板状态可视化与动效降级 | 行高恒定无布局抖动；进度条 `transform` 驱动且 60fps；状态切换 180ms 交叉淡化；`prefers-reduced-motion` 下全部即时切换且语义完整；`check:theme` 与 `check:i18n` 通过，明暗双主题人工核查通过 |
| **TC-AM-018** 配对页移动端闭环 | 6 位码输满自动校验；错误局部提示不抖动布局；连续 5 次失败锁定并显示倒计时；下载进度实时投影且页面服务持续至 `Ready` 终态；就绪语音播报与页面完成态同步出现 |
| **Gate 2** | 桌面设置页通过 `bun run check:theme` 与 `check:i18n`，reduced-motion 降级人工验收通过 |
