-- 学习图课程（Beta）：概念图实验功能升级为课程类型（破坏性更新，不迁移旧实验数据）。
--
-- 节点即 learning_lessons 行（挂在每个图课程唯一的隐含模块下），进度/练习/复习/
-- 打卡/日历全套复用课时层；前置关系独立成表 learning_graph_prerequisites；
-- graph_meta_json 为课程级扩展冗余（审计快照/生成留档/每节点渲染备注），不承载
-- 图结构本身——结构的事实来源是 learning_lessons + learning_graph_prerequisites
-- 两张表。未来若 graph_meta_json 出现稳定的关系型 ID 路径（如 nodes.<lesson_id>
-- 键控映射），须同步登记 id_schema_contract::JSON_LOGICAL_REFERENCES。
--
-- v3 contract: 仅追加列/新表/重建表，无物理外键、无触发器。

ALTER TABLE learning_courses ADD COLUMN course_kind TEXT NOT NULL DEFAULT 'traditional'
    CHECK (course_kind IN ('traditional', 'learning_graph'));
ALTER TABLE learning_courses ADD COLUMN learning_goal TEXT NOT NULL DEFAULT '';
ALTER TABLE learning_courses ADD COLUMN learning_scope TEXT NOT NULL DEFAULT '';
ALTER TABLE learning_courses ADD COLUMN graph_meta_json TEXT CHECK (
    graph_meta_json IS NULL OR (json_valid(graph_meta_json) AND json_type(graph_meta_json) = 'object'));

-- 前置边：from（prerequisite_lesson_id）应先于 to（lesson_id）被满足。
-- kind 预留边类型（beta 恒为 'prerequisite'），extra_json 承载未来属性
-- （权重/掌握度衰减系数等），方向之外的扩展不改表。
CREATE TABLE learning_graph_prerequisites (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id              TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    lesson_id              TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    prerequisite_lesson_id TEXT NOT NULL CHECK (
        length(prerequisite_lesson_id) = 36
        AND lower(prerequisite_lesson_id) = prerequisite_lesson_id
        AND prerequisite_lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(prerequisite_lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    reason                 TEXT NOT NULL DEFAULT '',
    kind                   TEXT NOT NULL DEFAULT 'prerequisite' CHECK (trim(kind) <> ''),
    extra_json             TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(extra_json) AND json_type(extra_json) = 'object'
    ),
    UNIQUE (lesson_id, prerequisite_lesson_id),
    CHECK (lesson_id <> prerequisite_lesson_id)
);

CREATE INDEX idx_learning_graph_prereq_course ON learning_graph_prerequisites (course_id);
CREATE INDEX idx_learning_graph_prereq_lesson ON learning_graph_prerequisites (lesson_id);
CREATE INDEX idx_learning_graph_prereq_pre    ON learning_graph_prerequisites (prerequisite_lesson_id);

-- 节点跳过（skipped）：学习者声明已掌握、跳过学习。skipped 不是 completed：
-- completed_at 保持 NULL、不种复习项；已 in_progress 的行跳过时保留 started_at
-- 历史。extra_json 承载未来按用户的节点级学习度量（掌握程度、遗忘曲线状态等），
-- 在它们毕业成正式列/表之前先落这里。SQLite 无法改 CHECK，按标准 rebuild
-- 流程扩列约束；旧行在旧 CHECK 下全部合法，纯放宽不改语义。
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
    status        TEXT NOT NULL CHECK (status IN ('not_started', 'in_progress', 'completed', 'skipped')),
    started_at    INTEGER,
    completed_at  INTEGER,
    updated_at    INTEGER NOT NULL,
    extra_json    TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(extra_json) AND json_type(extra_json) = 'object'
    ),
    UNIQUE (enrollment_id, lesson_id),
    CHECK (
        (status = 'not_started' AND started_at IS NULL AND completed_at IS NULL)
        OR (status = 'in_progress' AND started_at IS NOT NULL AND completed_at IS NULL)
        OR (status = 'completed' AND started_at IS NOT NULL AND completed_at IS NOT NULL)
        OR (status = 'skipped' AND completed_at IS NULL)
    )
);

INSERT INTO learning_lesson_progress_new
    (id, enrollment_id, lesson_id, status, started_at, completed_at, updated_at)
SELECT id, enrollment_id, lesson_id, status, started_at, completed_at, updated_at
FROM learning_lesson_progress;

DROP TABLE learning_lesson_progress;
ALTER TABLE learning_lesson_progress_new RENAME TO learning_lesson_progress;

CREATE INDEX idx_learning_lesson_progress_enrollment_id ON learning_lesson_progress (enrollment_id);
CREATE INDEX idx_learning_lesson_progress_lesson_id     ON learning_lesson_progress (lesson_id);
