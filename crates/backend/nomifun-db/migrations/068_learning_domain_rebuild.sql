-- 学习域合并基线（068）：分支内原 068-084 共十七个学习迁移，按原顺序合并为一个。
--
-- 合并前提：这些迁移尚未随任何正式版本发布，没有真实用户库需要逐段升级。
-- 合并只减少版本号占用，不改语句顺序与内容：新库仍从 065 世界的旧学习表出发，
-- 得到的最终 schema 与原十七段链逐对象一致。
--
-- 段名保留原迁移文件名，便于对照历史 tracker 文档。

-- ==== 068_retire_graph_dag_and_concepts.sql ====
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
-- 唯一落点是全局概念登记表（本迁移后段引入）。
--
-- 注意：learning_lesson_progress 的 skipped 状态保留——跳过对传统课时同样
-- 语义自洽。learning_review_events 的卡片历史按契约保留（打卡统计不随卡删）。
--
-- v3 contract: 仅删数据/删表/重建表，无物理外键、无触发器。

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
    -- kind CHECK 以 038 的宽口径为准（三种题型）：重建必须逐字保留既有的
    -- 约束集合，收窄会让 038 之后落库的自建填空题在新表上必然 CHECK 失败。
    kind                 TEXT NOT NULL CHECK (
        kind IN ('single_choice', 'true_false', 'fill_in_blank')
    ),
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

