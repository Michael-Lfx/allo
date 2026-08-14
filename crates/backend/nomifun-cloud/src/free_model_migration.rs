//! Migration of active model preferences away from the optional free provider.
//!
//! Historical conversations and message snapshots are intentionally outside
//! this module. They retain their original provider/model provenance; only
//! mutable client preferences are rewritten after a successful Cloud catalog
//! sync.

use std::sync::Arc;

use nomifun_api_types::{ModelTask, ModelTrait, derive_tasks_and_traits};
use nomifun_db::{IClientPreferenceRepository, IProviderModelRepository, IProviderRepository};
use serde_json::Value;

use crate::config_defaults::FLOWY_BUILTIN_PROVIDER_ID;

const DEFAULT_MODEL_KEYS: &[&str] = &[
    "nomi.defaultModel",
    "nomi.collaborationModels",
    "agent.model_failover",
    "knowledge.autogenModel",
    "conversation.titleModel",
    "tools.imageGenerationModel",
    "tools.imageAnalysisModel",
    "tools.speechToText",
    "speechToText",
    "tools.textToSpeech",
];
const IDMM_BACKUP_PROVIDER_KEY: &str = "idmm_backup_provider_id";
const IDMM_BACKUP_MODEL_KEY: &str = "idmm_backup_model";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CloudCandidate {
    model: String,
    tasks: Vec<ModelTask>,
    traits: Vec<ModelTrait>,
}

/// Migrate active client preferences that point at the managed free provider.
///
/// The operation is safe to run after every successful Cloud catalog sync:
/// only exact free-provider references are changed, replacements come from
/// the current catalog order, and the repository applies the batch atomically.
pub async fn migrate_free_model_preferences(
    provider_repo: &Arc<dyn IProviderRepository>,
    provider_model_repo: &Arc<dyn IProviderModelRepository>,
    preference_repo: &Arc<dyn IClientPreferenceRepository>,
) -> Result<usize, String> {
    if nomifun_common::free_models_enabled() {
        return Ok(0);
    }

    let free_provider_ids = provider_repo
        .list()
        .await
        .map_err(|error| format!("list providers for free-model migration: {error}"))?
        .into_iter()
        .filter(|provider| nomifun_common::is_free_model_platform(&provider.platform))
        .map(|provider| provider.provider_id)
        .collect::<Vec<_>>();
    if free_provider_ids.is_empty() {
        return Ok(0);
    }

    let candidates = cloud_candidates(provider_repo, provider_model_repo).await?;
    let preferences = preference_repo
        .get_all()
        .await
        .map_err(|error| format!("read client preferences for free-model migration: {error}"))?;

    let idmm_provider_is_free = preferences
        .iter()
        .find(|preference| preference.key == IDMM_BACKUP_PROVIDER_KEY)
        .is_some_and(|preference| {
            free_provider_ids
                .iter()
                .any(|id| id == preference.value.trim())
        });
    let mut upserts: Vec<(String, String)> = Vec::new();
    let mut deletes: Vec<String> = Vec::new();
    for preference in preferences {
        let Some((task, required_trait)) = preference_requirement(&preference.key) else {
            continue;
        };
        let target = select_candidate(&candidates, task, required_trait);
        let old_value = preference.value.as_str();

        if preference.key == IDMM_BACKUP_PROVIDER_KEY {
            if free_provider_ids.iter().any(|id| id == old_value.trim()) {
                if let Some(candidate) = target {
                    upserts.push((
                        preference.key,
                        serde_json::to_string(FLOWY_BUILTIN_PROVIDER_ID)
                            .map_err(|error| format!("serialize migrated provider id: {error}"))?,
                    ));
                    tracing::info!(
                        key = "idmm_backup_provider_id",
                        model = %candidate.model,
                        "migrated free-model provider preference to Flowy Cloud"
                    );
                }
            }
            continue;
        }
        if preference.key == IDMM_BACKUP_MODEL_KEY {
            // The provider and model are stored as separate scalar rows. The
            // provider row is the authority: only replace its model when the
            // pair was pointing at the disabled managed provider and a
            // compatible Cloud candidate exists. With no candidate, retain
            // both old values so IDMM remains visibly unavailable rather than
            // being silently pointed at an unrelated model.
            if idmm_provider_is_free && target.is_some() {
                upserts.push((
                    preference.key,
                    target.expect("checked above").model.clone(),
                ));
            }
            continue;
        }

        let Ok(mut value) = serde_json::from_str::<Value>(old_value) else {
            tracing::warn!(key = %preference.key, "skipping invalid JSON during free-model migration");
            continue;
        };
        let has_free_reference = contains_provider_reference(&value, &free_provider_ids);
        if !has_free_reference {
            continue;
        }

        let changed = rewrite_model_references(&mut value, &free_provider_ids, target);
        if !changed {
            if target.is_none() && is_optional_model_key(&preference.key) {
                deletes.push(preference.key);
            }
            continue;
        }

        upserts.push((preference.key, value.to_string()));
    }

    if upserts.is_empty() && deletes.is_empty() {
        return Ok(0);
    }

    let upsert_refs = upserts
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let delete_refs = deletes.iter().map(String::as_str).collect::<Vec<_>>();
    preference_repo
        .update_batch(&upsert_refs, &delete_refs)
        .await
        .map_err(|error| format!("commit free-model preference migration: {error}"))?;

    Ok(upserts.len() + deletes.len())
}

