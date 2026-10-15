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
