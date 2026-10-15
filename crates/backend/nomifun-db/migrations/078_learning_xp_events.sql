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
