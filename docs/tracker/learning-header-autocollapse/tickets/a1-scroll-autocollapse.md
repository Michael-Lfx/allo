---
id: a1
title: 滚动深度驱动自动折叠（临时层，不落盘）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

学习图课程详情页：滚动到下方时以合适的时机/程度自动折叠头部，让正文拿到
版面；滚动回顶部时恢复。折叠不该依赖用户找到那个隐蔽的钮。

## 已核验事实（2026-11-17 对码）

- 头部 shrink-0 永不滚出视口（d1 高链：Tabs 内容区自滚），「头部让位」
  只能靠折叠态切换，不能靠原生滚动。
- 阅读滚动容器 = 学习 Tab 右栏正文 pane（:1107 `h-full overflow-y-auto`）。
- 现有状态机（:722-729）：`headerForceExpanded`（processing/growth）优先级
  最高；`headerCollapsed` 来自持久层（headerCollapse.ts per-course 只记
  true）。手动 chevron 双向写持久层。
- f3 语义：持久折叠 = 用户偏好，跨会话生效。

## Terms（方案草案，施工前过裁决）

- **R-a（裁决 2026-11-17：按推荐）滚动自动折叠 = 临时层**：
  - 触发：正文 pane `scrollTop` 下滑超过 **H=160px** → 临时折叠；回滚
    **<80px**（H/2 迟滞防临界抖动）→ 临时展开。常量进 constants.ts 可
    调参；rAF 节流，不逐帧 setState。
  - **不落盘**：`autoCollapsed` 不写 headerCollapse 存储——滚动深度是视口
    语境不是偏好；有效折叠 = `persisted || auto`；手动 chevron 仍走 f3
    持久层语义。
  - **用户否决臂**：临时折叠后用户手动点展开 → 本滚动周期抑制自动折叠
    （override 标记，滚回顶部阈值区复位）——否则自动层与用户意图打架。
  - **processing 优先级行已被 a3 修订**：processing 不再是折叠状态机的
    否决输入（a3 删 headerForceExpanded），只是徽章信息源——a1 施工按
    此合并。
  - 触发源**只挂学习 Tab 正文阅读容器**（:1107 正文 pane）；大纲列表/
    其它页签滚动不触发。
  - 切课 / 切 TabPane / 卸载正文容器时复位临时态。
- **R-b 语义矩阵（实现对照表）**：
  - persisted=collapsed：任何滚动状态都折叠（尊重偏好）。
  - persisted=expanded + 未滚动：展开。
  - persisted=expanded + 滚过 H：临时折叠；滚回 <H/2：展开。
  - 临时折叠中手动展开：override 生效直到滚回顶部。
  - processing：恒展开（现状不变）。
- **R-c 抽象落点**：状态机抽纯函数（输入 scrollTop/persisted/override/
  forceExpanded → 输出 collapsed/override'），表驱动单测；组件只做接线
  （headerCollapse.ts 同款分层纪律）。
- **R-d 不做**：其它 Tab（学习记录/概念表/结构图）不挂触发源（阅读主场景
  是学习 Tab；结构图画布有自己的滚动语义）——留观察；传统工作区不动；
  不做头部高度动画（如目验觉得生硬再议）。

## Acceptance

- 纯函数状态机表驱动单测（阈值/迟滞/override/复位/force 优先全分支）；
  存储层零改动断言（自动态不落盘）。
- 渲染测试：滚过阈值折叠、回顶展开、手动展开后抑制、processing 恒展开。
- 双 locale 新词条（若有）；typecheck / i18n / theme / dead-css 绿。
- 真机目验：滚动折叠跟手不闪烁、临界区不抖动（留欠账惯例）。

## Resolution

（未结案）
