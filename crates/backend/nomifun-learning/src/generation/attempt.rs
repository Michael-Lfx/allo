//! 学习侧 LLM 失败分流（y02）：把「模型没收到」与「收到但不可用」分开。
//!
//! - [`AttemptFailure::Transport`]：模型没有产出可修正的文本（超时/网关/
//!   限流/无可用 provider/取消）。重试环必须**立即上抛原错误**——不写重试
//!   反馈、不占契约轮次、不进同指纹早停。模型对网络错误无能为力，
//!   「逐条修正网络错误」是纯噪声（learnhub #198 真机实证）。
//! - [`AttemptFailure::Rejected`]：响应已到达但非法（JSON/形状/契约）。
//!   报告进重试反馈、占契约轮次、参与同指纹早停。
//!
//! 只做分类，不新增/不修改公共错误类型；分类面只用学习侧已能看到的
//! [`AppError`] 变体（v1 边界：不动 `nomifun-common` / `nomifun-ai-agent` /
//! provider 错误映射）。
//!
//! # 推翻条件（防止将来悄悄回退）
//!
//! 若真机日志显示「传输失败被终止」后任务成功率显著下降，再评估在
//! completer 层补**一次性**退避（learnhub `RetryExecutor` 形态）；不要在
//! 引擎环里恢复「错误串回灌重试反馈」——模型对网络错误无能为力，回灌只会
//! 烧预算。观测读数：每轮失败日志带 `failure_kind=transport|parse`、
//! `attempt`、`stage`（无新增 WS phase/事件）。

use nomifun_common::AppError;

use crate::service::CANCELLED_MESSAGE;

/// 一次学习侧 LLM 尝试的失败面（穷尽两桶：Transport 立即上抛，Rejected
/// 占轮）。取消挤在 Transport 变体里只为**原样携带** `AppError`——它不是
/// 传输失败，[`AttemptFailure::is_cancelled`] 负责区分。
#[derive(Debug)]
pub(crate) enum AttemptFailure {
    /// 模型没收到：Timeout/BadGateway/RateLimited/ProviderUnavailable/
    /// Conflict(非取消)，以及取消（原样上抛，零反馈零轮次）。
    Transport(AppError),
    /// 收到但不可用：非法 JSON/形状/契约——报告进反馈并占契约轮次。
    Rejected(String),
}

impl AttemptFailure {
    /// 按 [`AppError`] 变体分桶（唯一分类单源）。
    pub(crate) fn from_app_error(error: AppError) -> Self {
        match &error {
            AppError::Timeout(_)
            | AppError::BadGateway(_)
            | AppError::RateLimited
            | AppError::ProviderUnavailable(_)
            // 取消与 provider 通道不可用同为 Conflict：立即上抛，原样保留。
            | AppError::Conflict(_) => Self::Transport(error),
            _ => Self::Rejected(error.to_string()),
        }
    }

    /// LLM 调用结果的失败包装：`Err(AppError)` 走同一分桶，`Ok` 透传。
    pub(crate) fn from_call<T>(result: Result<T, AppError>) -> Result<T, Self> {
        result.map_err(Self::from_app_error)
    }

