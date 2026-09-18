//! Optional cloud sink for eval-lab failed cases. Implemented in nomifun-app
//! so this crate does not depend on nomifun-cloud.

use async_trait::async_trait;
use nomifun_common::AppError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalQualityCase {
    pub case_id: String,
    pub suite: String,
    pub category: String,
    pub error: Option<String>,
    pub prompt: String,
    pub scorer_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalQualityReport {
    pub run_id: String,
    pub suite: String,
    pub model: Option<String>,
    #[serde(default)]
    pub pass_at_1: f64,
    #[serde(default)]
    pub pass_hat_k: f64,
    #[serde(default)]
    pub passed: u32,
    #[serde(default)]
    pub failed: u32,
    #[serde(default)]
    pub unique_cases: u32,
    #[serde(default)]
    pub n_trials: u32,
    pub cases: Vec<EvalQualityCase>,
}

#[async_trait]
pub trait EvalQualitySink: Send + Sync {
    async fn report_failed_cases(&self, report: EvalQualityReport) -> Result<(), AppError>;
    async fn report_badcase(&self, case: EvalQualityCase) -> Result<(), AppError>;
    async fn fetch_promoted(&self) -> Result<Vec<EvalQualityCase>, AppError>;
}
