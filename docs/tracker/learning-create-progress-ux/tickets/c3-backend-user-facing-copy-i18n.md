---
id: c3
title: "学习模块 UI 文案本地化收口——生长 Pill step 映射、恢复态 topic 组装；text 帧与 companion 零改动"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# c3: 学习模块 UI 文案本地化收口（裁决 2：只动学习模块，后端 text 帧保留）

**What to build:** 学习模块的 UI 消费面停止展示后端中文文案，人面改由前端
词典本地化。**后端 WS `text` 帧原样保留**（companion 宠物气泡是在网消费面，
删帧即跨模块行为变化——裁决 2 明确不碰），本票改动全部落在
`ui/src/renderer/pages/learning/` + `nomifun-learning` 内的注释订正。三件事：

1. **生长 Pill step→key 全档映射**：`GrowthTaskPill.tsx:47-48` 现状是
   「`draft` 档有 key、其余档裸渲染 `item.detail`（后端 `set_step` 中文，
   9 处生产点：`generate.rs:391/439/508`、`learning_graph.rs:289/546/873/
   1851`、`lesson.rs:692/770`）」。改为按机器面 `step` 常量（结构未动，
   `generation_tasks.rs:82`）映射 `growthPillStep*` i18n key，双语补齐
   （现状仅 `zh-CN/learning.json:83` 一条 draft）。覆盖档位：structure/
   mount/repair/generate/draft/triage/source_plan/expansion（施工时以
   `set_step` 全量调用点为准）。failed 态的 detail 展示维持现状
   （`:50`，诊断语义，不入本票）。
2. **恢复态 topic 组装**：悬浮 Pill 的 `topic` prop
   （`index.tsx:448-450` = `useCourseCreation.ts:187` 恢复时写入的后端
   topic：「知识结构生成」「学习图生长 · {title}」「课时内容 · {title}」，
   生产点 `learning_graph.rs:236/5011`、`lesson.rs:612/636`）改为前端按
   `GenerationTaskView` 的 `kind + course_id`（+标题）组装显示文案，
   新增 `learning.taskKind*` 双语 key；`growthCourseTitle` 剥前缀逻辑保留
   （滚动兼容期旧快照 topic 仍带前缀，不炸）。
3. **对话框 started 行防御加固（一行）**：`CourseGenerationProgress.tsx:164`
   不再读 `event.text`，渲染一律走词典——c1 裁决后学习图流整段隐藏、v2 流
   本就无 text 帧（全仓 `"text"` 扫描仅 learning_graph.rs 4 处），该面实际
   已闭合，此行为防御性收口防未来回归。

**契约订正（nomifun-learning 内，零行为变化）**：`generation_tasks.rs:82-85`
注释明写「step 机器面（前端不造步骤文案）/ detail 人面（后端单源的单句
中文）」——本票后 detail 降级为诊断面（UI 不再作进度人面），注释如实订正。
该注释属于学习后端 crate，不涉其他模块。

## 法证证据（2026-11-10 实码扫描，全量无扩散）

- `set_step` 9 处、topic 3 处、生长 Pill 消费现状（draft 单点先例）、恢复态
  topic 消费链：见 map §影响面「文案泄漏面全量」。
- **宠物气泡跨模块事实（划出范围依据）**：`companion/messages/project.ts:
  106-125` `petMessageFromCourseGeneration` 订阅同一条
  `learning.course-generation` 流，标题本就按 phase 走 i18n（`:111`），
  仅 detail 兜底行 `:117/134` 原样渲染 `evt.text`——删后端 text 帧会让
  companion 的 detail 静默变 undefined，属跨模块行为变化，故帧保留、残留
  （en-US 下宠物气泡 detail 可能中文）明记归 companion 后置。
- 生长 Pill 渲染测试存在：`GrowthTaskPill.render.test.tsx`——施工时核对其中
  detail 断言并改为 step→key 断言。

## 实施要点

- [ ] `GrowthTaskPill.tsx` step→key 全档映射 + `growthPillStep*` 双语补齐；
      render 测试断言更新。
