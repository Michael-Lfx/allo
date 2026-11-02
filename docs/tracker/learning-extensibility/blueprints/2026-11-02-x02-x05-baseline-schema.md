# 施工图 · x05+x02 基线 schema 收口——枚举集去 CHECK 与退役死物

## 1. 元信息

- **目标**：学习域全部枚举集列不再带 `CHECK IN(...)`（校验收口 Rust，ADR-0015）；跨列规则 CHECK 移到 Rust；退役孤儿表 `learning_course_jobs` 与两处死列——全部在 x21 基线文件内完成
- **版本**：v1
- **状态**：定稿（用户确认 2026-11-02；施工按本图，变更走 §8 记录）
- **日期**：2026-11-02
- **覆盖票**：x05、x02；依赖图 1（基线节号）与 Q8 fixture 助手

## 2. 现状摘要

- 工具：捆绑 SQLite **3.46.0**（libsqlite3-sys 0.30.1，sqlx-sqlite bundled）→ `DROP COLUMN` 可用（先例 074:31-32）；改 CHECK 无捷径，只能整表重建。
- 重建模板两式（既有先例）：**保 id 式** `CREATE X_new` → `INSERT INTO X_new (id,…) SELECT id,… ` → `DROP` → `RENAME` → 重建索引（050:46-81）；**换 id 式**（069:104-156）。本图统一用保 id 式。
- 全部涉事表的完整列/索引/CHECK 逐字定义、Rust 写入位点表、`validate_settlement`（settle.rs:256-302，已比 DB 七条更严）、quiz 三形态与 progress 形态的 Rust 现状，见 2026-11-02 图 2 读码底稿 A/B/C 节（施工时以底稿为列清单来源，本图不复抄）。
- 关键事实：`ReviewSource` **无** `as_str`/`TryFrom`（只有自由函数 `source_text()`，review.rs:1707）；`sections.status` 生产代码只写 `'ready'`（lesson.rs:2112/1969），`pending/failed` 零写入；`courses.course_kind` 传统流靠列 DEFAULT，图课程写字面量（learning_graph.rs:111-115）。
- `generation_mode` 的 `'full'` CHECK 在 **learning_course_jobs** 上（040:19），不在 courses——随本图退役该表一起消失。
- DB 层断言「非法值被拒」的测试：learning_sections_migration.rs:108/:169/:222-234、learning_xp_events_migration.rs:84-130（8 组）、learning_quiz_rounds_migration.rs:74/86/98、learning_concept_web_migration.rs:326-327（role/tier）、:519（batch status）。
- 还原契约：`PRODUCT_TABLES` 枚举表，被删表静默跳过；谱系必须 `Current`——删表删列无还原风险（裁决 Q9 依据）。

## 3. 总体结构

- **基线文件追加两节**（图 1 的 §1–§9 不动，追加节号稳定供后续图引用）：
  - **§10 已发布表枚举集去 CHECK（重建）**：`learning_activities`、`learning_lesson_sections`、`learning_review_events`、`learning_review_log`、`learning_question_tags`、`learning_lesson_progress`（顺带消失 `extra_json`）、`learning_courses`。每表一段「CREATE _new（列定义逐字照抄原表，仅删枚举 CHECK）→ INSERT…SELECT（保 id、逐列具名）→ DROP → RENAME → 重建全部索引」。
  - **§11 退役死物**：`DROP TABLE learning_course_jobs;`、`ALTER TABLE learning_courses DROP COLUMN graph_meta_json;`。位置在 §10 之后（表数 125→124）。
- **未发布表的 CHECK**：直接改基线既有节——§1 custom_questions.kind（x01 的 9 值 CHECK 整条删）、§2 lesson_concepts.role/tier + growth_batches.status、§5 xp_events.kind/basis + 七条跨列 CHECK、§6 findings.source + repairs.result、§7 quiz_rounds.status + 三条形态 CHECK。数值域/布尔/json_valid/非空/UUID7 CHECK **全数保留**。
- **Rust 面**：新增两个校验函数 + 一组单测（4.3）；既有写路径全部 enum/字面量驱动，无新写点。
- **全局不变量与禁区**：
  - 重建只删枚举 CHECK，其余列定义（含 UUID7、json_valid、NOT NULL/DEFAULT、UNIQUE）与索引**逐字不变**——验收靠 schema diff 恰好只少 CHECK 行。
  - `forgot IN (0,1)`、`predicted` 区间、`rating 0..4`、`elapsed_days>=0`、`review_day>=19000101`、`difficulty 1..10`、`stability_days>=0`、`xp_bonus>=0`、`ratio 0..1` 保留（非枚举集）。
  - 不改任何 Rust 写入点（本票不新增枚举值；x13 才扩 quiz）。
  - 基线 §1–§9 已折叠内容不再重排。

## 4. 分块图纸

### 4.1 · §10 重建规范

