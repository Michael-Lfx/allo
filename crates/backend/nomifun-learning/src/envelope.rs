//! 扁平 JSON 信封（x09）：`{ "version": N, ...载荷键 }`——version 与载荷键
//! 并列，不包 `data` 层（与 `CourseSourcePlanSnapshot` / `structure_json`
//! 同形）。单对象列用它；多载荷行（quiz_rounds / concept_registry）用行级
//! `doc_version` 列，不硬套对象信封。

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// `learning_attempts.response_json` 的文档代际（x09）。
pub const ATTEMPT_RESPONSE_SCHEMA_VERSION: u32 = 1;
/// `learning_structure_repairs.ops_json` 的文档代际（x09）。
pub const REPAIR_OPS_SCHEMA_VERSION: u32 = 1;

/// 作答/响应单载荷文档（扁平信封载荷）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseDoc {
    pub response: serde_json::Value,
}

/// 修补回合 op 列表（扁平信封载荷）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpsLog {
    pub ops: Vec<serde_json::Value>,
}

/// 扁平信封：`version` 与载荷键并列。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlatEnvelope<T> {
    pub version: u32,
    #[serde(flatten)]
    pub payload: T,
}

/// 读侧三态：历史裸对象（无 version）= v0；当前版本；不认识的更高版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeRead<T> {
    Bare(T),
    Current(T),
    Unsupported(u64),
}

/// 单对象列读侧：先看 `version` 键——缺省回退裸形（v0 历史行，读通、
/// 不回填）；等于 `current` 解信封；其它版本显式 `Unsupported`（禁止猜形，
/// 由调用方按列定义降级）。真坏 JSON 仍返回解析错误，维持各列现有错误面。
pub fn read_envelope<T: DeserializeOwned>(
    raw: &str,
    current: u32,
) -> Result<EnvelopeRead<T>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let Some(version) = value.get("version").and_then(serde_json::Value::as_u64) else {
        return Ok(EnvelopeRead::Bare(serde_json::from_value(value)?));
    };
    if version == u64::from(current) {
        let envelope: FlatEnvelope<T> = serde_json::from_value(value)?;
        Ok(EnvelopeRead::Current(envelope.payload))
    } else {
        Ok(EnvelopeRead::Unsupported(version))
    }
}

/// 写侧：恒打当前版本，禁止散落 `json!` 手拼。
pub fn write_envelope<T: Serialize>(current: u32, payload: &T) -> serde_json::Result<String> {
    serde_json::to_string(&FlatEnvelope {
        version: current,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Doc {
        answer: String,
    }

    #[test]
    fn write_then_read_current_round_trips() {
        let raw = write_envelope(1, &Doc { answer: "42".into() }).unwrap();
        assert_eq!(raw, r#"{"version":1,"answer":"42"}"#);
        assert_eq!(
            read_envelope::<Doc>(&raw, 1).unwrap(),
            EnvelopeRead::Current(Doc { answer: "42".into() })
        );
    }

    #[test]
    fn legacy_bare_object_reads_as_v0() {
        assert_eq!(
            read_envelope::<Doc>(r#"{"answer":"x"}"#, 1).unwrap(),
            EnvelopeRead::Bare(Doc { answer: "x".into() })
        );
    }

    #[test]
    fn unknown_version_is_explicit_not_guessed() {
        assert_eq!(
            read_envelope::<Doc>(r#"{"version":99,"answer":"x"}"#, 1).unwrap(),
            EnvelopeRead::Unsupported(99)
        );
        // 真坏 JSON 仍是解析错误（调用方维持原错误面）。
        assert!(read_envelope::<Doc>("not json", 1).is_err());
    }
}