async fn cloud_candidates(
    provider_repo: &Arc<dyn IProviderRepository>,
    provider_model_repo: &Arc<dyn IProviderModelRepository>,
) -> Result<Vec<CloudCandidate>, String> {
    let Some(provider) = provider_repo
        .find_by_id(FLOWY_BUILTIN_PROVIDER_ID)
        .await
        .map_err(|error| format!("read Flowy Cloud provider for migration: {error}"))?
    else {
        return Ok(Vec::new());
    };
    if !provider.enabled {
        return Ok(Vec::new());
    }

    let rows = provider_model_repo
        .list_for_provider(FLOWY_BUILTIN_PROVIDER_ID)
        .await
        .map_err(|error| format!("read Flowy Cloud models for migration: {error}"))?;
    Ok(rows
        .into_iter()
        .filter(|row| row.enabled)
        .map(|row| {
            let tasks = serde_json::from_str::<Vec<ModelTask>>(&row.tasks).unwrap_or_default();
            let traits = serde_json::from_str::<Vec<ModelTrait>>(&row.traits).unwrap_or_default();
            let (tasks, traits) = if tasks.is_empty() {
                derive_tasks_and_traits(&provider.platform, &row.model)
            } else {
                (tasks, traits)
            };
            CloudCandidate {
                model: row.model,
                tasks,
                traits,
            }
        })
        .collect())
}

fn preference_requirement(key: &str) -> Option<(ModelTask, Option<ModelTrait>)> {
    if key == IDMM_BACKUP_PROVIDER_KEY
        || key == IDMM_BACKUP_MODEL_KEY
        || DEFAULT_MODEL_KEYS.contains(&key)
    {
        return Some((if key == "tools.imageGenerationModel" {
            ModelTask::ImageGeneration
        } else if key == "tools.imageAnalysisModel" {
            ModelTask::Chat
        } else if key == "tools.speechToText" || key == "speechToText" {
            ModelTask::SpeechRecognition
        } else if key == "tools.textToSpeech" {
            ModelTask::SpeechSynthesis
        } else {
            ModelTask::Chat
        }, (key == "tools.imageAnalysisModel").then_some(ModelTrait::VisionInput)));
    }
    if key.starts_with("channels.") && key.ends_with(".defaultModel") {
        return Some((ModelTask::Chat, None));
    }
    None
}

fn is_optional_model_key(key: &str) -> bool {
    matches!(
        key,
        "tools.imageGenerationModel"
            | "tools.imageAnalysisModel"
            | "tools.speechToText"
            | "speechToText"
            | "tools.textToSpeech"
            | "conversation.titleModel"
    )
}

