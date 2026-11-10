---
id: d7
title: "长任务在途恢复同一纪律——补充节点守卫 / 挂载修补派生 / 课时生成派生 / 建课恢复误判 / 409 统一友好化"
type: task
status: open
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# d7: 其余长任务的同款在途纪律（审计外延）

**What to build:** d6 审计外延出的四类同族问题，统一按「服务端真相恢复 +
409 友好化」收敛：

1. **补充节点按钮无服务端守卫**：`disabled={growing || busyId !== null}`
   全是易失本地态；`graph.growth_running`（服务端真相）就在手边却没进
   disabled 条件——切页回来按钮可点，再点被 `graph-growth-{course}` 槽锁
   409。修：disabled/loading 纳入 `graph.growth_running`；grow() 409 → 友
   好提示 + onRefresh。
2. **挂载/修补回合**：同步命令内跑 LLM（分钟级），在途态同样是组件本地。
   修：挂载时从 tasks 快照派生（kind=structure_mount / structure_repair +
   course + queued/running）→ 按钮 loading 不可点；完成帧
   （structure_mount_completed / structure_repair_completed WS）已有的
   refetch 链路自然收口。
3. **课时内容生成**：202 返回 task_key，但 useCourseLearning 只轮询自己拿
   到的 key——重挂载后 key 丢失，生成按钮恢复可点，再点被注册表 key 冲突
   拒（kind=lesson, `lesson:{course}:{lesson}`）。修：注册表是唯一真相
   （审计确认 LessonView/SectionView 均不含生成态）——工作区挂载时按
   kind=lesson + node_id 匹配派生「生成中」集，注入 LessonBlock 的 busy
   判定（新可选 prop，传统课程工作区不受影响）。
4. **建课状态恢复误判（审计顺带实锤的存量 bug）**：
   `refreshGenerationStatus` 过滤条件是「除 growth 外全部」——重挂载后会把
   正在跑的 lesson/structure_mount/structure_repair 行误认成「课程正在创
   建」，拿「课时内容 · X」当建课主题显示建课进度视图。修：只认
   kind ∈ {outline, structure}（建课管线两段；structure 也是图课程创建的
   结构段）。

**统一纪律**：在途操作发起时写会话级 pending（d6 的 learningInFlight），
挂载时 ∪ tasks 快照派生，已知槽冲突 409 一律转非阻塞提示 + 数据重拉，不再
弹原始 JSON。

## 法证证据（2026-11-10 审计，file:line 实证）

- 补充节点：`LearningGraphWorkspace.tsx:511` disabled 条件无
  `graph.growth_running`；:502 的「生长中」tag 用了它（全局口径，任何课程
  的任意任务 running 都会点亮——generation_running 是全局 any-kind 判定，
  learning_graph.rs:4986-4991，跨课程误报记档后置）。
- 课时生成：`useCourseLearning.ts:46-70` 只轮询 202 的 task_key；注册表
  kind=lesson 的 key = `lesson:{course}:{lesson}`（lesson.rs:632-637），
  generation-tasks 可按 node_id 恢复。
- 建课恢复：`useCourseCreation.ts:178-180` 过滤「除 growth 外全部」——
  kind 集合见 generation_tasks.rs:48-53。
- 409 形状：409 + code=CONFLICT（nomifun-common error.rs:178/209），错误文
  本恒含槽 key（`course generation for this source is already running
  ("…")`，service/mod.rs:276-296）。

## 实施要点

- [ ] learningInFlight key 集扩展：`grow:{course}` / `mount:{course}` /
      `repair:{course}` / `lesson:{lessonId}`；相应 handler mark/finally
      clear。
- [ ] 工作区：tasks 快照派生（挂载 + 相关 WS 帧时重算）→ mount/repair/声明
      之外的 loading 注入；补充节点 disabled 纳入 `graph.growth_running`。
- [ ] LessonBlock 新可选 prop（外部生成中），图工作区按 tasks 派生注入；
      CourseWorkspace 不传，行为不变。
- [ ] useCourseCreation 过滤收窄到 kind ∈ {outline, structure}。
- [ ] 409 友好化统一：grow / mount / repair / 课时生成 catch 判槽冲突 →
      非阻塞提示 + 数据重拉（文案双侧对称）。

## 验收

- `bun run typecheck`（基线一致）+ 学习域单测绿 + `check:i18n`；
  `cargo check --workspace`（d6 已含后端面）。
- 真机判据（用户侧）：生长/挂载/修补/课时生成进行中切页往返，按钮保持
  loading 或禁用；竞态双击全部友好提示；建课对话框不再把课时生成误显为建
  课进度。

## 非目标

- growth_running 全局→按课程口径（badge 跨课程误报，后置清单）；section
  rewrite / activity generate / figure repair 的注册表面（审计确认无槽无
  注册，量大面窄，后置）；后端除 d6 注册行外的改动。
