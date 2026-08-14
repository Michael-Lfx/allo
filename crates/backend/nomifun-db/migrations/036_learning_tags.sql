-- 学习模块标签：名称全局唯一，手动输入不存在的标签时自动新增；
-- 保存端按 trim 后精确匹配去重（大小写敏感）。
CREATE TABLE learning_tags (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    tag_id     TEXT NOT NULL UNIQUE CHECK (
        length(tag_id) = 36
        AND lower(tag_id) = tag_id
        AND tag_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(tag_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    name       TEXT NOT NULL UNIQUE CHECK (trim(name) <> '' AND length(name) <= 50),
    created_at INTEGER NOT NULL
);

-- 课程-标签关联；课程删除时关联一并清除。
CREATE TABLE learning_course_tags (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    tag_id    TEXT NOT NULL CHECK (
        length(tag_id) = 36
        AND lower(tag_id) = tag_id
        AND tag_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(tag_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    UNIQUE (course_id, tag_id)
);

-- 问题-标签关联：source 区分课程问题（course，question_id 为 activity_id）
-- 与自定义问题（custom，question_id 为 custom_question_id）。
CREATE TABLE learning_question_tags (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    question_id TEXT NOT NULL CHECK (
        length(question_id) = 36
        AND lower(question_id) = question_id
        AND question_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(question_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    source      TEXT NOT NULL CHECK (source IN ('course', 'custom')),
    tag_id      TEXT NOT NULL CHECK (
        length(tag_id) = 36
        AND lower(tag_id) = tag_id
        AND tag_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(tag_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    UNIQUE (question_id, source, tag_id)
);

CREATE INDEX idx_learning_course_tags_course_id
    ON learning_course_tags(course_id);
CREATE INDEX idx_learning_course_tags_tag_id
    ON learning_course_tags(tag_id);
CREATE INDEX idx_learning_question_tags_activity_id
    ON learning_question_tags(question_id) WHERE source = 'course';
CREATE INDEX idx_learning_question_tags_custom_question_id
    ON learning_question_tags(question_id) WHERE source = 'custom';
CREATE INDEX idx_learning_question_tags_tag_id
    ON learning_question_tags(tag_id);