    /// 传输失败（模型没收到）。取消归同一变体但此处仍返回 true（都要立即
    /// 上抛）；语义细分用 [`Self::is_cancelled`]。
    pub(crate) fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    /// 用户取消（`Conflict(CANCELLED_MESSAGE)`）：立即上抛，原样保留错误。
    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Transport(AppError::Conflict(message)) if message.contains(CANCELLED_MESSAGE)
        )
    }

    /// 失败原因文本（日志与耗尽消息共用；不改变错误类型）。
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Transport(error) => error.to_string(),
            Self::Rejected(report) => report.clone(),
        }
    }

    /// 每次尝试的失败日志（票面 C：`failure_kind=transport|parse`、
    /// `attempt`、`stage`；不新增 WS phase/事件）。取消只留 info。
    pub(crate) fn log(&self, attempt: usize, stage: &str) {
        match self {
            Self::Transport(_) if self.is_cancelled() => {
                tracing::info!(attempt, stage, "learning LLM attempt cancelled");
            }
            Self::Transport(error) => {
                tracing::warn!(
                    failure_kind = "transport",
                    attempt,
                    stage,
                    error = %error,
                    "learning LLM attempt failed (model produced no usable output)"
                );
            }
            Self::Rejected(report) => {
                tracing::warn!(
                    failure_kind = "parse",
                    attempt,
                    stage,
                    report_head = %report.chars().take(400).collect::<String>(),
                    "learning LLM attempt rejected (parse/contract)"
                );
            }
        }
    }

    /// 收敛为 `AppError`：Transport 原样返回（含取消）；Rejected 落
    /// `UnprocessableEntity`（学习侧既有的「产物不可用」出口）。
    pub(crate) fn into_app_error(self) -> AppError {
        match self {
            Self::Transport(error) => error,
            Self::Rejected(report) => AppError::UnprocessableEntity(report),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_variants_are_split_from_rejected_ones() {
        for error in [
            AppError::Timeout("json coach call exceeded 480s".into()),
            AppError::BadGateway("LLM provider error: API error 502".into()),
            AppError::RateLimited,
            AppError::ProviderUnavailable("no provider".into()),
            AppError::Conflict("learning structured output unavailable".into()),
        ] {
            let label = error.to_string();
            let failure = AttemptFailure::from_app_error(error);
            assert!(failure.is_transport(), "{label} must be transport");
            assert!(!failure.is_cancelled(), "{label} is not cancellation");
        }
        for error in [
            AppError::UnprocessableEntity("invalid JSON: expected ','".into()),
            AppError::Internal("coach reply type drift".into()),
            AppError::NotFound("missing".into()),
        ] {
            let label = error.to_string();
            let failure = AttemptFailure::from_app_error(error);
            assert!(!failure.is_transport(), "{label} must be rejected");
            assert!(!failure.is_cancelled(), "{label} is not cancellation");
        }
    }

    #[test]
    fn rejected_carries_the_report_and_maps_to_unprocessable_entity() {
        let failure = AttemptFailure::from_app_error(AppError::UnprocessableEntity(
            "- [ng.parse] 教练回包不是单个 JSON 对象".into(),
        ));
        assert_eq!(failure.reason(), "Unprocessable entity: - [ng.parse] 教练回包不是单个 JSON 对象");
        assert!(matches!(
            failure.into_app_error(),
            AppError::UnprocessableEntity(message) if message.contains("ng.parse")
        ));
    }

    #[test]
    fn transport_round_trips_the_original_error_unchanged() {
        let failure =
            AttemptFailure::from_app_error(AppError::Timeout("json coach call exceeded 480s".into()));
        assert_eq!(failure.reason(), "Request timeout: json coach call exceeded 480s");
        assert!(matches!(
            failure.into_app_error(),
            AppError::Timeout(message) if message.contains("480s")
        ));
    }

    /// 取消优先（票面验收）：`Conflict(CANCELLED_MESSAGE)` 原样保留，
    /// 不得被降级成 `UnprocessableEntity`（「3 轮均未通过」形态）。
    #[test]
    fn cancellation_keeps_the_conflict_and_is_marked() {
        let failure = AttemptFailure::from_app_error(AppError::Conflict(
            CANCELLED_MESSAGE.to_owned(),
        ));
        assert!(failure.is_transport(), "取消立即上抛，与传输同路");
        assert!(failure.is_cancelled(), "取消必须可识别");
        let error = failure.into_app_error();
        assert!(
            matches!(&error, AppError::Conflict(message) if message == CANCELLED_MESSAGE),
            "取消必须原样返回：{error:?}"
        );
    }

    #[test]
    fn from_call_only_wraps_errors() {
        let ok = AttemptFailure::from_call(Ok::<_, AppError>("reply"));
        assert!(matches!(ok, Ok("reply")));
        let err = AttemptFailure::from_call::<()>(Err(AppError::BadGateway("502".into())));
        assert!(matches!(err, Err(failure) if failure.is_transport()));
    }
}
