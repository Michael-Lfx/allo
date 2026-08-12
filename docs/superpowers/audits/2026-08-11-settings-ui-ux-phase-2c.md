# Flowy 设置体系 UI/UX：阶段 2C 审计记录

## 触发证据

实机截图显示市场卡片的主要操作中加号没有继承主题前景色；页面在 Tabs 后重复显示大
标题并显得贴边；摘要卡片高度不一致，且完整信息只能通过外部来源查看。

## 实施记录

- 主操作加号改为 `fill='currentColor'`，卡片与按钮统一依赖主题语义前景色。
- 删除市场区重复的可视大标题，增加 Tabs 后 20px 垂直呼吸空间。
- 网格改为同行等高，卡片不再通过底部命令或 `mt-auto` 强撑高度。
- 添加应用内详情 Drawer，提供完整说明、来源、统计、标签与安装命令；外部来源和复制命令仍留在明确操作中。
- Preset Drawer 采用单一内容滚动、固定 Header/Footer、Dirty Guard 和嵌入式 Agent Skill 导入。
- 市场目录、视图模型和活动操作状态已从卡片/页面组合层抽离；Skills、Presets、MCP 与 Plugins
  继续通过同一 `MarketSettingsPanel` 组合层提供各自的真实主操作。
- Preset 编辑器已收敛为 `PresetDraft` + 规范化签名，公开 `dirty`、`valid`、`saving` 和
  `fieldErrors`；校验错误在名称或应用目标附近就地表达。

## 自动化证据

- `MarketAndPresetPolish.structure.test.ts` 覆盖市场主操作状态、无随机色、无卡片 Hover 阴影、详情入口、单 Drawer 和 Dirty Guard。
- `marketViewModel.test.ts` 覆盖两标签摘要、完整详情标签、全零统计隐藏和统计单位本地化。
- `presetDraft.test.ts` 覆盖集合字段排序不产生 Dirty、目标/模型顺序仍保持语义。
- 聚焦 Bun 测试、i18n、主题、图标、类型检查和构建的结果在本阶段交付中分开陈述。

## 验证记录（2026-08-11）

- 通过：聚焦设置 Bun 测试（13 tests / 82 expects）；覆盖视图模型、目录/卡片结构、草稿快照、
  Drawer 生命周期和嵌入式 Agent Skill 导入。
- 通过：`node scripts/generate-i18n-types.mjs --check`、`node scripts/check-theme-contract.mjs`、
  `node scripts/check-icon-imports.mjs`、`git diff --check`。
- 类型检查使用 `bun --bun node_modules/typescript/bin/tsc --noEmit --pretty false`；本阶段代码
  无新增错误，剩余错误来自既有 `reasoning_effort`、MediaPipe 和视频 Blob 类型基线。
- `bun run check:i18n`、`bun run check`、`bun run build:ui` 在当前受限环境中被 Bun 报告
  `Operation not permitted`，未将该环境结果冒充为通过。
- 自动化未替代人工验收：经典浅色、六套主题、150% 缩放、Drawer 焦点恢复、长文案和窄窗口
  仍需 Web/Windows 开发版实机检查。

## 人工边界

待开发版检查：经典浅色与全主题的加号前景色、Drawer 焦点恢复、长标签、同一行卡片高度、
高缩放与窄窗口。自动化结构测试不能代替这些视觉和交互判断。

后续市场回归证据见
[`2026-08-12-settings-ui-ux-phase-2c1.md`](./2026-08-12-settings-ui-ux-phase-2c1.md)，最终视觉契约收敛见
[`2026-08-12-home-settings-visual-contract-phase-2d.md`](./2026-08-12-home-settings-visual-contract-phase-2d.md)。
