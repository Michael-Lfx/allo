//! Observing [`LearningCompleter`] (wayfinder ticket s1): wraps any
//! completer, archiving every call — success or failure — as one JSONL line
//! under `<obs_root>/<run-ts>/calls.jsonl` (system + user + response full
//! text, elapsed time and outcome). Smoke runs MUST mount it; production runs
//! never do (zero overhead).
//!
//! Probe-style calls (the `learning-smoke json` capability read-out) bypass
//! this wrapper and hit the inner completer directly — they are not pipeline
//! calls and must not pollute the pipeline archive (learnhub smoke-json
//! discipline).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use nomifun_common::AppError;
use nomifun_learning::LearningCompleter;

pub struct ArchivingCompleter {
    inner: std::sync::Arc<dyn LearningCompleter>,
    /// 追加句柄：每次调用一行 JSONL（进程内串行写，Mutex 防交错）。
    log: Mutex<std::fs::File>,
    run_dir: PathBuf,
}

impl ArchivingCompleter {
    /// 包住 `inner`，观测目录 = `<obs_root>/<run-ts>/`（一次冒烟运行一个
    /// 目录，calls.jsonl 落其中）。
    pub fn new(
        inner: std::sync::Arc<dyn LearningCompleter>,
        obs_root: &Path,
    ) -> Result<Self, AppError> {
        let run_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let run_dir = obs_root.join(format!("run-{run_ts}"));
        std::fs::create_dir_all(&run_dir)
            .map_err(|error| AppError::Internal(format!("observation dir: {error}")))?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir.join("calls.jsonl"))
            .map_err(|error| AppError::Internal(format!("observation log: {error}")))?;
        Ok(Self {
            inner,
            log: Mutex::new(log),
            run_dir,
        })
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    fn archive(&self, method: &str, started: Instant, outcome: Result<&str, &AppError>, system: &str, user: &str) {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let record = match outcome {
            Ok(response) => serde_json::json!({
                "ts": now_ms(),
                "method": method,
                "ok": true,
                "elapsed_ms": elapsed_ms,
                "system_chars": system.chars().count(),
                "user_chars": user.chars().count(),
                "response_chars": response.chars().count(),
                "system": system,
                "user": user,
                "response": response,
            }),
            Err(error) => serde_json::json!({
                "ts": now_ms(),
                "method": method,
                "ok": false,
                "elapsed_ms": elapsed_ms,
                "system_chars": system.chars().count(),
                "user_chars": user.chars().count(),
                "system": system,
                "user": user,
                "error": error.to_string(),
            }),
        };
        if let Ok(mut log) = self.log.lock() {
            let _ = writeln!(log, "{record}");
            let _ = log.flush();
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[async_trait::async_trait]
impl LearningCompleter for ArchivingCompleter {
    async fn complete(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        let started = Instant::now();
        let outcome = self
            .inner
            .complete(model_override, system, user, max_tokens)
            .await;
        self.archive("complete", started, outcome.as_deref(), system, user);
        outcome
    }

    async fn complete_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        let started = Instant::now();
        let outcome = self
            .inner
            .complete_with_thinking(model_override, system, user, max_tokens)
            .await;
        self.archive("complete_with_thinking", started, outcome.as_deref(), system, user);
        outcome
    }

    async fn complete_json(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let started = Instant::now();
        let outcome = self
            .inner
            .complete_json(model_override, system, user, max_tokens, temperature)
            .await;
        self.archive("complete_json", started, outcome.as_deref(), system, user);
        outcome
    }

    async fn complete_json_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let started = Instant::now();
        let outcome = self
            .inner
            .complete_json_with_thinking(model_override, system, user, max_tokens, temperature)
            .await;
        self.archive(
            "complete_json_with_thinking",
            started,
            outcome.as_deref(),
            system,
            user,
        );
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    #[async_trait::async_trait]
    impl LearningCompleter for Echo {
        async fn complete(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, AppError> {
            Ok("{\"ok\":true}".into())
        }

        async fn complete_json(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            user: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, AppError> {
            if user == "fails" {
                return Err(AppError::BadGateway("probe failure".into()));
            }
            Ok("{\"ok\":true}".into())
        }
    }

    #[tokio::test]
    async fn every_call_lands_one_jsonl_line() {
        let obs = tempfile::tempdir().unwrap();
        let completer =
            ArchivingCompleter::new(std::sync::Arc::new(Echo), obs.path()).unwrap();
        completer
            .complete_json(None, "sys", "usr", 128, None)
            .await
            .unwrap();
        completer.complete_json(None, "sys", "fails", 128, None).await;

        let log = std::fs::read_to_string(completer.run_dir().join("calls.jsonl")).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{log}");
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["method"], "complete_json");
        assert_eq!(first["ok"], serde_json::json!(true));
        assert_eq!(first["response"], "{\"ok\":true}");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["ok"], serde_json::json!(false));
        assert!(second["error"].as_str().is_some_and(|error| !error.is_empty()));
    }
}
