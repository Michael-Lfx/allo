---
id: v2
title: "工作区结构真话面——徽章实时读数（轮次/耗时）+ 刷新失忆修复 + 生长防呆"
type: task
status: open
assignee:
blocked_by: [v1]
part_of: ../map.md
---

# v2: 工作区结构真话面（G2 哑巴面 / G3 失忆面 / G5 防呆）

**What to build:** `LearningGraphWorkspace.tsx` 的结构生成 running 判定改
三源并集，running 期间徽章升级为实时读数：

1. **三源并集**（修 G3）：`structureRunning` = WS 帧（现网 `:176-185`）∪
   快照行（`refreshRegistryBusy` `:315-350` 的 active 过滤**补
   `kind === 'structure'`**——d7 四类恰好漏了本尊）∪ 既有 `load()` 消解
   （`:166` 不动）。刷新后不再「结构未生成 + 可点生成按钮（点了才 409）」。
2. **实时读数**（修 G2）：running 期间 5s 轮询 `learningApi.generationTasks`
   （复用 c2 收敛轮询形状：终态即停、快照不可达保留现值不臆造、假数据源可
   注入），徽章 Tag 从「生成中…」升级为
   `结构生成中 · 第 {attempt}/3 次 · 已 {mm:ss}`（attempt/elapsed 来自 v1
   快照行；attempt=None 或快照缺行回退静态「生成中…」；max 与后端
   `MAX_ATTEMPTS` 同源常量——前端机器面常量带注释对齐，或帧/快照携带，
   施工时取侵入最小者）。WS 终态帧/快照终态 → 停轮询 + 既有 load() 接棒。
3. **防呆**（修 G5）：生长按钮 disabled（`:611`）追加 `structureRunning`；
   「补充节点」入口在结构未就绪期间一律不可点。

文案全部前端词典（c3 契约）：新增 key 双语对称 + 双语断言（红线 2，教训 F6：
断言遍历全部新 key 且用真实词典 JSON）。

## 法证证据（2026-11-11 真机 + 实码 @06091fd45）

- 徽章现状：`:560-563` running Tag 单值「生成中…」（`learningGraphStructureG
  enerating`）——12 分钟真实窗口零变化（用户 12:08:45 进入后全程所见）。
- G3：`structureRunning` 仅 WS 帧驱动（`:176-185`，best-effort 不重放）；
  `refreshRegistryBusy`（`:315-350`）active 过滤只认 basis_declare/
  structure_mount/structure_repair/lesson 四类——`structure` 缺席；刷新后
  `:623` 面板带「生成」按钮且 `structureRunning=false` 不禁用。
- G5：`:611` `disabled={growing || graph.growth_running || busyId !== null}`
  ——不含 structureRunning；零节点停摆臂只拦自动臂（e3/m10 语义），显式
  POST force 恒过。
- 轮询先例：c2 `useCourseCreation` 收敛轮询（`useCourseCreation.test.ts`
  假数据源形状）+ d7 快照并集（`refreshRegistryBusy`）。

## 实施要点

- [ ] `refreshRegistryBusy` 补 structure 类（字段加入 `registryBusy` state
      或独立 state，取与现形状最贴者）。
- [ ] running 轮询 effect：5s、终态即停、卸载清理；elapsed 从快照行
      `elapsed_secs` 取（不自算钟，服务端真相单源）。
- [ ] 徽章文案 key：`structureBadgeRunning`（含 attempt/elapsed 插值）+
      回退态复用现有 `learningGraphStructureGenerating`；双语 + 断言。
- [ ] G5：`:611` disabled 追加；票面只动按钮守卫，不改 grow() 函数体
      （后端语义不碰）。
- [ ] 渲染/单测：轮询翻读数（attempt/elapsed 呈现）、快照缺行回退静态文案、
      失败清态（`:181-185` 不回归）、刷新后快照恢复 running、生长按钮
      structureRunning 禁用。

## 验收

- `bun run typecheck` 零新增（存量 54 错基线对照）+ `gen:i18n`/`check:i18n`
  绿 + learning 测试绿。
- 真机（与 map 欠账合并）：新图课程建课 → 刷新页面 → 徽章 running 且带
  轮次/耗时（不出现「结构未生成」诱点面）→ 落库后转摘要行；生长按钮
  running 期间禁用。

## 非目标

- 对话框内呈现（v4）与悬浮 Pill（v3）。
- WS 帧字段变更；后端结构生成行为任何变更（含 terrain 删键）。
- `regenerateStructure` 重生成路径的呈现语义（d3 已移除产品面，仅纠错通道）。
