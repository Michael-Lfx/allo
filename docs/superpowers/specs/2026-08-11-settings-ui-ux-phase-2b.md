# Flowy 设置体系 UI/UX：阶段 2B 实施规格

## 目标与边界

阶段 2B 将 Phase 2A 的导航和系统页面样板扩展到设置详情页。视觉继续遵循
Ink Studio + Quiet Kinetic：工作区静止时密度克制，选择、保存、错误和权限变化时
提供短而明确的反馈。

- 保持后端 API、路由、查询参数、配置键、持久化与跨窗口同步不变。
- `SettingsPageWrapper` 是普通设置页唯一的垂直滚动所有者；页面内容不得再叠加
  `NomiScrollArea`。
- `form` 最大宽度为 960px，`hub` 最大宽度为 1180px。
- 页面内容切换仅 120ms opacity；不滑入、不抬升、不动画高度或位置；Reduced Motion
  下立即切换。
- 不新增设置搜索、依赖或主题必填变量；扩展 iframe/Webview 不接受宿主 CSS 注入。

## 共享详情页契约

`SettingsPageHeader → SettingsSection → SettingsList → SettingsRow` 是设置页的默认
骨架。`SettingsStatus` 表达 neutral、success、warning、error、restart-required；
`SettingsPermissionRow` 补充权限和恢复动作；`SettingsEmptyState` 处理无内容与失败；
`SettingsActionBar` 只服务显式保存且有草稿的页面。

桌面 `SettingsRow` 的控制槽为 220–280px，普通行最小高度 60px。低于 768px 时控件
移至说明下方并占满宽度。只有整行本身可操作时才出现行 Hover；Switch、Select、路径
和状态行只反馈实际控件。Panel 内部用细分隔线，不将每行包成卡片。

能力中心的二级导航统一采用 `flowy-settings-tabs`：文本标签、底部移动指示器、
Focus Ring 和既有 URL 深链接；不使用填充型 Tab 或胶囊。

## 实施批次

1. 基础契约：Header、Section、List、Row、Status、Permission、Empty、Action Bar，
   以及 form/hub 宽度与单滚动规则。
2. 应用：系统维持五个既有分组；Browser Use 和 Computer Use 迁入页标题、分组、
   权限状态与单滚动壳，登录、授权、资源策略和失败恢复流程不变。
3. 智能与内容：兴趣主题、学习、洞察和 MoA 使用共享面板/行密度；PoI、媒体和 MoA
   的显式保存通过 Action Bar 表达草稿、保存中、失败与撤销。
4. 能力中心：Skills、Presets、MCP 共享文字 Tab；Presets/MCP 移除竞争滚动层；
   已有 Library、Market、深链接、Drawer 与安装流程保持不变。
5. 账户、关于、扩展：云账户的服务器配置使用共享行与草稿操作条；关于改为紧凑信息
   列表；扩展页面在宿主侧统一 Header 与加载/空/失败状态，内容仍完全隔离。

## 非目标

- 不对视频模块进行专项视觉改版。
- 隐藏兼容路由只继承设置壳、宽度、滚动和主题回归修复，不重做内部信息架构。
- 不以静态测试或 Web 开发版替代 Windows/macOS 实机及用户视觉验收。

## 后续硬化

截图审计发现了多语言控制槽、原始翻译键和能力中心卡片密度问题。后续实施由
`2026-08-11-settings-ui-ux-hardening.md` 记录，保留本规格作为阶段 2B 的原始
架构决策。

市场 Hub 与设定编辑器的共享契约、详情 Drawer 和草稿边界继续记录在
[`2026-08-11-settings-ui-ux-phase-2c.md`](2026-08-11-settings-ui-ux-phase-2c.md)。

## 验收矩阵

- 六套内置主题的 Light/Dark、1440×900、1280×800、767/768px 与 150% 缩放。
- Default、Hover、Press、Focus、Disabled、Loading、Error、Dirty、Restart Required。
- Tab/Enter/Space、Tabs 方向键、Popover/Drawer 焦点返回、长中英文与长目录。
- Web 开发版、Windows 桌面、macOS 平台边界和用户视觉接受分别记录。

后续视觉契约收敛见
[`2026-08-12-home-settings-visual-contract-phase-2d.md`](2026-08-12-home-settings-visual-contract-phase-2d.md)。
