//! Atomic evidence persistence.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

use super::{
    CaseCategory, EvaluationMode, EvaluationProfile, FetchEvaluationResult, PeerMode, RunStatus,
    SCORING_VERSION,
};

pub(crate) fn atomic_json_write<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), value)?;
    std::io::Write::flush(temp.as_file_mut())?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(crate) fn read_json<T: DeserializeOwned>(
    path: &Path,
) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn default_status_path(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension("status.json");
    path
}

pub(crate) fn default_safety_path(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension("safety.json");
    path
}

pub(crate) fn atomic_json_write_locked<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut lock_path = path.to_path_buf();
    lock_path.set_extension("lock");
    if let Some(parent) = lock_path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)?;
    lock.lock_exclusive()?;
    let result = atomic_json_write(path, value);
    let _ = lock.unlock();
    result
}

/// Validate the typed JSONL evidence for one complete Admission triple.
///
/// Evidence recovery is deliberately owned by this module: callers may decide
/// whether a run should be registered or discarded, but they cannot treat an
/// arbitrary JSONL file as a completed run.  The validator checks provenance,
/// schema/scoring, the exact case set, and the fixed Compare/Cold plus two
/// E2E/Warm phases without retaining URLs or response content.
pub(crate) fn admission_result_file_is_complete(
    path: &Path,
    status: &RunStatus,
    expected_case_ids: &[String],
    expected_category: CaseCategory,
    campaign_git_sha: &str,
    campaign_corpus_version: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str::<FetchEvaluationResult>(&line)?);
    }

    let expected_records = expected_case_ids.len().saturating_mul(3);
    if records.len() != expected_records
        || records.len() != status.planned_attempts
        || records.len() != status.completed_attempts
    {
        return Ok(false);
    }

    let expected_ids = expected_case_ids.iter().collect::<BTreeSet<_>>();
    let mut phases = BTreeSet::new();
    for record in records {
        if record.schema_version != 3
            || record.scoring_version != SCORING_VERSION
            || record.evaluation_profile != EvaluationProfile::Admission
            || record.run_id != status.run_id
            || record.git_sha != campaign_git_sha
            || record.corpus_version != campaign_corpus_version
            || record.category != expected_category
            || !expected_ids.contains(&record.case_id)
        {
            return Ok(false);
        }

        match record.attempt {
            1 if record.mode == EvaluationMode::Compare && record.peer_mode == PeerMode::Cold => {}
            2 | 3 if record.mode == EvaluationMode::E2e && record.peer_mode == PeerMode::Warm => {}
            _ => return Ok(false),
        }
        if !phases.insert((record.case_id, record.attempt)) {
            return Ok(false);
        }
    }

    Ok(expected_case_ids.iter().all(|case_id| {
        (1..=3).all(|attempt| phases.contains(&(case_id.clone(), attempt)))
    }))
}