- **接口契约**：每表一个子段，头注释 `-- x05: drop enum-set CHECK on <table>`；列清单来源 = 图 2 读码底稿 A 节（禁止凭记忆手写）。
- **数据形状**（仅列差异，其余照抄）：

| 表 | 删除的 CHECK | 顺带变化 |
|---|---|---|
| learning_activities | kind 9 值（050:60-63） | 无 |
| learning_lesson_sections | kind 5 值（050:26）、status 3 值（050:33） | 无 |
| learning_review_events | source 2 值（042:28） | 无（081 五列照抄） |
| learning_review_log | source（052:27）、rating_source（052:35） | 无 |
| learning_question_tags | source（036:43） | **三个 partial 索引照建**（036:57-62，含 `WHERE source=`） |
| learning_lesson_progress | status 4 值（048:74）、跨列形态（048:82-87） | **不建 extra_json 列**，INSERT SELECT 不拷它（x02 死列顺带消失） |
| learning_courses | course_kind（048:12-13）、teaching_style（050:145-146） | 列清单 = 底稿 A7 全表（含 structure_json/samples_meta_json/source_kb_ids/enabled/course_source_plan_json/deleted_at）；另执行 §11 的 graph_meta_json DROP 由本表重建直接不带（§11 只剩 course_jobs） |

- **失败路径**：INSERT SELECT 列遗漏/序错 → Q8 fixture 行数守恒断言失败；索引漏建 → schema diff 断言失败（4.4 步 3）。

### 4.2 · §11 退役

- `DROP TABLE learning_course_jobs;`
- 同步删 `id_schema_contract.rs` 六处登记：PRODUCT_TABLES:73、UUIDV7:180、NON_REFERENCE:296-297、text_ref:927-929（含 :924-926 注释）；`tests/id_schema_contract.rs:146` 表数 125→**124**。
- `tests/learning_course_jobs_migration.rs` 整文件改为单测：全链迁移后 `sqlite_master` 无 `learning_course_jobs`。
- 文档：`learning.zh.md:208` 删 `/api/learning/course-jobs*`、:240-243 删 `CourseJobTable`/`useCourseJobs`；x20 登记时引用 ADR-0011（双写不变量），`generation_mode` 记「随表退役」。

### 4.3 · Rust 校验收口

- **接口契约**：

```rust
// settle.rs：已有 validate_settlement(kind, &Settlement)（七条规则 + 更严的 bonus 约束）
//   → 本票只补单测：逐条 CHECK 原规则一个 #[test]，共 8 组（对齐被删的 081:74-80 + xp_bonus/ratio）

// quiz.rs 新增：
fn validate_round_shape(status: &str, finished_at: Option<i64>,
    answers: Option<&str>, report: Option<&str>, earned_xp: Option<i64>) -> Result<(), AppError>
// 三条形态规则（084:65-76 逐条）；调用点 = INSERT（quiz.rs:355 前）、
// UPDATE→abandoned（:342 前）、UPDATE→completed（:555 前）

// progress.rs 新增：
fn validate_progress_shape(status: LessonStatus, started_at: Option<i64>,
    completed_at: Option<i64>) -> Result<(), AppError>
// 四态形态规则（048:82-87 逐条）；调用点 = upsert（progress.rs:61 组装后、写库前）
```

- **失败路径**：校验失败 → `AppError::BadRequest`（与现路径错误面一致）；不静默纠偏。

### 4.4 · 测试翻面清单

| 测试 | 现断言（DB 拒非法枚举） | 改为 |
|---|---|---|
| learning_sections_migration.rs:108/:169 | 非法 kind `.is_err()` | 非法 kind **可插入**（DB 不设防）+ 注释指 Rust 层执法 |
| learning_sections_migration.rs:222-234 | teaching_style `'montessori'` `.is_err()` | 同上 |
| learning_xp_events_migration.rs:84-130 | 8 组跨列违规 `.is_err()` | 删 DB 断言；规则单测落 settle.rs（4.3）；DB 侧保留 xp_bonus>=0 / ratio 区间两条保留 CHECK 的正反样 |
| learning_quiz_rounds_migration.rs:74/86/98 | 形态违规 `.is_err()` | 删；形态规则单测落 quiz.rs `validate_round_shape`（合法 3 态 + 非法 3 样） |
| learning_concept_web_migration.rs:326-327/:519 | role/tier/batch status 非法 `.is_err()` | 同翻面；role/tier 的唯一 UNIQUE 约束断言保留 |
| 新增 Q8 fixture 测试 | — | v1.7.0 形状种子 → 跑基线（§1–§11）→ 行数守恒、业务行存活、9 种 kind 自建题全存活 |

## 5. 决策卡片

### A1 · 重建用保 id 式

