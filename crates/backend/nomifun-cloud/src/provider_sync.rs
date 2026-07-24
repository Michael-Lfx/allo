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

use nomi_config::ServerConfig;
use nomifun_common::encrypt_string;
use nomifun_db::{
    CreateProviderParams, IProviderRepository, UpdateProviderParams,
};
use tracing::{info, warn};

use crate::config_defaults::FLOWY_BUILTIN_PROVIDER_ID;
use crate::flowy::{ClawModelEntry, FlowyApiClient};
use crate::session::ServerSession;

const FLOWY_CAPABILITIES_JSON: &str =
    r#"[{"type":"text"},{"type":"vision"},{"type":"function_calling"}]"#;

/// Upsert Flowy Cloud provider with JWT + server model catalog for the model selector.
pub async fn sync_flowy_builtin_provider(
    provider_repo: &Arc<dyn IProviderRepository>,
    encryption_key: &[u8; 32],
    server: &ServerConfig,
    data_dir: &std::path::Path,
) -> Result<(), String> {
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
    let catalog_fields = match fetch_chat_models(server, &session, data_dir).await {
        Ok(entries) if !entries.is_empty() => Some(build_model_fields(&entries, server)),
        Ok(_) => {
            warn!(
                "Flowy server returned empty chat model catalog; clearing local projection to default model only"
            );
            Some(fallback_model_fields(server))
        }
        Err(e) => {
            warn!(
                "Failed to fetch Flowy chat model catalog: {e}; keeping existing local model list"
            );
            None
        }
    };

    let models_json = catalog_fields
        .as_ref()
        .map(|(ids, _)| serde_json::to_string(ids))
        .transpose()
        .map_err(|e| format!("serialize models: {e}"))?;
    let descriptions_json = catalog_fields
        .as_ref()
        .map(|(_, desc)| desc.as_str());

    // Drop per-model enable flags for ids no longer in the catalog so delisted
    // models cannot linger in auxiliary maps after a successful replace.
    let pruned_enabled_json = if let Some((ids, _)) = catalog_fields.as_ref() {
        let existing_enabled = provider_repo
            .find_by_id(FLOWY_BUILTIN_PROVIDER_ID)
            .await
            .map_err(|e| e.to_string())?
            .and_then(|row| row.model_enabled);
        Some(prune_model_enabled_json(existing_enabled.as_deref(), ids)?)
    } else {
        None
    };

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
                    platform: Some("openai"),
                    name: Some("Flowy Cloud"),
                    base_url: Some(&base_url),
                    api_key_encrypted: Some(&api_key_encrypted),
                    models: models_json.as_deref(),
                    enabled: Some(true),
                    capabilities: Some(FLOWY_CAPABILITIES_JSON),
                    model_descriptions: descriptions_json.map(Some),
                    model_enabled: pruned_enabled_json
                        .as_deref()
                        .map(|s| if s == "{}" { None } else { Some(s) }),
                    is_full_url: Some(false),
                    sort_order: Some(0),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let (models, descriptions) = match &catalog_fields {
            Some((ids, desc)) => (
                serde_json::to_string(ids).map_err(|e| format!("serialize models: {e}"))?,
                desc.clone(),
            ),
            None => {
                let (ids, desc) = fallback_model_fields(server);
                (
                    serde_json::to_string(&ids).map_err(|e| format!("serialize models: {e}"))?,
                    desc,
                )
            }
        };
        provider_repo
            .create(CreateProviderParams {
                id: Some(FLOWY_BUILTIN_PROVIDER_ID),
                platform: "openai",
                name: "Flowy Cloud",
                base_url: &base_url,
                api_key_encrypted: &api_key_encrypted,
                models: &models,
                enabled: true,
                capabilities: FLOWY_CAPABILITIES_JSON,
                context_limit: None,
                model_context_limits: None,
                model_protocols: None,
                model_descriptions: Some(descriptions.as_str()),
                model_enabled: None,
                model_health: None,
                bedrock_config: None,
                is_full_url: false,
                sort_order: Some(0),
            })
            .await
            .map_err(|e| e.to_string())?;
    }

    disable_non_flowy_providers(provider_repo).await?;
    let model_count = catalog_fields
        .as_ref()
        .map(|(ids, _)| ids.len())
        .unwrap_or(0);
    info!(
        flowy_models = model_count,
        catalog_replaced = catalog_fields.is_some(),
        "Synced Flowy Cloud provider from server catalog"
    );
    Ok(())
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

fn build_model_fields(
    entries: &[ClawModelEntry],
    server: &ServerConfig,
) -> (Vec<String>, String) {
    let default_model = server.effective_default_llm_model();
    let mut model_ids: Vec<String> = entries.iter().map(|e| e.api_model_id()).collect();
    model_ids.sort();
    model_ids.dedup();
    model_ids.retain(|m| !m.trim().is_empty());

    promote_default_model(&mut model_ids, &default_model);

    let mut descriptions = HashMap::new();
    for entry in entries {
        descriptions.insert(entry.api_model_id(), display_name_for_entry(entry));
    }
    for id in &model_ids {
        descriptions
            .entry(id.clone())
            .or_insert_with(|| display_name_for_id(id));
    }

    let model_descriptions_json =
        serde_json::to_string(&descriptions).unwrap_or_else(|_| "{}".to_string());
    (model_ids, model_descriptions_json)
}

fn fallback_model_fields(server: &ServerConfig) -> (Vec<String>, String) {
    let default_model = server.effective_default_llm_model();
    let model_ids = vec![default_model.clone()];
    let descriptions = HashMap::from([(
        default_model.clone(),
        display_name_for_id(&default_model),
    )]);
    let model_descriptions_json =
        serde_json::to_string(&descriptions).unwrap_or_else(|_| "{}".to_string());
    (model_ids, model_descriptions_json)
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

async fn disable_non_flowy_providers(
    provider_repo: &Arc<dyn IProviderRepository>,
) -> Result<(), String> {
    let rows = provider_repo.list().await.map_err(|e| e.to_string())?;
    for row in rows {
        if row.id == FLOWY_BUILTIN_PROVIDER_ID {
            continue;
        }
        if row.enabled {
            provider_repo
                .update(
                    &row.id,
                    UpdateProviderParams {
                        enabled: Some(false),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
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
        let (ids, _) = build_model_fields(&entries, &server);
        assert_eq!(ids, vec!["AIPC-also".to_string(), "AIPC-keep".to_string()]);
        assert!(!ids.iter().any(|id| id == "AIPC-delisted"));
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
}
