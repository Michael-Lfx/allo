-- ==== 070_learning_maintenance_state.sql ====
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
-- 没了水位失去意义；findings 留痕本身仍 KeepHistory 保留，迁移 068 口径）。
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
