//! Single EffectiveModel resolution order for runtime construction.
//!
//! Precedence (first non-empty wins):
//! 1. request / send overrides (`use_model` preferred over `model`)
//! 2. conversation.model JSON (authoritative row)
//! 3. template lead participant snapshot model
//! 4. preset model preferences (already ordered)
//! 5. optional catalog fallback when the caller allows it
//!
//! Callers must not invent a parallel precedence. Failover `use_model` is
//! expressed as an override at layer 1.

use nomifun_common::{AppError, ProviderWithModel};
use nomifun_db::models::ConversationRow;

use crate::runtime_options::provider_model_from_conversation_row;

/// Inputs for [`resolve_effective_model`]. Empty layers are skipped.
#[derive(Debug, Clone, Default)]
pub struct EffectiveModelLayers {
    /// Explicit override from the current request or failover seam.
    pub override_model: Option<ProviderWithModel>,
    /// Optional template lead snapshot after create-time resolve.
    pub template_lead_model: Option<ProviderWithModel>,
    /// First successful preset preference (already filtered by PresetService).
    pub preset_model: Option<ProviderWithModel>,
    /// Last-resort catalog pick when the preset permits fallback.
    pub catalog_fallback: Option<ProviderWithModel>,
}

/// Resolve the model that should drive Agent runtime construction.
pub fn resolve_effective_model(
    conversation: &ConversationRow,
    layers: EffectiveModelLayers,
) -> Result<Option<ProviderWithModel>, AppError> {
    if let Some(model) = layers.override_model {
        return Ok(Some(prefer_use_model(model)));
    }
    if let Some(model) = provider_model_from_conversation_row(conversation)? {
        return Ok(Some(prefer_use_model(model)));
    }
    if let Some(model) = layers.template_lead_model {
        return Ok(Some(prefer_use_model(model)));
    }
    if let Some(model) = layers.preset_model {
        return Ok(Some(prefer_use_model(model)));
    }
    Ok(layers.catalog_fallback.map(prefer_use_model))
}

fn prefer_use_model(mut model: ProviderWithModel) -> ProviderWithModel {
    if let Some(use_model) = model
        .use_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        model.model = use_model.to_owned();
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_common::ConversationId;

    const PROVIDER_ID: &str = "prov_0190f5fe-7c00-7a00-8000-000000000001";

    fn row(model: Option<&str>) -> ConversationRow {
        ConversationRow {
            id: ConversationId::new().into_string(),
            user_id: "user-1".into(),
            name: "test".into(),
            r#type: "nomi".into(),
            model: model.map(ToOwned::to_owned),
            extra: "{}".into(),
            delegation_policy: "automatic".into(),
            execution_model_pool: None,
            decision_policy: "automatic".into(),
            execution_template_id: None,
            status: None,
            source: None,
            channel_chat_id: None,
            pinned: false,
            pinned_at: None,
            cron_job_id: None,
            preset_id: None,
            preset_revision: None,
            preset_snapshot: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn pm(model: &str) -> ProviderWithModel {
        ProviderWithModel {
            provider_id: PROVIDER_ID.to_owned(),
            model: model.to_owned(),
            use_model: None,
        }
    }

    #[test]
    fn override_beats_conversation() {
        let conversation = row(Some(&format!(
            r#"{{"provider_id":"{PROVIDER_ID}","model":"conv-model"}}"#
        )));
        let resolved = resolve_effective_model(
            &conversation,
            EffectiveModelLayers {
                override_model: Some(pm("override-model")),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.model, "override-model");
    }

    #[test]
    fn conversation_beats_preset() {
        let conversation = row(Some(&format!(
            r#"{{"provider_id":"{PROVIDER_ID}","model":"conv-model"}}"#
        )));
        let resolved = resolve_effective_model(
            &conversation,
            EffectiveModelLayers {
                preset_model: Some(pm("preset-model")),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.model, "conv-model");
    }

    #[test]
    fn use_model_override_wins_inside_layer() {
        let conversation = row(None);
        let resolved = resolve_effective_model(
            &conversation,
            EffectiveModelLayers {
                override_model: Some(ProviderWithModel {
                    provider_id: PROVIDER_ID.to_owned(),
                    model: "base".into(),
                    use_model: Some("failover".into()),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.model, "failover");
    }
}
