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
//!
//! 观测面脱敏（n4 D9，n5 随票）：判分冒烟档把判底（题目 explanation）与
//! 作答原文经 [`ArchivingCompleter::register_redaction`] 注册进来，落盘前
//! learnhub 式占位替换——判底/作答不进观测面；response 是反馈面（评审
//! 说了什么），原文保留供审计回读。注册表为空 = 逐字节原样（通用管线
//! 档零行为变化）；归档/脱敏失败一律降级不阻断判分。

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
    /// 脱敏占位表（注册序逐个 replace；生产从不挂本 completer，表恒空 =
    /// 零开销零行为变化）。归档失败与脱敏失败同样只降级——观测面永不
    /// 阻断判分。
    redactions: Mutex<Vec<(String, String)>>,
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
            redactions: Mutex::new(Vec::new()),
        })
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// 注册一条脱敏占位：归档 system/user 文本中的 `original` 逐字替换为
    /// `placeholder`（判分冒烟档在布景后注册判底与作答原文；learnhub
    /// smoke-grade 脱敏回读同律——判底不进观测面）。空串跳过（空判底无
    /// 可脱敏文本）。
    pub fn register_redaction(&self, original: impl Into<String>, placeholder: impl Into<String>) {
        let original = original.into();
        if original.is_empty() {
            return;
        }
        self.redactions
            .lock()
            .expect("archiving redaction registry lock poisoned")
            .push((original, placeholder.into()));
    }

    /// 占位替换（注册序逐个 replace；只作用于 system/user 观测面——
    /// response 是反馈面原文保留）。
    fn redact(&self, text: &str) -> String {
        let table = self
            .redactions
            .lock()
            .expect("archiving redaction registry lock poisoned");
        let mut out = text.to_string();
        for (original, placeholder) in table.iter() {
            if out.contains(original.as_str()) {
                out = out.replace(original.as_str(), placeholder);
            }
        }
        out
    }

    fn archive(&self, method: &str, started: Instant, outcome: Result<&str, &AppError>, system: &str, user: &str) {
        // 脱敏先于计量：chars 字段描述的是落盘文本。
        let system = self.redact(system);
        let user = self.redact(user);
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

    /// 脱敏占位（n4 D9）：注册的判底/作答原文在归档 system/user 面逐字
    /// 替换为占位符，response 反馈面原文保留；未注册调用逐字节原样。
    #[tokio::test]
    async fn registered_redactions_replace_grounding_and_response_in_archive() {
        let obs = tempfile::tempdir().unwrap();
        let completer =
            ArchivingCompleter::new(std::sync::Arc::new(Echo), obs.path()).unwrap();
        let grounding = "复利的判底原文：利息并入本金再生息。";
        let answer = "学习者敲下的原始作答句子。";
        completer.register_redaction(
            grounding,
            nomifun_learning::REDACTED_GROUNDING_PLACEHOLDER,
        );
        completer.register_redaction(
            answer,
            nomifun_learning::REDACTED_RESPONSE_PLACEHOLDER,
        );
        let system = format!("评审 system，含判底 {grounding}");
        let user = format!("评审 user，含作答：{answer}");
        completer
            .complete_json(None, &system, &user, 128, None)
            .await
            .unwrap();

        let log = std::fs::read_to_string(completer.run_dir().join("calls.jsonl")).unwrap();
        let record: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
        let archived_system = record["system"].as_str().unwrap();
        let archived_user = record["user"].as_str().unwrap();
        assert!(
            !archived_system.contains(grounding) && !archived_user.contains(answer),
            "判底与作答原文不得进观测面：{log}"
        );
        assert!(archived_system.contains(nomifun_learning::REDACTED_GROUNDING_PLACEHOLDER));
        assert!(archived_user.contains(nomifun_learning::REDACTED_RESPONSE_PLACEHOLDER));
        // 计量字段描述落盘文本（脱敏后）。
        assert_eq!(
            record["user_chars"].as_u64().unwrap(),
            archived_user.chars().count() as u64
        );
    }

    /// 未注册 = 原样（通用管线档零行为变化）；空串注册被跳过（空判底无
    /// 可脱敏文本）。
    #[tokio::test]
    async fn empty_registry_and_empty_spans_leave_text_verbatim() {
        let obs = tempfile::tempdir().unwrap();
        let completer =
            ArchivingCompleter::new(std::sync::Arc::new(Echo), obs.path()).unwrap();
        completer.register_redaction("", "(已脱敏:不应生效)");
        completer
            .complete_json(None, "sys raw", "usr raw", 128, None)
            .await
            .unwrap();
        let log = std::fs::read_to_string(completer.run_dir().join("calls.jsonl")).unwrap();
        assert!(log.contains("\"sys raw\"") && log.contains("\"usr raw\""), "{log}");
    }
}
