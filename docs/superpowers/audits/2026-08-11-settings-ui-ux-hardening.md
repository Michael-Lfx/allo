# Flowy 设置体系 UI/UX：多语言与响应式硬化审计

## 触发证据

经典浅色实机截图暴露以下问题：应用分组显示 `settings.groupApplicatio…`，
Computer Use 显示英文 `Yes`，Browser Use 分段控件断成两行，长说明与右侧控件
争夺宽度，Skills、Presets、MCP 的工具栏和 232px 卡片对多语言过窄。

## 实施记录

- 将导航键绑定到生成的 i18n 联合类型，并统一到已存在的 `settings.groupApp`。
- 采用容器感知控制槽与不换行操作组，替代固定 220–280px 的单一控制槽。
- 迁移 System、Browser Use、Computer Use、POI、Learning、Insights、MoA 的主要
  控件类型与稳定依赖状态。
- 能力中心改为 270px 最小卡片，最多显示三个标签，移除外层装饰性灰色容器。

## 自动化证据

- 设置 primitives、导航、详情页、Hub、Skills 导入和卡片结构测试覆盖本轮契约。
- `check:i18n`、`check:theme`、`check:icons`、TypeScript、完整质量门禁和 UI 构建
  的结果在最终交付中独立记录。

## 人工边界

仍需在开发版中检查经典浅色与六套主题的 Light/Dark、150% 缩放、真实长英文、
键盘 Focus、Popover/菜单焦点恢复及 Windows 标题栏环境。自动化不能替代这些
实机视觉判断。
