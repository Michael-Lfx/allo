# 设置体系 UI/UX：阶段 2A 验证记录

## 已实现边界

- 设置导航改为四分组共享模型；`SettingsSider` 与 `SettingsPageWrapper` 不再各自维护菜单定义。
- 扩展锚点和历史映射仍可用；无效锚点统一进入能力扩展组末尾。云账户门控与现有路由不变。
- 主侧栏与设置侧栏复用同一个本地选中指示器 Hook。设置指示器仅在设置滚动容器内定位，
  不跨语义区域移动。
- 扩展设置页区分 Loading、空、未找到、失败和重试；iframe/Webview 内部不注入新样式。
- 新增 `SettingsList`、`SettingsRow`、`SettingsStatus`。系统设置迁移为 Page Header + 五个 Section，
  删除通知 Collapse，但保留定时通知节点并在总通知关闭时 Disabled。
- 系统设置的自动保存失败会恢复之前值并显示本地化错误；工作目录迁移、重启、重试、取消、
  备份、日志打开与 Factory Reset 的业务调用边界仍在原组件中。

## 自动化与构建证据

| 检查 | 结果 | 边界 |
| --- | --- | --- |
| 2A 聚焦 Bun 结构/模型测试 | 10/10 通过 | 覆盖四组顺序、扩展落点、可访问性结构、指示器、扩展状态、系统五分组与保留流程 |
| `bun run check:i18n` | 通过 | 新增中英文设置、扩展状态文案与类型定义一致（6,483 keys） |
| `bun run check:theme` / `check:icons` | 通过 | 六套内置主题通过；289 个 IconPark 导入文件符合契约 |
| `bun run typecheck` | 基线阻塞已复核 | 本次设置文件无新增错误；仍为 reasoning effort、视频画布依赖和 Blob 类型错误 |
| `bun run check` | 基线阻塞已复核 | 在 typecheck 阶段因同一既有错误停止，后续质量入口未执行 |
| `bun run build:ui` | 基线阻塞已复核 | 10,614 个模块转换后，仍因视频画布缺少 `@mediapipe/tasks-vision` 停止 |
| 开发版 HTTP 健康检查 | 通过 | `bun run dev:ui` 的 `http://127.0.0.1:5173` 返回 HTTP 200；这不代表视觉验收 |
| `git diff --check` / 工作流检查 | 通过 | 无空白错误，`.github/workflows/` 下无 `.yml` 或 `.yaml` |

## 人工体验检查点

开发版启动后，需要由用户在六套主题 Light/Dark、1440×900、1280×800、767/768px 下重点检查：

- 四组设置导航、活动项滚动定位、开发者模式显隐、扩展增删后的指示器稳定性。
- 系统设置的留白、行密度、长中文/英文、长路径、Switch/Select/Tooltip、Disabled/失败状态。
- 从设置页返回首页时主侧栏与设置侧栏之间不出现横条或跨区域指示器。
- Reduced Motion、150% 缩放和键盘 Tab/Enter/Space。

本机缺少 `npx`，Playwright CLI 无法启动，因此没有伪造浏览器自动化或截图证据。Web 开发版、
Windows 桌面 WebView、macOS 平台边界和用户视觉接受应分别报告。用户确认前，媒体、Skills、
Presets、MCP 仍不迁移。
