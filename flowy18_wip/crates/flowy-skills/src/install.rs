use crate::SkillId;
use crate::plan::percent_encode;
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStep {
    Resolve,
    Verify,
    Fetch,
    Hash,
    Scan,
    Stage,
    Commit,
}

impl InstallStep {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Verify => "verify",
            Self::Fetch => "fetch",
            Self::Hash => "hash",
            Self::Scan => "scan",
            Self::Stage => "stage",
            Self::Commit => "commit",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "resolve" => Some(Self::Resolve),
            "verify" => Some(Self::Verify),
            "fetch" => Some(Self::Fetch),
            "hash" => Some(Self::Hash),
            "scan" => Some(Self::Scan),
            "stage" => Some(Self::Stage),
            "commit" => Some(Self::Commit),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallRecord {
    pub id: SkillId,
    pub step: InstallStep,
    pub auto_invoke: bool,
    pub scripts_enabled: bool,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallEvent {
    Verified { ok: bool },
    Fetched,
    Hashed { expected: String, actual: String },
    Scanned,
    Staged,
    Committed,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallError {
    #[error("skill verification rejected installation")]
    Rejected,
    #[error("content hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("event is not valid at install step {step:?}")]
    Unexpected { step: InstallStep },
}

#[must_use]
pub fn begin_community_install(id: &SkillId) -> InstallRecord {
    InstallRecord {
        id: id.clone(),
        step: InstallStep::Resolve,
        auto_invoke: false,
        scripts_enabled: false,
        content_hash: String::new(),
    }
}

pub fn advance(
    record: &InstallRecord,
    event: InstallEvent,
) -> Result<InstallRecord, InstallError> {
    let mut next = record.clone();
    match (record.step, event) {
        (InstallStep::Resolve, InstallEvent::Verified { ok: true }) => {
            next.step = InstallStep::Verify;
        }
        (InstallStep::Resolve, InstallEvent::Verified { ok: false }) => {
            return Err(InstallError::Rejected);
        }
        (InstallStep::Verify, InstallEvent::Fetched) => {
            next.step = InstallStep::Fetch;
        }
        (InstallStep::Fetch, InstallEvent::Hashed { expected, actual }) => {
            if expected != actual {
                return Err(InstallError::HashMismatch { expected, actual });
            }
            next.content_hash = actual;
            next.step = InstallStep::Hash;
        }
        (InstallStep::Hash, InstallEvent::Scanned) => {
            next.step = InstallStep::Scan;
        }
        (InstallStep::Scan, InstallEvent::Staged) => {
            next.step = InstallStep::Stage;
        }
        (InstallStep::Stage, InstallEvent::Committed) => {
            next.step = InstallStep::Commit;
        }
        (step, _) => return Err(InstallError::Unexpected { step }),
    }
    Ok(next)
}

#[must_use]
pub fn verify_url(id: &SkillId) -> String {
    format!(
        "https://clawhub.ai/api/v1/skills/{}/verify?ownerHandle={}&tag=latest",
        percent_encode(&id.slug),
        percent_encode(&id.owner_handle)
    )
}

#[must_use]
pub fn download_url(id: &SkillId) -> String {
    format!(
        "https://clawhub.ai/api/v1/download?slug={}&ownerHandle={}&version={}",
        percent_encode(&id.slug),
        percent_encode(&id.owner_handle),
        percent_encode(&id.version)
    )
}

pub fn accept_verify_body(body: &str) -> Result<(), InstallError> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| InstallError::Rejected)?;
    if value.get("ok").and_then(Value::as_bool) == Some(true)
        && value.get("decision").and_then(Value::as_str) == Some("pass")
    {
        Ok(())
    } else {
        Err(InstallError::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_skill_id;

    fn id() -> SkillId {
        parse_skill_id("awspace", "pdf", "1.2.3").unwrap()
    }

    fn record_at(step: InstallStep) -> InstallRecord {
        InstallRecord {
            id: id(),
            step,
            auto_invoke: false,
            scripts_enabled: false,
            content_hash: String::new(),
        }
    }

    #[test]
    fn state_machine_reaches_commit_only_in_order() {
        let mut record = begin_community_install(&id());
        record = advance(&record, InstallEvent::Verified { ok: true }).unwrap();
        record = advance(&record, InstallEvent::Fetched).unwrap();
        record = advance(
            &record,
            InstallEvent::Hashed {
                expected: "abc".into(),
                actual: "abc".into(),
            },
        )
        .unwrap();
        record = advance(&record, InstallEvent::Scanned).unwrap();
        record = advance(&record, InstallEvent::Staged).unwrap();
        record = advance(&record, InstallEvent::Committed).unwrap();
        assert_eq!(record.step, InstallStep::Commit);
        assert_eq!(record.content_hash, "abc");
        assert!(!record.auto_invoke);
        assert!(!record.scripts_enabled);
    }

    #[test]
    fn rejected_verify_does_not_advance() {
        let record = record_at(InstallStep::Resolve);
        assert_eq!(
            advance(&record, InstallEvent::Verified { ok: false }),
            Err(InstallError::Rejected)
        );
        assert_eq!(record.step, InstallStep::Resolve);
    }

    #[test]
    fn mismatched_hash_is_rejected() {
        let record = record_at(InstallStep::Fetch);
        assert!(matches!(
            advance(
                &record,
                InstallEvent::Hashed {
                    expected: "expected".into(),
                    actual: "actual".into(),
                }
            ),
            Err(InstallError::HashMismatch { .. })
        ));
    }

    #[test]
    fn urls_match_clawhub_contract() {
        assert_eq!(
            verify_url(&id()),
            "https://clawhub.ai/api/v1/skills/pdf/verify?ownerHandle=awspace&tag=latest"
        );
        assert_eq!(
            download_url(&id()),
            "https://clawhub.ai/api/v1/download?slug=pdf&ownerHandle=awspace&version=1.2.3"
        );
    }

    #[test]
    fn verify_requires_ok_and_pass_decision() {
        assert!(accept_verify_body(r#"{"ok":true,"decision":"pass"}"#).is_ok());
        for body in [
            r#"{"ok":true,"decision":"pending"}"#,
            r#"{"ok":true,"decision":"warn"}"#,
            r#"{"ok":true,"decision":"malicious"}"#,
            r#"{"ok":false,"decision":"pass"}"#,
            r"{}",
        ] {
            assert!(matches!(
                accept_verify_body(body),
                Err(InstallError::Rejected)
            ));
        }
    }
}
