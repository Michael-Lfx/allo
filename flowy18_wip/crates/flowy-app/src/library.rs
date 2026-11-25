//! Maps host-owned conversation artifacts into the library's display model.
//! Legacy library.json records are intentionally neither loaded nor migrated.

use chrono::{Local, TimeZone};
use flowy_agent::{ArtifactRecord, SessionSummary};
use flowy_studio::library::{ArtifactAvailability, Category, LibraryItem};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn items(
    records: Vec<ArtifactRecord>,
    sessions: &[SessionSummary],
    deleted_sources: &HashSet<String>,
) -> Vec<LibraryItem> {
    let titles: HashMap<_, _> = sessions
        .iter()
        .map(|session| (session.id.as_str(), session.title.as_str()))
        .collect();
    records
        .into_iter()
        .map(|record| {
            let original_name = record.manifest.original_basename.clone();
            let receipt = record.manifest.receipt;
            let extension = Path::new(&receipt.display_name)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let date = i64::try_from(receipt.created_at_ms).ok().and_then(
                |timestamp| Local.timestamp_millis_opt(timestamp).single(),
            );
            let delivered_date = date.map_or_else(String::new, |date| {
                date.format("%Y-%m-%d").to_string()
            });
            let delivered_time = date.map_or_else(String::new, |date| {
                date.format("%H:%M").to_string()
            });
            let source_deleted = deleted_sources.contains(&receipt.session_id);
            let current_title =
                titles.get(receipt.session_id.as_str()).copied();
            let source_title = current_title
                .filter(|title| !title.is_empty())
                .unwrap_or(&record.manifest.origin_title_snapshot)
                .to_owned();
            LibraryItem {
                id: receipt.artifact_id,
                title: receipt.display_name,
                original_name,
                description: String::new(),
                category: category(&extension),
                date: delivered_date.clone(),
                time: delivered_time.clone(),
                pending: false,
                preview: None,
                size_bytes: Some(receipt.size_bytes),
                extension,
                media_type: receipt.media_type,
                delivered_at_ms: receipt.created_at_ms,
                delivered_date,
                delivered_time,
                source_session_id: receipt.session_id,
                source_title,
                source_deleted,
                source_unavailable: !source_deleted && current_title.is_none(),
                favorite: record.preference.favorite,
                availability: if record.corrupt {
                    ArtifactAvailability::Corrupt
                } else if record.available {
                    ArtifactAvailability::Available
                } else {
                    ArtifactAvailability::Missing
                },
                override_used: false,
            }
        })
        .collect()
}

fn category(extension: &str) -> Category {
    match extension {
        "pdf" | "md" | "markdown" | "txt" | "doc" | "docx" | "odt" | "rtf" => {
            Category::Document
        }
        "ppt" | "pptx" | "odp" => Category::Slides,
        "csv" | "tsv" | "xls" | "xlsx" | "ods" => Category::Table,
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "svg" | "avif" | "bmp" => {
            Category::Image
        }
        "mp4" | "mov" | "webm" | "avi" | "mkv" => Category::Video,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" => Category::Audio,
        _ => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_agent::ArtifactPreference;
    use serde_json::json;

    fn record() -> ArtifactRecord {
        ArtifactRecord {
            manifest: serde_json::from_value(json!({
                "schema_version": 1,
                "receipt": {
                    "artifact_id": "artifact-1",
                    "delivery_key": "report",
                    "session_id": "session-1",
                    "submission_id": "submission-1",
                    "tool_call_id": "call-1",
                    "display_name": "report.csv",
                    "media_type": "text/csv",
                    "size_bytes": 8,
                    "sha256": "fixture-hash",
                    "created_at_ms": 1_795_219_200_000_u64,
                    "origin_title_snapshot": "Original title"
                },
                "original_basename": "source-report.csv",
                "origin": { "kind": "home" },
                "origin_title_snapshot": "Original title",
                "payload_relative_path": "items/artifact-1/payload",
                "publication_state": "committed"
            }))
            .unwrap(),
            preference: ArtifactPreference {
                favorite: true,
                trashed_at_ms: None,
            },
            available: true,
            corrupt: false,
        }
    }

    #[test]
    fn source_titles_follow_current_session_but_fallback_after_deletion() {
        let session = SessionSummary {
            id: "session-1".into(),
            title: "Renamed conversation".into(),
            updated_at: 1,
            project_id: None,
        };
        let current =
            items(vec![record()], &[session], &HashSet::new()).remove(0);
        assert_eq!(current.source_title, "Renamed conversation");
        assert!(!current.source_deleted && !current.source_unavailable);
        let deleted =
            items(vec![record()], &[], &HashSet::from(["session-1".into()]))
                .remove(0);
        assert_eq!(deleted.source_title, "Original title");
        assert!(deleted.source_deleted && !deleted.source_unavailable);
        assert_eq!(deleted.availability, ArtifactAvailability::Available);
        let unavailable = items(vec![record()], &[], &HashSet::new()).remove(0);
        assert!(!unavailable.source_deleted && unavailable.source_unavailable);
    }

    #[test]
    fn corrupt_payload_is_not_reported_as_available() {
        let mut artifact = record();
        artifact.available = false;
        artifact.corrupt = true;
        let projected = items(vec![artifact], &[], &HashSet::new()).remove(0);
        assert_eq!(projected.availability, ArtifactAvailability::Corrupt);
    }

    #[test]
    fn original_basename_remains_searchable_after_display_rename() {
        let projected = items(vec![record()], &[], &HashSet::new());
        let model = flowy_studio::library::LibraryModel::new(projected);
        assert_eq!(model.filtered("source-report").len(), 1);
        assert_eq!(model.filtered("missing").len(), 0);
    }
}
