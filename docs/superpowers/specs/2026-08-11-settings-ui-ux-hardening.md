# Flowy 设置体系 UI/UX：多语言与响应式硬化规格

## 目标

在 Ink Studio + Quiet Kinetic 的既有设置体系上，解决经典浅色实机截图暴露的
i18n 泄漏、右侧控制槽挤压、多按钮换行与能力中心卡片密度问题。桌面端优先，
窄窗口和高缩放通过容器响应保持兼容。

## 已落实的设计契约

- 内置设置导航的标签键使用 `I18nKey`；应用分组统一为 `settings.groupApp`。
- `common.yes` 与 `common.no` 是正式本地化文字；普通开关不重复显示布尔状态。
- `SettingsRow` 的控制槽为 `compact`、`field`、`actions`、`compound` 四类。
- 文案列不超过约 70ch；操作组整体换行，按钮内部永不拆开图标和标签。
- 行容器在 760px 与 640px 发生结构性堆叠，不依赖单一视口断点。
- POI 的基础、提取、自动处理分组保持稳定 DOM，依赖控件在关闭时 Disabled。
- MoA 使用 Header、Section、List、Row，并以空状态代替伪输入框。
- Skills、Presets、MCP 使用最小 270px 的响应式网格、最多三个标签和不换行操作。
- Skills 三个导入路径收敛为一个菜单；市场来源在 720px 以下切换为 Select。

后续的市场卡片、详情交互与设定编辑器精修记录在
[`2026-08-11-settings-ui-ux-phase-2c.md`](2026-08-11-settings-ui-ux-phase-2c.md)。

## 兼容边界

- 不更改后端 API、配置键、持久化、路由、Tabs 查询参数或扩展协议。
- 组件接口仅新增内部 `SettingsRow.controlLayout` 与 `SettingsControlGroup`。
- 主题仍拥有颜色和开关选中态，设置组件只消费现有语义表面、边界与 Focus token。
- 经典浅色优先检查输入控件的 Raised 表面和细边界；所有内置主题仍须通过主题契约。

## 验收

- 简体中文、English 与 30% 长文案压力样本不显示原始翻译键、横向滚动或图标文本断裂。
- 1440×900、1280×800、150% 缩放及 767/768px 边界下，控制槽不重叠。
- 六套主题 Light/Dark 保持文本可读、Focus 可见、Disabled 和错误状态可辨。
- Web 开发版、Windows 桌面实机、macOS 平台边界和用户视觉验收分别记录。
