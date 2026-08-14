//! Sync the logged-in Flowy JWT and server model catalog into the built-in provider row.
//!
//! The local `providers.models` JSON is a **projection** of the upstream
//! `availableListClaw` catalog. On a successful catalog fetch it is fully
//! replaced (delisted models must disappear). Transient fetch failures must
//! **not** wipe or invent models — that left stale delisted entries in place
//! when callers ignored soft errors, and previously also re-injected a config
//! default that the server no longer lists.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nomifun_api_types::{ModelTrait, derive_tasks_and_traits};
use nomi_config::ServerConfig;
use nomifun_common::encrypt_string;
use nomifun_db::{
    CreateProviderParams, IProviderModelRepository, IProviderRepository, ProviderModelProfileSeed,
    UpdateProviderParams,
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
    let (catalog_entries, catalog_fields, catalog_synced) = match fetch_chat_models(server, &session, data_dir).await {
        Ok(entries) => {
            let fields = build_model_fields(&entries, server);
            (Some(entries), Some(fields), true)
        }
        Err(e) => {
            warn!(
                "Failed to fetch Flowy chat model catalog: {e}; keeping existing local model list"
            );
            (None, None, false)
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

    let profile_seeds = catalog_entries
        .as_deref()
        .map(|entries| build_profile_seeds(entries, "openai"))
        .transpose()?
        .unwrap_or_default();
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
    entries: &[ClawModelEntry],
    platform: &str,
) -> Result<Vec<ProviderModelProfileSeed>, String> {
    let mut seen = HashSet::new();
    let mut profiles = Vec::new();

    for entry in entries {
        let model = entry.api_model_id();
        if model.trim().is_empty() || !seen.insert(model.clone()) {
            continue;
        }
        let extra = entry.model_extra();
        let (mut tasks, mut traits) = derive_tasks_and_traits(platform, &model);
        // Catalog `extra` is authoritative for Flowy chat capabilities that
        // name heuristics cannot see (reasoning + selectable effort levels).
        if extra.reasoning && !traits.contains(&nomifun_api_types::ModelTrait::Reasoning) {
            traits.push(nomifun_api_types::ModelTrait::Reasoning);
        }
        if extra.tools && !traits.contains(&nomifun_api_types::ModelTrait::FunctionCalling) {
            traits.push(nomifun_api_types::ModelTrait::FunctionCalling);
        }
        let supports_vision = extra.supports_vision();
        traits.retain(|trait_value| *trait_value != ModelTrait::VisionInput);
        if supports_vision {
            traits.push(nomifun_api_types::ModelTrait::VisionInput);
        }
        if tasks.is_empty() {
            tasks.push(nomifun_api_types::ModelTask::Chat);
        }
        profiles.push(ProviderModelProfileSeed {
            model,
            tasks: serde_json::to_string(&tasks)
                .map_err(|error| format!("serialize Flowy model tasks: {error}"))?,
            traits: serde_json::to_string(&traits)
                .map_err(|error| format!("serialize Flowy model traits: {error}"))?,
            catalog_max_tokens: extra.max_output_tokens(),
            catalog_reasoning_effort: extra.reasoning_effort_levels(),
            catalog_credit_rate: extra.credit_rate_multiplier(),
            catalog_vision: Some(supports_vision),
        });
    }

    Ok(profiles)
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
    for entry in &resp.cloud {
        info!(
            model = %entry.api_model_id(),
            max_tokens = ?entry.model_extra().max_output_tokens(),
            "Flowy cloud catalog model output limit"
        );
    }
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

    use nomifun_db::{
        CreateProviderParams, ProviderModelUpdate, SqliteProviderModelRepository,
        SqliteProviderRepository, FLOWY_CATALOG_MAX_TOKENS_PARAM,
    };

    fn catalog_entry(id: &str, extra: &str) -> ClawModelEntry {
        ClawModelEntry {
            id: id.into(),
            name: id.into(),
            extra: extra.into(),
            endpoint: String::new(),
            anthropic_endpoint: String::new(),
            icon: String::new(),
            category: 1,
        }
    }

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
    fn build_profile_seeds_projects_catalog_credit_rate() {
        let entries = vec![
            catalog_entry(
                "AIPC-qwen-long",
                r#"{"credit_rate":1,"context_window":200000}"#,
            ),
            catalog_entry("AIPC-tiny", r#"{"credit_rate":0.5}"#),
            catalog_entry("AIPC-invalid", r#"{"credit_rate":0}"#),
            catalog_entry("AIPC-missing", "{}"),
        ];

        let seeds = build_profile_seeds(&entries, "openai").unwrap();
        assert_eq!(seeds[0].catalog_credit_rate, Some(1.0));
        assert_eq!(seeds[1].catalog_credit_rate, Some(0.5));
        assert_eq!(seeds[2].catalog_credit_rate, None);
        assert_eq!(seeds[3].catalog_credit_rate, None);
    }

    #[test]
    fn build_profile_seeds_projects_catalog_max_tokens() {
        let entries = vec![
            catalog_entry("AIPC-gpt-4o", r#"{"max_tokens":4096}"#),
            // Duplicate catalog ids do not create duplicate profile rows; the
            // first server entry remains authoritative for the sync.
            catalog_entry("AIPC-gpt-4o", r#"{"max_tokens":8192}"#),
            catalog_entry("AIPC-no-output-limit", r#"{"max_tokens":0}"#),
        ];

        let seeds = build_profile_seeds(&entries, "openai").unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].model, "AIPC-gpt-4o");
        assert_eq!(seeds[0].catalog_max_tokens, Some(4096));
        assert_eq!(seeds[1].model, "AIPC-no-output-limit");
        assert_eq!(seeds[1].catalog_max_tokens, None);
    }

    #[test]
    fn build_profile_seeds_projects_reasoning_effort_when_reasoning() {
        let entries = vec![
            catalog_entry(
                "AIPC-think",
                r#"{"reasoning":true,"reasoning_effort":["low","medium","xhigh"],"tools":true}"#,
            ),
            catalog_entry(
                "AIPC-no-think",
                r#"{"reasoning":false,"reasoning_effort":["low","medium"]}"#,
            ),
        ];

        let seeds = build_profile_seeds(&entries, "openai").unwrap();
        assert_eq!(
            seeds[0].catalog_reasoning_effort.as_deref(),
            Some(["low".to_owned(), "medium".to_owned(), "xhigh".to_owned()].as_slice())
        );
        let traits: Vec<String> = serde_json::from_str(&seeds[0].traits).unwrap();
        assert!(traits.iter().any(|t| t == "reasoning"));
        assert!(traits.iter().any(|t| t == "function_calling"));
        assert_eq!(seeds[1].catalog_reasoning_effort, None);
        let traits_no: Vec<String> = serde_json::from_str(&seeds[1].traits).unwrap();
        assert!(!traits_no.iter().any(|t| t == "reasoning"));
    }

    #[test]
    fn build_profile_seeds_uses_catalog_image_input_for_vision_trait() {
        let seeds = build_profile_seeds(
            &[
                catalog_entry("vision", r#"{"input":["text","image"]}"#),
                catalog_entry("text-only", r#"{"input":["text"]}"#),
                catalog_entry("invalid-extra", "not json"),
            ],
            "openai",
        )
        .unwrap();

        assert!(seeds[0].traits.contains("vision_input"));
        assert_eq!(seeds[0].catalog_vision, Some(true));
        assert!(!seeds[1].traits.contains("vision_input"));
        assert_eq!(seeds[1].catalog_vision, Some(false));
        assert!(!seeds[2].traits.contains("vision_input"));
        assert_eq!(seeds[2].catalog_vision, Some(false));
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

        let seeds = build_profile_seeds(
            &[catalog_entry("gpt-4o", r#"{"max_tokens":4096}"#)],
            "openai",
        )
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
        let row = provider_model_repo
            .get(FLOWY_BUILTIN_PROVIDER_ID, "gpt-4o")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.tasks, r#"["chat"]"#);
        assert_eq!(row.source, "catalog");
        let params: serde_json::Value = serde_json::from_str(&row.params).unwrap();
        assert_eq!(
            params
                .get(FLOWY_CATALOG_MAX_TOKENS_PARAM)
                .and_then(serde_json::Value::as_u64),
            Some(4096)
        );

        let user_tasks = r#"["image_generation"]"#;
        let user_traits = "[]";
        let user_source = "user";
        let user_params = r#"{"temperature":0.2,"_flowy_catalog_max_tokens":4096}"#;
        provider_model_repo
            .update(
                FLOWY_BUILTIN_PROVIDER_ID,
                "gpt-4o",
                &ProviderModelUpdate {
                    tasks: Some(user_tasks),
                    traits: Some(user_traits),
                    source: Some(user_source),
                    params: Some(user_params),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let updated_seeds = build_profile_seeds(
            &[catalog_entry("gpt-4o", r#"{"max_tokens":2048}"#)],
            "openai",
        )
        .unwrap();
        provider_repo
            .update_with_model_profiles(
                FLOWY_BUILTIN_PROVIDER_ID,
                UpdateProviderParams::default(),
                &updated_seeds,
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
        let params: serde_json::Value = serde_json::from_str(&user_row.params).unwrap();
        assert_eq!(params.get("temperature"), Some(&serde_json::json!(0.2)));
        assert_eq!(
            params
                .get(FLOWY_CATALOG_MAX_TOKENS_PARAM)
                .and_then(serde_json::Value::as_u64),
            Some(2048)
        );

        let no_limit_seeds =
            build_profile_seeds(&[catalog_entry("gpt-4o", "{}")], "openai").unwrap();
        provider_repo
            .update_with_model_profiles(
                FLOWY_BUILTIN_PROVIDER_ID,
                UpdateProviderParams::default(),
                &no_limit_seeds,
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
        let params: serde_json::Value = serde_json::from_str(&user_row.params).unwrap();
        assert_eq!(params.get("temperature"), Some(&serde_json::json!(0.2)));
        assert!(params.get(FLOWY_CATALOG_MAX_TOKENS_PARAM).is_none());
    }
}
