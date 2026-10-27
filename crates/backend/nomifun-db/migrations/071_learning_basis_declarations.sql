-- ==== 071_learning_basis_declarations.sql ====
-- 已会声明锚（m2，ADR-0032 域中立移植）：学习者对已具备入门内容的显式声明
-- ——勾选地形门口判据，系统按声明铸概念（走既有铸名纪律），**只铸概念不落
-- 节点**：不排课、不计 XP、不建任何学习行为。已知集 = 已教 ∪ 已会声明。
--
-- 一行一次声明（幂等增量语义 = 累计并集差）：每行记本次声明勾选的门口、
-- 命中的目标块（door_refs AND 口径）与铸出/复用的 concept_id。**锚表即铸造
-- 出处**（allo 无 mint-log）：概念库可经 concept_ids_json 反查「来自哪次声
-- 明」。撤销声明 = 删锚行——概念留池（登记表全局有效，来源标记保留不抹）：
-- 「声明可撤、概念不灭」。
--
-- v3 contract: 仅新表，无物理外键、无触发器。

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
