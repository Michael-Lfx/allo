# bsk-1-01 验证剧本：设置行实时进度与动效降级 (Progress & Motion Verification)

> **对应波次**：Wave 1 (Phase 0 + Phase 1 / Gate 1)
> **任务卡**：T1-09
> **上游契约**：[ui-motion.zh.md](../ui-motion.zh.md) §3 与 [wave-1-acoustic.zh.md](wave-1-acoustic.zh.md) §4
> **验收映射**：TC-AM-014（部分）/ TC-AM-017（部分）

---

## 1. 目标与范围

验证桌面设置页在模型下载与自检生命周期中的实时进度呈现与动效契约：
1. 点击【下载并启用】向 `POST /api/voice/models/install` 触发真实安装任务；
2. 订阅 `/api/voice/models/progress` SSE 进度长连接，状态流转覆盖 `ProbingSource → Downloading → VerifyingSha256 → Ready`（或网络异常时的 `Failed` 态与行内重试）；
3. **动效总线合规**：发丝轨道恒为 2px，仅由 `transform: scaleX()` 推进，**严格禁止 `width` / `height` 属性动画**；
4. **排版稳定性**：百分比与速率数字使用 `font-variant-numeric: tabular-nums`，文本更新时行宽不抖动；行高在全状态流转中严格保持恒定；
5. **帧率与节流**：前端对 SSE 进度事件按 200ms / 每帧进行节流合并，文本与进度条同帧更新。

---

## 2. 前置条件

1. 启动 WebUI 开发服务：`bun run dev:web`（`http://localhost:5173`）；
2. 后端探针或 fixtures HTTP 模拟源可用；
3. 打开桌面端设置面板导航至 `#/settings/voice`。

---

## 3. 执行步骤 (Steps)

| 步序 | 操作 | 预期现象 |
| :--- | :--- | :--- |
| 1 | 访问 `http://localhost:5173/#/settings/voice` | 渲染未安装态行：文案“AI Mate 语音交互套件（约 249MB）”，主按钮【下载并启用】可用 |
| 2 | 点击【下载并启用】按钮 | 触发 `POST /api/voice/models/install`，按钮原地进入加载态 |
| 3 | 后端切入 `ProbingSource` 状态 | 阶段文案变为“正在选择加速源”；发丝轨道 180ms 淡入（微光脉冲） |
| 4 | 后端推入 `Downloading` SSE 事件 | 阶段文案切换为“正在下载语音模型 (n/7)”；控制槽展示百分比与实时下载速率（如 `4.8 MB/s`） |
| 5 | 观察发丝轨道推进过程 | 发丝轨道由 `transform: scaleX(progress)` 缓动平滑推进；检查 DOM 样式，确认无 `transition: width` |
| 6 | 观察数字跳动 | 百分比与速率数字等宽排列（`tabular-nums`），无横向微小抖动 |
| 7 | 后端切入 `VerifyingSha256` 状态 | 阶段文案显示“正在校验完整性 (n/7)”；发丝轨道保持完整可见 |
| 8 | 后端切入 `Ready` 终态 | 行内呈现就绪图标 +“已就绪”，右侧呈现启用开关与【释放空间】按钮；就绪图标 240ms 内 scale 0.9→1，发丝轨道淡出 |
| 9 | [异常分支] 模拟下载中断/失败 | 状态转入 `Failed`，行内展示错误图标与本地化错误原因，右侧呈现【重试】按钮，行高恒定无跳跃 |
| 10| 点击【重试】按钮 | 重新触发 `POST /api/voice/models/install` 并重新建立 SSE 连接 |

---

## 4. 机械断言口径 (Assertions)

1. **动效材质与属性断言 (Motion Channel & Property Assertion)**：
   - 提取发丝轨道内部填充条元素 `computedStyle`：
     - `computedStyle.transitionProperty` 包含 `transform`（或 `all` 中的 scaleX），**不得包含 `width` / `max-width` / `flex-basis`**；
     - 填充进度读取为 `computedStyle.transform` 中的矩阵（对应 `scaleX` 缩放值）；
     - 发丝轨道高度：`computedStyle.height === '2px'`。
2. **Tabular-nums 断言**：
   - 百分比及速率文本节点包含 `tabular-nums` 类；
   - 其计算样式 `fontVariantNumeric` 包含 `tabular-nums`。
3. **行高恒定断言 (Geometric Invariance)**：
   - 在 `NotInstalled`、`ProbingSource`、`Downloading`、`VerifyingSha256`、`Failed`、`Ready` 六个状态下，获取设置行元素 `getBoundingClientRect().height`；
   - 极差 $\Delta H = |H_{\max} - H_{\min}| \le 1.0\text{px}$（允许 1px 亚像素误差），绝不产生布局拉伸。
4. **节流更新断言 (Throttle Assertion)**：
   - 高频连续灌入 10 个 SSE 进度包（间隔 20ms）；
   - 前端组件实际 state 触发与 DOM 渲染次数被合并节流至 $\le 2$ 次（对应 200ms 合并窗）。

---

## 5. 通过标准 (Pass Criteria)

- 全部 4 项机械断言 100% 通过；
- 浏览器无警告或错误输出；
- 界面在状态流转期间保持流畅，符合 Quiet Kinetic 克制设计基调。
