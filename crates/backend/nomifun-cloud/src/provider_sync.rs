//! Sync the logged-in Flowy JWT and server model catalog into the built-in provider row.

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

    let catalog = fetch_chat_models(server, &session, data_dir).await;
    let (model_ids, model_descriptions_json) = match catalog {
        Ok(entries) if !entries.is_empty() => build_model_fields(&entries, server),
        Ok(_) => {
            warn!("Flowy server returned empty chat model catalog; using default model only");
            fallback_model_fields(server)
        }
        Err(e) => {
            warn!("Failed to fetch Flowy chat model catalog: {e}; using default model only");
            fallback_model_fields(server)
        }
    };

    let models_json =
        serde_json::to_string(&model_ids).map_err(|e| format!("serialize models: {e}"))?;

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
                    models: Some(&models_json),
                    enabled: Some(true),
                    capabilities: Some(FLOWY_CAPABILITIES_JSON),
                    model_descriptions: Some(Some(model_descriptions_json.as_str())),
                    is_full_url: Some(false),
                    sort_order: Some(0),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;
    } else {
        provider_repo
            .create(CreateProviderParams {
                id: Some(FLOWY_BUILTIN_PROVIDER_ID),
                platform: "openai",
                name: "Flowy Cloud",
                base_url: &base_url,
                api_key_encrypted: &api_key_encrypted,
                models: &models_json,
                enabled: true,
                capabilities: FLOWY_CAPABILITIES_JSON,
                context_limit: None,
                model_context_limits: None,
                model_protocols: None,
                model_descriptions: Some(model_descriptions_json.as_str()),
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
    info!(
        flowy_models = model_ids.len(),
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

fn promote_default_model(model_ids: &mut Vec<String>, default_model: &str) {
    if default_model.trim().is_empty() {
        return;
    }
    model_ids.retain(|m| m != default_model);
    model_ids.insert(0, default_model.to_string());
    if model_ids.len() > 1 {
        model_ids.retain(|m| !m.trim().is_empty());
    }
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
