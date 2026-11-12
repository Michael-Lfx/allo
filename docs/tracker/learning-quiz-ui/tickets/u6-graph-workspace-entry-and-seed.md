# u6: 图工作区检测入口 + 图课程造数（seed v3）

## Status

施工中（2026-11-12 开票；grill 裁决 Q2+Q7）。

## 事实源

[分析文档](../assets/2026-11-12-course-kind-quiz-surface-analysis.md) 第一/二/四节；
裁决记录第六节 Q2/Q7。后端 quiz 形态无关已坐实（xp.rs:405-425 等零
course_kind）；图课程学节点走同一条 progress→结算→种卡链，会自然积累 due。

## Terms

- **R1（图工作区入口）**：`LearningGraphWorkspace.tsx` 学习视图挂知识检测
  入口——位置：就绪集推荐区**之后**；复用 QuizEntryCard + QuizSession；
  **视觉/文案区分前向推荐 vs 回向检测**（推荐 = 未学节点就绪集；检测 =
  已结算节点弱先——词典 quiz* 词条可复用，语境句缺再对称补）。挂载链：
  quizSessionCourseId/onFinished/onClose 状态在 index.tsx——图分支同样
  挂 QuizSession（与 CourseWorkspace 分支对称，见 index.tsx:334-370 两分支）。
- **R2（seed v3 图课程模式）**：`seed-learning-triggers.mjs` 加图课程模式
  （`--graph` 或独立子模式，票面裁决）：最小 5 表——`learning_courses`
  （course_kind='learning_graph' + learning_goal + structure_json 全量信封，
  形态抄 learning_smoke.rs:3387 `valid_structure_seed_envelope`）→ 1 隐含
  模块 → 节点 lessons（est/content_generated=1，挂 activities 复用 v2 config
  形态）→ `learning_concept_registry` → `learning_lesson_concepts`
  （teaches/assumes 即先修与挂接，就绪集读时自动成立）；progress/xp/cards/
  checkins 四段复用 v2；**软闸验收臂**：某节点的供给节点名下造低 R 卡
  （低 stability + 近 last_reviewed_at）触发 graph.blocked。清理段同步删
  概念/挂接/批次行。
- **R3（真机验收）**：seed v3 灌库 → 图课程走 LearningGraphWorkspace →
  就绪集/软闸条正常 → 节点可学可结算 → 检测入口出现（N/K 读数）→ 开测
  全链 → 完成归零。

## Acceptance

- typecheck 绿；图分支入口渲染测试（due/not-due 两态 + 前向/回向视觉区分）。
- seed v3 副本验证：detail.graph 走图工作区、5 表齐全、软闸臂 blocked 非空。
- 真机走查记录进 Resolution（IAB 受阻则留用户桌面走查清单，launch-sim
  波先例）。

## Resolution

已施工收口（2026-11-12，5d43c8ad8 经 merge 合入 learn0928；双轴评审随本波
code-review 统一轮）。

- R1：图工作区学习 Tab 外层 flex-col 包裹，QuizEntryCard 挂**两栏区之后
  视口底缘常驻**（票面「推荐区之后」的落地解：两栏区 h-full 自滚，塞滚动
  流会没入折叠——感知轴缺口不允许入口隐形）；卡上回向语境句
  `quizGraphEntryContext`（新键 1 对，zh/en 对称）；index.tsx 图分支
  QuizSession 挂载与传统分支逐字对称（onFinished 清位+bump / onConflict
  bump / onClose 卸载）；归档课程不渲染不拉读数。
- R2：seed v3 `--graph` 模式（缺省传统逐字不变，独立 MARKER「launch-sim
  图课程触发数据」）：8 节点盘面——前 6 completed（54 XP/天）→ quiz due
  6/6；节点 7 assumes 节点 6 teaches（软闸候选）+ 节点 6 低 R 卡
  （stability 1.0d/3 天前 push/lapse 序列 → R≈0.783 < 0.85 触发 blocked）；
  节点 8 零 assumes → recommended[0]；5 表齐 + applied 批次行（历史时间线）；
  cleanupFresh 补删概念/挂接/批次 + 备份名同秒去碰撞（顺手修重跑撞
  VACUUM INTO 目标的既有坑）。
- 验证：typecheck / check:i18n（12435 keys）/ 渲染 3 用例 / learning 回归
  388 pass / theme+dead-css 绿 / **seed 副本 41 项断言 EXIT 0**（--graph 与
  --graph --fresh 双跑，含软闸 blocked SQL 断言）。
- 已知限制（留痕）：图/传统两模式 checkin 指纹相同（goal 40 + count 54），
  两课共存时 --fresh 互清对方指纹打卡——v2 既有限制，脚本注释留痕。
- 真机走查：副本已覆盖 SQL 面；桌面走查清单 = `--graph` 灌库 → 开图课程 →
  就绪集见节点 8 / 软闸条见节点 7 / 检测入口 6/6 → 开测全链 → 归零。
