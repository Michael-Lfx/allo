//! 修补后自动重挂（m2，map D1）：结构修补 `applied` 之后唤醒维护回合，唤醒时
//! 只跑挂载、不跑修补——把刚被修补改了的结构重新对齐。
//!
//! 为什么不直接用 sb2 的生长唤醒（`growth_wake.rs`）：那条通道的准入**第一道
//! 就拒挂载失效**（ADR-0018 决定 4：挂载失效时不由唤醒触发，退回手动「结构
//! 对齐」），而修补 `applied` 之后这门课的挂载必然失效——照原样复用等于空操
//! 作。那道边界是有意的（防止生长唤醒突然附带一次大调用），这里不碰它。
//!
//! 所以修补后重挂是**另一种意图**，与生长唤醒并列：
//! - **登记不设准入**：它只跟在一次刚改了结构的修补之后；没有待挂节点阈值、
//!   没有去抖——全量重挂正是它的目的。
//! - **执行时另有一套准入**（[`LearningService::run_repair_followup_mount`]）：
//!   自动维护运行期关不启动；不在维护面的课程不启动；从未挂载过的课程不启动
//!   （首次对齐仍是手动的，ADR-0018 这一条照旧）；已经有人重挂过就不重复挂。
//! - **共用 sb2 的通道设施**：同一个 `Notify` 信号与调度器的同一个唤醒分支，
//!   只是各有一张登记表——两种意图在登记表与账本里都可区分（触发源 =
//!   `repair_followup`）。
//!
//! 没挂回去的只留痕、不重试：失败时 Failed 动作簿行由挂载回合自己写；挂载
//! 保持失效，结构图页签的挂载卡一直挂着「结构已更新，待重新检查」。之后的
//! 补挂看开关——自动维护开着：定时臂到点补挂；运行期关着：重挂不启动、定
//! 时臂也被按住，只有用户点「重新检查」或把自动维护打开（之后的下一个到期
//! tick 才挂）才挂得回去。

use super::*;

/// 一次修补后重挂的意图：对 `course_id` 跑一个仅挂载回合（全量重挂），以
/// `user_id` 的身份（修补以谁的身份跑，重挂就以谁的身份跑）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairFollowupWake {
    pub course_id: LearningCourseId,
    pub user_id: UserId,
}

impl LearningService {
    /// 登记一次修补后重挂（R-b）：结构修补 `applied` 的结构写盘**之后**立即
    /// 调用——在修补审计行与 Done 账本行之前（见
    /// `run_structure_repair_with_trigger`）。**不设准入**——它只跟在一次刚改了
    /// 结构的修补之后，全量重挂正是它的目的；生长唤醒的 stale / 待挂阈值 /
    /// 去抖三道闸不适用于它，也不动它们。准入在执行时另核（见
    /// [`Self::run_repair_followup_mount`]）。只登记 + 发信号，不在这里挂载。
    pub async fn register_repair_followup_wake(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) {
        self.lock_repair_followup_wakes()
            .insert(course_id.clone(), user_id.clone());
        // 复用 sb2 的唤醒信号：调度器同一个 select 就能被两种意图唤醒。
        self.growth_wake_notify.notify_one();
        tracing::info!(
            course = course_id.as_str(),
            "structure repair followup remount registered"
        );
    }

    /// 调度器取走全部修补后重挂意图（一次性排空）。一课至多一条。
    pub fn take_repair_followup_wakes(&self) -> Vec<RepairFollowupWake> {
        let mut wakes = self.lock_repair_followup_wakes();
        std::mem::take(&mut *wakes)
            .into_iter()
            .map(|(course_id, user_id)| RepairFollowupWake {
                course_id,
                user_id,
            })
            .collect()
    }

    /// 修补后重挂回合（R-b）：对**被登记的课程**走挂载分支，跳过修补。回合纪
    /// 律与账本沿用既有挂载回合，触发源 = `repair_followup`（账本里与生长唤醒
    /// 的挂载行可区分）。
    ///
    /// 不设待挂节点阈值、不去抖：全量重挂正是这个意图的目的，而它只跟在一次
    /// 刚改了结构的修补之后。
    ///
    /// 回合不启动时返回 `None`（零任务零 LLM 零账本），原因写 info 日志：
    /// - 自动维护运行期关（与调度器同一组开关）；
    /// - 课程不在维护面（归档 / 软删 / 非图课程 / 无结构）；
    /// - **从未挂载过**——首次对齐是手动的（ADR-0018 这一条照旧）；
    /// - 挂载 fresh 且无待挂节点——登记到执行之间已经有人重挂过（定时臂在
    ///   同一回合内补挂、用户点了「重新检查」）。
    pub async fn run_repair_followup_mount(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<Option<crate::models::MountRoundSummary>, AppError> {
        // 自动维护运行期关（`learning.maintenanceAutoEnabled=false`）同样不启
        // 动：重挂回合是自动维护的一种，与调度器同一组开关（环境闸
        // `NOMI_LEARNING_AUTO_MAINTENANCE=off` 本就不注册调度器）。未启动的重挂
        // 不重排，而关着期间定时臂同样被按住——挂载保持失效，结构图页签的
        // 挂载卡一直挂着「结构已更新，待重新检查」，直到用户点「重新检查」，
        // 或把自动维护打开（之后的下一个到期 tick 在维护回合里挂）。
        if !self.maintenance_settings().await.enabled {
            tracing::info!(
                course = course_id.as_str(),
                "repair followup remount round not started: auto maintenance is off"
            );
            return Ok(None);
        }
        let needs = self.maintenance_needs(course_id).await?;
        // 归档 / 软删 / 非图课程 / 无结构：不在维护面。单独判而不并进下面
        // 「无事可重挂」——不启动的原因要如实写日志（R-e）。
        if !needs.eligible {
            tracing::info!(
                course = course_id.as_str(),
                reason = "ineligible",
                "repair followup remount round not started"
            );
            return Ok(None);
        }
        // 从未挂载过 → 不启动：首次对齐是手动的（ADR-0018 这一条照旧）。
        // `maintenance_needs` 把「没有挂载行」也读作 `mounts_stale`，分不出
        // 「结构变过」和「从没挂过」——所以直接看挂载行在不在。
        let mounted_before: Option<i64> = sqlx::query_scalar(
            "SELECT updated_at FROM learning_structure_mounts WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        if mounted_before.is_none() {
            tracing::info!(
                course = course_id.as_str(),
                reason = "never_mounted",
                "repair followup remount round not started"
            );
            return Ok(None);
        }
        // 挂载 fresh 且无待挂节点 → 不重复挂：登记到执行之间已经有人重挂过
        // （定时臂在同一回合内补挂、用户点了「重新检查」）。
        if !(needs.mounts_stale || needs.pending_mounts > 0) {
            tracing::info!(
                course = course_id.as_str(),
                reason = "nothing_to_remount",
                "repair followup remount round not started"
            );
            return Ok(None);
        }
        self.run_structure_mount_with_trigger(
            user_id,
            course_id,
            model_override,
            SystemActionTrigger::RepairFollowup,
        )
        .await
        .map(Some)
    }

    /// 重挂登记表是旁路：锁中毒不外溢（取出内层继续用），不让一次 panic 永久
    /// 卡死重挂通道。
    fn lock_repair_followup_wakes(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<LearningCourseId, UserId>> {
        self.repair_followup_wakes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
