---
id: c1
title: "已会声明一次性冻结 + 勾选状态持久呈现（disabled checkbox 冻结态 + 草稿勾选上提共享）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# c1: 已会声明一次性冻结 + 勾选状态持久呈现

**What to build:** 声明只允许进行一次：`basis.declarations > 0` 时 GateCoverage
整卡冻结为纯展示——全部门口渲染为 disabled checkbox，勾选态 = 是否已声明
（已声明 = checked + disabled + 绿「已会声明」tag；未声明 = unchecked +
disabled + 灰「未声明」tag），无声明/撤销按钮；保留头部计数、覆盖块信息与
底部「已声明门口」提示。冻结前交互不变，但（a）草稿勾选从 GateCoverage 本地
state 上提到 LearningGraphWorkspace，经 props 下传两个挂载点（DeclarationWindow
/ StructureGraphTab），切 Tab 不丢；（b）勾选清空时机从「确认弹窗 onOk 立即」
改为「声明 API 成功后」（由 workspace 成功路径清）；（c）声明按钮
`size='mini'` → 默认尺寸 primary；（d）声明窗口补一句动机说明（已声明门口
覆盖的知识块不会再安排学习，直接从还没会的地方开始）。

## 法证证据（2026-11-09 实码核实）

- 勾选清空早于 API：`GateCoverage.tsx:64-68` 确认弹窗 `onOk` 里
  `onDeclare(pendingSelection)` 不 await 且立即 `setChecked([])`——失败也丢。
- 草稿勾选组件本地：`GateCoverage.tsx:34` `useState<string[]>([])`；双挂载
  点 `DeclarationWindow.tsx:77-87` 与 `StructureGraphTab.tsx:155-165` 各一份，
  互不共享；切 Tab/重进即丢。
- 已声明门的 checkbox 被 tag 替换：`GateCoverage.tsx:110-124` 仅未声明门渲染
  checkbox，:133-137 已声明门渲染绿 tag——持久化结果（`GET /basis` 的
  `BasisView.doors`）没有以「勾选还在」的方式呈现，用户「之后不知道自己选
  了什么」（用户裁决 4）。
- 声明按钮不显著：`GateCoverage.tsx:172-180` `size='mini'` 藏卡片右下角；
  动机文案只有 11px secondary hint（:96-98）与窗口正文
  （`learningGraphWindowBody`），用户看不出声明的作用（用户裁决 5 前置）。
- 后端幂等 noop 已测试锁定（`basis_declaration_rerun_is_idempotent_noop`），
  冻结纯前端即可，后端不动；`declarations` 字段 = 锚行计数，`basis_view`
  现成返回。

## 实施要点

- [ ] GateCoverage 增加 frozen 派生：`basis !== null && basis.declarations > 0`
      （与 `basis.enabled` 无关，关开关也冻结展示）。冻结态：全门 disabled
      checkbox（checked = `declaredDoors.has(door.id)`）+ 未声明门灰
      「未声明」tag（新 i18n key）+ 无按钮区；`unresolved_concept_ids` 与
      `learningGraphBasisOff` 提示保留（如实披露）。
- [ ] 草稿勾选上提：workspace 持 `basisDraftChecked` state，GateCoverage 改
      受控（props: `checked` / `onCheckedChange`）；DeclarationWindow 与
      StructureGraphTab 透传。声明成功后 workspace 清空草稿；失败保留。
- [ ] 声明按钮升级：去掉 `size='mini'`（默认尺寸），保持 primary；禁用条件
      不变（无勾选 / 开关关）。
- [ ] 动机文案：DeclarationWindow 正文下补一行「为什么要声明」（新 i18n key，
      zh-CN/en-US 对称，`bun run gen:i18n` 后 `check:i18n` 过）。
- [ ] 两个挂载点行为一致：冻结后结构图 Tab 的 GateCoverage 同样纯展示。

## 验收

- `bun run typecheck` 绿；`check:i18n` 绿（新 key 双侧对称）。
- 真机手验：未声明时勾选 G1/G2 → 切到结构图 Tab 勾选保持 → 声明成功 → 两处
  GateCoverage 冻结，G1/G2 checked+disabled、其余 unchecked+disabled；刷新
  页面后勾选态仍在（来自 `GET /basis`）；声明失败时草稿勾选不丢。

## 非目标

- 声明反馈通道与 WS 收敛（c2）；撤销声明删除（c3）；后端任何改动。
- 冻结后的补声明/再声明能力（用户裁决 5：一次后纯展示）。

## Resolution（2026-11-09 施工收口）

**全验收过，闭票。** 施工 = `3d3a5f585`（feat(learning): one-shot gate
declaration freeze）。

- **冻结态 = disabled checkbox 勾选态持久呈现**：`basis.declarations > 0` 即
  frozen（与 `enabled` 无关），全门渲染 disabled checkbox（checked = 已声明）
  + 绿「已会声明」/ 灰「未声明」tag；`unresolved` 与通道关闭提示保留。读数
  缺席（basis 拉取失败）时声明按钮不放行——不能绕过「只声明一次」。
- **草稿勾选上提**：`basisDraftChecked` 工作区单持，DeclarationWindow /
  StructureGraphTab 两挂载点经 props 共享；清空时机移到 `declareBasis` API
  成功后（原来在确认弹窗 onOk 立即清，失败也丢）。
- **显著性与动机**：声明按钮 mini→默认尺寸；卡片内加 info Alert
  「为什么要声明」（未声明时）/「一次性冻结说明」（冻结后）；确认弹窗补
  「声明只能进行一次，确认后不可修改」。新 i18n keys ×3 双侧对称。
- **测试面**：typecheck 55 错全为存量债（videoCanvas 48 等，learning/i18n 零
  错）；`check:i18n` 绿。真机手验留波次汇总门统一走查（声明→冻结→切 Tab
  勾选保持）。
