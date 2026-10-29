# bsk-0-01 验证剧本：桌面端语音伴侣设置行壳 (Settings Shell)

> **对应波次**：Wave 0 (Phase S / Gate S)
> **任务卡**：T0-16
> **上游契约**：[ui-motion.zh.md](../ui-motion.zh.md) §3 与 [wave-0-skeleton.zh.md](wave-0-skeleton.zh.md) §5

---

## 1. 目标与范围

验证桌面端 SPA 设置面板中“语音伴侣”详情页行壳的正确渲染与交互规范：
1. 侧边栏导航在【能力与扩展】分组下正确展示【语音伴侣】入口（含 `Voice` 图标与高亮状态）；
2. 页面遵循 `Page Header → Section → List → Row` 结构契约，宽度上限 960px（`layout="form"`），严禁独立卡片容器 (`Card`) 与弹窗 (`Modal`)；
3. 行高在未安装状态下恒定，底部渲染 2px 发丝轨道（空闲时 `opacity: 0`，以 `scaleX` 驱动）；
4. 文案 100% 走 `voice.*` 双字典；
5. 在 M1 骨架阶段，右侧 240px 控制槽内主按钮【下载并启用】呈现禁用态 (`disabled`)。

---

## 2. 前置条件

1. 启动 Web 开发服务器：`bun run dev:web`（运行于 `http://localhost:5173`，免登录认证）；
2. 后端提供 `GET /api/voice/models/status` 接口，当前返回 `{ "state": { "status": "NotInstalled" } }`。

---

## 3. 执行步骤 (Steps)

| 步序 | 操作 | 预期现象 |
| :--- | :--- | :--- |
| 1 | 访问 `http://localhost:5173/#/settings/system` | 打开桌面端设置面板 |
| 2 | 在左侧导航栏定位【能力与扩展】分组中的【语音伴侣】选项 | 展示 `Voice` 麦克风图标与“语音伴侣”标签 |
| 3 | 点击【语音伴侣】导航项 | 路由切换至 `#/settings/voice`，激活项滑动指示器平滑对齐 |
| 4 | 观察主内容区域渲染 | 页面标题展示“语音伴侣”，描述文案展示“管理本地声学模型与语音交互功能” |
| 5 | 检查“语音能力”Section 下的“模型管理”设置行 | 行左侧展示“AI Mate 语音交互套件（约 249MB）”与功能说明；右侧展示【下载并启用】按钮 |
| 6 | 检查按钮交互态与样式 | 按钮为 Arco primary 风格，处于不可点击禁用态 (`disabled`) |
| 7 | 检查行底发丝轨道 | 底部常驻渲染 2px 发丝轨道，当前处于 `opacity: 0` 与 `scaleX(0)` 状态 |

---

## 4. 机械断言口径 (Assertions)

1. **结构契约**：
   - 容器属性：`data-settings-page-content` 包含 `settings-page-content--form`（宽度上限 960px）；
   - DOM 树结构：`SettingsPageHeader → SettingsSection → SettingsList → SettingsRow`；
   - 负向断言：页面 DOM 中 `document.querySelectorAll('.arco-card, [role="dialog"], .arco-modal').length === 0`。
2. **文案 i18n 完整度**：
   - 标题元素文本严格等于 `t('voice.title')` / `t('voice.settings.sectionTitle')`；
   - 行标题文本严格等于 `t('voice.phases.notInstalled')`（zh-CN: "AI Mate 语音交互套件（约 249MB）"；en-US: "AI Mate Voice Interaction Suite (~249MB)"）；
   - 主按钮文本严格等于 `t('voice.actions.downloadAndEnable')`（zh-CN: "下载并启用"；en-US: "Download & Enable"）。
3. **控制槽与发丝轨道**：
   - 控制槽容器宽度在 220px ~ 280px 区间（实测 240px）；
   - 主按钮存在 `disabled` 属性与 `arco-btn-disabled` 类；
   - 2px 发丝轨道在行底部，计算样式 `height: 2px`，`opacity: 0`，`transform: scaleX(0)`，无任何 `width` 过渡。

---

## 5. 通过标准 (Pass Criteria)

- 全部 3 项机械断言 100% 通过；
- 页面切换与渲染帧率保持 60fps，行高在渲染前后严格一致，无布局跳变；
- 控制台无未捕获异常或 React Warning。