-- ==== 069_concept_web_growth.sql ====
-- 学习图重构（ADR-0009）第二步：概念网 + 生长模型的关系型落点。
--
-- 四张新表 + 两列课程扩展：
-- - learning_concept_registry：全局概念登记表（跨课程唯一）。canonical 出生
--   即冻结；别名数组联合唯一（canonical ∪ 别名全库不重名）由代码层校验，
--   SQLite 无法对 JSON 数组做约束。
-- - learning_course_endpoints：终点锚。每条终点同时是一条零正文的标记
--   lesson 行（lesson_id UNIQUE 指向它），title + goal_note（一句程度声明）
--   + completed（教练裁决完成位）。终点不是学习节点，调度与可学列表剔除。
-- - learning_lesson_concepts：概念网边——节点 teaches/assumes 概念 × 档位。
--   表名沿用旧概念绑定表（旧表已在本迁移前段 DROP），语义完全更换：role 区分教学/
--   假定，tier 为 locale 无关档位代码（know < apply < teach）。
-- - learning_growth_batches：生长批次出生档案（序号/节点清单/备注），
--   node_ids_json 是 lesson id 数组（按契约登记 JSON 逻辑引用）。
-- - learning_courses.compass_md / compass_updated_at：罗盘（逐终点剩余路线
--   摘要）正文与重画时刻；终点变更即重画。
--
-- v3 contract: 仅新表/追加列，无物理外键、无触发器。

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
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE learning_course_endpoints (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id TEXT NOT NULL UNIQUE CHECK (
        length(endpoint_id) = 36
        AND lower(endpoint_id) = endpoint_id
        AND endpoint_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(endpoint_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id   TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    lesson_id   TEXT NOT NULL UNIQUE CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    goal_note   TEXT NOT NULL DEFAULT '',
    completed   INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    declared_at INTEGER NOT NULL
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
    role       TEXT NOT NULL CHECK (role IN ('teaches', 'assumes')),
    tier       TEXT NOT NULL CHECK (tier IN ('know', 'apply', 'teach')),
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
    UNIQUE (course_id, seq)
);

ALTER TABLE learning_courses ADD COLUMN compass_md TEXT;
ALTER TABLE learning_courses ADD COLUMN compass_updated_at INTEGER;

CREATE INDEX idx_learning_concept_registry_canonical
    ON learning_concept_registry (canonical);
CREATE INDEX idx_learning_course_endpoints_course
    ON learning_course_endpoints (course_id);
CREATE INDEX idx_learning_course_endpoints_lesson
    ON learning_course_endpoints (lesson_id);
CREATE INDEX idx_learning_lesson_concepts_lesson_id
    ON learning_lesson_concepts (lesson_id);
CREATE INDEX idx_learning_lesson_concepts_concept_id
    ON learning_lesson_concepts (concept_id);
CREATE INDEX idx_learning_growth_batches_course
    ON learning_growth_batches (course_id, seq);
CREATE INDEX idx_learning_growth_batches_nodes
    ON learning_growth_batches (node_ids_json);

-- ==== 070_growth_batch_status.sql ====
-- 生长治理（ADR-0009 Amendment 1）：批行状态机。
--
-- learning_growth_batches 从"出生档案"升级为"生长任务的持久化载体"：
-- 生长开始即插 pending 行（含 seq 预留），成功填 node_ids 转 applied，
-- 失败留 failed 行（应用重启后 pending 即中断痕迹，下次生长起扫为
-- failed），空手批（idle）删行不残留。历史时间线据此展示"生长中/失败"。
--
-- 既有行全部是已落库的成功批，默认 applied。
--
-- v3 contract: 仅追加列，无物理外键、无触发器。

ALTER TABLE learning_growth_batches ADD COLUMN status TEXT NOT NULL DEFAULT 'applied'
    CHECK (status IN ('pending', 'applied', 'failed'));

CREATE INDEX idx_learning_growth_batches_pending
    ON learning_growth_batches (status, created_at);

-- ==== 071_learning_course_soft_delete.sql ====
-- 学习图课程目录删除改为软删除（ADR-0009 后续裁定的删除语义）：
-- 课程行保留为墓碑（名称留作标识、deleted_at 即删除状态位），课时/模块/
-- 概念网边按勾选物理删除；遗留数据凭 course_id 归账，清理时有据可查。
ALTER TABLE learning_courses ADD COLUMN deleted_at INTEGER;

-- ==== 072_learning_lesson_difficulty.sql ====
-- 节点难度（1-5）：教练生长批的深度水位执法面——本批最高难度不得超过
-- 已落库批次数 + 2，形式化/证明类内容（4-5）必须等台阶铺满低档后再长。
ALTER TABLE learning_lessons ADD COLUMN difficulty INTEGER NOT NULL DEFAULT 2;

-- ==== 073_retire_endpoints_compass_structure_snapshot.sql ====
-- 学习模块吸收 learnhub（ADR-0010）：终点锚体系与罗盘整体退役 + 结构快照落点。
--
-- 破坏性迁移（沿用图课程退役的先例）：建课从「目标 → AI 提议终点 → 多终点选择 → 生长」
-- 改为「目标 → 一次生成知识结构（结构 + 前置地形）→ 生长」，结构仅作参考
-- 地图，学习顺序权威归教练 + 已教概念账：
-- - learning_course_endpoints 整表 DROP：终点不再是交互与判定面的一等公民
--   （提议端点、完成裁决、停摆臂第二臂、终点保护条款随之退役）；方向语义
--   由结构产物的读侧投影承担（路线图/覆盖徽章，永不落盘）。
-- - 每条终点兼作一行零正文标记课时（lesson_id UNIQUE），先清其进度行再删
--   课时行，随后整表 DROP（两个专用索引随表消失）。
-- - learning_courses.compass_md / compass_updated_at DROP：罗盘的分节前提
--   （终点）消失，逐终点剩余路线摘要连同重画管道整体退役。
-- - learning_courses.structure_json ADD：知识结构 + 前置地形两产物的单字段
--   JSON 快照（ADR-0002 铁律：快照是唯一落盘真相，一切视图运行时重算、
--   永不落盘第二份；不建关系表存块间边）。存量课程为 NULL = 结构未生成
--   （Missing 合法空状态）；重跑生成即原子覆盖。课程行与 learning_goal
--   原样保留（终点只是目标语句的派生，原始目标仍在）。
--
-- 存量失败批次不受影响（learning_growth_batches.status 状态机不触碰）。
--
-- v3 contract: 删数据/删表/删列/加列，无物理外键、无触发器。

DELETE FROM learning_lesson_progress WHERE lesson_id IN
    (SELECT lesson_id FROM learning_course_endpoints);

DELETE FROM learning_lessons WHERE lesson_id IN
    (SELECT lesson_id FROM learning_course_endpoints);

DROP TABLE learning_course_endpoints;

ALTER TABLE learning_courses DROP COLUMN compass_md;
ALTER TABLE learning_courses DROP COLUMN compass_updated_at;

ALTER TABLE learning_courses ADD COLUMN structure_json TEXT CHECK (
    structure_json IS NULL
    OR (json_valid(structure_json) AND json_type(structure_json) = 'object')
);

-- ==== 074_learning_course_sampling_meta.sql ====
-- 目标感知采样（t2）：`learning_courses` 增加采样披露快照列。
--
-- 采样的既有两列各司其职、都不动：`samples_json` 承载 `Vec<(String,
-- String)>`（路径 → 摘录），既有消费方（课时生成上下文、课件邻居上下文）
-- 按该形状解析；本列是它的**伴随结构**——完整文件树（目录结构保留）、
-- 已采/未采计数、因低分未采的文件清单，供 UI 披露与诊断展示。
--
-- 目标文本本身不进本列：它复用 `learning_courses.learning_goal`
-- （048 已有，NOT NULL DEFAULT ''，旧行读出为空串），与学习图课程同列
-- 承载同类语义，因此本条迁移不涉目标列。
--
-- Nullable、无 default：NULL 表示"无目标路径（字母序回退）或旧课程"——
-- 与空对象 {} 必须可区分，旧课程快照继续按旧形状读取，不做回填。
ALTER TABLE learning_courses ADD COLUMN samples_meta_json TEXT CHECK (
    samples_meta_json IS NULL OR (
        json_valid(samples_meta_json) AND json_type(samples_meta_json) = 'object'
    )
);

-- ==== 075_learning_course_source_kb_ids.sql ====
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

-- ==== 076_learning_course_source_kb_ids_index.sql ====
-- 多知识库（t4 评审修复）：`learning_courses.source_kb_ids` 登记进逻辑引用
-- 契约所需的具名索引。
--
-- 前段 source_kb_ids 迁移加了列表列，但漏了契约登记：`id_schema_contract.rs` 的 JSON 列表
-- 登记（`cs_agents.knowledge_base_ids` 先例，迁移 019）要求该列上有具名索引，
-- 登记条目本身才是一条活的 schema 断言（缺索引 = 启动即失败）。
-- 索引口径与先例逐字一致：普通列进索引（读侧 json_each 展开与孤儿审计按
-- 索引名引用），不是表达式索引。
CREATE INDEX idx_learning_courses_source_kb_ids_json
    ON learning_courses(source_kb_ids);

-- ==== 077_learning_lesson_source_paths.sql ====
-- 课时多引用（t4 评审补齐：t3 把"课时 source 多引用"划给 t4，t4 只做了
-- `kb_id` 归属）：`learning_lessons` 增加来源路径列表。
--
-- 列口径仿 `learning_courses.source_kb_ids`（前段 source_kb_ids 迁移）与
-- `cs_agents.knowledge_base_ids`（迁移 019）：JSON 数组，CHECK 只放行数组。
-- **首个元素 = 主来源**，且与既有的三列（`source_path`/`source_start`/
-- `source_end`，迁移 015）同值——三列保留不动（既有读侧与索引不退役），
-- 本列是它的伴生扩展：老行由三列回填为单元素数组（无来源的 NULL 行回填为
-- 空数组，读侧回退三列）。
--
-- 元素形状与 `SourceSpan` 序列化同构（`{"path":…,"start":…,"end":…}`）；
-- 库归属（`kb_id`）不落库——读侧按采样命名空间实时解析，与课时单列
-- 路径的读侧口径逐字一致（双读与已删库过滤见 source_kb_ids 的 ADR）。
ALTER TABLE learning_lessons ADD COLUMN source_paths_json TEXT NOT NULL DEFAULT '[]' CHECK (
    json_valid(source_paths_json) AND json_type(source_paths_json) = 'array'
);

UPDATE learning_lessons
SET source_paths_json = json_array(
        json_object('path', source_path, 'start', source_start, 'end', source_end)
    )
WHERE source_path IS NOT NULL;

-- ==== 078_learning_course_enabled.sql ====
-- 课程级「现存且启用」口径（三轮 g3）：enabled 翻转 = 归档/恢复（数据零删
-- 零改，可逆暂停；删除仍走软删墓碑不可逆）。活跃课程集单源 =
-- deleted_at IS NULL AND enabled = 1：安排面（到期队列/就绪推荐/教练入口/
-- 课时内容生成）只服务现存且启用的课程；历史面（日历/统计聚合/打卡历史）
-- 不过筛，已计入的学习记录如实保留。
ALTER TABLE learning_courses ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;

-- ==== 079_concept_alias_records.sql ====
-- 概念别名结构化升级（三轮 g2，j04）：aliases_json 元素从字符串升为轻量
-- 记录 {name, definition?, attestation?, status}——别名 = 概念的投影命名
-- 面，携带该名字的意义与出处；status = active/retired（退役不占名池，与
-- deprecated 口径衔接）。存量字符串逐个回填 {name, status:"active"}
-- （definition/attestation 缺席 = 与概念定义同义/出处未知）。
--
-- 同时给概念网边加 ref_name（节点引用该概念时使用的名字，可空）：跨课程
-- 反查按投影名分组的数据源（二轮 g2 引用名开讲的存储面），生长落库时写入。
-- 老行为名（空 = 未知，读侧回退概念正名）。

UPDATE learning_concept_registry
SET aliases_json = COALESCE((
    SELECT json_group_array(
        json_object('name', je.value, 'status', 'active')
    )
    FROM json_each(learning_concept_registry.aliases_json) je
), '[]');

ALTER TABLE learning_lesson_concepts ADD COLUMN ref_name TEXT;

-- ==== 080_learning_xp_events.sql ====
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
    kind      TEXT NOT NULL CHECK (kind IN ('node-completed', 'review-completed', 'quiz-completed')),
    -- 净入账（full 时含 xp_bonus 分量）；guess-penalty 必为负，quiz 恒非负。
    earned_xp INTEGER NOT NULL,
    -- 「全对奖励」分量（缺省 2）；仅 full 形态允许非零。
    xp_bonus  INTEGER NOT NULL DEFAULT 0 CHECK (xp_bonus >= 0),
    basis     TEXT NOT NULL CHECK (basis IN ('full', 'partial', 'trimmed', 'guess-penalty')),
    -- 部分正确的授予比例（earned_xp = (基数 × ratio).floor()）；仅 partial 携带。
    ratio     REAL CHECK (ratio IS NULL OR (ratio >= 0.0 AND ratio <= 1.0)),
    -- 结算来源注记（lesson-progress / review-course / review-custom），审计面。
    origin    TEXT,
    -- 幂等键：教新 = 'node:{lesson_id}'；复习 = 'review:{course|custom}:{item_id}:{review_day}'。
    anchor    TEXT NOT NULL CHECK (trim(anchor) <> ''),
    -- 写侧条款 validate_settlement 的数据库地板（四形态 + quiz 恒非负）：
    CHECK (basis = 'guess-penalty' OR earned_xp >= 0),
    CHECK (basis <> 'guess-penalty' OR earned_xp < 0),
    CHECK (kind <> 'quiz-completed' OR earned_xp >= 0),
    CHECK (basis = 'partial' OR ratio IS NULL),
    CHECK (basis <> 'partial' OR ratio IS NOT NULL),
    CHECK (basis = 'full' OR xp_bonus = 0),
    CHECK (basis <> 'trimmed' OR earned_xp = 0)
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

-- ==== 081_learning_structure_mounts.sql ====
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
--
-- v3 contract: 无物理外键、无触发器。

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
    -- 落盘时刻 structure_json 列字节的 FNV-1a 64（u64 存为有符号整型，
    -- 读侧按无符号语义比较）。
    structure_hash INTEGER NOT NULL,
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
    -- 生产方：'concept-review'（评审 warn 通道，随 j09）/ 'mount-observation'
    -- （挂载观察派生，纯读函数不打点）。
    source    TEXT NOT NULL CHECK (source IN ('concept-review', 'mount-observation')),
    -- 上下文锚：节点标题或块 slug（自由文本，允许空串 = 全课面）。
    node      TEXT NOT NULL DEFAULT '',
    field     TEXT NOT NULL DEFAULT '',
    issue     TEXT NOT NULL CHECK (trim(issue) <> ''),
    advice    TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_learning_structure_findings_course
    ON learning_structure_findings(course_id, ts);

-- 逻辑引用前缀索引（挂载行 UNIQUE(course_id) 已保证身份，这里补具名索引
-- 供 v3 逻辑引用契约挂靠）。
CREATE INDEX idx_learning_structure_mounts_course
    ON learning_structure_mounts(course_id);

-- ==== 082_learning_structure_repairs.sql ====
-- 结构修补审计表（j09，三轮 g1「结构转活文档·下」）。
--
-- learning_structure_repairs：一次修补回合一行（applied / noop 两种结论；
-- skipped 零 LLM 不落行）。ops_json = 逐 op 数组（含 type 与字段、reason、
-- 级联注记），结构 hash 迁移（old→new）与「应用后 gd.* 全量重过零放宽」的
-- 结论都在行内——learnhub 修补档案（文件 + Git 出处）的 DB 等价物。三重
-- 护栏 = 全量 validator 门 + 观测审计 + 出处，不建受理门机器。
--
-- v3 contract: 无物理外键、无触发器。

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
    -- applied | noop（ops 空 = 合法结论；noop 不写结构快照，仅留痕）。
    result     TEXT NOT NULL CHECK (result IN ('applied', 'noop')),
    -- 逐 op 数组：[{op, ...字段, reason, 跟随改写说明}]；noop 为空数组。
    ops_json   TEXT NOT NULL CHECK (
        json_valid(ops_json) AND json_type(ops_json) = 'array'
    ),
    note       TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_learning_structure_repairs_course
    ON learning_structure_repairs(course_id, ts);

-- ==== 083_learning_quiz_rounds.sql ====
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
--
-- v3 contract: 无物理外键、无触发器。

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
    status      TEXT NOT NULL CHECK (status IN ('active', 'completed', 'abandoned')),
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
    earned_xp   INTEGER,
    CHECK (status <> 'active' OR (
        finished_at IS NULL AND answers_json IS NULL
        AND report_json IS NULL AND earned_xp IS NULL
    )),
    CHECK (status <> 'completed' OR (
        finished_at IS NOT NULL AND answers_json IS NOT NULL
        AND report_json IS NOT NULL AND earned_xp IS NOT NULL
    )),
    CHECK (status <> 'abandoned' OR (
        finished_at IS NOT NULL AND answers_json IS NULL
        AND report_json IS NULL AND earned_xp IS NULL
    ))
);

-- 同用户同课程至多一个进行中回合（start supersede 流的并发地板）。
CREATE UNIQUE INDEX idx_learning_quiz_rounds_active
    ON learning_quiz_rounds(user_id, course_id) WHERE status = 'active';
-- v3 契约具名索引（逻辑引用扫描面）。
CREATE INDEX idx_learning_quiz_rounds_user
    ON learning_quiz_rounds(user_id);
CREATE INDEX idx_learning_quiz_rounds_course
    ON learning_quiz_rounds(course_id);

-- ==== 084_learning_concept_source.sql ====
-- 概念铸名出处列（smoke-hardening t8，t2 条款 3 补完）。
--
-- learning_concept_registry.source：铸名的 attestation 出处说明（「教练提
-- 名：……」——谁在哪个语境铸的名），可空、纯簿记不进门禁（learnhub 口径
-- 「出处是簿记不是判定」）。**不做书目引用**：allo 无教材数据源，模型写
-- 「人教版七年级上册」必是编造（never-invent）；learnhub 有真实教材数据
-- 才写书引用。旧行缺列读侧默认 NULL。
--
-- v3 contract: 无物理外键、无触发器。append-only（ALTER ADD COLUMN）。

ALTER TABLE learning_concept_registry ADD COLUMN source TEXT;

