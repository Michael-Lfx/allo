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
--   表名沿用旧概念绑定表（已在 066 DROP），语义完全更换：role 区分教学/
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
