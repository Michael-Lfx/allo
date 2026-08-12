# Flowy 设置体系 UI/UX：阶段 2C 市场与设定编辑器精修

## 背景与目标

经典浅色实机体验表明，市场页仍带有旧版等高卡片的空白、随机序号色、重复来源和
“只能跳转外站才能看完整信息”的交互缺口；设定编辑器则叠加了大面积灰色容器和第二层
导入 Drawer。本阶段收敛为可扫描、可就地查阅、可安全编辑的能力中心。

## 已落实契约

- 市场来源只在工具栏表达；卡片使用中性序号、标题、主要操作和更多菜单。
- 卡片仅展示两枚高价值标签与 `+N`，隐藏全零统计和概览内安装命令；同一网格行对齐高度。
- “查看详情”在应用内展示完整描述、来源、统计、标签与安装命令；打开来源和复制命令保留为独立操作。
- 所有市场共享单一主操作状态：当前项 Loading，其余主操作暂时禁用；业务流程仍由各市场拥有。
- 市场区在 Tabs 后保留 20px 间距，并以说明取代重复的大标题。
- 设定编辑器最大 760px，只有一个内容滚动区；创建聚焦名称，编辑聚焦标题，禁止自动跳到指令文本框。
- 设定草稿覆盖身份、偏好、目标、上下文、标签、指令和技能；关闭前检测未保存修改。
- Agent Skill 导入由共享 `AgentSkillImportContent` 承担业务内容；独立入口使用
  `AgentSkillImportDrawer`，设定编辑器直接嵌入内容，不通过一个组件的
  `presentation` 分支承担两套布局，返回编辑器不丢草稿。

## 2C 重构落地状态

- `MarketItemViewModel` 集中完成描述、统计、来源和标签的规范化；卡片只消费摘要，详情
  Drawer 消费完整字段。原始 `ISkillMarketItem` 只在市场主操作边界保留。
- `useMarketCatalog` 统一缓存、后台刷新、搜索和标签筛选，并暴露 `loading`、
  `cached-refresh`、`ready`、`empty`、`no-match` 与 `partial-error` 状态；缓存刷新失败时
  不清空已有结果。
- `useMarketActionState` 维护跨卡片和详情 Drawer 的单一活动项，防重复提交并在失败后恢复。
- `MarketToolbar` 使用 `ResizeObserver` 测量自身控制槽，在 Segmented 与 Select 间切换，
  不依赖视口猜测断点；Tabs 后仅保留说明和工具栏。
- `PresetDraft` 与规范化签名集中处理 Dirty 比较，`usePresetEditor` 暴露草稿、保存、放弃、
  校验和字段错误契约。详情 Drawer 的名称与应用目标错误会定位到对应字段，保存失败保留草稿。

## 不变边界

不修改后端 API、市场缓存、来源数据、深链接、查询参数、安装安全审查、Preset DTO 或
持久化顺序。市场详情是前端临时状态，不伪造“已安装”长期状态。

## 体验验收

- 主操作的加号和文字均从按钮的 `currentColor` 继承主题语义色。
- 市场首屏无重复大标题和贴边拥挤；卡片在同一行对齐，跨行仍随内容自然排布。
- 用户无需离开 Flowly 即可阅读完整说明、全部标签与安装命令。
- 创建/编辑设定可取消、Escape、点击遮罩；Dirty 时必须确认，成功或取消后焦点恢复到触发点。
- 经典浅色先验收，再检查六套主题的 Light/Dark、150% 缩放和长中英文。

市场回归后续先见
[`2026-08-12-settings-ui-ux-phase-2c1.md`](2026-08-12-settings-ui-ux-phase-2c1.md)，最终视觉契约收敛见
[`2026-08-12-home-settings-visual-contract-phase-2d.md`](2026-08-12-home-settings-visual-contract-phase-2d.md)。
