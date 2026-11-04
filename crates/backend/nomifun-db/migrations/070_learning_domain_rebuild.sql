-- 学习域上线前收口（x21）：把未发布的 069–088 二十段迁移折叠为单一净形基线。
--
-- 折叠自 069_retire_graph_dag_and_concepts – 088_learning_basis_declarations
-- （先例 217030a7c）。净形：未发布表直接建终态、已发布表的数据变换逐字保留；
-- 已发布迁移 001–068 一字不动，文件不重命名、不重排号。
--
-- 节号（后续票据的引用地址）：
-- §1 069 退役旧图与 per-course 概念体系（含 x01：自建题 kind 恢复 050 九值全集）
-- §2 概念网/生长/登记表终态（070 + 071 + 080 ref_name + 085 source；
--    终点表与罗盘两列随 ADR-0010 退役，从未存在）
-- §3 已发布 courses/lessons 的增量列（072/073/074 structure_json/075/076/077/078/079/086）
-- §4 已发布数据回填（076 source_kb_ids / 078 source_paths_json）
-- §5 XP 账本与复习作答流水列（081）
-- §6 挂载/findings/维护水位/已会声明（082/083/087/088）
-- §7 quiz 回合（084）
--
-- 折叠后旧 dev 库谱系不再是新谱系前缀，启动即拒；运行 `bun run reset:dev`
-- 后重启一次（旧数据集进 retired-datasets/）。smoke 临时库不受影响。

-- ==== 原 069_retire_graph_dag_and_concepts（v3 contract: 仅删数据/删表/重建表，无物理外键、无触发器）====
-- 学习图重构（ADR-0009）第一步：退役旧 DAG 学习图实验与 per-course 概念体系。
--
-- 旧学习图（course_kind='learning_graph'，048 引入）被「概念网 + 生长」模型
-- 取代：先修边表 learning_graph_prerequisites 与全部旧图课程数据一并删除，
-- 不迁移旧实验数据（沿用 048 自身的破坏性更新先例）。
--
-- 传统课程的 per-course 概念体系（015 引入）从未开发完整，随本次重构整体
-- 下线：learning_concepts / learning_concept_prerequisites / 
-- learning_lesson_concepts / learning_activity_concepts / learning_mastery_states
-- 五张表 DROP；自建题的 concept_id 列随之移除（表 rebuild）。概念的未来
-- 唯一落点是全局概念登记表（067 引入）。
--
-- 注意：learning_lesson_progress 的 skipped 状态保留——跳过对传统课时同样
-- 语义自洽。learning_review_events 的卡片历史按契约保留（打卡统计不随卡删）。

-- ── 1. 旧图课程派生数据（按 activity → lesson → course 自底向上）────────

