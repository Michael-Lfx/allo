//! 生长唤醒维护回合（sb2，ADR-0018 决定 4）：非空手生长批落库后**唤醒**维护
//! 回合，唤醒时只跑挂载、不跑修补。
//!
//! 形态（ADR-0012「挂载不内联生长批」不变）：
//! - **唤醒而非内联**：生长只在批次事务提交之后登记一条唤醒意图并发信号；
//!   挂载仍由维护调度器（app-server）在回合纪律里跑（单课串行、可取消、
//!   自己的重试与账本）——两条链路的失败、取消、预算不缠在一起。
//! - **仅挂载**：被唤醒的课程只走挂载分支，跳过修补。修补会改结构、使全部
//!   挂载失效，是全局动作，仍只按定时节奏跑。
//! - **自动路径的边界**（不唤醒，退回手动「结构对齐」）：挂载失效（结构变
//!   过，或从未挂载）、待挂节点数超过 8。条件不满足只写日志，**不写
//!   findings**——findings 是修补回合的触发条件与输入，不是惰性日志。
//! - **去抖**：同课一次生长只登记一次；距该课上次成功挂载不足 5 分钟不再
//!   唤醒。
//! - **失败只留痕**：挂载失败走既有 Failed 动作簿行；不回滚已落库的生长批、
//!   不重试整链。节点仍在待挂集，下次唤醒再试。

use super::*;

/// 待挂节点数上限（含）：超过即不由唤醒触发——课程从未挂载过而节点已很多
/// 时，第一次唤醒不会突然附带一次大调用。
const GROWTH_WAKE_PENDING_MOUNT_LIMIT: usize = 8;

/// 去抖窗口（毫秒）：距该课上次**成功**挂载不足此时长不再唤醒。
const GROWTH_WAKE_DEBOUNCE_MS: i64 = 5 * 60 * 1000;

/// 一次生长唤醒的意图：对 `course_id` 跑一个仅挂载维护回合，以 `user_id`
/// 的身份（生长以谁的身份跑，挂载就以谁的身份跑）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowthWake {
    pub course_id: LearningCourseId,
    pub user_id: UserId,
}

/// 不唤醒的原因（日志 `reason` 字段的机器名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrowthWakeSkipReason {
    /// 挂载失效：结构变过需全量重挂，或从未挂载（读数同为 `mounts_stale`）。
    Stale,
    /// 待挂节点数超过上限。
    PendingOverThreshold,
    /// 距上次成功挂载不足去抖窗口。
    Debounced,
}

impl GrowthWakeSkipReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::PendingOverThreshold => "pending_over_threshold",
            Self::Debounced => "debounced",
        }
    }
}

/// 唤醒准入判定的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrowthWakeVerdict {
    /// 唤醒：有待挂节点，且三道准入全过。
    Wake,
    /// 无事可做：待挂集空（课程不在维护面时读数同为零）。不是被边界拦下，
    /// 不留 warn。
    NothingPending,
    /// 不唤醒：自动路径的边界，或去抖。
    Skip(GrowthWakeSkipReason),
}

/// 唤醒准入判定（R-c 纯函数，时钟经参数注入）。
///
/// 输入：`needs` = 该课的维护需求读数（挂载面两字段——**从未挂载**在这里
/// 读作 `mounts_stale = true`，须先手动「结构对齐」一次）；`last_success_
/// at_ms` = 该课挂载行最近一次成功落盘的时刻（无行 = `None`）；`now_ms` =
/// 当前时刻。判定顺序：stale → 无事可做 → 待挂阈值 → 去抖。
pub(crate) fn growth_wake_verdict(
    needs: &crate::models::MaintenanceNeedsView,
    last_success_at_ms: Option<i64>,
    now_ms: i64,
) -> GrowthWakeVerdict {
    if needs.mounts_stale {
        return GrowthWakeVerdict::Skip(GrowthWakeSkipReason::Stale);
    }
    if needs.pending_mounts == 0 {
        return GrowthWakeVerdict::NothingPending;
    }
    if needs.pending_mounts > GROWTH_WAKE_PENDING_MOUNT_LIMIT {
        return GrowthWakeVerdict::Skip(GrowthWakeSkipReason::PendingOverThreshold);
    }
    // 未来时刻（时钟回拨 / 还原的备份）按「刚挂过」处理：saturating 归零后落在
    // 去抖窗口内。
    match last_success_at_ms {
        Some(at_ms) if now_ms.saturating_sub(at_ms) < GROWTH_WAKE_DEBOUNCE_MS => {
            GrowthWakeVerdict::Skip(GrowthWakeSkipReason::Debounced)
        }
        _ => GrowthWakeVerdict::Wake,
    }
}

