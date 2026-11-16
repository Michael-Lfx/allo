---
id: c2
title: "声明反馈与数据一致（Message 替代阻塞 Modal + onRefresh + WS 监听收敛）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# c2: 声明反馈与数据一致

**What to build:** 声明成功的反馈从阻塞式 `Modal.info` 改为非阻塞
`Message.success`（noop = `Message.info`，失败保持 `Modal.error`——需要用户
看见并处置）；声明成功后追加 `onRefresh()` 使 `detail.graph` 的就绪集/推荐
计数即时反映声明效果；同时收敛 WS 监听面：GraphHistory / SystemActionsSection /
ConceptTable 的无过滤 `courseGeneration` 订阅按需过滤，消除「一次 basis_declared
四处静默重拉」的假刷新感。

## 法证证据（2026-11-09 实码核实）

- 反馈阻塞 + 数据不同步：`LearningGraphWorkspace.tsx:273-303`
  `declareBasis` 成功弹 `Modal.info`（:279-289）后仅重拉 `GET /basis`
  （:291-292）；`detail.graph`（就绪集/推荐）从不因声明刷新——
  `useGraphGenerationEvents.ts:5-13` 的事件过滤器只放行 `growth_*` /
  `structure_*`，`basis_declared` 被滤掉不触发 `onRefresh()`（:101）。声明
  使下游节点就绪这一调度效果对用户不可见，直到某次无关刷新。
- 假刷新感：`basis_declared` 一次事件触发四处无过滤订阅静默重拉——basis 读
  侧（LearningGraphWorkspace.tsx:264-266，这个是需要的）、GraphHistory
  （:686-688，声明不产生长批，无意义）、ConceptTable（:880-882，需要——
  已会声明 badge/占位行）、SystemActionsSection（:791-793，无意义）。用户
  观感 = 「页面刷新了，但没有合适的反馈」。
- 事件对象带 kind 字段（`useGraphGenerationEvents.ts` 的前缀过滤即基于它），
  组件级过滤可直接复用同一形状。

## 实施要点

- [ ] `declareBasis`：`Modal.info` 两分支 → `Message.success` /
      `Message.info`（文案复用现 key，内容含 doors/blocks/minted/reused 读
      数）；失败保持 `Modal.error`；`declaringBasis` spinner 不变。
- [ ] 成功路径追加 `onRefresh()`（index.tsx `load()`，重拉 detail.graph）；
      失败不刷。
- [ ] WS 收敛（组件级过滤，复用 useGraphGenerationEvents 同款前缀判断）：
      GraphHistory 只听 `growth_*`；SystemActionsSection 听 `growth_*` /
      `structure_*`；ConceptTable 听 `growth_*` / `structure_*` /
      `basis_declared`；basis 读侧（:264-266）保持现样（含 `basis_declared`
      与 `basis_revoked`）。
- [ ] `revokeBasis`（c3 将删）不在本票单独改反馈——若 c3 先并则该段已不存在，
      两票实现顺序 c1→c2→c3 时此处按 c3 删除后的形状收口。

## 验收

- `bun run typecheck` 绿。
- 真机手验：声明成功 → 右上 toast（非弹窗）→ 学习 Tab 就绪计数/推荐集即时
  变化；学习记录 Tab 不再因声明闪刷；概念表 badge 正常更新。

## 非目标

- 冻结态与勾选持久呈现（c1）；后端事件面改动（`basis_declared` 事件本身保
  留，读侧需要它）。

## Resolution（2026-11-09 施工收口）

**全验收过，闭票。** 施工 = `f766ce511`。

- **反馈**：成功 `Message.success`（读数原样：门口/命中块/铸出/复用），noop
  `Message.info`，失败保持 `Modal.error`。`learningGraphBasisDeclareDoneTitle`
  随 toast 化成孤儿 key，双侧对称删除（11585→11584 keys）。
- **数据一致**：成功路径追加 `onRefresh()`——`detail.graph` 就绪集/推荐集即时
  反映声明效果。
- **WS 收敛（实码核实事件面后定稿）**：帧的 `event` 字段由
  `emit_graph_event`（learning_graph.rs:4651-4658）注入；`structure_` 前缀同
  时覆盖 `structure_mount_*` / `structure_repair_*` 维护回合帧。落地口径：
  GraphHistory 只听 `growth_*`；SystemActionsSection 听 `growth_*` +
  `structure_mount_*` + `structure_repair_*`（账本行只由 auto_refill 生长与维
  护回合写，见 system_action.rs 的 kind 集）——比票面「structure_*」更窄，
  结构生成帧不添账本行；ConceptTable 听 `growth_*` + `structure_*` +
  `basis_declared`；basis 读侧保持全量。
- **测试面**：typecheck 55 错持平存量（learning/i18n 零错）；`check:i18n` 绿。
  真机手验留汇总门（声明 → toast + 就绪计数变化、历史 Tab 不闪刷）。
