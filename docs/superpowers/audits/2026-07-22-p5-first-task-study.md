# P5 · 首任务用户测试协议（成果启动台）

## 目标

5–8 名目标用户；每人独立完成「登录 → 成果启动台 → 首任务 → 检查成果」。主持人只观察，不提示操作路径。

## 原型入口

- 高保真原型：`/#/test/commercial-slice`（localStorage `flowy.commercialSlice=1`）
- 真实产品：登录后默认 `/#/guid`

## 记录字段

| 用户 | 完成 | TTFV(s) | 犹豫点 | 错误恢复 | 品质 1–5 |
|------|------|---------|--------|----------|----------|
| U1 | | | | | |
| U2 | | | | | |

- **完成**：是否到达可检查成果（文件/摘要/可追问）
- **TTFV**：从进入首页到确认首个有价值成果的秒数
- **犹豫点**：停顿 >5s 的界面元素（重点看：运行设置、双侧栏开关、模型弹窗、项目选择）
- **错误恢复**：缺模型 / 缺项目 / 网络 / 模型失败是否自行恢复

## 必测三态

1. **就绪**：已有模型 +（可选）项目 → 输入或点模板 → 看到执行预览 → 一次发送
2. **缺模型**：无模型 → CTA 变为原位连接 → 连接后自动续接草稿
3. **缺项目**：点「修测试」类模板 → 强制选文件夹 → 选后自动续接

## 发布门槛

- 首任务完成率相对基线上升
- TTFV 明显下降
- 路由无整屏闪烁
- 核心动画稳定约 60fps
- 键盘可达 + `prefers-reduced-motion` 可用
- 缺模型/缺项目/创建失败均保留草稿并提供单一恢复动作

## 漏斗事件（本地 cohort A/B）

`auth_completed` → `home_interactive` → `task_drafted` → `prerequisite_resolved` → `task_accepted` / `first_task_started` → `first_token` → `first_artifact_visible` / `answer_completed` → `first_value_confirmed`（用户确认：追问 / 复制 / 打开产物）→ `d1_retained` / `d7_retained`

注意：`first_token` 只衡量响应开始，不算激活。`answer_completed` / `first_artifact_visible` 也不结束首胜聚焦；只有 `first_value_confirmed`（成果卡确认 / 复制 / 追问）才算激活。
