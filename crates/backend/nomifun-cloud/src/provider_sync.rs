//! Sync the logged-in Flowy JWT and server model catalog into the built-in provider row.
//!
//! The local `providers.models` JSON is a **projection** of the upstream
//! `availableListClaw` catalog. On a successful catalog fetch it is fully
//! replaced (delisted models must disappear). Transient fetch failures must
//! **not** wipe or invent models — that left stale delisted entries in place
//! when callers ignored soft errors, and previously also re-injected a config
//! default that the server no longer lists.

use std::collections::HashMap;
use std::sync::Arc;

use nomifun_api_types::derive_tasks_and_traits;
use nomi_config::ServerConfig;
use nomifun_common::encrypt_string;
use nomifun_db::{
    CreateProviderParams, IProviderModelRepository, IProviderRepository, ProviderModelProfileSeed,
    ProviderModelUpdate, UpdateProviderParams,
};
use tracing::{info, warn};

use crate::config_defaults::FLOWY_BUILTIN_PROVIDER_ID;
use crate::flowy::{ClawModelEntry, FlowyApiClient};
use crate::session::ServerSession;

/// Upsert Flowy Cloud provider with JWT + server model catalog for the model
/// selector. The boolean reports whether the upstream catalog request
/// succeeded; `Ok(false)` deliberately preserves an existing local projection
/// while making the soft sync failure visible to the caller.
pub async fn sync_flowy_builtin_provider(
    provider_repo: &Arc<dyn IProviderRepository>,
    provider_model_repo: &Arc<dyn IProviderModelRepository>,
    encryption_key: &[u8; 32],
    server: &ServerConfig,
    data_dir: &std::path::Path,
) -> Result<bool, String> {
    let session = ServerSession::from_config(server, data_dir);
    let token = session
        .access_token()
        .await
        .map_err(|e| e.to_string())?
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| "not logged in to Flowy server".to_string())?;

    let base_url = server.effective_llm_base_url();
    let api_key_encrypted =
        encrypt_string(&token, encryption_key).map_err(|e| format!("encrypt token: {e}"))?;

    // Only replace the local model projection when the upstream catalog fetch
    // succeeds. On failure, still refresh JWT/base_url but leave models alone
    // so a blip cannot silently leave callers believing a soft-failed sync
    // "updated" anything — and so we never invent a fake one-model catalog
    // that masks the real failure mode (stale DB until the next success).
    let (catalog_fields, catalog_synced) = match fetch_chat_models(server, &session, data_dir).await {
        Ok(entries) => (Some(build_model_fields(&entries, server)), true),
        Err(e) => {
            warn!(
                "Failed to fetch Flowy chat model catalog: {e}; keeping existing local model list"
            );
            (None, false)
        }
    };

    let profile_fields = catalog_fields.clone().unwrap_or_else(|| BuiltModelFields {
        model_ids: Vec::new(),
        descriptions_json: "{}".to_string(),
        context_limits_json: "{}".to_string(),
    });
    let models_json = catalog_fields
        .as_ref()
        .map(|fields| serde_json::to_string(&fields.model_ids))
        .transpose()
        .map_err(|e| format!("serialize models: {e}"))?;
    let descriptions_json = catalog_fields
        .as_ref()
        .map(|fields| fields.descriptions_json.as_str());
    let context_limits_json = catalog_fields
        .as_ref()
        .map(|fields| fields.context_limits_json.as_str());

    let profile_seeds = if catalog_synced {
        build_profile_seeds(&profile_fields.model_ids, "openai")?
    } else {
        Vec::new()
    };
    let provider_exists = provider_repo
        .find_by_id(FLOWY_BUILTIN_PROVIDER_ID)
        .await
        .map_err(|e| e.to_string())?
        .is_some();

    if provider_exists {
        provider_repo
            .update_with_model_profiles(
                FLOWY_BUILTIN_PROVIDER_ID,
                UpdateProviderParams {
                    platform: Some("openai"),
                    name: Some("Flowy Cloud"),
                    base_url: Some(&base_url),
                    api_key_encrypted: Some(&api_key_encrypted),
                    models: models_json.as_deref(),
                    enabled: Some(true),
                    // Replace per-model context limits from catalog `extra` on
                    // successful sync so Context Usage / compact budgets track
                    // each model's declared window (not the 128k engine default).
                    model_context_limits: context_limits_json.map(|s| {
                        if s == "{}" {
                            None
                        } else {
                            Some(s)
                        }
                    }),
                    model_descriptions: descriptions_json.map(Some),
                    // Enabled flags live on `provider_models` since migration 016;
                    // leave them untouched here (membership sync still drops
                    // delisted model rows via `models`).
                    model_enabled: None,
                    is_full_url: Some(false),
                    sort_order: Some(0),
                    ..Default::default()
                },
                &profile_seeds,
                provider_model_repo.as_ref(),
            )
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let fields = profile_fields.clone();
        let models =
            serde_json::to_string(&fields.model_ids).map_err(|e| format!("serialize models: {e}"))?;
        let context_limits = if fields.context_limits_json == "{}" {
            None
        } else {
            Some(fields.context_limits_json.as_str())
        };
        provider_repo
            .create_with_model_profiles(
                CreateProviderParams {
                provider_id: Some(FLOWY_BUILTIN_PROVIDER_ID),
                platform: "openai",
                name: "Flowy Cloud",
                base_url: &base_url,
                api_key_encrypted: &api_key_encrypted,
                models: &models,
                enabled: true,
                model_context_limits: context_limits,
                model_protocols: None,
                model_descriptions: Some(fields.descriptions_json.as_str()),
                model_enabled: None,
                bedrock_config: None,
                is_full_url: false,
                sort_order: Some(0),
                },
                &profile_seeds,
                provider_model_repo.as_ref(),
            )
            .await
            .map_err(|e| e.to_string())?;
    }

    let model_count = catalog_fields
        .as_ref()
        .map(|fields| fields.model_ids.len())
        .unwrap_or(0);
    info!(
        flowy_models = model_count,
        catalog_replaced = catalog_fields.is_some(),
        "Synced Flowy Cloud provider from server catalog"
    );
    Ok(catalog_synced)
}

