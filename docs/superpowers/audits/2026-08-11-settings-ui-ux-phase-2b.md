# Flowy 设置体系 UI/UX：阶段 2B 验证记录

## 已实施

- 新增 Settings Page Header、Section、List、Row、Status、Permission Row、Empty State
  与显式保存 Action Bar；设置页面壳支持 `form` 与 `hub` 宽度。
- Browser Use、Computer Use 取消自身滚动壳，进入统一详情页层级；Computer Use 将
  macOS 权限状态与恢复动作收敛为 Permission Row。
- 学习、兴趣主题、媒体、MoA、云账户采用共享行/表面；媒体、兴趣主题、MoA、云账户
  的显式保存仅在草稿或失败时显示操作条，失败不丢弃草稿。
- Presets、Skills、MCP 采用作用域化文字 Tab；Presets 与 MCP 删除竞争滚动容器。
- 关于与扩展设置提供统一页面头和宿主状态；iframe/Webview 的 CSS、URL、locale 初始化
  与消息协议未改变。

## 自动化证据

- 聚焦 Bun 测试通过：`SettingsPagePrimitives`、`SettingsPageWrapper`、详情页结构、
  Browser Use 资源策略、系统设置结构与扩展状态，共 40 项断言；另一次完整聚焦批次
  34 项测试全部通过。
- i18n 类型已重新生成，包含 Browser/Extension 新增的中英文文案。
- `check:i18n`、`check:theme`、`check:icons` 与 `git diff --check` 通过；六套内置主题
  的变量契约均通过。
- TypeScript 完整检查仍被既有 `reasoning_effort`、视频画布缺少 `@mediapipe/tasks-vision`
  及 Blob 类型问题阻断；该输出未显示本阶段文件的新增错误。
- `build:ui` 同样在既有视频画布的 `@mediapipe/tasks-vision` 解析错误后停止；
  `check:agent-vocabulary` 仍有两处既有退休词汇注释（`runner.rs` 与
  `image_analyze.rs`）。仓库未发现 `.github/workflows/` 下的 YAML 工作流。

阶段 2C 的市场视图模型、目录状态、详情 Drawer 和 PresetDraft 验证见
[`2026-08-11-settings-ui-ux-phase-2c.md`](2026-08-11-settings-ui-ux-phase-2c.md)。

## 待人工确认

- Web 开发版在 `http://127.0.0.1:5173/` 返回 HTTP 200；其页面要求本地账户登录，
  因未提供凭据，本轮未越过登录页执行受保护设置的视觉操作。
- 六套内置主题 Light/Dark 下的按钮、Tab、状态和 Action Bar 对比度。
- Windows 开发版的长文案、独立滚动、保存失败、权限恢复和 Drawer 焦点返回。
- macOS 真实权限状态与重启提示；当前 Windows 环境只能保留平台边界检查。
- 767/768px、150% 缩放和 Reduced Motion 的最终手感。

后续自动化收敛与剩余人工边界见
[`2026-08-12-home-settings-visual-contract-phase-2d.md`](./2026-08-12-home-settings-visual-contract-phase-2d.md)。
