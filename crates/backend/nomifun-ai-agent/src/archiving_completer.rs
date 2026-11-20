//! Observing [`LearningCompleter`] (wayfinder ticket s1): wraps any
//! completer, archiving every call — success or failure — as one JSONL line
//! under the caller-chosen observation directory (system + user + response
//! full text, elapsed time and outcome). Mounting policy lives at the wiring
//! seams: smoke runs MUST mount it (`learning_smoke.rs`, per-run dirs);
//! production mounts only behind an opt-in switch — rs1
//! `NOMI_LEARNING_LIVE_ARCHIVE`（per-day，services.rs）或 o2
//! `NOMI_LEARNING_CALL_ARCHIVE`（per-run dev 面）——开关 off = 零写入零建
//! 目录（not mounted = zero overhead）。
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
        Self::new_in_dir(inner, &obs_root.join(format!("run-{run_ts}")))
    }

    /// 目录策略放宽面（rs1 live 档案）：调用方自带完整观测目录（live 按
    /// 日 `observation/learning-live/YYYY-MM-DD/`，live 无 run 概念），
    /// 建目录 + 开日志与 [`new`] 同律，行格式零改动。目录粒度决策留在
    /// 调用方——本类型只管逐调用一行。
    pub fn new_in_dir(
        inner: std::sync::Arc<dyn LearningCompleter>,
        run_dir: &Path,
    ) -> Result<Self, AppError> {
        std::fs::create_dir_all(run_dir)
            .map_err(|error| AppError::Internal(format!("observation dir: {error}")))?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir.join("calls.jsonl"))
            .map_err(|error| AppError::Internal(format!("observation log: {error}")))?;
        Ok(Self {
            inner,
            log: Mutex::new(log),
            run_dir: run_dir.to_path_buf(),
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

    fn archive(
        &self,
        method: &str,
        started: Instant,
        outcome: Result<&str, &AppError>,
        system: &str,
        user: &str,
        usage: Option<nomifun_learning::CallUsage>,
    ) {
        // 脱敏先于计量：chars 字段描述的是落盘文本。
        let system = self.redact(system);
        let user = self.redact(user);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        // usage 读数（o2）：provider 回报了就落对象；没回报如实标 "absent"
        // （不造数不填零冒充）。失败臂无回包 → 无 usage 键（error 行形状
        // 不变；ok 臂恒有 usage 键，旧行缺席该键 = o2 前存档，向后兼容）。
        let usage_json = match usage {
            Some(row) => serde_json::json!({
                "input_tokens": row.input_tokens,
                "output_tokens": row.output_tokens,
                "reasoning_tokens": row.reasoning_tokens,
            }),
            None => serde_json::json!("absent"),
        };
        let record = match outcome {
            Ok(response) => serde_json::json!({
                "ts": now_ms(),
                "method": method,
                "ok": true,
                "elapsed_ms": elapsed_ms,
                "system_chars": system.chars().count(),
                "user_chars": user.chars().count(),
                "response_chars": response.chars().count(),
                "usage": usage_json,
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
        self.archive("complete", started, outcome.as_deref(), system, user, None);
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
        self.archive(
            "complete_with_thinking",
            started,
            outcome.as_deref(),
            system,
            user,
            None,
        );
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
        self.archive(
            "complete_json",
            started,
            outcome.as_deref(),
            system,
            user,
            None,
        );
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
        // o2 调用留痕：coach 通道走 inner 的 usage 变体取数——未覆写的
        // inner（测试替身/保守实现者）走默认实现如实缺席。
        let started = Instant::now();
        let result = self
            .inner
            .complete_json_with_thinking_usage(model_override, system, user, max_tokens, temperature)
            .await;
        let usage = match &result {
            Ok((_, usage)) => *usage,
            Err(_) => None,
        };
        let outcome = match &result {
            Ok((text, _)) => Ok(text.as_str()),
            Err(error) => Err(error),
        };
        self.archive(
            "complete_json_with_thinking",
            started,
            outcome,
            system,
            user,
            usage,
        );
        result.map(|(text, _)| text)
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
        // error 臂与 ok 臂计量形状对称：user_chars 同样在计（口径与 ok 臂
        // 一致 = 落盘文本的 chars 数）。
        assert_eq!(
            second["user_chars"].as_u64().unwrap(),
            "fails".chars().count() as u64
        );
        assert!(second["system_chars"].as_u64().is_some());
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

    /// o2 usage 读数：inner 覆写 usage 变体 → 行带用量对象；inner 走默认
    /// 实现（Echo 未覆写）→ 行如实标 "absent"（不造数不填零冒充）。
    #[tokio::test]
    async fn usage_row_reflects_inner_report_or_absent() {
        struct Reporting;
        #[async_trait::async_trait]
        impl LearningCompleter for Reporting {
            async fn complete(
                &self,
                _: Option<(&str, &str)>,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<String, AppError> {
                unreachable!("本测试只走 json thinking 通道");
            }

            async fn complete_json_with_thinking_usage(
                &self,
                _: Option<(&str, &str)>,
                _: &str,
                _: &str,
                _: u32,
                _: Option<f32>,
            ) -> Result<(String, Option<nomifun_learning::CallUsage>), AppError> {
                Ok((
                    "{\"ok\":true}".into(),
                    Some(nomifun_learning::CallUsage {
                        input_tokens: Some(101),
                        output_tokens: Some(202),
                        reasoning_tokens: Some(87),
                    }),
                ))
            }
        }

        let obs = tempfile::tempdir().unwrap();
        let completer =
            ArchivingCompleter::new(std::sync::Arc::new(Reporting), obs.path()).unwrap();
        completer
            .complete_json_with_thinking(None, "sys", "usr", 128, None)
            .await
            .unwrap();
        let obs2 = tempfile::tempdir().unwrap();
        let absent =
            ArchivingCompleter::new(std::sync::Arc::new(Echo), obs2.path()).unwrap();
        absent
            .complete_json_with_thinking(None, "sys", "usr", 128, None)
            .await
            .unwrap();

        let logged =
            std::fs::read_to_string(completer.run_dir().join("calls.jsonl")).unwrap();
        let record: serde_json::Value = serde_json::from_str(logged.trim()).unwrap();
        assert_eq!(record["method"], "complete_json_with_thinking");
        assert_eq!(
            record["usage"],
            serde_json::json!({
                "input_tokens": 101,
                "output_tokens": 202,
                "reasoning_tokens": 87,
            }),
            "{logged}"
        );

        let absent_log =
            std::fs::read_to_string(absent.run_dir().join("calls.jsonl")).unwrap();
        let absent_record: serde_json::Value = serde_json::from_str(absent_log.trim()).unwrap();
        assert_eq!(absent_record["usage"], serde_json::json!("absent"), "{absent_log}");
    }

    /// 目录策略放宽面（rs1 live 档案）：new_in_dir 落调用方自带的完整观测
    /// 目录（live 按日 `learning-live/YYYY-MM-DD/`，无 run 概念），嵌套
    /// 目录自动创建；行格式零改动（与 new 的 run-* 臂同一条 archive 通路
    /// ——method/ok/response 字段同形）。
    #[tokio::test]
    async fn new_in_dir_archives_into_exact_directory() {
        let obs = tempfile::tempdir().unwrap();
        let day_dir = obs.path().join("learning-live").join("2026-11-20");
        let completer =
            ArchivingCompleter::new_in_dir(std::sync::Arc::new(Echo), &day_dir).unwrap();
        assert_eq!(completer.run_dir(), day_dir);
        completer
            .complete_json(None, "sys", "usr", 128, None)
            .await
            .unwrap();

        let log = std::fs::read_to_string(day_dir.join("calls.jsonl")).unwrap();
        let record: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
        assert_eq!(record["method"], "complete_json");
        assert_eq!(record["ok"], serde_json::json!(true));
        assert_eq!(record["response"], "{\"ok\":true}");
    }

    /// 冒烟臂的按 run 目录策略不被放宽面波及：new 仍落
    /// `<obs_root>/run-<ts>/`（一次冒烟运行一个目录）。
    #[test]
    fn new_keeps_run_ts_directory_policy() {
        let obs = tempfile::tempdir().unwrap();
        let completer =
            ArchivingCompleter::new(std::sync::Arc::new(Echo), obs.path()).unwrap();
        assert_eq!(completer.run_dir().parent().unwrap(), obs.path());
        let name = completer
            .run_dir()
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert!(name.starts_with("run-"), "{name}");
    }
}