fn select_candidate<'a>(
    candidates: &'a [CloudCandidate],
    task: ModelTask,
    required_trait: Option<ModelTrait>,
) -> Option<&'a CloudCandidate> {
    candidates.iter().find(|candidate| {
        candidate.tasks.contains(&task)
            && required_trait.is_none_or(|required| candidate.traits.contains(&required))
    })
}

fn contains_provider_reference(value: &Value, provider_ids: &[String]) -> bool {
    match value {
        Value::Object(object) => {
            object
                .get("provider_id")
                .and_then(Value::as_str)
                .is_some_and(|provider_id| provider_ids.iter().any(|id| id == provider_id))
                || object.values().any(|value| contains_provider_reference(value, provider_ids))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| contains_provider_reference(value, provider_ids)),
        _ => false,
    }
}

fn rewrite_model_references(value: &mut Value, provider_ids: &[String], target: Option<&CloudCandidate>) -> bool {
    match value {
        Value::Array(values) => {
            let mut changed = false;
            values.retain_mut(|item| {
                let is_free_model = item
                    .get("provider_id")
                    .and_then(Value::as_str)
                    .is_some_and(|provider_id| provider_ids.iter().any(|id| id == provider_id));
                if is_free_model && target.is_none() {
                    changed = true;
                    return false;
                }
                changed |= rewrite_model_references(item, provider_ids, target);
                true
            });
            changed
        }
        Value::Object(object) => {
            let is_free_model = object
                .get("provider_id")
                .and_then(Value::as_str)
                .is_some_and(|provider_id| provider_ids.iter().any(|id| id == provider_id));
            if is_free_model {
                let Some(target) = target else {
                    return false;
                };
                object.insert(
                    "provider_id".to_owned(),
                    Value::String(FLOWY_BUILTIN_PROVIDER_ID.to_owned()),
                );
                if object.contains_key("model") {
                    object.insert("model".to_owned(), Value::String(target.model.clone()));
                }
                if object.contains_key("use_model") {
                    object.insert("use_model".to_owned(), Value::String(target.model.clone()));
                }
                return true;
            }
            object
                .values_mut()
                .map(|value| rewrite_model_references(value, provider_ids, target))
                .any(|changed| changed)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use nomifun_db::{
        CreateProviderParams, IClientPreferenceRepository, IProviderModelRepository,
        IProviderRepository, NewProviderModel, SqliteClientPreferenceRepository,
        SqliteProviderModelRepository, SqliteProviderRepository, init_database_memory,
    };

    const FREE_ID: &str = "0190f5fe-7c00-7a00-8000-000000000001";

    fn candidate(model: &str, task: ModelTask) -> CloudCandidate {
        CloudCandidate {
            model: model.to_owned(),
            tasks: vec![task],
            traits: vec![],
        }
    }

    #[test]
    fn first_catalog_candidate_is_selected_without_hard_coding_a_model_id() {
        let candidates = vec![candidate("cloud-default", ModelTask::Chat), candidate("other", ModelTask::Chat)];
        assert_eq!(
            select_candidate(&candidates, ModelTask::Chat, None).unwrap().model,
            "cloud-default"
        );
    }

    #[test]
    fn model_reference_is_rewritten_and_free_array_entries_can_be_removed() {
        let ids = vec![FREE_ID.to_owned()];
        let target = candidate("cloud-default", ModelTask::Chat);
        let mut value = serde_json::json!([
            {"provider_id": FREE_ID, "model": "free", "use_model": "free"},
            {"provider_id": "0190f5fe-7c00-7a00-8000-000000000002", "model": "keep"}
        ]);
        assert!(rewrite_model_references(&mut value, &ids, Some(&target)));
        assert_eq!(value[0]["provider_id"], FLOWY_BUILTIN_PROVIDER_ID);
        assert_eq!(value[0]["model"], "cloud-default");

        let mut value = serde_json::json!([
            {"provider_id": FREE_ID, "model": "free"},
            {"provider_id": "0190f5fe-7c00-7a00-8000-000000000002", "model": "keep"}
        ]);
        assert!(rewrite_model_references(&mut value, &ids, None));
        assert_eq!(value.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn migrates_idmm_pair_and_is_idempotent() {
        if nomifun_common::free_models_enabled() {
            return;
        }

        let database = init_database_memory().await.unwrap();
        let provider_repo = Arc::new(SqliteProviderRepository::new(database.pool().clone()));
        let provider_model_repo = Arc::new(SqliteProviderModelRepository::new(database.pool().clone()));
        let preference_repo = Arc::new(SqliteClientPreferenceRepository::new(database.pool().clone()));
        let free_id = nomifun_common::ProviderId::new().into_string();
        let encrypted = nomifun_common::encrypt_string("free-key", &[0x42; 32]).unwrap();

        for (provider_id, platform, name, models) in [
            (
                &free_id,
                nomifun_common::FREE_MODEL_PLATFORM,
                "Managed free",
                "[\"free-chat\"]",
            ),
            (
                &FLOWY_BUILTIN_PROVIDER_ID.to_owned(),
                "openai",
                "Flowy Cloud",
                "[]",
            ),
        ] {
            provider_repo
                .create(CreateProviderParams {
                    provider_id: Some(provider_id),
                    platform,
                    name,
                    base_url: "https://example.invalid/v1",
                    api_key_encrypted: &encrypted,
                    models,
                    enabled: true,
                    model_context_limits: None,
                    model_protocols: None,
                    model_descriptions: None,
                    model_enabled: None,
                    bedrock_config: None,
                    is_full_url: false,
                    sort_order: None,
                })
                .await
                .unwrap();
        }
        provider_model_repo
            .create(
                FLOWY_BUILTIN_PROVIDER_ID,
                &NewProviderModel {
                    model: "cloud-chat",
                    enabled: true,
                    sort_order: 0,
                    tasks: "[\"chat\"]",
                    traits: "[]",
                    protocol: None,
                    params: "{}",
                    context_limit: None,
                    description: None,
                    source: "inferred",
                    health: None,
                },
            )
            .await
            .unwrap();

        preference_repo
            .upsert_batch(&[
                (IDMM_BACKUP_PROVIDER_KEY, &free_id),
                (IDMM_BACKUP_MODEL_KEY, "free-chat"),
                (
                    "nomi.defaultModel",
                    &serde_json::json!({"provider_id": free_id, "model": "free-chat"}).to_string(),
                ),
            ])
            .await
            .unwrap();

        let provider_repo_dyn: Arc<dyn IProviderRepository> = provider_repo.clone();
        let provider_model_repo_dyn: Arc<dyn IProviderModelRepository> = provider_model_repo.clone();
        let preference_repo_dyn: Arc<dyn IClientPreferenceRepository> = preference_repo.clone();
        let changed = migrate_free_model_preferences(
            &provider_repo_dyn,
            &provider_model_repo_dyn,
            &preference_repo_dyn,
        )
        .await
        .unwrap();
        assert_eq!(changed, 3);

        let rows = preference_repo.get_all().await.unwrap();
        let values = rows
            .into_iter()
            .map(|row| (row.key, row.value))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(values[IDMM_BACKUP_PROVIDER_KEY], FLOWY_BUILTIN_PROVIDER_ID);
        assert_eq!(values[IDMM_BACKUP_MODEL_KEY], "cloud-chat");
        assert_eq!(
            serde_json::from_str::<Value>(&values["nomi.defaultModel"]).unwrap(),
            serde_json::json!({
                "provider_id": FLOWY_BUILTIN_PROVIDER_ID,
                "model": "cloud-chat"
            })
        );

        let changed_again = migrate_free_model_preferences(
            &provider_repo_dyn,
            &provider_model_repo_dyn,
            &preference_repo_dyn,
        )
        .await
        .unwrap();
        assert_eq!(changed_again, 0);
    }
}
