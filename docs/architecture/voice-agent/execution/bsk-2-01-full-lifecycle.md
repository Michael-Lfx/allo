# bsk-2-01 验证剧本：设置页全态流转与两步确认 (Full Lifecycle & Inline Deletion)

> **对应波次**：Wave 2 (Phase 2 / Gate 2)
> **任务卡**：T2-07
> **上游契约**：[ui-motion.zh.md](../ui-motion.zh.md) §3 与 [wave-2-decision-tts.zh.md](wave-2-decision-tts.zh.md) §4
> **验收映射**：TC-AM-017

---

## 1. 目标与范围

验证桌面端 SPA 设置面板中“语音伴侣”详情页在全生命周期全部七个状态下的交互规范、动效节奏与两步删除闭环：
1. **全七态覆盖**：遍历 `NotInstalled → ProbingSource → Downloading → VerifyingSha256 → Ready → Removing → NotInstalled` 完整生命周期，并在异常分支覆盖 `Failed` 态与行内重试；
2. **状态映射严格一致**：七态行内表现、文案、图标与控制槽完全契合 [ui-motion.zh.md](../ui-motion.zh.md) §3 映射表；
3. **就绪微动效**：`Ready` 终态唯一愉悦时刻，就绪图标在 240ms 内完成 `scale(0.9) → scale(1)` 缓动（`cubic-bezier(0.16, 1, 0.3, 1)`），全页无彩带无弹跳；
4. **行内两步确认与防误触**：【释放空间】采用行内两步确认，点击后原地变为“确认释放？”，3 秒无操作自动还原；确认后向 `DELETE /api/voice/models` 发起请求，进入 `Removing` 态后静默过渡回 `NotInstalled`，**严格禁止使用 Modal 弹窗**；
5. **几何恒定**：全生命周期行高保持恒定，发丝轨道常驻 2px 且仅以 `transform: scaleX()` 驱动，数字排版采用 `tabular-nums`。

---

## 2. 前置条件

1. 启动 WebUI 开发服务：`bun run dev:web`（`http://localhost:5173`，免认证）；
2. 后端模型管理服务与 SSE 进度推送通道运行正常（或使用 fixtures 模拟接口）；
3. 浏览器访问 `#/settings/voice`。

---

## 3. 执行步骤 (Steps)

| 步序 | 操作 | 预期现象 |
| :--- | :--- | :--- |
| 1 | 访问 `http://localhost:5173/#/settings/voice` | 渲染 `NotInstalled` 态：文案“AI Mate 语音交互套件（约 249MB）”，右侧展示【下载并启用】主按钮 |
| 2 | 点击【下载并启用】按钮 | 触发 `POST /api/voice/models/install`，状态转为 `ProbingSource`，文案变为“正在选择加速源”，按钮原地加载态，发丝轨道 180ms 淡入 |
| 3 | 后端推送 `Downloading` SSE 事件 | 状态转入 `Downloading`，文案更新为“正在下载语音模型 (n/7)”，控制槽展示百分比与实时下载速率，发丝轨道 `scaleX` 推进 |
| 4 | 后端推送 `VerifyingSha256` SSE 事件 | 状态转入 `VerifyingSha256`，文案更新为“正在校验完整性 (n/7)”，发丝轨道维持完整可见 |
| 5 | 后端推送 `Ready` 终态事件 | 状态切入 `Ready`，发丝轨道淡出；左侧渲染 `CheckOne` 绿色就绪图标并在 240ms 内从 scale 0.9 平滑缩放至 1.0；右侧呈现启用开关与【释放空间】文本按钮 |
| 6 | 点击【释放空间】文本按钮（首次点击） | 按钮原地切换为危险态【确认释放？】（`voice.actions.confirmRelease`），同时启动 3 秒无操作倒计时；页面无任何 Modal 遮罩弹出 |
| 7 | [自动还原分支] 静置 3 秒不做任何操作 | 倒计时归零，按钮原地自动还原为初始【释放空间】状态，防误触机制生效 |
| 8 | 再次点击【释放空间】按钮进入确认态 | 按钮原地再次变为【确认释放？】 |
| 9 | 3 秒内点击【确认释放？】（确认删除） | 立即清理倒计时定时器，向 `DELETE /api/voice/models` 发送删除请求；状态切换为 `Removing`，文案显示“正在释放磁盘空间”，控制槽呈现【释放空间】禁用加载态 |
| 10| 后端返回 200/204 删除成功 | 状态静默过渡回初始 `NotInstalled` 态，控制槽还原为【下载并启用】主按钮，行高全程无抖动 |
| 11| [异常与重试分支] 模拟安装失败 | 状态转入 `Failed`，展示红色错误图标与本地化错误原因，右侧呈现【重试】按钮；点击【重试】可重新发起安装 |

---

## 4. 机械断言口径 (Assertions)

1. **七态语义与视觉映射断言**：
   - `NotInstalled`：包含【下载并启用】按钮且未显示进度轨道；
   - `ProbingSource`：按钮处于 loading disabled 状态；
   - `Downloading`：控制槽包含 `tabular-nums` 速率与百分比文案；
   - `VerifyingSha256`：控制槽展示校验进度比率 `n/7`；
   - `Ready`：呈现 `CheckOne` 图标、Switch 开关及【释放空间】按钮；
   - `Failed`：呈现 `CloseOne` 错误图标及【重试】按钮；
   - `Removing`：呈现【释放空间】loading 状态并展示“正在释放磁盘空间”。
2. **Ready 终态愉悦微动效断言**：
   - 就绪图标元素应用了 `240ms` 动效过渡；
   - 过渡时间函数为 `cubic-bezier(0.16, 1, 0.3, 1)`；
   - 图标初始缩放值为 `scale(0.9)`，完成态为 `scale(1)`；
   - 页面无弹跳、无彩带、无附加无关动画。
3. **行内两步确认断言 (Inline Two-Step Confirmation)**：
   - 首次点击后，按钮文本从 `t('voice.actions.releaseSpace')` 变为 `t('voice.actions.confirmRelease')`；
   - 未点击确认时，3000ms 后自动还原为 `t('voice.actions.releaseSpace')`；
   - 点击确认触发 HTTP `DELETE` 请求至 `/api/voice/models`，附带后端认证头；
   - 全程 `document.querySelectorAll('.arco-modal, [role="dialog"]').length === 0`。
4. **几何恒定与帧率断言 (Geometric Invariance & 60fps)**：
   - 全七态流转过程中，行容器高度极差 $\Delta H \le 1.0\text{px}$；
   - 动效执行期间持续帧率 $\ge 55\text{fps}$（目标 60fps）；
   - 发丝轨道高度恒为 2px，仅响应 `transform: scaleX()`。

---

## 5. 通过标准 (Pass Criteria)

- 全部 4 项机械断言 100% 通过；
- 两步确认在 3 秒超时未响应时 100% 自动撤销；
- 确认删除后状态机正确回到 `NotInstalled` 并可再次开始循环；
- 控制台无未处理错误或警告。