-- 复习日志：卡片（课程题复习项）被删后，其推进历史一并清除——旧图课程
-- 连同学史整体退场，不保留孤儿日志。
DELETE FROM learning_review_log WHERE source = 'course' AND item_id IN (
    SELECT r.review_item_id FROM learning_review_items r
    JOIN learning_activities a ON a.activity_id = r.activity_id
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_question_tags WHERE source = 'course' AND question_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_review_items WHERE activity_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_attempts WHERE activity_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_activity_concepts WHERE activity_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_activities WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lesson_sections WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lesson_concepts WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lesson_progress WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lessons WHERE module_id IN (
    SELECT module_id FROM learning_modules
    WHERE course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_enrollments WHERE course_id IN (
    SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph');

DELETE FROM learning_modules WHERE course_id IN (
    SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph');

DELETE FROM learning_course_tags WHERE course_id IN (
    SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph');

-- ── 2. 旧图课程行与先修边表 ─────────────────────────────────────────────

DELETE FROM learning_graph_prerequisites;

DELETE FROM learning_courses WHERE course_kind = 'learning_graph';

DROP TABLE learning_graph_prerequisites;

-- ── 3. per-course 概念体系（残余行随表 DROP 一并消失）───────────────────

DROP TABLE learning_mastery_states;
DROP TABLE learning_activity_concepts;
DROP TABLE learning_lesson_concepts;
DROP TABLE learning_concept_prerequisites;
DROP TABLE learning_concepts;

-- ── 4. 自建题移除 concept_id 列（SQLite 无法 DROP COLUMN，标准 rebuild）──

CREATE TABLE learning_custom_questions_new (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    custom_question_id   TEXT NOT NULL UNIQUE CHECK (
        length(custom_question_id) = 36
        AND lower(custom_question_id) = custom_question_id
        AND custom_question_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(custom_question_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id              TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 枚举集不设 DB CHECK（ADR-0015）：ActivityKind 单源在 Rust，写侧严格
    -- 拒绝未知值、读侧宽容。x01 的九值 CHECK 修复随本票收口（列表仍在
    -- ActivityKind::as_str，永不收窄）。
    kind                 TEXT NOT NULL,
    prompt               TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json          TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    due_at               INTEGER NOT NULL,
    stability_days       REAL NOT NULL DEFAULT 0.0 CHECK (stability_days >= 0.0),
    difficulty           REAL NOT NULL DEFAULT 5.0 CHECK (difficulty >= 1.0 AND difficulty <= 10.0),
    review_count         INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
    lapse_count          INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
    last_reviewed_at     INTEGER,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    archived_at          INTEGER,
    edit_pending_at      INTEGER,
    edit_note            TEXT
);

INSERT INTO learning_custom_questions_new
    (custom_question_id, user_id, kind, prompt, config_json, due_at, stability_days,
     difficulty, review_count, lapse_count, last_reviewed_at, created_at, updated_at,
     archived_at, edit_pending_at, edit_note)
SELECT custom_question_id, user_id, kind, prompt, config_json, due_at, stability_days,
       difficulty, review_count, lapse_count, last_reviewed_at, created_at, updated_at,
       archived_at, edit_pending_at, edit_note
FROM learning_custom_questions;

DROP TABLE learning_custom_questions;
ALTER TABLE learning_custom_questions_new RENAME TO learning_custom_questions;

CREATE INDEX idx_learning_custom_questions_user_due
    ON learning_custom_questions (user_id, due_at);

CREATE INDEX idx_learning_custom_questions_user_id
    ON learning_custom_questions (user_id);

-- ==== 原 070_concept_web_growth + 071_growth_batch_status + 080(ref_name) + 085(source)（v3 contract: 仅新表/追加列，无物理外键、无触发器）====
-- 净形合并：概念登记表直接带 085 的 source；概念网边直接带 080 的 ref_name；
-- 生长批次直接带 071 的 status。070 的 learning_course_endpoints 与罗盘两列
-- 已由 074 整体退役，净形下从未存在（074 的端点课时 DELETE 随之无对象）。

CREATE TABLE learning_concept_registry (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    concept_id  TEXT NOT NULL UNIQUE CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    canonical   TEXT NOT NULL CHECK (trim(canonical) <> ''),
    aliases_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(aliases_json) AND json_type(aliases_json) = 'array'
    ),
    definition  TEXT NOT NULL DEFAULT '',
    deprecated  INTEGER NOT NULL DEFAULT 0 CHECK (deprecated IN (0, 1)),
    -- 行级文档代际（x09）：别名记录形状跨版本演进；未知版本读侧降级为空池。
    doc_version INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    source TEXT
);

-- role: 'teaches'（本课教该概念）/ 'assumes'（本课假定该概念）。
-- tier: 'know' < 'apply' < 'teach'（知道 / 会用 / 能教）。
CREATE TABLE learning_lesson_concepts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    lesson_id  TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    concept_id TEXT NOT NULL CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 枚举集（role / tier）不设 DB CHECK（ADR-0015）：Rust 单源在
    -- ActivityKind/ConceptRole/ConceptTier，写侧严格、读侧宽容。
    role       TEXT NOT NULL,
    tier       TEXT NOT NULL,
    ref_name TEXT,
    UNIQUE (lesson_id, concept_id, role)
);

CREATE TABLE learning_growth_batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id      TEXT NOT NULL UNIQUE CHECK (
        length(batch_id) = 36
        AND lower(batch_id) = batch_id
        AND batch_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(batch_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id     TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    seq           INTEGER NOT NULL CHECK (seq > 0),
    node_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(node_ids_json) AND json_type(node_ids_json) = 'array'
    ),
    note          TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL,
    -- 071 的状态机（pending 出生即落，成功 applied，失败 failed，用户取消
    -- cancelled——y04 起取消是中性终态，不再记 failed）；枚举集校验在
    -- Rust（ADR-0015）。
    status TEXT NOT NULL DEFAULT 'applied',
    UNIQUE (course_id, seq)
);

CREATE INDEX idx_learning_concept_registry_canonical
    ON learning_concept_registry (canonical);
CREATE INDEX idx_learning_lesson_concepts_lesson_id
    ON learning_lesson_concepts (lesson_id);
CREATE INDEX idx_learning_lesson_concepts_concept_id
    ON learning_lesson_concepts (concept_id);
CREATE INDEX idx_learning_growth_batches_course
    ON learning_growth_batches (course_id, seq);
CREATE INDEX idx_learning_growth_batches_nodes
    ON learning_growth_batches (node_ids_json);
CREATE INDEX idx_learning_growth_batches_pending
    ON learning_growth_batches (status, created_at);

-- ==== 原 072_learning_course_soft_delete ====
-- 学习图课程目录删除改为软删除（ADR-0009 后续裁定的删除语义）：
-- 课程行保留为墓碑（名称留作标识、deleted_at 即删除状态位），课时/模块/
-- 概念网边按勾选物理删除；遗留数据凭 course_id 归账，清理时有据可查。
ALTER TABLE learning_courses ADD COLUMN deleted_at INTEGER;

-- ==== 原 073_learning_lesson_difficulty ====
-- 节点难度（1-5）：教练生长批的深度水位执法面——本批最高难度不得超过
-- 已落库批次数 + 2，形式化/证明类内容（4-5）必须等台阶铺满低档后再长。
ALTER TABLE learning_lessons ADD COLUMN difficulty INTEGER NOT NULL DEFAULT 2;

-- ==== 原 074_retire_endpoints_compass_structure_snapshot（仅保留 structure_json 落地）====
-- 终点锚体系与罗盘已随本基线净形消失（070 从未建端点/罗盘）；此处只保留
-- 知识结构 + 前置地形两产物的单字段 JSON 快照列。存量课程为 NULL = 结构未
-- 生成（Missing 合法空状态）；重跑生成即原子覆盖。
ALTER TABLE learning_courses ADD COLUMN structure_json TEXT CHECK (
    structure_json IS NULL
    OR (json_valid(structure_json) AND json_type(structure_json) = 'object')
);

-- ==== 原 075_learning_course_sampling_meta ====
-- 目标感知采样（t2）：`learning_courses` 增加采样披露快照列。
--
-- Nullable、无 default：NULL 表示"无目标路径（字母序回退）或旧课程"——
-- 与空对象 {} 必须可区分，旧课程快照继续按旧形状读取，不做回填。
ALTER TABLE learning_courses ADD COLUMN samples_meta_json TEXT CHECK (
    samples_meta_json IS NULL OR (
        json_valid(samples_meta_json) AND json_type(samples_meta_json) = 'object'
    )
);

-- ==== 原 076_learning_course_source_kb_ids（列 + 回填）====
-- 多知识库（t4）：`learning_courses` 增加来源库列表。
--
-- 列口径仿 `cs_agents.knowledge_base_ids`（迁移 019 先例）：JSON 数组，
-- CHECK 只放行数组。**首个元素 = 主来源**，且与既有的单列
-- `source_kb_id`（迁移 015）同值——单列保留不动（既有读侧、SetNull 引用
-- 条目与索引都不退役），本列是它的伴生扩展：老行由单列回填为单元素数组
-- （描述流/已删库的 NULL 行回填为空数组，读侧对已删库 id 过滤）。
--
-- 不做物理外键（v3 规则）：引用完整性由应用层读侧过滤 + 单列既有契约承担；
-- 库里残留的已删库 id 由读侧剔除，不外泄悬空 id（t4 裁决）。
ALTER TABLE learning_courses ADD COLUMN source_kb_ids TEXT NOT NULL DEFAULT '[]' CHECK (
    json_valid(source_kb_ids) AND json_type(source_kb_ids) = 'array'
);

UPDATE learning_courses
SET source_kb_ids = json_array(source_kb_id)
WHERE source_kb_id IS NOT NULL;

-- ==== 原 077_learning_course_source_kb_ids_index ====
-- 多知识库（t4 评审修复）：`learning_courses.source_kb_ids` 登记进逻辑引用
-- 契约所需的具名索引。
--
-- `id_schema_contract.rs` 的 JSON 列表登记（`cs_agents.knowledge_base_ids`
-- 先例，迁移 019）要求该列上有具名索引。索引口径与先例逐字一致：普通列进
-- 索引（读侧 json_each 展开与孤儿审计按索引名引用），不是表达式索引。
CREATE INDEX idx_learning_courses_source_kb_ids_json
    ON learning_courses(source_kb_ids);

-- ==== 原 078_learning_lesson_source_paths（列 + 回填）====
-- 课时多引用（t4 评审补齐）：`learning_lessons` 增加来源路径列表。
--
-- 列口径仿 `learning_courses.source_kb_ids` 与
-- `cs_agents.knowledge_base_ids`（迁移 019）：JSON 数组，CHECK 只放行数组。
-- **首个元素 = 主来源**，且与既有的三列（`source_path`/`source_start`/
-- `source_end`，迁移 015）同值——三列保留不动（既有读侧与索引不退役），
-- 本列是它的伴生扩展：老行由三列回填为单元素数组（无来源的 NULL 行回填为
-- 空数组，读侧回退三列）。
--
-- 元素形状与 `SourceSpan` 序列化同构（`{"path":…,"start":…,"end":…}`）；
-- 库归属（`kb_id`）不落库——读侧按采样命名空间实时解析。
ALTER TABLE learning_lessons ADD COLUMN source_paths_json TEXT NOT NULL DEFAULT '[]' CHECK (
    json_valid(source_paths_json) AND json_type(source_paths_json) = 'array'
);

UPDATE learning_lessons
SET source_paths_json = json_array(
        json_object('path', source_path, 'start', source_start, 'end', source_end)
    )
WHERE source_path IS NOT NULL;

-- ==== 原 079_learning_course_enabled ====
-- 课程级「现存且启用」口径（三轮 g3）：enabled 翻转 = 归档/恢复（数据零删
-- 零改，可逆暂停；删除仍走 072 软删墓碑不可逆）。活跃课程集单源 =
-- deleted_at IS NULL AND enabled = 1：安排面（到期队列/就绪推荐/教练入口/
-- 课时内容生成）只服务现存且启用的课程；历史面（日历/统计聚合/打卡历史）
-- 不过筛，已计入的学习记录如实保留。
ALTER TABLE learning_courses ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;

-- ==== 原 086_learning_course_source_plan ====
-- 学习课程 v2（t4）：`learning_courses` 增加来源计划快照列。
--
-- 一列承载建课时点的原子 JSON 信封（version + t1 清点/分诊 + t2 计划 +
-- t3 展开覆盖）：`CourseSourcePlanSnapshot { version, inventory, triage,
-- plan, coverage }`。字段演进靠信封里的 `version`（读侧对未知版本降级为
-- unavailable，不崩溃、不伪造空计划），不再为 v2 拆列。
--
-- Nullable、无 default：NULL = 旧课程/描述流/未启用新管线的课程——与空
-- 对象 {} 必须可区分，旧行不回填。写侧只在课程导入的同一事务写列（与
-- modules/lessons 同生共死，无半成品）；读侧只读快照，不实时重算。
-- 回滚 = 读侧忽略本列（迁移只加列，不删数据、不改写既有列）。
ALTER TABLE learning_courses ADD COLUMN course_source_plan_json TEXT CHECK (
    course_source_plan_json IS NULL OR (
        json_valid(course_source_plan_json) AND json_type(course_source_plan_json) = 'object'
    )
);

-- ==== 原 081_learning_xp_events（复习流水五列 + XP 账本）====
-- XP 结算账本 + 复习作答流水补列（j05：三轮 t1 + 三轮 t3）。
--
-- learning_xp_events 是学习线 XP 的追加只增真相（量纲宣言：1 XP = 1 est 分钟
-- = 1 分钟专注，二轮 g1）：读数与日历全部回放派生，零新状态。kind 三线 ——
-- node-completed（教新：课时/图节点完成，est 计价）、review-completed（复习：
-- 队列推进按题型工作量当量）、quiz-completed（检测线，载体随 g4 后置，条款
-- 先入防带病上线）。earned_xp 是净入账（含 bonus 分量），xp_bonus 单独记账
-- 仅为「全对奖励」的审计面。幂等键 anchor：教新 = 节点一次性；复习 = 课程×
-- 卡×学习日一笔（UNIQUE(user_id, anchor) 兜底，重复结算写入即忽略）。
-- 写侧条款 validate_settlement（nomifun-learning settle.rs）是完整判官；下方
-- 表级 CHECK 只是同条款的数据库地板。
--
-- 同批迁移补复习作答流水（learning_review_events）五行：elapsed_ms /
-- response（原始作答逐字）/ session_id（学习行为组锚，g4 数据面前置列，
-- 会话实体缺位时为 NULL）/ forgot（承认遗忘）/ predicted（作答时刻的 FSRS
-- 预测可提取性）——同日折叠语义的「只记作答流水」行从此携带完整作答锚。

ALTER TABLE learning_review_events ADD COLUMN elapsed_ms INTEGER;
ALTER TABLE learning_review_events ADD COLUMN response TEXT;
ALTER TABLE learning_review_events ADD COLUMN session_id TEXT;
ALTER TABLE learning_review_events ADD COLUMN forgot INTEGER NOT NULL DEFAULT 0
    CHECK (forgot IN (0, 1));
ALTER TABLE learning_review_events ADD COLUMN predicted REAL
    CHECK (predicted IS NULL OR (predicted > 0.0 AND predicted <= 1.0));

CREATE TABLE learning_xp_events (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id  TEXT NOT NULL UNIQUE CHECK (
        length(event_id) = 36
        AND lower(event_id) = event_id
        AND event_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(event_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id   TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    ts        INTEGER NOT NULL,
    -- 复习线自建题无课程归属（course_id/node_id 双空）；课程卡两者齐全，
    -- node_id = 题目所属课时（图节点即课时）。
    course_id TEXT CHECK (
        course_id IS NULL
        OR (
            length(course_id) = 36
            AND lower(course_id) = course_id
            AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    node_id   TEXT CHECK (
        node_id IS NULL
        OR (
            length(node_id) = 36
            AND lower(node_id) = node_id
            AND node_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(node_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    -- 枚举集（kind / basis）不设 DB CHECK；跨列结算规则（四形态 + quiz
    -- 恒非负）也移到 Rust：validate_settlement（settle.rs）是完整判官，
    -- 每条规则一个有单测（ADR-0015）。
    kind      TEXT NOT NULL,
    -- 净入账（full 时含 xp_bonus 分量）；guess-penalty 必为负，quiz 恒非负。
    earned_xp INTEGER NOT NULL,
    -- 「全对奖励」分量（缺省 2）；仅 full 形态允许非零。
    xp_bonus  INTEGER NOT NULL DEFAULT 0 CHECK (xp_bonus >= 0),
    basis     TEXT NOT NULL,
    -- 部分正确的授予比例（earned_xp = (基数 × ratio).floor()）；仅 partial 携带。
    ratio     REAL CHECK (ratio IS NULL OR (ratio >= 0.0 AND ratio <= 1.0)),
    -- 结算来源注记（lesson-progress / review-course / review-custom），审计面。
    origin    TEXT,
    -- 幂等键：教新 = 'node:{lesson_id}'；复习 = 'review:{course|custom}:{item_id}:{review_day}'。
    anchor    TEXT NOT NULL CHECK (trim(anchor) <> '')
);

-- 幂等兜底：同用户同锚只允许一行，重复结算 INSERT OR IGNORE 即失效。
CREATE UNIQUE INDEX idx_learning_xp_events_user_anchor
    ON learning_xp_events(user_id, anchor);
-- 读数面扫描：今日 XP / 日历按 (user, ts) 回放。
CREATE INDEX idx_learning_xp_events_user_ts
    ON learning_xp_events(user_id, ts);
-- 进度尺分子：现存且启用课程内的已结算节点数（DISTINCT node_id）。
CREATE INDEX idx_learning_xp_events_user_kind_node
    ON learning_xp_events(user_id, kind, node_id);
-- 逻辑引用前缀索引（账本永不随课程/课时删除失真，回放历史面）。
CREATE INDEX idx_learning_xp_events_course
    ON learning_xp_events(course_id);
CREATE INDEX idx_learning_xp_events_node
    ON learning_xp_events(node_id);

-- ==== 原 082_learning_structure_mounts（v3 contract: 无物理外键、无触发器）====
-- 挂载事实层 + findings 留痕（j08，三轮 g1「结构转活文档·上」）。
--
-- learning_structure_mounts：挂载表一课一行（与 structure_json 单字段快照
-- 先例同构）——回合内单行原子覆盖，无中间态。mounts_json 是节点级条目
-- 数组（{node, block, reason}；block 为 NULL = 已评估、声明在结构外），
-- 读侧三态投影（Mounted / Outside / NotEvaluated）的单源。structure_hash
-- = 落盘时刻 structure_json 列字节的 FNV-1a 64（手写跨版本稳定，勿用语
-- 言默认 hash）——读侧用它判 mounts_stale（结构变了挂载即显影）。
--
-- learning_structure_findings：findings 留痕表（learnhub JSONL 的 DB 等
-- 价物，追加只增、坏行跳过不 Broken）。j08 只建表；生产方（概念评审门
-- warn 通道）与消费方（修补回合输入源）随 j09。

CREATE TABLE learning_structure_mounts (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id      TEXT NOT NULL UNIQUE CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 节点级挂载条目数组：[{node, block, reason}]；block 为 null = 结构外。
    mounts_json    TEXT NOT NULL CHECK (
        json_valid(mounts_json) AND json_type(mounts_json) = 'array'
    ),
    -- 落盘时刻 structure_json 的 FNV-1a 64（u64 存为有符号整型，读侧按
    -- 无符号语义比较）。v2 = 内层 structure 规范化投影；v1 = 历史原样字节。
    structure_hash INTEGER NOT NULL,
    -- 算法代际（x07）：写侧恒 v2；读侧按存量版本选算法比较，不回写升级。
    hash_version   INTEGER NOT NULL DEFAULT 1,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE learning_structure_findings (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    ts        INTEGER NOT NULL,
    course_id TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 每课程单调业务游标（x06）：水位锚本列而非自增 id——自增 id 不可移植，
    -- 备份还原重编号后水位会漂移；seq 随 bundle 原值复制。
    seq       INTEGER NOT NULL CHECK (seq > 0),
    -- 生产方：'concept-review'（评审 warn 通道，随 j09）/ 'mount-observation'
    -- （挂载观察派生，纯读函数不打点）；枚举集校验在 Rust（ADR-0015）。
    source    TEXT NOT NULL,
    -- 上下文锚：节点标题或块 slug（自由文本，允许空串 = 全课面）。
    node      TEXT NOT NULL DEFAULT '',
    field     TEXT NOT NULL DEFAULT '',
    issue     TEXT NOT NULL CHECK (trim(issue) <> ''),
    advice    TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_learning_structure_findings_course
    ON learning_structure_findings(course_id, seq);

-- 逻辑引用前缀索引（挂载行 UNIQUE(course_id) 已保证身份，这里补具名索引
-- 供 v3 逻辑引用契约挂靠）。
CREATE INDEX idx_learning_structure_mounts_course
    ON learning_structure_mounts(course_id);

-- ==== 原 083_learning_structure_repairs（v3 contract: 无物理外键、无触发器）====
-- 结构修补审计表（j09，三轮 g1「结构转活文档·下」）。
--
-- learning_structure_repairs：一次**应用了 op** 的修补回合一行（result 恒
-- applied）；noop（评估过、无 op）与 skipped（零 LLM）都不落行——没发生的
-- 事不造档案。ops_json = 逐 op 数组（含 type 与字段、reason、级联注记），
-- 结构 hash 迁移（old→new）与「应用后 gd.* 全量重过零放宽」的结论都在行
-- 内——learnhub 修补档案（文件 + Git 出处）的 DB 等价物。三重护栏 = 全量
-- validator 门 + 观测审计 + 出处，不建受理门机器。

CREATE TABLE learning_structure_repairs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    ts         INTEGER NOT NULL,
    course_id  TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 结构 hash 迁移（FNV-1a 64，有符号整型承载）；noop 时两者相等。
    old_hash   INTEGER NOT NULL,
    new_hash   INTEGER NOT NULL,
    -- 算法代际（x07）：行内两 hash 自洽即可（写侧恒 v2）；跨表与 mounts 行
    -- 比较必须先对齐 hash_version。
    hash_version INTEGER NOT NULL DEFAULT 1,
    -- applied | noop（ops 空 = 合法结论；noop 不写结构快照，仅留痕）；
    -- 枚举集校验在 Rust（ADR-0015）。
    result     TEXT NOT NULL,
    -- 扁平信封（x09）：`{version, ops:[{op, ...字段, reason, 跟随改写说明}]}`；
    -- noop 为空数组。历史裸形（无 version）读侧按 v0 兼容。
    ops_json   TEXT NOT NULL CHECK (
        json_valid(ops_json) AND json_type(ops_json) = 'object'
    ),
    note       TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_learning_structure_repairs_course
    ON learning_structure_repairs(course_id, ts);

-- ==== 原 087_learning_maintenance_state ====
-- 学习维护水位（k02，四轮吸收 learnhub #146/#147 + ADR-0028）。
--
-- 一行一课：`findings_watermark` = 已被成功修补回合消费的最大
-- `learning_structure_findings.id`（SQLite 行自增单调，等价 learnhub 的
-- 行数水位且免坏行处理）。修补回合只读 `id > 本值` 的留痕；回合终态
-- applied/noop 才推进，failed/skipped/cancelled 不推（失败回合的 findings
-- 仍是下次输入）。越界（手工写超 max id）读侧钳制到 MAX(id)，不阻断。
--
-- 派生面不是真相面：水位行缺失 = 0（全量重读一次，只是多花一次修补）；
-- 写侧恒取 max(旧值, 新值)，水位只升不降。
--
-- 行身份按 v3 契约：本地 `id INTEGER PRIMARY KEY AUTOINCREMENT` + 业务键
-- `course_id` 唯一（逻辑引用挂 `learning_courses`，Cascade 派生面——课程
-- 没了水位失去意义；findings 留痕本身仍 KeepHistory 保留）。
CREATE TABLE learning_maintenance_state (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id            TEXT NOT NULL UNIQUE CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    findings_watermark INTEGER NOT NULL DEFAULT 0 CHECK (findings_watermark >= 0),
    updated_at           INTEGER NOT NULL
);

-- 逻辑引用前缀索引（v3 契约：具名索引供逻辑引用挂靠）。
CREATE INDEX idx_learning_maintenance_state_course
    ON learning_maintenance_state(course_id);

-- ==== 原 088_learning_basis_declarations（v3 contract: 仅新表，无物理外键、无触发器）====
-- 已会声明锚（m2，ADR-0032 域中立移植）：学习者对已具备入门内容的显式声明
-- ——勾选地形门口判据，系统按声明铸概念（走既有铸名纪律），**只铸概念不落
-- 节点**：不排课、不计 XP、不建任何学习行为。已知集 = 已教 ∪ 已会声明。
--
-- 一行一次声明（幂等增量语义 = 累计并集差）：每行记本次声明勾选的门口、
-- 命中的目标块（door_refs AND 口径）与铸出/复用的 concept_id。**锚表即铸造
-- 出处**（allo 无 mint-log）：概念库可经 concept_ids_json 反查「来自哪次声
-- 明」。撤销声明 = 删锚行——概念留池（登记表全局有效，来源标记保留不抹）：
-- 「声明可撤、概念不灭」。

CREATE TABLE learning_basis_declarations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id   TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    doors_json  TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(doors_json) AND json_type(doors_json) = 'array'
    ),
    blocks_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(blocks_json) AND json_type(blocks_json) = 'array'
    ),
    concept_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(concept_ids_json) AND json_type(concept_ids_json) = 'array'
    ),
    declared_at INTEGER NOT NULL,
    note        TEXT NOT NULL DEFAULT ''
);

-- 逻辑引用前缀索引（v3 契约：具名索引供逻辑引用挂靠）。声明随课删
-- （Cascade 派生面——声明是本课事实）；行序按声明时刻。
CREATE INDEX idx_learning_basis_declarations_course
    ON learning_basis_declarations (course_id, declared_at);

-- concept_ids_json 反查索引（v3 契约 JSON 逻辑引用挂靠）：概念库 → 声明锚
-- 的来源反查面。
CREATE INDEX idx_learning_basis_declarations_concepts
    ON learning_basis_declarations (concept_ids_json);

-- ==== 原 084_learning_quiz_rounds（v3 contract: 无物理外键、无触发器）====
-- quiz 回合记录表（q2，quiz 结算线载体）。
--
-- learning_quiz_rounds：一次 quiz 回合一行情境快照——触发时刻的覆盖节点与
-- 选题清单（plan_json）、学习者作答与逐节点结算对账（completed 时）、终态。
-- 载体形态 = 瘦回合制（方案 A，mount/repair 回合先例；无会话实体——g4 会
-- 话命令面后置，q3 载体票不依赖它）。
--
-- 纪律（learnhub domain/sessions.rs 同案）：
-- - **首写为终**：completed 是单向转移（UPDATE ... WHERE status = 'active'
--   兜并发），终态行不被后到事实改写——崩溃恢复重跑不会用折叠后事实覆盖
--   首跑结算读数；结算本身的幂等在 learning_xp_events 的 anchor（quiz:
--   course:node:day）。
-- - **判底不入表**：plan_json 只存选题快照（activity_id + kind——kind 是
--   结算定价锚），answer/explanation 住 learning_activities.config_json
--   （服务端域持答案纪律）。
-- - 同课程同用户至多一个 active 回合（部分唯一索引地板）：start 遇既有
--   active 回合 = 显式 supersede（旧回合标 abandoned）——learnhub 内存会话
--   簿「重启即弃」的落表显式化。

CREATE TABLE learning_quiz_rounds (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    round_id   TEXT NOT NULL UNIQUE CHECK (
        length(round_id) = 36
        AND lower(round_id) = round_id
        AND round_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(round_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id    TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id  TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    -- active（进行中）/ completed（走完结算）/ abandoned（被新回合取代）。
    -- 枚举集与三条形态规则（status × finished/answers/report/earned 的组合）
    -- 都移到 Rust：validate_round_shape（quiz.rs），每条规则一个有单测
    -- （ADR-0015）。
    status      TEXT NOT NULL,
    -- 行级文档代际（x09）：plan/answers/report 三载荷共享一个代际（同生同灭）；
    -- 未知版本在活动状态面报错、统计面跳过。
    doc_version INTEGER NOT NULL DEFAULT 1 CHECK (doc_version >= 1),
    -- 选题快照：[{node, node_name, activity, kind}]（判底不入表）。
    plan_json   TEXT NOT NULL CHECK (
        json_valid(plan_json) AND json_type(plan_json) = 'array'
    ),
    -- 学习者作答：{activity: response}（completed 时在场）。
    answers_json TEXT CHECK (
        answers_json IS NULL OR (
            json_valid(answers_json) AND json_type(answers_json) = 'object'
        )
    ),
    -- 逐节点对账：[{node, node_name, answered, correct, earned_xp}]
    -- （earned_xp null = 同日已结算折叠——coverage 保持不重复计薪）。
    report_json TEXT CHECK (
        report_json IS NULL OR (
            json_valid(report_json) AND json_type(report_json) = 'array'
        )
    ),
    -- 全卷净入账（completed 时在场；折叠不计薪）。
    earned_xp   INTEGER
);

-- 同用户同课程至多一个进行中回合（start supersede 流的并发地板）。
CREATE UNIQUE INDEX idx_learning_quiz_rounds_active
    ON learning_quiz_rounds(user_id, course_id) WHERE status = 'active';
-- v3 契约具名索引（逻辑引用扫描面）。
CREATE INDEX idx_learning_quiz_rounds_user
    ON learning_quiz_rounds(user_id);
CREATE INDEX idx_learning_quiz_rounds_course
    ON learning_quiz_rounds(course_id);


-- ==== §10 已发布表枚举集去 CHECK（x05，保 id 式重建，ADR-0015）====
-- 只删枚举集 CHECK（kind/source/status/course_kind/teaching_style）与
-- lesson_progress 的跨列形态 CHECK；其余列定义（UUID7、json_valid、
-- NOT NULL/DEFAULT、UNIQUE、数值域）与索引逐字保留。x02 的两处死列
-- （lesson_progress.extra_json、learning_courses.graph_meta_json）在重建时
-- 自然不再拷贝。跨列规则改由 Rust 写路径执法（settle.rs / quiz.rs /
-- progress.rs），每条原规则一个有单测。

-- x05: drop enum-set CHECK on learning_activities
CREATE TABLE learning_activities_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id TEXT NOT NULL UNIQUE CHECK (
        length(activity_id) = 36
        AND lower(activity_id) = activity_id
        AND activity_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(activity_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    lesson_id   TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    kind        TEXT NOT NULL,
    prompt      TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    section_key TEXT,
    position    INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (lesson_id, position)
);
INSERT INTO learning_activities_new
    (id, activity_id, lesson_id, kind, prompt, config_json, section_key, position)
SELECT id, activity_id, lesson_id, kind, prompt, config_json, section_key, position
FROM learning_activities;
DROP TABLE learning_activities;
ALTER TABLE learning_activities_new RENAME TO learning_activities;
CREATE INDEX idx_learning_activities_lesson_id
    ON learning_activities(lesson_id, position);

-- x05: drop enum-set CHECK on learning_lesson_sections
CREATE TABLE learning_lesson_sections_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    section_key TEXT NOT NULL CHECK (length(section_key) > 0 AND length(section_key) <= 32),
    lesson_id   TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    kind        TEXT NOT NULL,
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    points      TEXT NOT NULL DEFAULT '',
    body_md     TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'pending',
    version     INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
    position    INTEGER NOT NULL CHECK (position >= 0),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    visual      TEXT NOT NULL DEFAULT '',
    degraded    INTEGER NOT NULL DEFAULT 0,
    UNIQUE (lesson_id, section_key),
    UNIQUE (lesson_id, position)
);
INSERT INTO learning_lesson_sections_new
    (id, section_key, lesson_id, kind, title, points, body_md, status, version,
     position, created_at, updated_at, visual, degraded)
SELECT id, section_key, lesson_id, kind, title, points, body_md, status, version,
       position, created_at, updated_at, visual, degraded
FROM learning_lesson_sections;
DROP TABLE learning_lesson_sections;
ALTER TABLE learning_lesson_sections_new RENAME TO learning_lesson_sections;
CREATE INDEX idx_learning_lesson_sections_lesson_id
    ON learning_lesson_sections (lesson_id, position);

-- x05: drop enum-set CHECK on learning_review_events
CREATE TABLE learning_review_events_new (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id   TEXT NOT NULL UNIQUE CHECK (
        length(event_id) = 36
        AND lower(event_id) = event_id
        AND event_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(event_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id    TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    source     TEXT NOT NULL,
    item_id    TEXT NOT NULL CHECK (trim(item_id) <> ''),
    created_at INTEGER NOT NULL,
    elapsed_ms INTEGER,
    response   TEXT,
    session_id TEXT,
    forgot     INTEGER NOT NULL DEFAULT 0 CHECK (forgot IN (0, 1)),
    predicted  REAL CHECK (predicted IS NULL OR (predicted > 0.0 AND predicted <= 1.0))
);
INSERT INTO learning_review_events_new
    (id, event_id, user_id, source, item_id, created_at, elapsed_ms, response,
     session_id, forgot, predicted)
SELECT id, event_id, user_id, source, item_id, created_at, elapsed_ms, response,
       session_id, forgot, predicted
FROM learning_review_events;
DROP TABLE learning_review_events;
ALTER TABLE learning_review_events_new RENAME TO learning_review_events;
CREATE INDEX idx_learning_review_events_user_created
    ON learning_review_events (user_id, created_at);
CREATE INDEX idx_learning_review_events_user_id
    ON learning_review_events (user_id);

-- x05: drop enum-set CHECK on learning_review_log
CREATE TABLE learning_review_log_new (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    log_id    TEXT NOT NULL UNIQUE CHECK (
        length(log_id) = 36
        AND lower(log_id) = log_id
        AND log_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(log_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id   TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    source    TEXT NOT NULL,
    item_id   TEXT NOT NULL CHECK (trim(item_id) <> ''),
    rating    INTEGER NOT NULL CHECK (rating >= 0 AND rating <= 4),
    rating_source TEXT NOT NULL,
    elapsed_days INTEGER NOT NULL CHECK (elapsed_days >= 0),
    stability_before REAL,
    difficulty_before REAL,
    r_pred REAL,
    review_day INTEGER NOT NULL CHECK (review_day >= 19000101),
    created_at INTEGER NOT NULL,
    CHECK ((stability_before IS NULL) = (difficulty_before IS NULL)),
    CHECK (stability_before IS NULL OR stability_before >= 0.0),
    CHECK (difficulty_before IS NULL OR (difficulty_before >= 1.0 AND difficulty_before <= 10.0)),
    CHECK (r_pred IS NULL OR (r_pred > 0.0 AND r_pred <= 1.0))
);
INSERT INTO learning_review_log_new
    (id, log_id, user_id, source, item_id, rating, rating_source, elapsed_days,
     stability_before, difficulty_before, r_pred, review_day, created_at)
SELECT id, log_id, user_id, source, item_id, rating, rating_source, elapsed_days,
       stability_before, difficulty_before, r_pred, review_day, created_at
FROM learning_review_log;
DROP TABLE learning_review_log;
ALTER TABLE learning_review_log_new RENAME TO learning_review_log;
CREATE INDEX idx_learning_review_log_item_day
    ON learning_review_log(source, item_id, review_day);
CREATE INDEX idx_learning_review_log_user_day
    ON learning_review_log(user_id, review_day);

-- x05: drop enum-set CHECK on learning_question_tags
CREATE TABLE learning_question_tags_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    question_id TEXT NOT NULL CHECK (
        length(question_id) = 36
        AND lower(question_id) = question_id
        AND question_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(question_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    source      TEXT NOT NULL,
    tag_id      TEXT NOT NULL CHECK (
        length(tag_id) = 36
        AND lower(tag_id) = tag_id
        AND tag_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(tag_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    UNIQUE (question_id, source, tag_id)
);
INSERT INTO learning_question_tags_new (id, question_id, source, tag_id)
SELECT id, question_id, source, tag_id FROM learning_question_tags;
DROP TABLE learning_question_tags;
ALTER TABLE learning_question_tags_new RENAME TO learning_question_tags;
CREATE INDEX idx_learning_question_tags_activity_id
    ON learning_question_tags(question_id) WHERE source = 'course';
CREATE INDEX idx_learning_question_tags_custom_question_id
    ON learning_question_tags(question_id) WHERE source = 'custom';
CREATE INDEX idx_learning_question_tags_tag_id
    ON learning_question_tags(tag_id);

-- x05 + x02: drop enum-set CHECK and the dead extra_json column on
-- learning_lesson_progress
CREATE TABLE learning_lesson_progress_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    enrollment_id TEXT NOT NULL CHECK (
        length(enrollment_id) = 36
        AND lower(enrollment_id) = enrollment_id
        AND enrollment_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(enrollment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    lesson_id     TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    status        TEXT NOT NULL,
    started_at    INTEGER,
    completed_at  INTEGER,
    updated_at    INTEGER NOT NULL,
    UNIQUE (enrollment_id, lesson_id)
);
INSERT INTO learning_lesson_progress_new
    (id, enrollment_id, lesson_id, status, started_at, completed_at, updated_at)
SELECT id, enrollment_id, lesson_id, status, started_at, completed_at, updated_at
FROM learning_lesson_progress;
DROP TABLE learning_lesson_progress;
ALTER TABLE learning_lesson_progress_new RENAME TO learning_lesson_progress;
CREATE INDEX idx_learning_lesson_progress_enrollment_id ON learning_lesson_progress (enrollment_id);
CREATE INDEX idx_learning_lesson_progress_lesson_id     ON learning_lesson_progress (lesson_id);

-- x05 + x02: drop enum-set CHECK and the dead graph_meta_json column on
-- learning_courses
CREATE TABLE learning_courses_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id    TEXT NOT NULL UNIQUE CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    title        TEXT NOT NULL CHECK (trim(title) <> ''),
    description  TEXT NOT NULL DEFAULT '',
    domain       TEXT NOT NULL DEFAULT 'general' CHECK (trim(domain) <> ''),
    source_kb_id TEXT CHECK (
        source_kb_id IS NULL OR (
            length(source_kb_id) = 36
            AND lower(source_kb_id) = source_kb_id
            AND source_kb_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(source_kb_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    version      INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    blueprint_json TEXT,
    samples_json TEXT,
    course_kind  TEXT NOT NULL DEFAULT 'traditional',
    learning_goal TEXT NOT NULL DEFAULT '',
    learning_scope TEXT NOT NULL DEFAULT '',
    teaching_style TEXT NOT NULL DEFAULT 'standard',
    deleted_at   INTEGER,
    structure_json TEXT CHECK (
        structure_json IS NULL
        OR (json_valid(structure_json) AND json_type(structure_json) = 'object')
    ),
    samples_meta_json TEXT CHECK (
        samples_meta_json IS NULL OR (
            json_valid(samples_meta_json) AND json_type(samples_meta_json) = 'object'
        )
    ),
    source_kb_ids TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(source_kb_ids) AND json_type(source_kb_ids) = 'array'
    ),
    enabled      INTEGER NOT NULL DEFAULT 1,
    course_source_plan_json TEXT CHECK (
        course_source_plan_json IS NULL OR (
            json_valid(course_source_plan_json) AND json_type(course_source_plan_json) = 'object'
        )
    )
);
INSERT INTO learning_courses_new
    (id, course_id, title, description, domain, source_kb_id, version, created_at,
     updated_at, blueprint_json, samples_json, course_kind, learning_goal,
     learning_scope, teaching_style, deleted_at, structure_json, samples_meta_json,
     source_kb_ids, enabled, course_source_plan_json)
SELECT id, course_id, title, description, domain, source_kb_id, version, created_at,
       updated_at, blueprint_json, samples_json, course_kind, learning_goal,
       learning_scope, teaching_style, deleted_at, structure_json, samples_meta_json,
       source_kb_ids, enabled, course_source_plan_json
FROM learning_courses;
DROP TABLE learning_courses;
ALTER TABLE learning_courses_new RENAME TO learning_courses;
CREATE INDEX idx_learning_courses_source_kb_id
    ON learning_courses(source_kb_id);
CREATE INDEX idx_learning_courses_source_kb_ids_json
    ON learning_courses(source_kb_ids);

-- ==== §11 退役死物（x02）====
-- learning_course_jobs：自 j03 生成任务注册表接管后零消费者（无路由、无
-- service、ui 零引用）；直接 DROP，不走 RENAME 观察期（还原按 PRODUCT_TABLES
-- 枚举表跳过，谱系必须 Current——删表不新增还原风险，Q9）。
DROP TABLE learning_course_jobs;

-- ==== §12 FSRS 算法代际列（x10）====
-- learning_review_log（052 已发布 → 基线承载）：记录推进时的算法代际。
-- NULL = 历史行（按 fsrs6 口径）；新真实行写 'fsrs6'，合成种卡行写 NULL。
ALTER TABLE learning_review_log ADD COLUMN algo TEXT;

-- ==== §13 学习系统动作簿（y04）====
-- learning_system_actions：学习图课程的自主/后台动作**终态**留痕（一轮吸收
-- learnhub `ActionRecord`，main@22d016d）。一次动作一行终态（done|failed）：
-- 进行中状态看 generation task 注册表（内存），终态看本表——同一动作不会
-- 同时有两行（开始不落账，取消/忙拒零行）。
--
-- kind：maintenance_repair | maintenance_mount | growth（Rust 单源
-- `learning_graph::system_action::SystemActionKind`）。
-- trigger_source：timer | manual | settlement | auto_refill
--（`SystemActionTrigger`；列名避开 TRIGGER 保留字——v3 契约禁止物理触发器
-- 令牌，读侧视图字段仍叫 `trigger`）。
-- result：done | failed（`SystemActionResult`）。枚举集不设 DB CHECK
-- （ADR-0015）：写侧严格、读侧宽容。
-- basis：触发时的一句判据读数（unconsumed=2 pending_mount=1 stale=0 一类，
-- 写侧截断 ≤200 字）；reason：failed 原因（done 为空）；ref_id：关联对象
-- 的**引用不复制载荷**（growth = batch_id / repair = 修补审计行 id /
-- mount = 挂载行 id）；actor_user_id：仅手动动作的操作者（自动动作 NULL）。
-- 读侧按课程倒序（course_id, at）+ limit，见
-- GET /api/learning/courses/{id}/system-actions。
CREATE TABLE learning_system_actions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    action_id     TEXT NOT NULL UNIQUE CHECK (
        length(action_id) = 36
        AND lower(action_id) = action_id
        AND action_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(action_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id     TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    kind          TEXT NOT NULL,
    trigger_source TEXT NOT NULL,
    basis         TEXT NOT NULL DEFAULT '',
    result        TEXT NOT NULL,
    reason        TEXT NOT NULL DEFAULT '',
    -- 引用不复制载荷：批次/审计行/挂载行的业务或行标识；多态，不做逻辑引用。
    ref_id        TEXT NOT NULL DEFAULT '',
    actor_user_id TEXT CHECK (
        actor_user_id IS NULL OR (
            length(actor_user_id) = 36
            AND lower(actor_user_id) = actor_user_id
            AND actor_user_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(actor_user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    at            INTEGER NOT NULL
);

-- 读侧索引：按课程 + 时间倒序截断（同毫秒行以 id 倒序稳定）。
CREATE INDEX idx_learning_system_actions_course
    ON learning_system_actions (course_id, at);
-- 逻辑引用（actor_user_id → users）的具名索引（v3 契约对账要求）。
CREATE INDEX idx_learning_system_actions_actor
    ON learning_system_actions (actor_user_id);
