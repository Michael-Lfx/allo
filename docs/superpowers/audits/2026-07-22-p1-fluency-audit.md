# P1 · 实机视觉与流畅度审计基线

生成时间以 `p1-fluency-baselines.json` 为准。复测：

```powershell
bun run --filter=./ui dev
bun scripts/ux-fluency-probe.mjs --url http://127.0.0.1:5173
```

完整路径（冷启动 → 登录 → 首页 → 首任务 → 执行 → 成果）需 `bun run dev:web` 或桌面壳 + 账号。本轮 probe 覆盖未鉴权入口、三视口 × 浅/深采样与可复测目标。

## 本轮实测（2026-07-22，Windows Edge headless）

| 指标 | 目标 p50 / p95 | 实测 | 方法 |
|------|----------------|------|------|
| 冷启动 | ≤2500 / ≤4500ms | **p50 1984 / p95 2125ms** | Edge screenshot 就绪 |
| 路由切换 | ≤180 / ≤450ms | shell fetch 仅数 ms | 鉴权后 SPA 交互需复测 |
| 输入响应 | ≤50 / ≤120ms | 未测 | 需登录态 |
| 长任务/分钟 | ≤8 / ≤20 | 未测 | 需登录态 |
| CLS | ≤0.05 / ≤0.10 | 未测 | 需登录态 |
| 动画帧率 | ≥50 / ≥45 | 未测 | 需登录态 |

产物：`artifacts/p1-2026-07-22T14-40-23-418Z/`（6 张登录视口截图）。

## 视口观察（登录）

- 1280×720 / 1440×900 / 2560×1440：登录卡居中，无明显溢出/截断。
- 登录卡视觉基本自管；`data-theme=dark` 不完全改写分栏配色（深色主题矩阵需进壳后采样）。
- 证据：首屏曾因 `opacity:0` 入场动画在 headless/慢帧下呈空白（已改为高起始透明度 + 轻 scale）。

## 组件审计与修复

| 项 | 根因 | 处置 |
|----|------|------|
| AppLoader 链式整页替换 | ProtectedLayout 卸载 Shell | Shell 常驻 + `AppLoader fill` |
| Session 双层 lazy | Shell + page Suspense | ConversationShell 静态 import |
| 登录二次全屏 Loader | page `checking` | `null`（交给路由 Suspense） |
| 右侧 rail 首帧跳变 | state 默认折叠再 hydrate | 同步读 localStorage |
| 左侧 ContentSider 滚动归零 | 折叠卸载 | 桌面 keep-mounted + width:0 |
| Popover 焦点丢失 | `unmountOnExit` | `false` |
| Guid 720p 重叠 | `-5vh` 无底 padding | `@media (max-height:760px)` |
| 登录首帧空白 | shell `opacity:0` | 可见入场 |

## 状态覆盖

- [x] 加载（cold / AppLoader 主题底）
- [x] 登录入口（三视口截图）
- [ ] 空 / 错误 / 成功（鉴权后路径）
