//! Async generation of Guid conversation starters for interest topics.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nomi_auxiliary::AuxiliaryClient;
use nomi_config::InterestConfig;
use tracing::{debug, info, warn};

use super::llm::generate_starters_for_topic_llm;
use super::store::InterestStore;
use super::types::TopicStatus;

/// How many missing-starter active topics to backfill per ingest flush.
const MISSING_STARTERS_BACKFILL_LIMIT: usize = 8;

/// Merge insert/promote hooks with active topics that still lack starters.
pub fn collect_starter_topic_ids(
    store: &InterestStore,
    mut hooked: Vec<String>,
) -> Vec<String> {
    if let Ok(missing) = store.list_active_topic_ids_missing_starters(MISSING_STARTERS_BACKFILL_LIMIT)
    {
        if !missing.is_empty() {
            info!(
                count = missing.len(),
                "interest starters: backfilling active topics without starters"
            );
            hooked.extend(missing);
        }
    }
    hooked.sort();
    hooked.dedup();
    hooked
}

/// Spawn background starter generation for topics that just became active.
pub fn spawn_starters_for_topics(
    data_dir: PathBuf,
    config: InterestConfig,
    topic_ids: Vec<String>,
    auxiliary: Option<Arc<AuxiliaryClient>>,
) {
    if !config.enabled || !config.starter_enabled || topic_ids.is_empty() {
        return;
    }
    if tokio::runtime::Handle::try_current().is_err() {
        debug!("interest starters: skip generation without tokio runtime");
        return;
    }
    let Some(aux) = auxiliary else {
        warn!(
            count = topic_ids.len(),
            "interest starters: no auxiliary client; skip generation"
        );
        return;
    };
    tokio::spawn(async move {
        generate_starters_for_topics(&data_dir, &config, &topic_ids, &aux).await;
    });
}

/// Variant that reuses an already-open store mutex (session-end ingest path).
pub fn spawn_starters_for_topics_with_store(
    store: Arc<Mutex<InterestStore>>,
    config: InterestConfig,
    topic_ids: Vec<String>,
    auxiliary: Option<Arc<AuxiliaryClient>>,
) {
    if !config.enabled || !config.starter_enabled || topic_ids.is_empty() {
        return;
    }
    if tokio::runtime::Handle::try_current().is_err() {
        debug!("interest starters: skip generation without tokio runtime");
        return;
    }
    let Some(aux) = auxiliary else {
        warn!(
            count = topic_ids.len(),
            "interest starters: no auxiliary client; skip generation"
        );
        return;
    };
    tokio::spawn(async move {
        generate_starters_with_store(store, &config, &topic_ids, &aux).await;
    });
}

/// Generate starters in the current async task (preferred: no nested spawn / reopen race).
pub async fn generate_starters_for_topics(
    data_dir: &Path,
    config: &InterestConfig,
    topic_ids: &[String],
    auxiliary: &AuxiliaryClient,
) {
    if !config.enabled || !config.starter_enabled || topic_ids.is_empty() {
        return;
    }
    let db_path = data_dir.join("interest.db");
    let Ok(store) = InterestStore::open(&db_path, config.clone()) else {
        warn!(
            path = %db_path.display(),
            "interest starters: failed to open interest.db"
        );
        return;
    };
    let store = Arc::new(Mutex::new(store));
    generate_starters_with_store(store, config, topic_ids, auxiliary).await;
}

/// Generate starters using an already-open store (same connection as ingest when possible).
pub async fn generate_starters_with_store(
    store: Arc<Mutex<InterestStore>>,
    config: &InterestConfig,
    topic_ids: &[String],
    auxiliary: &AuxiliaryClient,
) {
    if !config.enabled || !config.starter_enabled || topic_ids.is_empty() {
        return;
    }
    let per_topic = config.starters_per_topic.max(2) as usize;
    let max_global = config.max_starters_global.max(8) as u32;

    info!(
        count = topic_ids.len(),
        per_topic,
        "interest starters: generation starting"
    );

    for topic_id in topic_ids {
        let topic = {
            let Ok(guard) = store.lock() else {
                warn!("interest starters: store lock poisoned");
                return;
            };
            match guard.get_topic(topic_id) {
                Ok(Some(t)) if t.status == TopicStatus::Active || t.pinned => {
                    let existing = guard.count_starters_for_topic(topic_id).unwrap_or(0);
                    if existing >= per_topic as u32 {
                        debug!(
                            topic_id = %topic_id,
                            existing,
                            "interest starters: already populated"
                        );
                        continue;
                    }
                    let global = guard.count_starters_global().unwrap_or(0);
                    if global >= max_global {
                        info!(
                            global,
                            max_global, "interest starters: global cap reached; stop generating"
                        );
                        return;
                    }
                    t
                }
                Ok(Some(t)) => {
                    debug!(
                        topic_id = %topic_id,
                        status = t.status.as_str(),
                        "interest starters: skip non-active topic"
                    );
                    continue;
                }
                Ok(None) => {
                    warn!(
                        topic_id = %topic_id,
                        "interest starters: topic missing after ingest"
                    );
                    continue;
                }
                Err(err) => {
                    warn!(
                        topic_id = %topic_id,
                        error = %err,
                        "interest starters: load topic failed"
                    );
                    continue;
                }
            }
        };

        let prompts = generate_starters_for_topic_llm(auxiliary, &topic, per_topic).await;
        if prompts.is_empty() {
            warn!(
                topic_id = %topic_id,
                label = %topic.label,
                "interest starters: LLM returned no usable prompts"
            );
            continue;
        }

        let locale = infer_locale(&topic.label, &topic.summary);
        let texts: Vec<(String, String)> = prompts
            .into_iter()
            .map(|text| (text, locale.clone()))
            .collect();

        match store.lock() {
            Ok(guard) => match guard.replace_starters_for_topic(topic_id, &texts, "llm") {
                Ok(n) => info!(
                    topic_id = %topic_id,
                    count = n,
                    "interest starters: persisted"
                ),
                Err(err) => warn!(
                    topic_id = %topic_id,
                    error = %err,
                    "interest starters: persist failed"
                ),
            },
            Err(_) => {
                warn!("interest starters: store lock poisoned while persisting");
                return;
            }
        }
    }
}

fn infer_locale(label: &str, summary: &str) -> String {
    let sample = format!("{label}{summary}");
    if sample.chars().any(|c| {
        ('\u{4e00}'..='\u{9fff}').contains(&c)
            || ('\u{3400}'..='\u{4dbf}').contains(&c)
            || ('\u{3040}'..='\u{30ff}').contains(&c)
    }) {
        "zh-CN".to_string()
    } else {
        "en-US".to_string()
    }
}
