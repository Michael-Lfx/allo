//! Sync the logged-in Flowy JWT and server model catalog into the built-in provider row.
//!
//! The local `providers.models` JSON is a **projection** of the upstream
//! `/api/v2/model/availableListClaw` catalog (chat `category=1`, ASR `category=7`, TTS
//! `category=8`).
//! On a successful chat catalog fetch it is fully replaced (delisted models
//! must disappear). Transient chat-catalog failures must **not** wipe or invent
//! models. A failed ASR or TTS fetch is soft: chat models still replace, and
//! the missing modality is omitted until the next successful pull.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nomifun_api_types::{ModelTask, ModelTrait, derive_tasks_and_traits};
use nomi_config::ServerConfig;
use nomifun_common::encrypt_string;
use nomifun_db::{
    CreateProviderParams, IProviderModelRepository, IProviderRepository, ProviderModelProfileSeed,
    UpdateProviderParams,
};
use tracing::{info, warn};

use crate::config_defaults::FLOWY_BUILTIN_PROVIDER_ID;
use crate::flowy::{ClawModelEntry, FlowyApiClient, MODEL_CATEGORY_ASR, MODEL_CATEGORY_TTS};
use crate::session::ServerSession;

const CATALOG_FAMILY_AUTO: &str = "auto";
const CATALOG_FAMILY_CLOUD: &str = "cloud";

const AUTO_MODEL_INTELLIGENCE: &str = "AIPC-auto-intelligence";
const AUTO_MODEL_BALANCE: &str = "AIPC-auto-balance";
const AUTO_MODEL_COST: &str = "AIPC-auto-cost";

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
    let (catalog_entries, catalog_fields, catalog_synced) =
        match fetch_catalog_models(server, &session, data_dir).await {
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
    let reasoning_effort_model_count = profile_seeds
        .iter()
        .filter(|seed| seed.catalog_reasoning_effort.is_some())
        .count();
    let auto_model_count = profile_seeds
        .iter()
        .filter(|seed| seed.catalog_family.as_deref() == Some(CATALOG_FAMILY_AUTO))
        .count();
    info!(
        flowy_models = model_count,
        auto_models = auto_model_count,
        catalog_replaced = catalog_fields.is_some(),
        reasoning_effort_models = reasoning_effort_model_count,
        "[reasoning-effort-diagnosis] Synced Flowy Cloud provider from server catalog"
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
        let is_asr = entry.category == MODEL_CATEGORY_ASR;
        let is_tts = entry.category == MODEL_CATEGORY_TTS;
        let (mut tasks, mut traits) = if is_asr {
            (vec![ModelTask::SpeechRecognition], Vec::new())
        } else if is_tts {
            (vec![ModelTask::SpeechSynthesis], Vec::new())
        } else {
            derive_tasks_and_traits(platform, &model)
        };
        // Catalog `extra` is authoritative for Flowy chat capabilities that
        // name heuristics cannot see (reasoning + selectable effort levels).
        if !is_asr && !is_tts {
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
                tasks.push(ModelTask::Chat);
            }
        }
        let catalog_vision = if is_asr || is_tts {
            None
        } else {
            Some(extra.supports_vision())
        };
        profiles.push(ProviderModelProfileSeed {
            model,
            tasks: serde_json::to_string(&tasks)
                .map_err(|error| format!("serialize Flowy model tasks: {error}"))?,
            traits: serde_json::to_string(&traits)
                .map_err(|error| format!("serialize Flowy model traits: {error}"))?,
            catalog_max_tokens: extra.max_output_tokens(),
            catalog_reasoning_effort: extra.reasoning_effort_levels(),
            catalog_credit_rate: extra.credit_rate_multiplier(),
            catalog_vision,
            catalog_family: if is_asr || is_tts {
                None
            } else {
                Some(
                    entry
                        .catalog_family
                        .clone()
                        .unwrap_or_else(|| CATALOG_FAMILY_CLOUD.to_string()),
                )
            },
            catalog_auto_tier: if is_asr || is_tts {
                None
            } else if entry.catalog_family.as_deref() == Some(CATALOG_FAMILY_AUTO) {
                entry.catalog_auto_tier.clone()
            } else {
                None
            },
        });
    }

    Ok(profiles)
}

