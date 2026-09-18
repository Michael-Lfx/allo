//! Cloud-backed `EvalQualitySink`. Lives here so `nomifun-ai-agent` stays free of `nomifun-cloud`.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_ai_agent::{EvalQualityCase, EvalQualityReport, EvalQualitySink};
use nomifun_api_types::{AgentQualityBadcaseRequest, AgentQualityPromotedItem, AgentQualityRunRequest};
use nomifun_cloud::CloudService;
use nomifun_common::AppError;
use uuid::Uuid;

const EXCERPT_RUNES: usize = 800;
const SCORER_CHARS: usize = 8_192;

pub struct CloudEvalQualitySink {
    cloud: Arc<CloudService>,
}

impl CloudEvalQualitySink {
    pub fn new(cloud: Arc<CloudService>) -> Self {
        Self { cloud }
    }
}

#[async_trait]
impl EvalQualitySink for CloudEvalQualitySink {
    async fn report_failed_cases(&self, report: EvalQualityReport) -> Result<(), AppError> {
        match self
            .cloud
            .submit_agent_eval_run(run_request(&report))
            .await
        {
            Ok(_) => {}
            Err(AppError::Unauthorized(_)) => return Ok(()),
            Err(error) => return Err(error),
        }
        for case in report.cases {
            match self.cloud.submit_agent_badcase(badcase_request(&case)).await {
                Ok(_) => {}
                Err(AppError::Unauthorized(_)) => return Ok(()),
                Err(error) => {
                    tracing::warn!(error = %error, case_id = %case.case_id, "eval badcase auto-report skipped");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn report_badcase(&self, case: EvalQualityCase) -> Result<(), AppError> {
        self.cloud
            .submit_agent_badcase(badcase_request(&case))
            .await
            .map(|_| ())
    }

    async fn fetch_promoted(&self) -> Result<Vec<EvalQualityCase>, AppError> {
        Ok(self
            .cloud
            .list_promoted_agent_badcases()
            .await?
            .into_iter()
            .map(promoted_item_to_case)
            .collect())
    }
}

fn run_request(report: &EvalQualityReport) -> AgentQualityRunRequest {
    AgentQualityRunRequest {
        event_id: Uuid::now_v7().to_string(),
        suite: report.suite.clone(),
        model: report.model.clone(),
        pass_at_1: report.pass_at_1,
        pass_hat_k: report.pass_hat_k,
        passed: report.passed,
        failed: report.failed,
        unique_cases: report.unique_cases,
        n_trials: report.n_trials.max(1),
        summary_json: Some(
            serde_json::json!({
                "runId": report.run_id,
                "failedCaseIds": report.cases.iter().map(|c| &c.case_id).collect::<Vec<_>>(),
            })
            .to_string(),
        ),
    }
}

fn badcase_request(case: &EvalQualityCase) -> AgentQualityBadcaseRequest {
    AgentQualityBadcaseRequest {
        event_id: Uuid::now_v7().to_string(),
        suite_hint: omit_empty(&case.suite),
        category: omit_empty(&case.category),
        case_id: omit_empty(&case.case_id),
        prompt_excerpt: omit_empty(&clip_runes(&case.prompt, EXCERPT_RUNES)),
        error_excerpt: case
            .error
            .as_deref()
            .map(|s| clip_runes(s, EXCERPT_RUNES))
            .and_then(|s| omit_empty(&s)),
        scorer_json: omit_empty(&clip_chars(&case.scorer_json, SCORER_CHARS)),
        artifact_oss_id: None,
    }
}

fn promoted_item_to_case(item: AgentQualityPromotedItem) -> EvalQualityCase {
    EvalQualityCase {
        case_id: item.case_id.unwrap_or_default(),
        suite: item.suite_hint.unwrap_or_else(|| "private_badcases".into()),
        category: item.category.unwrap_or_else(|| "private".into()),
        error: item.error_excerpt,
        prompt: item.prompt_excerpt.unwrap_or_default(),
        scorer_json: item.scorer_json.unwrap_or_default(),
    }
}

fn clip_runes(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.chars().count() > max {
        true => s.chars().take(max).collect(),
        false => s.to_owned(),
    }
}

fn clip_chars(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.len() > max {
        true => s.chars().take(max).collect(),
        false => s.to_owned(),
    }
}

fn omit_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}