impl LearningService {
    /// 登记一次生长唤醒（R-a）：非空手生长批落库、事务提交之后调用（见
    /// `growth_body`）。先过准入判定（R-c）；不唤醒只写一条 `warn`（R-d），
    /// **不写 findings**——findings 是修补回合的触发条件与输入，不是惰性日志。
    /// 只登记 + 发信号，**不在这里挂载**——挂载由维护调度器在回合纪律里跑
    /// （ADR-0012「挂载不内联生长批」）。唤醒是旁路：任何读失败也只留日志，
    /// 永远不影响已经落库的生长批。
    pub async fn register_growth_wake(&self, course_id: &LearningCourseId, user_id: &UserId) {
        let (needs, verdict) = match self.growth_wake_admission(course_id).await {
            Ok(admission) => admission,
            Err(error) => {
                tracing::warn!(
                    course = course_id.as_str(),
                    %error,
                    "growth followup mount wake check failed"
                );
                return;
            }
        };
        match verdict {
            GrowthWakeVerdict::Wake => {}
            // 无事可做（待挂集空）：不是被边界拦下，不留 warn。
            GrowthWakeVerdict::NothingPending => return,
            GrowthWakeVerdict::Skip(reason) => {
                tracing::warn!(
                    course = course_id.as_str(),
                    reason = reason.as_str(),
                    pending_mounts = needs.pending_mounts,
                    mounts_stale = needs.mounts_stale,
                    "growth followup mount wake skipped"
                );
                return;
            }
        }
        self.lock_growth_wakes()
            .insert(course_id.clone(), user_id.clone());
        self.growth_wake_notify.notify_one();
        tracing::info!(
            course = course_id.as_str(),
            pending_mounts = needs.pending_mounts,
            "growth followup mount wake registered"
        );
    }