- [ ] 恢复态 Pill 显示文案改 `kind`+id 组装（`useCourseCreation.ts` 恢复
      路径 + `index.tsx` prop 链）；`taskKind*` 双语 key 落词典。
- [ ] `CourseGenerationProgress.tsx:164` 一行加固（去 `event.text` 读取）。
- [ ] `generation_tasks.rs:82-85` 注释订正（detail = 诊断面）；cargo 侧零
      行为改动。
- [ ] `bun run gen:i18n` + `bun run check:i18n` + `bun run typecheck` 绿；
      `cargo check -p nomifun-learning`（仅注释改动）。

## 验收

- en-US 环境目验学习模块 UI 全链：建课（学习图）→ 结构生成 → 生长 Pill →
  课时生成——对话框、悬浮 Pill、生长 Pill 无任何中文泄漏。
- zh-CN 环境同链目验：文案语义与改动前等价（换由词典出，不是丢文案）。
- 学习模块 UI 内 `event.text`/`item.detail` 人面渲染点 grep 归零
  （failed detail 诊断展示除外，票面已界定）。

## 非目标

- **companion 宠物气泡与后端 `text` 帧**（裁决 2：帧保留、companion 零改动；
  en-US 残留归 companion 后置，现状保持非回归）。
- AppError 错误文案 i18n（error-surface contract 管辖）。
- observation `learning-tasks.jsonl` 诊断日志（保留中文）。
- WS 事件名/phase 常量/`step` 机器面常量（双语无关）。
- 后端多语言能力（人面全在前端词典）。

## Resolution（2026-11-10 施工收口）

**代码面验收全过，已闭票；en-US/zh-CN 全链目验留待用户实测，FAIL 按票重开。**

- 施工 = `37db88047`（`STEP_KEYS` 八档 step→词典映射、未知/缺席 step 不回退
  detail；恢复态 Pill topic 经 `restoredTaskTopic` 按 kind 出 `taskKind*`
  词典文案；started 行去 `event.text`）+ `454775748`（`generation_tasks.rs:
  82-85` 契约注释订正）。
- 评审修复：`2619cc2e0`（未知 kind/课时前缀失配回退中性词典键
  `taskKindUnknown`，不再原样回退后端中文 topic；`taskKind*`/`growthPillStep*`
  断言改用真实 zh-CN + en-US 词典、遍历 `STEP_KEYS` 全档——补齐红线 2 双语
  断言缺口）+ `a8bb2700a`（`generation_tasks.rs:47/190` 残留旧契约注释统一
  为 detail = 诊断面；`CHANGELOG.md` Unreleased 三条）。
- **验收「grep 归零」修正**：round 行的 `event.text` **保留**——立票时
  「v2 流无 text 帧」前提有误（当时扫描只覆盖 `nomifun-learning`，漏了桥接
  crate）：round 帧的 text 是模型叙述（`crates/backend/nomifun-ai-agent/src/
  learning_loop.rs:292-299`），属动态模型输出而非后端硬编码文案，不在本票
  收口范围；错误前提注释已订正。started 行人面渲染归零达成；failed detail
  诊断展示按票面维持。
- 实际偏离票面一处（已留痕）：topic 组装为 kind + 剥「课时内容 · 」前缀
  （跨语言镜像 `lesson.rs:612/636`，已加注释），未用 `course_id` 取标题
  （需额外拉详情）；outline 的 topic 是用户自己的输入，原样返回。课时前缀
  失配时丢标题只显「后台任务」，为宁缺不泄漏的直接后果。
- 留痕：`STEP_KEYS` 八档中七档当前不可达（生长 Pill 只收 growth 行
  `growthTaskPill.ts:92`，growth 任务只设 `draft`）——按票面保留为防御
  映射；票面「其余档裸渲染 detail」指代码形状上的泄漏面，非线上可达泄漏。
- 裁决 2 守住：companion 与后端 4 处 `text` 帧零改动；en-US 下宠物气泡
  detail 中文残留归 companion 后置（现状保持，非本波回归）。