async fn fetch_catalog_models(
    server: &ServerConfig,
    session: &ServerSession,
    data_dir: &std::path::Path,
) -> Result<Vec<ClawModelEntry>, String> {
    let _ = data_dir;
    let api = FlowyApiClient::new(server).map_err(|e| e.to_string())?;
    let (chat_res, asr_res, tts_res) = tokio::join!(
        api.get_available_models_claw(session, None),
        api.get_available_models_claw(session, Some(MODEL_CATEGORY_ASR)),
        api.get_available_models_claw(session, Some(MODEL_CATEGORY_TTS)),
    );
    let chat = chat_res.map_err(|e| e.to_string())?;
    for entry in chat.auto.iter().chain(chat.cloud.iter()) {
        let raw_extra = serde_json::from_str::<serde_json::Value>(&entry.extra).ok();
        let raw_reasoning_effort = raw_extra
            .as_ref()
            .and_then(|value| value.get("reasoning_effort"));
        let raw_reasoning_effort_kind = raw_reasoning_effort.map_or("missing", |value| match value {
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Null => "null",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::Object(_) => "object",
        });
        let extra = entry.model_extra();
        info!(
            diagnosis = "reasoning_effort",
            model = %entry.api_model_id(),
            max_tokens = ?extra.max_output_tokens(),
            raw_extra_present = !entry.extra.trim().is_empty(),
            raw_extra_json_valid = raw_extra.is_some(),
            raw_reasoning_present = raw_extra
                .as_ref()
                .and_then(|value| value.get("reasoning"))
                .is_some(),
            parsed_reasoning = extra.reasoning,
            raw_reasoning_effort_present = raw_reasoning_effort.is_some(),
            raw_reasoning_effort_kind,
            parsed_reasoning_effort = ?extra.reasoning_effort,
            catalog_reasoning_effort = ?extra.reasoning_effort_levels(),
            "[reasoning-effort-diagnosis] Flowy cloud catalog model capabilities"
        );
    }
    let asr = match asr_res {
        Ok(resp) => resp.cloud,
        Err(e) => {
            warn!("Failed to fetch Flowy ASR catalog (category=7): {e}; syncing without ASR models");
            Vec::new()
        }
    };
    let tts = match tts_res {
        Ok(resp) => resp.cloud,
        Err(e) => {
            warn!("Failed to fetch Flowy TTS catalog (category=8): {e}; syncing without TTS models");
            Vec::new()
        }
    };
    let chat_entries = merge_chat_catalogs(chat.auto, chat.cloud);
    Ok(merge_catalog_modalities(chat_entries, asr, tts))
}

fn merge_chat_catalogs(auto: Vec<ClawModelEntry>, cloud: Vec<ClawModelEntry>) -> Vec<ClawModelEntry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for mut entry in auto {
        entry.category = 1;
        entry.catalog_family = Some(CATALOG_FAMILY_AUTO.to_string());
        entry.catalog_auto_tier = auto_tier_for_model_id(&entry.api_model_id()).map(str::to_owned);
        if seen.insert(entry.api_model_id()) {
            out.push(entry);
        }
    }

    for mut entry in cloud {
        entry.category = 1;
        entry.catalog_family = Some(CATALOG_FAMILY_CLOUD.to_string());
        entry.catalog_auto_tier = None;
        if seen.insert(entry.api_model_id()) {
            out.push(entry);
        }
    }

    out
}

fn auto_tier_for_model_id(model_id: &str) -> Option<&'static str> {
    match model_id.trim() {
        AUTO_MODEL_INTELLIGENCE => Some("intelligence"),
        AUTO_MODEL_BALANCE => Some("balance"),
        AUTO_MODEL_COST => Some("cost"),
        _ => None,
    }
}

#[cfg(test)]
fn merge_chat_and_asr_catalogs(
    chat: Vec<ClawModelEntry>,
    asr: Vec<ClawModelEntry>,
) -> Vec<ClawModelEntry> {
    merge_catalog_modalities(chat, asr, Vec::new())
}

