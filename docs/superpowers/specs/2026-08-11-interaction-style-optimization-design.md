# Flowy 交互样式优化：第 0、1 阶段实施规格

## 目标

在保留 Ink Studio、现有信息架构和业务行为的前提下，交付一个“明显但克制”的
Quiet Kinetic 样板。样板覆盖全局视觉基线、应用壳、主侧栏和首页首屏。用户通过开发版
实际操作确认方向后，才允许进入对话、聊天记录、设置和定时任务阶段。

## 修改范围

- 根级语义 token、Arco 通用控件状态和内置主题契约。
- 应用 Canvas、标题栏、侧栏及侧栏选中指示器。
- 首页标题、任务入口、Composer 表面和基础交互反馈。
- PRODUCT/DESIGN 文档、现状基线和验证记录。

## 非目标

- 不修改视频、知识库、学习等模块的专属布局或业务组件。
- 不修改 Composer 的输入、粘贴、发送、模型、权限或工作区行为。
- 不修改后端、路由、持久化、自定义 CSS 注入或跨窗口同步。
- 不新增动画依赖，不进行全仓样式清理。

## 视觉与交互决策

- 表面层级为 Canvas、Panel、Raised、Interactive。
- 采用 tinted neutral + restrained accent；主色只承担行动和状态。
- 普通控件 8px、面板 12px、Composer 16px；pill 仅用于短标签。
- 动效只使用 opacity/transform，反馈 120ms、状态 180ms、空间移动 240ms。
- 侧栏使用单一可移动选中指示器；Reduced Motion 下立即切换。
- 首页 Composer 是视觉中心，配置和推荐入口保持可用但降低静态噪声。

## 主题与可读性

- 六套内置预设分别验证 Light/Dark。
- `text-primary` 对 Canvas、Panel、Raised 的最终合成对比度不低于 4.5:1。
- Hover、Selected、Focus、Disabled、Popover 和 Composer 进行人工可读性检查。
- 用户和扩展主题保持现有自由度，不增加阻止、修复或警告。

## 验收矩阵

| 维度 | 取值 |
| --- | --- |
| 主题 | 经典、律动暗黑、暗夜霓虹、冰晶幻境、落日余晖、极简 Notion |
| 模式 | Light、Dark |
| 视口 | 1440×900、1280×800 |
| 状态 | Default、Hover、Press、Focus、Selected、侧栏折叠、Composer 聚焦 |
| 辅助 | 键盘、Reduced Motion |

## 阶段门禁

自动门禁包括主题契约、相关 Bun 测试、TypeScript、项目 `check` 和 UI 构建。
开发版启动只证明运行健康；截图只证明静态呈现。第一阶段是否符合用户喜好，只能由用户
实机操作后确认。未确认前不得进入第二阶段。

后续首页与设置页视觉契约的最终收敛记录见
[`2026-08-12-home-settings-visual-contract-phase-2d.md`](2026-08-12-home-settings-visual-contract-phase-2d.md)。
