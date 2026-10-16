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
