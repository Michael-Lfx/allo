# t3: 死链清理批（D1/D3/D4/D6 + D2 enroll HTTP 面）

## Status

已关（2026-11-13 施工收口，0a2994d7a 经 merge 28b6ebc1a 合入集成分支；整波经合并并回 learn0928 已推）。

## 已核验事实（2026-11-13 对码，D 路审计）

四条迁移遗留死链 + 一条 compat 遗留（用户裁决：D2 删 HTTP 面）：

- **D1** `GET /courses/generate/status`：g4 波被 generationTasks 快照轮询
  取代。路由 routes.rs:42 → handler :281-285 → service/learning_graph.rs
  `generation_status()`（:4893，无其它调用方）→ models.rs:126 DTO →
  api.ts:68-72 包装 → types.ts:75 类型。全链删。
- **D3** `GET /courses/{id}/diagnostic`：116cd2dbd 删诊断 UI 流时后端留存。
  路由 routes.rs:113-115 → handler :641-654 → **service/diagnostic.rs 整
  文件**（已核无其它调用方）→ DiagnosticPlan DTO。整链删。
- **D4** `learning.diagnosticLimit` 设置项（同 D3 同源）：后端零读取。
  LearningSettings.tsx:30,182-191 字段 + configKeys.ts:174 + i18n 键
  （learning.settings.diagnosticLimit(Hint) zh/en）+ DEFAULT_DIAGNOSTIC_LIMIT
  常量。全删。
- **D6** `POST /courses/generate/estimate`：d5 建课简化遗留。路由
  routes.rs:39-41 → handler :270-278 → generate.rs:557 → DTO。整链删。
- **D2** `POST /courses/{id}/enroll`：675241ee4 自动加入改造后 compat
  遗留。**删 HTTP 面**（路由 routes.rs:111 + handler :619-629 + api.ts:195
  包装）；service 层 `enroll()` 留给 Rust 单测直调（单测在用，不删）。

## Terms

- **R1（后端删）**：上述路由行/handler/纯死 service fn/DTO 逐链删除；
  `enroll` 只删 HTTP 面不动 service fn；`xp_calendar`、`quiz/calibration`
  （u4 有意不接）、`DELETE /basis`（纠错通道）**零触碰**。
- **R2（前端删）**：api.ts `generationStatus`/`enroll` 包装、types.ts 对应
  类型、LearningSettings 字段；**不碰其它设置项**。
- **R3（i18n）**：`learning.settings.diagnosticLimit(Hint)` zh/en 双语键删；
  `bun run gen:i18n` 重生成 + `bun run check:i18n` 验证。
- **R4（测试）**：受影响测试同步删/改（diagnostic/generation_status/estimate
  相关既有测试若存在）；`cargo test -p nomifun-learning` 全绿 + fmt 按包
  check + typecheck 绿 + i18n 绿。

## Acceptance

- 四链一端点全删无残留（全仓 grep 零引用）；typecheck/i18n/learning 测试
  全绿。
- 真机：设置页不再显示「诊断上限」字段；课程生成/学习/报名流程无感。

## Resolution

已施工收口（2026-11-13，0a2994d7a，净变更 +63/−603、18 文件）。

- D1：generation_status 全链删，连带孤儿 `queued_summary`/`running_newest`
  （删 status 后无调用方）；api.ts 过期注释改指 generation-tasks 快照端点。
- **D3 票面修正**：diagnostic.rs 并非全死——`ensure_enrollment`（三处生产
  调用）与 `enroll`（约 23 处单测直调）同住该文件，已原样迁入
  service/course.rs（报名域），文件才整删。enroll 文档注释改写为
  「service 级 seam，HTTP 面已退役」。
- D6：estimate 链删（`estimate_expansion_batches` + 私有常量 + 确认文件/
  调用常量 + DTO）；`run_kb_triage` 保留（建课管线在用）。
- D2：enroll 仅删 HTTP 面（路由+handler+api.ts 包装）；service fn 保留
  （单测直调）。
- D4：设置字段 + DEFAULT 常量 + configKeys + i18n zh/en 各 2 键
  （12441→12439）。
- R4 验证：gen:i18n/check:i18n 绿、typecheck 0、learning 全量 963/0
  （merge t1 后复跑）、fmt 干净、下游 nomifun-app-server/nomifun-ai-agent
  check 绿、全仓残留 grep 零命中；零触碰清单（xp_calendar/quiz-calibration/
  DELETE-basis/useCheckinStatus）评审核实原样。
- 真机复验点：设置页不再显示「诊断上限」；生成/学习/报名流程无感。