fn merge_catalog_modalities(
    chat: Vec<ClawModelEntry>,
    asr: Vec<ClawModelEntry>,
    tts: Vec<ClawModelEntry>,
) -> Vec<ClawModelEntry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for mut entry in chat {
        if entry.category == 0 {
            entry.category = 1;
        }
        if seen.insert(entry.api_model_id()) {
            out.push(entry);
        }
    }
    append_stamped_catalog(&mut out, &mut seen, asr, MODEL_CATEGORY_ASR);
    append_stamped_catalog(&mut out, &mut seen, tts, MODEL_CATEGORY_TTS);
    out
}

fn append_stamped_catalog(
    out: &mut Vec<ClawModelEntry>,
    seen: &mut HashSet<String>,
    entries: Vec<ClawModelEntry>,
    category: i32,
) {
    for mut entry in entries {
        entry.category = category;
        if seen.insert(entry.api_model_id()) {
            out.push(entry);
        }
    }
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
        catalog_entry_with_category(id, extra, 1)
    }

    fn catalog_entry_with_category(id: &str, extra: &str, category: i32) -> ClawModelEntry {
        ClawModelEntry {
            id: id.into(),
            name: id.into(),
            extra: extra.into(),
            endpoint: String::new(),
            anthropic_endpoint: String::new(),
            icon: String::new(),
            category,
            ..Default::default()
        }
    }

    fn seed_tasks(seed: &ProviderModelProfileSeed) -> Vec<String> {
        serde_json::from_str(&seed.tasks).unwrap()
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
                ..Default::default()
            },
            ClawModelEntry {
                id: "AIPC-also".into(),
                name: "Also".into(),
                extra: String::new(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
                ..Default::default()
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
                ..Default::default()
            },
            ClawModelEntry {
                id: "AIPC-tiny".into(),
                name: "tiny".into(),
                extra: r#"{"input":["text"],"reasoning":true,"tools":false,"context_window":32000,"credit_rate":0.5}"#.into(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
                ..Default::default()
            },
            ClawModelEntry {
                id: "AIPC-no-extra".into(),
                name: "no-extra".into(),
                extra: String::new(),
                endpoint: String::new(),
                anthropic_endpoint: String::new(),
                icon: String::new(),
                category: 1,
                ..Default::default()
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
    fn merge_chat_catalogs_keeps_auto_first_and_auto_wins_duplicate_ids() {
        let auto = vec![
            catalog_entry("AIPC-auto-intelligence", r#"{"input":["text"],"tools":true}"#),
            catalog_entry("AIPC-auto-balance", r#"{"input":["text"],"tools":true}"#),
            catalog_entry("AIPC-auto-cost", r#"{"input":["text"],"tools":true}"#),
        ];
        let mut cloud = vec![catalog_entry("AIPC-auto-balance", r#"{"reasoning":true}"#)];
        cloud.extend((0..11).map(|index| catalog_entry(&format!("AIPC-cloud-{index}"), "{}")));

        let merged = merge_chat_catalogs(auto, cloud);
        assert_eq!(merged.len(), 14);
        assert_eq!(
            merged[..3]
                .iter()
                .map(ClawModelEntry::api_model_id)
                .collect::<Vec<_>>(),
            vec![
                AUTO_MODEL_INTELLIGENCE.to_owned(),
                AUTO_MODEL_BALANCE.to_owned(),
                AUTO_MODEL_COST.to_owned(),
            ]
        );
        assert_eq!(merged[1].catalog_family.as_deref(), Some(CATALOG_FAMILY_AUTO));
        assert_eq!(merged[1].catalog_auto_tier.as_deref(), Some("balance"));
        assert!(!merged.iter().skip(3).any(|entry| entry.api_model_id() == AUTO_MODEL_BALANCE));
    }

    #[test]
    fn auto_profile_seeds_are_chat_tool_capable_text_only_and_have_no_reasoning_effort() {
        let entries = merge_chat_catalogs(
            vec![
                catalog_entry("AIPC-auto-intelligence", r#"{"input":["text"],"tools":true}"#),
                catalog_entry("AIPC-auto-balance", r#"{"input":["text"],"tools":true}"#),
                catalog_entry("AIPC-auto-cost", r#"{"input":["text"],"tools":true}"#),
            ],
            Vec::new(),
        );
        let seeds = build_profile_seeds(&entries, "openai").unwrap();

        for (model, tier) in [
            (AUTO_MODEL_INTELLIGENCE, "intelligence"),
            (AUTO_MODEL_BALANCE, "balance"),
            (AUTO_MODEL_COST, "cost"),
        ] {
            let seed = seeds.iter().find(|seed| seed.model == model).unwrap();
            assert_eq!(seed_tasks(seed), vec!["chat"]);
            let traits: Vec<String> = serde_json::from_str(&seed.traits).unwrap();
            assert!(traits.iter().any(|trait_name| trait_name == "function_calling"));
            assert!(!traits.iter().any(|trait_name| trait_name == "vision_input"));
            assert_eq!(seed.catalog_reasoning_effort, None);
            assert_eq!(seed.catalog_family.as_deref(), Some(CATALOG_FAMILY_AUTO));
            assert_eq!(seed.catalog_auto_tier.as_deref(), Some(tier));
        }
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

    #[test]
    fn asr_catalog_entries_seed_speech_recognition_only() {
        let seeds = build_profile_seeds(
            &[
                catalog_entry("AIPC-glm-4.7", "{}"),
                catalog_entry_with_category("AIPC-qwen3-asr-flash", "{}", MODEL_CATEGORY_ASR),
                catalog_entry_with_category("AIPC-plain-voice", "{}", MODEL_CATEGORY_ASR),
            ],
            "openai",
        )
        .unwrap();

        let chat = seeds.iter().find(|seed| seed.model == "AIPC-glm-4.7").unwrap();
        assert_eq!(seed_tasks(chat), vec!["chat"]);

        let asr = seeds
            .iter()
            .find(|seed| seed.model == "AIPC-qwen3-asr-flash")
            .unwrap();
        assert_eq!(seed_tasks(asr), vec!["speech_recognition"]);

        let unnamed_asr = seeds
            .iter()
            .find(|seed| seed.model == "AIPC-plain-voice")
            .unwrap();
        assert_eq!(seed_tasks(unnamed_asr), vec!["speech_recognition"]);
    }

    #[test]
    fn tts_catalog_entries_seed_speech_synthesis_only() {
        let seeds = build_profile_seeds(
            &[
                catalog_entry("AIPC-glm-4.7", "{}"),
                catalog_entry_with_category("AIPC-qwen3-tts", "{}", MODEL_CATEGORY_TTS),
            ],
            "openai",
        )
        .unwrap();

        let chat = seeds.iter().find(|seed| seed.model == "AIPC-glm-4.7").unwrap();
        assert_eq!(seed_tasks(chat), vec!["chat"]);

        let tts = seeds
            .iter()
            .find(|seed| seed.model == "AIPC-qwen3-tts")
            .unwrap();
        assert_eq!(seed_tasks(tts), vec!["speech_synthesis"]);
        assert_eq!(tts.catalog_vision, None);
    }

    #[test]
    fn merge_catalog_modalities_appends_tts_after_asr() {
        let merged = merge_catalog_modalities(
            vec![catalog_entry("AIPC-glm-4.7", "{}")],
            vec![catalog_entry("AIPC-qwen3-asr-flash", "{}")],
            vec![catalog_entry("AIPC-qwen3-tts", "{}")],
        );
        assert_eq!(
            merged
                .iter()
                .map(|entry| (entry.api_model_id(), entry.category))
                .collect::<Vec<_>>(),
            vec![
                ("AIPC-glm-4.7".to_string(), 1),
                ("AIPC-qwen3-asr-flash".to_string(), MODEL_CATEGORY_ASR),
                ("AIPC-qwen3-tts".to_string(), MODEL_CATEGORY_TTS),
            ]
        );
    }

    #[test]
    fn merge_catalog_modalities_does_not_let_tts_replace_a_chat_id() {
        let merged = merge_catalog_modalities(
            vec![catalog_entry("AIPC-shared", "{}")],
            Vec::new(),
            vec![catalog_entry_with_category("AIPC-shared", "{}", MODEL_CATEGORY_TTS)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].category, 1);
    }

    #[test]
    fn merge_chat_and_asr_catalogs_stamps_asr_category_and_keeps_chat_first() {
        let merged = merge_chat_and_asr_catalogs(
            vec![catalog_entry("AIPC-glm-4.7", "{}")],
            vec![catalog_entry("AIPC-qwen3-asr-flash", "{}")],
        );
        assert_eq!(
            merged.iter().map(|entry| entry.api_model_id()).collect::<Vec<_>>(),
            vec!["AIPC-glm-4.7".to_string(), "AIPC-qwen3-asr-flash".to_string()]
        );
        assert_eq!(merged[0].category, 1);
        assert_eq!(merged[1].category, MODEL_CATEGORY_ASR);
    }

    #[test]
    fn merge_chat_and_asr_catalogs_does_not_let_asr_replace_a_chat_id() {
        let merged = merge_chat_and_asr_catalogs(
            vec![catalog_entry("AIPC-shared", "{}")],
            vec![catalog_entry_with_category("AIPC-shared", "{}", MODEL_CATEGORY_ASR)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].category, 1);
    }

    #[tokio::test]
    async fn fetch_catalog_models_pulls_category_7_and_seeds_speech_recognition_only() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-glm-4.7","name":"GLM 4.7"}]}}"#,
            ))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "7"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-qwen3-asr-flash","name":"Qwen3 ASR Flash"}]}}"#,
            ))
            .mount(&mock)
            .await;

        let config = ServerConfig {
            base_url: mock.uri(),
            ..Default::default()
        };
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test-catalog-asr") };
        let session = ServerSession::from_config(&config, tmp.path());
        let entries = fetch_catalog_models(&config, &session, tmp.path())
            .await
            .expect("catalog");
        let seeds = build_profile_seeds(&entries, "openai").unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.api_model_id(), entry.category))
                .collect::<Vec<_>>(),
            vec![
                ("AIPC-glm-4.7".to_string(), 1),
                ("AIPC-qwen3-asr-flash".to_string(), MODEL_CATEGORY_ASR),
            ]
        );
        assert_eq!(
            seed_tasks(seeds.iter().find(|s| s.model == "AIPC-glm-4.7").unwrap()),
            vec!["chat"]
        );
        assert_eq!(
            seed_tasks(
                seeds
                    .iter()
                    .find(|s| s.model == "AIPC-qwen3-asr-flash")
                    .unwrap()
            ),
            vec!["speech_recognition"]
        );
    }

    #[tokio::test]
    async fn fetch_catalog_models_keeps_chat_when_asr_catalog_fails() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-glm-4.7","name":"GLM 4.7"}]}}"#,
            ))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "7"))
            .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
            .mount(&mock)
            .await;

        let config = ServerConfig {
            base_url: mock.uri(),
            ..Default::default()
        };
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test-catalog-asr-fail") };
        let session = ServerSession::from_config(&config, tmp.path());
        let entries = fetch_catalog_models(&config, &session, tmp.path())
            .await
            .expect("chat catalog still syncs");
        assert_eq!(
            entries.iter().map(|e| e.api_model_id()).collect::<Vec<_>>(),
            vec!["AIPC-glm-4.7".to_string()]
        );
    }

    #[tokio::test]
    async fn fetch_catalog_models_pulls_category_8_and_seeds_speech_synthesis_only() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-glm-4.7","name":"GLM 4.7"}]}}"#,
            ))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "7"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[]}}"#,
            ))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/model/availableListClaw"))
            .and(query_param("category", "8"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"cloud":[{"id":"AIPC-qwen3-tts","name":"qwen3-tts","category":8}]}}"#,
            ))
            .mount(&mock)
            .await;

        let config = ServerConfig {
            base_url: mock.uri(),
            ..Default::default()
        };
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test-catalog-tts") };
        let session = ServerSession::from_config(&config, tmp.path());
        let entries = fetch_catalog_models(&config, &session, tmp.path())
            .await
            .expect("catalog");
        let seeds = build_profile_seeds(&entries, "openai").unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.api_model_id(), entry.category))
                .collect::<Vec<_>>(),
            vec![
                ("AIPC-glm-4.7".to_string(), 1),
                ("AIPC-qwen3-tts".to_string(), MODEL_CATEGORY_TTS),
            ]
        );
        assert_eq!(
            seed_tasks(seeds.iter().find(|s| s.model == "AIPC-qwen3-tts").unwrap()),
            vec!["speech_synthesis"]
        );
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
