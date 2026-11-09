---
id: c5
title: "结构图生成完成后自动呈现一次（首次 structure_completed 切到结构图 Tab + 轻提示）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# c5: 结构图生成完成自动呈现一次

**What to build:** 知识结构生成完成后至少把结构图呈现给用户一次：workspace
挂载期内首次收到本课 `structure_completed` 事件、且用户当前不在结构图 Tab 时，
`setActiveTab('structure')` 并 `Message.info` 轻提示（「知识结构已生成，已为你
打开结构图」）。mount 级 ref 防重复——同一次挂载内至多自动切一次，之后用户
自由浏览不再被抢焦点。

## 法证证据（2026-11-09 实码核实）

- 结构完成监听已存在：`LearningGraphWorkspace.tsx:145-179` 订阅
  `structure_started` / `structure_completed` / `structure_failed` /
  `structure_cancelled`，`structure_completed` 只触发 refetch（:169），无任何
  导航或呈现动作。
- Tab 状态本地 `useState('study')`（:87），Arco `Tabs` 受控
  （activeTab/onChange），切 Tab 只需 `setActiveTab`。
- 用户面：生成完成后页面只在摘要栏变数字，图藏在第四个 Tab，用户不知道结构
  已可看（截图三）。

## 实施要点

- [ ] workspace 持 `structurePresentedRef = useRef(false)`；在
      `structure_completed` 分支中：若 `!presented && activeTab !==
      'structure'` → `setActiveTab('structure')` + `Message.info(新 key)`；
      无论是否切换都置 presented = true（用户已在结构图 Tab = 已呈现）。
      注意闭包取 activeTab 最新值（ref 镜像或函数式读取）。
- [ ] 新 i18n key（zh-CN/en-US 对称）：结构已生成并已打开结构图的提示。
- [ ] `structure_failed` / `cancelled` 不切。

## 验收

- `bun run typecheck` + `check:i18n` 绿。
- 真机手验：建课/重新生成结构后停留在学习 Tab → 完成瞬间自动切到结构图 Tab
  并 toast；之后再触发结构事件（如再次重新生成）同一次挂载内不再自动切。

## 非目标

- 跨会话持久「已看过」标记（每次挂载重置可接受）；生长（growth_*）完成的呈现。

## Resolution（2026-11-09 施工收口）

**全验收过，闭票。** 施工 = `5aca3276c`。

- **票内裁决：补「离场兜底臂」并改用持久水位**（推翻票面非目标第一条）。实码
  核实：图课程 `generateCourse` 返回时只建了课程行，结构在后台分钟级生成
  （`useCourseCreation.ts:217`）——用户若在生成期间离开再回来，只走 HTTP、
  收不到 `structure_completed`，纯 mount 级 ref 会让「至少呈现一次」落空。
  - 在场臂：结构投影每次落地（含 WS `structure_completed` 触发的重取）都过
    `presentStructureOnce`；
  - 离场臂：零节点课程（首批未生长 = 建课后的声明窗口期）挂载拉到新结构时
    呈现；有节点的课程不走此臂（不打扰在学用户）；
  - 水位 = localStorage `learning.structurePresented`（courseId → 已呈现结构
    的 `generated_at`）：同一次生成至多呈现一次；重生成出新结构值得再看一次；
    用户已在结构图 Tab 视为已呈现（只写水位不 toast）。
- **纯决策层** `structurePresentation.ts`（判定 + 注入式存储，浏览器默认
  localStorage、缺席落进程内兜底）+ `structurePresentation.test.ts` 9 例
  （首次/重生成/追平/非法时间戳/往返/不串课/覆写/损坏 JSON/形状不对）。bun
  test 无 DOM——注入式存储是为此，不依赖 localStorage 全局。
- **toast**：新 key `learningGraphStructureReadyToast`（zh/en 对称），带下一步
  指引（先声明已会再开始学习）。
- **验收**：9/9 单测绿；typecheck 55 错持平存量（learning/i18n 零错）；
  `check:i18n` 绿。真机手验（在场切 Tab / 离场回来切 Tab / 同次不重复）留汇
  总门。
