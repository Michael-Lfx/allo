-- 课程生成任务（learning course generation jobs）。
--
-- 课程生成（HTTP 直接生成 / agent 工具触发）统一改为持久化任务：
-- 学习页可查看阶段进度（sampling → blueprint → lessons → importing），
-- 取消后可从已完成课时继续，失败可重试。执行器在阶段边界持久化中间
-- 产物（samples_json / blueprint_json / lesson_outputs_json），并检查
-- cancel_requested 标志；进程中断遗留的非终态任务在启动时置为
-- interrupted，可恢复。
--
-- v3 contract: local AUTOINCREMENT row identity, no physical foreign keys,
-- no triggers; cross-entity links are plain indexed columns. job_id is a
-- canonical UUIDv7 registered in UUIDV7_BUSINESS_COLUMNS; session_id is a
-- non-reference identity column (NON_REFERENCE_ID_COLUMNS) mirroring goals.
CREATE TABLE learning_course_jobs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id             TEXT NOT NULL UNIQUE CHECK (
        length(job_id) = 36
        AND lower(job_id) = job_id
        AND job_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(job_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id            TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    session_id         TEXT,
    source             TEXT NOT NULL CHECK (source IN ('http', 'agent')),
    kb_id              TEXT NOT NULL CHECK (
        length(kb_id) = 36
        AND lower(kb_id) = kb_id
        AND kb_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(kb_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    request_json       TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'queued' CHECK (
        status IN ('queued', 'sampling', 'blueprint', 'lessons', 'importing', 'completed', 'failed', 'cancelled', 'interrupted')
    ),
    current_module     INTEGER NOT NULL DEFAULT 0,
    current_lesson     INTEGER NOT NULL DEFAULT 0,
    total_lessons      INTEGER NOT NULL DEFAULT 0,
    samples_json       TEXT,
    blueprint_json     TEXT,
    lesson_outputs_json TEXT,
    course_id          TEXT CHECK (
        course_id IS NULL OR (
            length(course_id) = 36
            AND lower(course_id) = course_id
            AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    error              TEXT,
    cancel_requested   INTEGER NOT NULL DEFAULT 0,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

CREATE INDEX idx_learning_course_jobs_user_id ON learning_course_jobs(user_id);
CREATE INDEX idx_learning_course_jobs_kb_id ON learning_course_jobs(kb_id);
CREATE INDEX idx_learning_course_jobs_course_id ON learning_course_jobs(course_id);
CREATE INDEX idx_learning_course_jobs_status ON learning_course_jobs(status);
CREATE INDEX idx_learning_course_jobs_user_created
    ON learning_course_jobs(user_id, created_at DESC);