    /// 准入判定的读侧（登记与回合启动前的复核共用）：维护需求读数 + 该课
    /// 挂载行最近一次成功落盘的时刻（`updated_at` 只在挂载成功落盘时写；
    /// 无行 = `None`）。
    async fn growth_wake_admission(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<
        (
            crate::models::MaintenanceNeedsView,
            GrowthWakeVerdict,
        ),
        AppError,
    > {
        let needs = self.maintenance_needs(course_id).await?;
        let last_success_at_ms: Option<i64> = sqlx::query_scalar(
            "SELECT updated_at FROM learning_structure_mounts WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let verdict = growth_wake_verdict(&needs, last_success_at_ms, now_ms());
        Ok((needs, verdict))
    }

    /// 仅挂载维护回合（R-b）：唤醒路径只对**被登记的课程**走挂载分支，跳过
    /// 修补——修补会改结构、使全部挂载失效，是全局动作，仍只按定时节奏走
    /// （定时臂 `run_maintenance_tick` 不受影响）。回合纪律与账本沿用既有挂载
    /// 回合，触发源 = `growth_followup`（R-f）。
    ///
    /// 回合不启动时返回 `None`（零任务零 LLM 零账本）：自动维护运行期关，
    /// 或启动前的准入复核不过——登记到执行之间（任务闸排队、同课其它回
    /// 合）结构、节点、挂载都可能已经变了，自动路径的边界（stale / 阈值 /
    /// 去抖）在执行时再核一遍，不做全量重挂。
    pub async fn run_growth_followup_mount(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<Option<crate::models::MountRoundSummary>, AppError> {
        // 自动维护运行期关（`learning.maintenanceAutoEnabled=false`）同样不启
        // 动：唤醒回合是自动维护的一种，与调度器同一组开关（环境闸
        // `NOMI_LEARNING_AUTO_MAINTENANCE=off` 本就不注册调度器）。未启动的
        // 回合不重排——节点仍在待挂集，下次生长再登记。
        if !self.maintenance_settings().await.enabled {
            tracing::info!(
                course = course_id.as_str(),
                "growth followup mount round not started: auto maintenance is off"
            );
            return Ok(None);
        }
        let (_, verdict) = self.growth_wake_admission(course_id).await?;
        match verdict {
            GrowthWakeVerdict::Wake => {}
            GrowthWakeVerdict::NothingPending => return Ok(None),
            GrowthWakeVerdict::Skip(reason) => {
                // 同 R-d：只写日志，不写 findings。
                tracing::warn!(
                    course = course_id.as_str(),
                    reason = reason.as_str(),
                    "growth followup mount round not started"
                );
                return Ok(None);
            }
        }
        self.run_structure_mount_with_trigger(
            user_id,
            course_id,
            model_override,
            SystemActionTrigger::GrowthFollowup,
        )
        .await
        .map(Some)
    }

    /// 调度器取走全部唤醒意图（一次性排空；R-h）。一课至多一条。
    pub fn take_growth_wakes(&self) -> Vec<GrowthWake> {
        let mut wakes = self.lock_growth_wakes();
        std::mem::take(&mut *wakes)
            .into_iter()
            .map(|(course_id, user_id)| GrowthWake { course_id, user_id })
            .collect()
    }

    /// 唤醒等待端点：调度循环 `tokio::select!` 的被唤醒臂（R-a）。通道跨
    /// crate——生长在本 crate 的后台任务里登记，等待方是 app-server 的调度器。
    pub fn growth_wake_notified(&self) -> impl Future<Output = ()> + Send + '_ {
        self.growth_wake_notify.notified()
    }

    /// 唤醒登记表是旁路：锁中毒不外溢（取出内层继续用），不让一次 panic
    /// 永久卡死唤醒通道。
    fn lock_growth_wakes(&self) -> std::sync::MutexGuard<'_, HashMap<LearningCourseId, UserId>> {
        self.growth_wakes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE_MS: i64 = 60_000;
    const NOW_MS: i64 = 1_700_000_000_000;

    fn needs(pending_mounts: usize, mounts_stale: bool) -> crate::models::MaintenanceNeedsView {
        crate::models::MaintenanceNeedsView {
            eligible: true,
            unconsumed_findings: 0,
            pending_mounts,
            mounts_stale,
            needs_repair: mounts_stale,
            needs_mount: pending_mounts > 0 || mounts_stale,
        }
    }

    #[test]
    fn growth_wake_verdict_skips_stale_mounts_before_anything_else() {
        // 结构变过 / 从未挂载 = stale：待挂数再小、上次成功挂载再久也不唤醒。
        for pending in [0usize, 1, 8, 40] {
            assert_eq!(
                growth_wake_verdict(&needs(pending, true), Some(NOW_MS - 60 * MINUTE_MS), NOW_MS),
                GrowthWakeVerdict::Skip(GrowthWakeSkipReason::Stale),
                "pending={pending}"
            );
        }
        assert_eq!(
            growth_wake_verdict(&needs(3, true), None, NOW_MS),
            GrowthWakeVerdict::Skip(GrowthWakeSkipReason::Stale)
        );
    }

    #[test]
    fn growth_wake_verdict_wakes_up_to_eight_pending_mounts_and_stops_at_nine() {
        let last_success = Some(NOW_MS - 60 * MINUTE_MS);
        assert_eq!(
            growth_wake_verdict(&needs(8, false), last_success, NOW_MS),
            GrowthWakeVerdict::Wake
        );
        assert_eq!(
            growth_wake_verdict(&needs(9, false), last_success, NOW_MS),
            GrowthWakeVerdict::Skip(GrowthWakeSkipReason::PendingOverThreshold)
        );
    }

    #[test]
    fn growth_wake_verdict_debounces_until_five_minutes_after_the_last_success() {
        // 不足 5 分钟 → 去抖；恰 5 分钟 → 唤醒。
        assert_eq!(
            growth_wake_verdict(&needs(1, false), Some(NOW_MS - 5 * MINUTE_MS + 1), NOW_MS),
            GrowthWakeVerdict::Skip(GrowthWakeSkipReason::Debounced)
        );
        assert_eq!(
            growth_wake_verdict(&needs(1, false), Some(NOW_MS - 5 * MINUTE_MS), NOW_MS),
            GrowthWakeVerdict::Wake
        );
        // 上次成功挂载的时刻在未来（时钟回拨 / 还原的备份）按「刚挂过」处理。
        assert_eq!(
            growth_wake_verdict(&needs(1, false), Some(NOW_MS + MINUTE_MS), NOW_MS),
            GrowthWakeVerdict::Skip(GrowthWakeSkipReason::Debounced)
        );
    }

    #[test]
    fn growth_wake_verdict_reports_nothing_pending_without_a_skip() {
        // 待挂集空：无事可做——即使落在去抖窗口内也不是「被去抖拦下」。
        assert_eq!(
            growth_wake_verdict(&needs(0, false), Some(NOW_MS - MINUTE_MS), NOW_MS),
            GrowthWakeVerdict::NothingPending
        );
        assert_eq!(
            growth_wake_verdict(&needs(0, false), Some(NOW_MS - 60 * MINUTE_MS), NOW_MS),
            GrowthWakeVerdict::NothingPending
        );
    }

    #[test]
    fn growth_wake_skip_reasons_have_stable_machine_names() {
        // 不唤醒的日志 `reason` 字段（R-d）与测试断言面同源。
        assert_eq!(GrowthWakeSkipReason::Stale.as_str(), "stale");
        assert_eq!(
            GrowthWakeSkipReason::PendingOverThreshold.as_str(),
            "pending_over_threshold"
        );
        assert_eq!(GrowthWakeSkipReason::Debounced.as_str(), "debounced");
    }
}
