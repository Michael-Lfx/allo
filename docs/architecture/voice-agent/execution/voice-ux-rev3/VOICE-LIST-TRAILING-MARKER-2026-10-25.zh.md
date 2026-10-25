# 会话列表右侧语音标识 · 实施与验证

> 2026-10-25，基准 `607545101`，分支 `feat/voice-next`。本轮只调整会话行呈现，不改变语音启动、任务执行、消息存储或积分逻辑。

## 1. 已实现的交互

| 场景 | 右侧表现 |
| --- | --- |
| 未通话，桌面默认 | 静态音波作为类型标识 |
| 未通话，悬浮/键盘聚焦/菜单打开 | 音波淡出，同一固定位置显示“更多”；无标题回流 |
| 正在通话 | 音波与连接点持续存在；为“更多”提前保留独立位置 |
| 通话中悬浮/聚焦/菜单打开 | 活动音波和更多按钮并存，不遮挡 |
| 移动布局或无 hover 的触屏 | 类型和更多同时可见，不要求悬浮 |
| 折叠侧栏/批量选择 | 保留类型标识，不执行桌面悬浮替换 |

普通桌面语音行预留 20px，活动通话/触屏预留 44px；悬浮、聚焦和菜单开关仅改变透明度，不改变标题宽度。音波继续是被动 `role=img`，类型由 metadata 判断，重命名不丢标识；点击列表行仍只导航。菜单与音波提示复用已有中英文文案。

## 2. 修改范围

- `ConversationRow.tsx`：移除标题前的音波，将其放入标题和元数据之后的固定尾部区域，保留现有菜单、删除保护与键盘行为。
- `voiceCallStatus.css`：用语音行作用域控制 hover/focus/menu 和触屏，不改变普通会话行规则；减少动画模式保留静态活动形态。
- `ConversationRow.voice.test.tsx`：保留原 metadata/重命名/普通任务隔离断言，补右侧顺序和状态属性检查。
- `ConversationRow.voice.browser-probe.tsx`：新增独立的真实组件夹具，未被生产路由引用；使用正式 Arco React 19 适配器和双语资源。

## 3. 验证事实

证据目录：`build.noindex/voice-workspace-display-review/voice-list-trailing-20261025/`。

- 组件定向回归 `targeted-unit.txt`：**16 passed / 0 failed / 772 assertions / 3 文件**。
- 真实 Chrome、生产生成 CSS 和实际 ConversationRow：**241 项 DOM 检查通过**。覆盖中英、浅深主题、240/320px、长标题和置顶/未读状态，检查音波在标题右侧、悬浮与聚焦原位替换、活动标识与菜单无重叠、菜单打开期间状态保持、Enter/Escape 与回焦、禁用删除不触发、折叠/批量/移动与 coarse pointer、Reduced Motion。见 `browser-result.json`；`browser-cleanup.json` 为 `connected=false`。
- 生产 `build:ui`：**exit 0**，1m56s，保留既有 bundle 大小提示。
- TypeScript：**61 条既有诊断逐条一致，新增/移除 0**；完整回执见 `final-checks.json`。
- `check:theme`、`check:motion`、`check:icons`、`check:i18n`、`check:button-layout-contract`：**全部 exit 0**；`git diff --check` 通过。
- `check:dead-css`：**exit 1**，命中均为未修改文件上的既有问题，本轮会话行/样式/夹具没有新增命中；未改检查基线。

扩大到 SessionList 全目录后为 **148 passed / 1 failed**：未修改的 `SessionRouteSafety.structure.test.ts` 对 `main.tsx` 的旧鉴权解构写法做源码正则断言，与 HEAD 已不匹配。两文件均与 HEAD Git blob 完全一致，证据为 `route-baseline.json`。本轮未删除/放宽该断言，也未改鉴权。

浏览器初次使用错误的 Arco 禁用菜单类名，导致探针失败；真实 DOM 确认类名为 `arco-dropdown-menu-disabled` 后修正检查，保留失败诊断。最终夹具使用正式 React 19 适配器及生产模式，不把开发模式的既有 element.ref 弃用告警算成此次组件故障，也未在生产代码屏蔽错误。

## 4. 验收边界

上述为真实组件/生成样式的隔离 DOM 验证，不等同已在完整桌面 SPA 中逐项操作。未运行物理音频、桌宠或云端付费请求；本轮也没有修改这些路径。既有全仓 TypeScript/dead-css 和鉴权结构断言问题单独保留，不扩大修复范围。本轮未提交或推送。
