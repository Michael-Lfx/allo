use super::*;

impl LearningService {

    /// Loads user-tunable scheduler knobs from client preferences, falling
    /// back to FSRS defaults for anything missing or malformed.
    pub(super) async fn scheduler_settings(&self) -> SchedulerSettings {
        let mut settings = SchedulerSettings::default();
        settings.tz_offset_minutes = self.tz_offset_minutes().await;
        let Ok(rows) = sqlx::query(
            "SELECT key, value FROM client_preferences \
             WHERE key IN ('learning.desiredRetention', 'learning.fsrsParameters')",
        )
        .fetch_all(&self.pool)
        .await
        else {
            return settings;
        };
        for row in rows {
            let (Ok(key), Ok(value)) = (
                row.try_get::<String, _>("key"),
                row.try_get::<String, _>("value"),
            ) else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&value) else {
                continue;
            };
            match key.as_str() {
                "learning.desiredRetention" => {
                    if let Some(v) = parsed.as_f64() {
                        settings.desired_retention = v.clamp(0.7, 0.99) as f32;
                    }
                }
                "learning.fsrsParameters" => {
                    match crate::scheduler::parse_fsrs_params(&parsed) {
                        crate::scheduler::FsrsParams::Fsrs6(params) => {
                            settings.parameters = params;
                        }
                        crate::scheduler::FsrsParams::Unsupported(algo) => {
                            // 未知算法整体回退默认（w 布局与算法绑定，不混搭）。
                            tracing::warn!(
                                algo = %algo,
                                "fsrs params algo unsupported, falling back to defaults"
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        settings
    }

    /// Local-time offset in minutes east of UTC, reported by the frontend
    /// (`learning.tzOffsetMinutes`); falls back to the server's local offset
    /// when the preference is missing or malformed.
    pub(super) async fn tz_offset_minutes(&self) -> i32 {
        let Ok(row) = sqlx::query_scalar::<_, String>(
            "SELECT value FROM client_preferences WHERE key = 'learning.tzOffsetMinutes'",
        )
        .fetch_optional(&self.pool)
        .await
        else {
            return SchedulerSettings::default().tz_offset_minutes;
        };
        match row {
            Some(value) => serde_json::from_str::<Value>(&value)
                .ok()
                .and_then(|parsed| parsed.as_i64())
                .and_then(|minutes| i32::try_from(minutes).ok())
                .filter(|minutes| (-24 * 60..=24 * 60).contains(minutes))
                .unwrap_or_else(|| SchedulerSettings::default().tz_offset_minutes),
            None => SchedulerSettings::default().tz_offset_minutes,
        }
    }

    /// Daily check-in status for the current review day — j05 起按 XP 口径
    /// （打卡计数口径退役：`dailyCheckinGoal` 的「复习事件计数」停用，日的
    /// 达标读数统一按 XP，日目标 = 应用设置 `learning.dailyXpGoal`，缺省 40；
    /// 学习日 02:00 翻转沿用）。完成即锁定：今日 XP 达标锁定当天，锁定后
    /// 新增到期卡不再改变打卡状态。`reviewed_count` 字段承载今日净入账
    /// （字段名保留到 j06 换 UI 数据源）。
    pub async fn checkin_today(&self, user_id: &UserId) -> Result<CheckinStatus, AppError> {
        let now = now_ms();
        let tz_offset_minutes = self.tz_offset_minutes().await;
        let review_day_start = review_day_start_utc(now, tz_offset_minutes);
        let review_day = review_day_number(now, tz_offset_minutes);

        let goal = self.daily_xp_goal().await;
        let today_xp: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
             WHERE user_id = ? AND ts >= ?",
        )
        .bind(user_id.as_str())
        .bind(review_day_start)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;

        let locked: Option<(i64, i64)> = sqlx::query_as(
            "SELECT reviewed_count, completed_at FROM learning_checkins \
             WHERE user_id = ? AND review_day = ?",
        )
        .bind(user_id.as_str())
        .bind(review_day)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;

        let (completed, locked_at) = if let Some((_, completed_at)) = locked {
            (true, Some(completed_at))
        } else if today_xp >= goal {
            // Lock the review day; the UNIQUE(user_id, review_day) constraint
            // keeps concurrent evaluations idempotent. The snapshot stores the
            // XP goal and the day's XP at lock time (was: count + count goal).
            sqlx::query(
                "INSERT OR IGNORE INTO learning_checkins \
                 (checkin_id, user_id, review_day, goal, reviewed_count, completed_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(generate_id())
            .bind(user_id.as_str())
            .bind(review_day)
            .bind(goal)
            .bind(today_xp)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
            let completed_at: i64 = sqlx::query_scalar(
                "SELECT completed_at FROM learning_checkins WHERE user_id = ? AND review_day = ?",
            )
            .bind(user_id.as_str())
            .bind(review_day)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
            (true, Some(completed_at))
        } else {
            (false, None)
        };

        Ok(CheckinStatus {
            review_day,
            goal,
            reviewed_count: today_xp,
            completed,
            locked_at,
        })
    }

}