- **类型**：数据结构
- **决策**：§10 七表重建全部保 id（050 先例式），AUTOINCREMENT 序列连续。
- **理由**：v3 契约「id 非可移植身份」不要求换 id；保 id 让 fixture 行数守恒断言可直接对 id，且 backup/恢复口径无感。
- **代价**：INSERT 列清单必须含 id（多一列出错面）——由 schema diff + 守恒断言兜底。
- **推翻条件**：无独立成立条件；若某表列过多导致清单错两次，改用 069 换 id 式并同步改守恒断言口径。

### A2 · learning_courses 一并重建

- **类型**：架构选型
- **决策**：courses 进 §10（删 course_kind/teaching_style 两条 CHECK），接受 20+ 列整表重建。
- **理由**：规则统一「枚举集列不设 CHECK」（ADR-0015）；courses 是后续最可能加枚举值的表（course_kind/teaching_style 都会扩）；上线前数据量小，此时重建最便宜，下次扩值零迁移。
- **代价**：列清单最长、出错面最大——底稿 A7 为唯一列清单来源 + schema diff 验收；JSON 四列的 json_valid CHECK 照抄不能丢。
- **推翻条件**：实现中 courses 重建出现两次以上列清单事故 → 该表退出 §10，保留 CHECK 并在 x20 记「下次扩值时再重建」。

### A3 · 跨列规则去 DB、留 Rust（quiz/progress 需新函数）

- **类型**：模块边界
- **决策**：xp 七条/quiz 三形态/progress 四态形态 CHECK 全删，执法点 = `validate_settlement`（已有）/`validate_round_shape`（新）/`validate_progress_shape`（新）。
- **理由**：xp 的 Rust 校验已存在且更严（settle.rs:261-301），DB 版是冗余地板；quiz/progress 的形态由写语句列集合隐式保证，但隐式保证在「新增写路径」时失效——显式函数 + 单测把规则钉在代码里。
- **代价**：脏写少了 DB 报错这层兜底；三个函数要跟着未来新状态维护。
- **推翻条件**：出现绕过 service 层直写这些表的第二写入口（如导入工具）时，为该入口复用同一校验函数而非恢复 DB CHECK。

### A4 · course_jobs 直接 DROP

- **类型**：接口契约（对外面为零的内部裁决，Q9 已定）
- **决策**：`DROP TABLE`，不走 RENAME 观察期；`ui/src` 零引用、还原按 PRODUCT_TABLES 跳过。
- **理由**：j03 注册表接管后零消费者；RENAME 观察期没有观察者。
- **代价**：用户库里旧管线的历史任务行永久消失（UI 早已不展示）。
- **推翻条件**：有人提出需要查历史建课任务记录（当前无此需求面）。

## 6. 施工顺序

| # | 施工步 | 图纸位置 | 验收 |
|---|---|---|---|
| 1 | 图 1 已定稿合入（前置） | — | 基线文件在，§1–§9 就位 |
| 2 | 写 Q8 fixture 助手（`migrate_range` 式：≤068 raw_sql → 播种 → 余下 raw_sql） | 图 1 §4.2 种子定义 | 助手单测：播种行数与断言基线成立 |
| 3 | 基线追加 §10/§11 + 未发布节 CHECK 删除 | 4.1/4.2 | schema diff：七张重建表 + courses 的 `sql` 恰好只少枚举/形态 CHECK 行；表数契约 124；`learning_course_jobs` 零残留 |
| 4 | Rust 三个校验函数 + 单测；翻面测试清单 | 4.3/4.4 | `cargo test -p nomifun-db -p nomifun-learning` 全绿 |
| 5 | 文档与契约登记同步 | 4.2 | `rg learning_course_jobs` 仅剩迁移历史/退役断言/契约删除行 |
| 6 | Q8 fixture 全量跑 + smoke quiz/xp/content 三档 | 4.4 | 守恒断言过；smoke EXIT=0 |

## 7. 未决事项

- **`ReviewSource` 补 `as_str`/`TryFrom`**：x05 只删 CHECK 不动 Rust；x13 扩 quiz 时必须先补（写入点现在是 `source_text()` 与字面量）。影响 x13。推荐默认：留给 x13 顺手做。触发重议：无。
- **`courses` 的 `domain` 列（015:11，`CHECK trim<>''`）**：非枚举集，保留，无需动作。仅记档防误删。

## 8. 变更记录

| 日期 | 类型 | 位置 | 变更 | 根因/触发 |
|---|---|---|---|---|
| 2026-11-02 | 修订 | 2/4.2 | `generation_mode 'full'` 勘误：在 learning_course_jobs 上（040:19），非 learning_courses；随本图退役表消失，x20 记档口径同步 | 图 2 读码底稿 A7 事实修正 |
| 2026-11-02 | 修订 | 4.4 | 测试翻面清单落实（5 文件 8 处断言翻面 + xp 规则单测搬家） | 读码底稿 E 节 |
