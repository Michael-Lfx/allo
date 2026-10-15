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
