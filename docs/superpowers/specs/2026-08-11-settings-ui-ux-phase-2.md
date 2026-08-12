# Flowy 设置体系 UI/UX：阶段 2A 实施规格

## 阶段目标

在既有 Ink Studio 静态视觉与 Quiet Kinetic 交互规则上，建立一套可复用的设置
导航、页面、分组和行契约。阶段 2A 只迁移系统设置，先让用户确认密度、层级、主题
表现和操作手感；媒体、Skills、Presets、MCP 继续保持原布局，属于后续 2B。

## 静态基线与范围

- 基线：用户提供的系统设置截图（巨型灰色容器、通知折叠项、控制器对齐不稳定）。
- 桌面设置导航：应用、智能与内容、能力扩展、账户与其他四组。
- 窄窗口/Web 兼容：复用同一数据模型；不是独立手机设计。
- 页面结构：`SettingsPageHeader → SettingsGroup → SettingsList → SettingsRow`。
- 本阶段不改变后端 API、路由、查询参数、保存协议、迁移流程、Factory Reset 或扩展 iframe 内容。

## 导航和扩展状态契约

- 内置入口和扩展入口由一个 `SettingsNavGroup[]` 模型生成；桌面和窄窗口壳均消费它。
- 扩展 `before/after` 锚点保留，历史 `skills-hub/tools` 映射保留；无锚点、退休或当前不可见
  锚点落到“能力扩展”末尾。
- 云账户继续由开发者模式控制；空分组不渲染标题。
- 设置侧栏使用容器本地的移动选中指示器。它不会穿越主侧栏、Footer 或标题栏。
- 扩展设置状态为 `loading`、`ready` 或 `error`。`ready + []` 表示真正为空，不再伪装加载中。

## 系统设置样板

| Section | 行 |
| --- | --- |
| 常规 | 语言、发送快捷键、开机启动、保持唤醒、上传保存、Office 预览 |
| 通知 | 通知总开关、定时任务通知（总开关关闭时仍挂载且 Disabled） |
| 存储 | 工作目录、日志目录、迁移/重启/重试/取消/备份状态 |
| 开发者 | 开发者模式与原有解锁流程 |
| 危险操作 | 原有确认输入与不可关闭 Loading 的恢复出厂设置 |

自动保存成功保持安静。失败必须恢复原值，并在当前页面显示本地化错误。工作目录迁移、
日志打开和 Factory Reset 的调用顺序与原有实现保持不变。

## 非目标与门禁

- 不迁移 Browser Use、Computer Use、媒体、Skills、Presets、MCP 或其余设置页。
- 不新增主题变量、UI 依赖、动画依赖或持久化字段。
- 动效只使用已有 120/180/240ms token；Reduced Motion 下状态立即变化。
- 阶段完成后停在用户体验检查点。确认前不得进入 2B。

## 验收矩阵

- 六套内置主题 × Light/Dark；1440×900、1280×800 与 767/768px。
- 四组导航、开发者模式显隐、动态扩展、选中定位与滚动。
- 长中英文、长目录、键盘 Tab/Enter/Space、Select、Switch、Disabled、错误与危险操作。
- Web 开发版和 Windows 桌面实机分别记录；浏览器自动化或静态测试不能替代主题和手感验收。

## 关联决策

第 0/1 阶段的设计根基和历史验证保留于
[`2026-08-11-interaction-style-optimization-design.md`](./2026-08-11-interaction-style-optimization-design.md)
及其 Phase 1 审计记录。本规格只定义设置阶段的增量实现。

后续详情页扩展、显式保存状态和能力中心 Tab 契约见
[`2026-08-11-settings-ui-ux-phase-2b.md`](./2026-08-11-settings-ui-ux-phase-2b.md)。
