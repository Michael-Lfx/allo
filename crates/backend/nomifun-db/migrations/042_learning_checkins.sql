-- 每日打卡：完成即锁定 + 复习事件明细。
--
-- learning_checkins 每天一行（按复习日），记录锁定时刻的快照（目标与已刷
-- 数量），锁定后当天新增到期卡不再改变打卡状态。幂等：UNIQUE(user_id,
-- review_day)，重复判定走 INSERT OR IGNORE。
--
-- learning_review_events 记录每次复习动作（answer_review / answer_custom_
-- review），供打卡按复习日聚合"当日累计复习"。不能复用 learning_attempts：
-- 它同时承载课程练习且无 user_id 列，无法区分练习与复习。
--
-- v3 contract: 无物理外键、无触发器。

CREATE TABLE learning_review_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id   TEXT NOT NULL UNIQUE CHECK (
        length(event_id) = 36
        AND lower(event_id) = event_id
        AND event_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(event_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id    TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 'course' = 课时客观题复习，'custom' = 自建题复习
    source     TEXT NOT NULL CHECK (source IN ('course', 'custom')),
    -- 课程复习为 review_item_id，自建题为 custom_question_id
    item_id    TEXT NOT NULL CHECK (trim(item_id) <> ''),
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_learning_review_events_user_created
    ON learning_review_events (user_id, created_at);

CREATE INDEX idx_learning_review_events_user_id
    ON learning_review_events (user_id);

CREATE TABLE learning_checkins (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    checkin_id     TEXT NOT NULL UNIQUE CHECK (
        length(checkin_id) = 36
        AND lower(checkin_id) = checkin_id
        AND checkin_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(checkin_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id        TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 复习日（本地 YYYYMMDD，与复习日定义一致）
    review_day     INTEGER NOT NULL CHECK (review_day >= 20000101 AND review_day <= 29991231),
    -- 锁定时刻的目标快照；0 表示"仅清空队列"目标
    goal           INTEGER NOT NULL CHECK (goal >= 0),
    reviewed_count INTEGER NOT NULL CHECK (reviewed_count >= 0),
    completed_at   INTEGER NOT NULL,
    UNIQUE (user_id, review_day)
);

CREATE INDEX idx_learning_checkins_user_id
    ON learning_checkins (user_id);