fn build_profile_seeds(
    catalog_models: &[String],
    platform: &str,
) -> Result<Vec<ProviderModelProfileSeed>, String> {
    catalog_models
        .iter()
        .filter(|model| !model.trim().is_empty())
        .map(|model| {
            let (tasks, traits) = derive_tasks_and_traits(platform, model);
            Ok(ProviderModelProfileSeed {
                model: model.clone(),
                tasks: serde_json::to_string(&tasks)
                    .map_err(|error| format!("serialize Flowy model tasks: {error}"))?,
                traits: serde_json::to_string(&traits)
                    .map_err(|error| format!("serialize Flowy model traits: {error}"))?,
            })
        })
        .collect()
}

async fn fetch_chat_models(
    server: &ServerConfig,
    session: &ServerSession,
    data_dir: &std::path::Path,
) -> Result<Vec<ClawModelEntry>, String> {
    let _ = data_dir;
    let api = FlowyApiClient::new(server).map_err(|e| e.to_string())?;
    let resp = api
        .get_available_models_claw(session, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(resp.cloud)
}

#[derive(Debug, Clone)]
struct BuiltModelFields {
    model_ids: Vec<String>,
    descriptions_json: String,
    /// Per-model context windows from catalog `extra.context_window`.
    context_limits_json: String,
}

fn build_model_fields(entries: &[ClawModelEntry], server: &ServerConfig) -> BuiltModelFields {
    let default_model = server.effective_default_llm_model();
    let mut model_ids: Vec<String> = entries.iter().map(|e| e.api_model_id()).collect();
    model_ids.sort();
    model_ids.dedup();
    model_ids.retain(|m| !m.trim().is_empty());

    promote_default_model(&mut model_ids, &default_model);

    let mut descriptions = HashMap::new();
    let mut context_limits = HashMap::new();
    for entry in entries {
        let id = entry.api_model_id();
        descriptions.insert(id.clone(), display_name_for_entry(entry));
        if let Some(window) = entry.model_extra().context_window_tokens() {
            // Keep first positive window if duplicate ids appear.
            context_limits.entry(id).or_insert(window as i64);
        }
    }
    for id in &model_ids {
        descriptions
            .entry(id.clone())
            .or_insert_with(|| display_name_for_id(id));
    }

    BuiltModelFields {
        model_ids,
        descriptions_json: serde_json::to_string(&descriptions).unwrap_or_else(|_| "{}".to_string()),
        context_limits_json: serde_json::to_string(&context_limits)
            .unwrap_or_else(|_| "{}".to_string()),
    }
}

fn display_name_for_entry(entry: &ClawModelEntry) -> String {
    let name = entry.name.trim();
    if !name.is_empty() {
        return display_name_for_id(name);
    }
    display_name_for_id(&entry.api_model_id())
}

fn display_name_for_id(id: &str) -> String {
    id.strip_prefix("AIPC-")
        .or_else(|| id.strip_prefix("aipc-"))
        .unwrap_or(id)
        .to_string()
}

/// Move the configured default to the front **only if it is already in the
/// server catalog**. Never invent / re-inject a delisted model id.
fn promote_default_model(model_ids: &mut Vec<String>, default_model: &str) {
    if default_model.trim().is_empty() {
        return;
    }
    if !model_ids.iter().any(|m| m == default_model) {
        return;
    }
    model_ids.retain(|m| m != default_model);
    model_ids.insert(0, default_model.to_string());
}

fn prune_model_enabled_json(
    existing: Option<&str>,
    keep_ids: &[String],
) -> Result<String, String> {
    let Some(raw) = existing.map(str::trim).filter(|s| !s.is_empty() && *s != "null") else {
        return Ok("{}".to_string());
    };
    let mut map: HashMap<String, bool> =
        serde_json::from_str(raw).map_err(|e| format!("parse model_enabled: {e}"))?;
    let keep: std::collections::HashSet<&str> = keep_ids.iter().map(String::as_str).collect();
    map.retain(|k, _| keep.contains(k.as_str()));
    serde_json::to_string(&map).map_err(|e| format!("serialize model_enabled: {e}"))
}

/// Disable built-in provider when the user logs out (token no longer valid).
pub async fn disable_flowy_builtin_provider(
    provider_repo: &Arc<dyn IProviderRepository>,
) -> Result<(), String> {
    if provider_repo
        .find_by_id(FLOWY_BUILTIN_PROVIDER_ID)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        provider_repo
            .update(
                FLOWY_BUILTIN_PROVIDER_ID,
                UpdateProviderParams {
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use nomifun_db::{CreateProviderParams, SqliteProviderModelRepository, SqliteProviderRepository};

    #[test]
    fn promote_default_model_reorders_when_present() {
        let mut ids = vec!["AIPC-b".into(), "AIPC-a".into(), "AIPC-glm-4.7".into()];
        promote_default_model(&mut ids, "AIPC-glm-4.7");
        assert_eq!(
            ids,
            vec![
                "AIPC-glm-4.7".to_string(),
                "AIPC-b".to_string(),
                "AIPC-a".to_string()
            ]
        );
    }

    #[test]
    fn promote_default_model_does_not_reinject_delisted_default() {
        let mut ids = vec!["AIPC-b".into(), "AIPC-a".into()];
        promote_default_model(&mut ids, "AIPC-glm-4.7");
        assert_eq!(ids, vec!["AIPC-b".to_string(), "AIPC-a".to_string()]);
    }

    #[test]
    fn empty_server_catalog_does_not_invent_a_default_model() {
        let server = ServerConfig {
            llm: nomi_config::ServerLlmConfig {
                default_model: "AIPC-configured-default".into(),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(build_model_fields(&[], &server).model_ids.is_empty());
    }

    #[test]
    fn build_model_fields_drops_ids_not_in_server_catalog() {
        let server = ServerConfig {
            llm: nomi_config::ServerLlmConfig {
                default_model: "AIPC-delisted".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let entries = vec![
            ClawModelEntry {
                id: "AIPC-keep".into(),
                name: "Keep".into(),
                extra: String::new(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
            },
            ClawModelEntry {
                id: "AIPC-also".into(),
                name: "Also".into(),
                extra: String::new(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
            },
        ];
        let fields = build_model_fields(&entries, &server);
        assert_eq!(
            fields.model_ids,
            vec!["AIPC-also".to_string(), "AIPC-keep".to_string()]
        );
        assert!(!fields.model_ids.iter().any(|id| id == "AIPC-delisted"));
        assert_eq!(fields.context_limits_json, "{}");
    }

    #[test]
    fn build_model_fields_projects_context_window_from_extra() {
        let server = ServerConfig::default();
        let entries = vec![
            ClawModelEntry {
                id: "AIPC-qwen-long".into(),
                name: "qwen-long".into(),
                extra: r#"{"input":["text","image"],"reasoning":false,"tools":true,"context_window":200000,"credit_rate":1}"#.into(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
            },
            ClawModelEntry {
                id: "AIPC-tiny".into(),
                name: "tiny".into(),
                extra: r#"{"input":["text"],"reasoning":true,"tools":false,"context_window":32000,"credit_rate":0.5}"#.into(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
            },
            ClawModelEntry {
                id: "AIPC-no-extra".into(),
                name: "no-extra".into(),
                extra: String::new(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
            },
        ];
        let fields = build_model_fields(&entries, &server);
        let limits: HashMap<String, i64> =
            serde_json::from_str(&fields.context_limits_json).unwrap();
        assert_eq!(limits.get("AIPC-qwen-long"), Some(&200_000));
        assert_eq!(limits.get("AIPC-tiny"), Some(&32_000));
        assert!(!limits.contains_key("AIPC-no-extra"));
    }

    #[test]
    fn prune_model_enabled_removes_delisted_keys() {
        let raw = r#"{"AIPC-keep":true,"AIPC-gone":false}"#;
        let pruned = prune_model_enabled_json(Some(raw), &["AIPC-keep".into()]).unwrap();
        let map: HashMap<String, bool> = serde_json::from_str(&pruned).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("AIPC-keep"), Some(&true));
        assert!(!map.contains_key("AIPC-gone"));
    }

    #[tokio::test]
    async fn cloud_sync_backfills_chat_profiles_without_overwriting_user_edits() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let provider_repo = SqliteProviderRepository::new(database.pool().clone());
        let provider_model_repo: Arc<dyn IProviderModelRepository> =
            Arc::new(SqliteProviderModelRepository::new(database.pool().clone()));

        provider_repo
            .create(CreateProviderParams {
                provider_id: Some(FLOWY_BUILTIN_PROVIDER_ID),
                platform: "openai",
                name: "Flowy Cloud",
                base_url: "https://example.test/v1",
                api_key_encrypted: "ciphertext",
                models: r#"["gpt-4o"]"#,
                enabled: true,
                model_context_limits: None,
                model_protocols: None,
                model_descriptions: None,
                model_enabled: None,
                bedrock_config: None,
                is_full_url: false,
                sort_order: Some(0),
            })
            .await
            .unwrap();

        let seeds = build_profile_seeds(&["gpt-4o".to_string()], "openai").unwrap();
        provider_repo
            .update_with_model_profiles(
                FLOWY_BUILTIN_PROVIDER_ID,
                UpdateProviderParams::default(),
                &seeds,
                provider_model_repo.as_ref(),
            )
            .await
            .unwrap();
        let row = provider_model_repo
            .get(FLOWY_BUILTIN_PROVIDER_ID, "gpt-4o")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.tasks, r#"["chat"]"#);
        assert_eq!(row.source, "inferred");

        let user_tasks = r#"["image_generation"]"#;
        let user_traits = "[]";
        let user_source = "user";
        provider_model_repo
            .update(
                FLOWY_BUILTIN_PROVIDER_ID,
                "gpt-4o",
                &ProviderModelUpdate {
                    tasks: Some(user_tasks),
                    traits: Some(user_traits),
                    source: Some(user_source),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        provider_repo
            .update_with_model_profiles(
                FLOWY_BUILTIN_PROVIDER_ID,
                UpdateProviderParams::default(),
                &seeds,
                provider_model_repo.as_ref(),
            )
            .await
            .unwrap();
        let user_row = provider_model_repo
            .get(FLOWY_BUILTIN_PROVIDER_ID, "gpt-4o")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user_row.tasks, user_tasks);
        assert_eq!(user_row.source, user_source);
    }
}
