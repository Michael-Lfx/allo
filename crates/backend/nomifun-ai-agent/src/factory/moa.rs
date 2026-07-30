//! Mixture-of-Agents (MoA) configuration bridge for the nomi factory.
//!
//! Converts the wire-level [`MoaSettings`] DTO from `NomiBuildExtra` into the
//! engine-level `nomi_config::config::MoaConfig`, and resolves each reference
//! slot's provider row into a ready `MoaResolvedSlot`. The bridge is
//! fail-soft: an unresolvable slot (deleted provider, bad key) is skipped
//! with a warning so a stale MoA setting can never break session build; when
//! nothing resolves, MoA is simply not enabled.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_agent::moa::MoaResolvedSlot;
use nomi_config::config::{MoaConfig, MoaSlot};
use nomifun_api_types::{MoaSettings, NomiBuildExtra};
use nomifun_db::IProviderRepository;
use tracing::warn;

use crate::types::{MoaSlotPrice, ResolvedMoaBridge};

/// Whether this session should run the MoA fan-out at all: settings present,
/// master switch on, at least one reference slot configured, and NOT a
/// companion session (companions stay single-model by product decision).
pub(crate) fn moa_enabled_for_session(extra: &NomiBuildExtra) -> bool {
    !extra.companion
        && extra
            .moa
            .as_ref()
            .is_some_and(|moa| moa.enabled && !moa.references.is_empty())
}

/// Convert the wire DTO into the engine config. Unset optional knobs fall
/// back to the `MoaConfig::default()` values (fanout `"user_turn"`,
/// timeout 120s, reference max tokens 4096). Slot temperatures are clamped
/// to the provider-safe [0.0, 2.0] range — non-UI clients can write
/// arbitrary values that would otherwise reach the provider and 4xx.
pub(crate) fn moa_settings_to_config(settings: &MoaSettings) -> MoaConfig {
    let defaults = MoaConfig::default();
    MoaConfig {
        enabled: settings.enabled,
        references: settings
            .references
            .iter()
            .map(|slot| MoaSlot {
                provider_id: slot.provider_id.clone(),
                model: slot.model.clone(),
                max_tokens: slot.max_tokens,
                temperature: slot.temperature.map(|v| v.clamp(0.0, 2.0)),
            })
            .collect(),
        fanout: settings.fanout.clone().unwrap_or(defaults.fanout),
        reference_timeout_secs: settings
            .reference_timeout_secs
            .unwrap_or(defaults.reference_timeout_secs),
        reference_max_tokens: settings
            .reference_max_tokens
            .unwrap_or(defaults.reference_max_tokens),
        privacy_filter: settings
            .privacy_filter
            .clone()
            .unwrap_or(defaults.privacy_filter),
        trace_enabled: settings.trace_enabled.unwrap_or(defaults.trace_enabled),
    }
}

/// Resolve every configured reference slot into a full provider `Config`.
/// A slot that fails to resolve is skipped (warn) — never a build error.
/// The advisor context window is taken from the provider resolution chain
/// (per-model limit / catalog) when available. Also returns the catalog
/// price (USD per million tokens) for each resolved slot — `None` costs
/// when the catalog has no entry — so the manager can report turn cost.
pub(crate) async fn resolve_moa_slots(
    provider_repo: &Arc<dyn IProviderRepository>,
    encryption_key: &[u8; 32],
    workspace: &Path,
    config: &MoaConfig,
) -> (Vec<MoaResolvedSlot>, Vec<MoaSlotPrice>) {
    let mut slots = Vec::with_capacity(config.references.len());
    let mut slot_prices = Vec::with_capacity(config.references.len());
    for slot in &config.references {
        let resolved = super::provider_config::resolve_provider_config(
            provider_repo,
            encryption_key,
            &slot.provider_id,
            &slot.model,
            workspace,
        )
        .await;
        let provider_config = match resolved {
            Ok(cfg) => cfg,
            Err(error) => {
                warn!(
                    provider_id = %slot.provider_id,
                    model = %slot.model,
                    error = %error,
                    "Skipping unresolvable MoA reference slot"
                );
                continue;
            }
        };
        // Model context window + raw platform from the same resolution chain
        // (per-model limit / catalog). Fetched separately because the Config
        // does not carry them; failure here just means "unknown".
        let fields = super::provider_config::resolve_provider_fields(
            provider_repo,
            encryption_key,
            &slot.provider_id,
            &slot.model,
        )
        .await
        .ok();
        let context_window_tokens = fields
            .as_ref()
            .and_then(|fields| fields.context_limit)
            .filter(|limit| *limit > 0)
            .map(|limit| limit as u64);
        // Catalog price lookup keyed by the provider row's raw platform name
        // (models.dev key) — NOT the mapped nomi provider name. Missing
        // catalog entry / price → None (tokens reported without cost).
        let (cost_input, cost_output) = fields
            .as_ref()
            .map(|fields| catalog_slot_prices(&fields.platform, &slot.model))
            .unwrap_or((None, None));
        let label = format!("{}/{}", slot.provider_id, slot.model);
        slot_prices.push(MoaSlotPrice {
            label: label.clone(),
            cost_input,
            cost_output,
        });
        slots.push(MoaResolvedSlot {
            config: provider_config,
            label,
            max_tokens: slot.max_tokens,
            temperature: slot.temperature,
            context_window_tokens,
        });
    }
    (slots, slot_prices)
}

/// models.dev catalog price (USD per million tokens) for one slot. Uses the
/// process-wide cached client; any lookup failure degrades to `(None, None)`.
fn catalog_slot_prices(platform: &str, model: &str) -> (Option<f64>, Option<f64>) {
    match nomifun_models_dev::resolve_catalog_capabilities(
        nomifun_models_dev::default_client(),
        platform,
        model,
    ) {
        Some(caps) => (caps.cost_input, caps.cost_output),
        None => (None, None),
    }
}

/// Full bridge: gate on the session shape, convert the DTO, resolve slots.
/// Returns `None` (MoA off, zero behavior change) when the gate fails or no
/// slot resolves. When `trace_enabled`, the trace file path is resolved as
/// `<data_dir>/moa-trace/<conversation_id>.jsonl` (file is only created on
/// first write by the sink).
pub(crate) async fn resolve_moa_bridge(
    extra: &NomiBuildExtra,
    provider_repo: &Arc<dyn IProviderRepository>,
    encryption_key: &[u8; 32],
    workspace: &Path,
    data_dir: &Path,
    conversation_id: &str,
) -> Option<ResolvedMoaBridge> {
    if !moa_enabled_for_session(extra) {
        return None;
    }
    let config = moa_settings_to_config(extra.moa.as_ref()?);
    let (slots, slot_prices) =
        resolve_moa_slots(provider_repo, encryption_key, workspace, &config).await;
    if slots.is_empty() {
        warn!("MoA is configured but no reference slot resolved; running single-model");
        return None;
    }
    let trace_path = config
        .trace_enabled
        .then(|| moa_trace_path(data_dir, conversation_id));
    Some(ResolvedMoaBridge {
        config,
        slots,
        slot_prices,
        trace_path,
    })
}

/// Trace file location for one conversation: `<data_dir>/moa-trace/<id>.jsonl`.
/// The id is sanitized to `[A-Za-z0-9_-]` (anything else → `_`) so a hostile
/// or malformed conversation id can never traverse out of the trace dir.
fn moa_trace_path(data_dir: &Path, conversation_id: &str) -> PathBuf {
    let sanitized: String = conversation_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir
        .join("moa-trace")
        .join(format!("{sanitized}.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::MoaSlotSetting;

    fn settings(references: Vec<MoaSlotSetting>) -> MoaSettings {
        MoaSettings {
            enabled: true,
            references,
            fanout: None,
            reference_timeout_secs: None,
            reference_max_tokens: None,
            privacy_filter: None,
            trace_enabled: None,
        }
    }

    fn one_slot() -> Vec<MoaSlotSetting> {
        vec![MoaSlotSetting {
            provider_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            model: "model-x".into(),
            max_tokens: None,
            temperature: None,
        }]
    }

    #[test]
    fn dto_conversion_backfills_defaults() {
        let converted = moa_settings_to_config(&settings(one_slot()));
        assert!(converted.enabled);
        assert_eq!(converted.fanout, "user_turn");
        assert_eq!(converted.reference_timeout_secs, 120);
        assert_eq!(converted.reference_max_tokens, 4096);
        assert_eq!(converted.references.len(), 1);
        assert_eq!(converted.references[0].model, "model-x");
        // Unset second-phase knobs fall back to engine defaults (off).
        assert_eq!(converted.privacy_filter, "");
        assert!(!converted.trace_enabled);
    }

    #[test]
    fn dto_conversion_passes_privacy_and_trace_through() {
        let mut dto = settings(one_slot());
        dto.privacy_filter = Some("basic".into());
        dto.trace_enabled = Some(true);
        let converted = moa_settings_to_config(&dto);
        assert_eq!(converted.privacy_filter, "basic");
        assert!(converted.trace_enabled);
    }

    #[test]
    fn trace_path_is_scoped_per_conversation() {
        let path = moa_trace_path(Path::new("/data"), "conv-42");
        assert_eq!(
            path,
            Path::new("/data").join("moa-trace").join("conv-42.jsonl")
        );
    }

    #[test]
    fn trace_path_sanitizes_hostile_conversation_ids() {
        // `../` and separators must never escape the moa-trace directory.
        let base = Path::new("/data");
        let path = moa_trace_path(base, "../../etc/passwd");
        assert_eq!(
            path,
            base.join("moa-trace").join("______etc_passwd.jsonl")
        );
        // The sanitized file name is a single component: no separators left.
        let file_name = path.file_name().unwrap().to_str().unwrap();
        assert!(!file_name.contains('/') && !file_name.contains('\\'));
        assert_eq!(path.parent(), Some(base.join("moa-trace")).as_deref());

        let windows_style = moa_trace_path(base, "..\\evil");
        assert_eq!(
            windows_style,
            base.join("moa-trace").join("___evil.jsonl")
        );
    }

    #[test]
    fn dto_conversion_clamps_out_of_range_temperatures() {
        let converted = moa_settings_to_config(&settings(vec![
            MoaSlotSetting {
                provider_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
                model: "hot".into(),
                max_tokens: None,
                temperature: Some(2.5),
            },
            MoaSlotSetting {
                provider_id: "0190f5fe-7c00-7a00-8000-000000000002".into(),
                model: "cold".into(),
                max_tokens: None,
                temperature: Some(-0.5),
            },
            MoaSlotSetting {
                provider_id: "0190f5fe-7c00-7a00-8000-000000000003".into(),
                model: "unset".into(),
                max_tokens: None,
                temperature: None,
            },
        ]));
        assert_eq!(converted.references[0].temperature, Some(2.0));
        assert_eq!(converted.references[1].temperature, Some(0.0));
        assert_eq!(converted.references[2].temperature, None);
    }

    #[test]
    fn dto_conversion_keeps_explicit_values() {
        let mut dto = settings(vec![MoaSlotSetting {
            provider_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            model: "model-x".into(),
            max_tokens: Some(1024),
            temperature: Some(0.3),
        }]);
        dto.fanout = Some("every_n:3".into());
        dto.reference_timeout_secs = Some(30);
        dto.reference_max_tokens = Some(2048);
        let converted = moa_settings_to_config(&dto);
        assert_eq!(converted.fanout, "every_n:3");
        assert_eq!(converted.reference_timeout_secs, 30);
        assert_eq!(converted.reference_max_tokens, 2048);
        assert_eq!(converted.references[0].max_tokens, Some(1024));
        assert_eq!(converted.references[0].temperature, Some(0.3));
    }

    #[test]
    fn companion_sessions_never_enable_moa() {
        let extra = NomiBuildExtra {
            companion: true,
            moa: Some(settings(one_slot())),
            ..Default::default()
        };
        assert!(!moa_enabled_for_session(&extra));

        // The same settings on a non-companion session pass the gate.
        let extra = NomiBuildExtra {
            moa: Some(settings(one_slot())),
            ..Default::default()
        };
        assert!(moa_enabled_for_session(&extra));
    }

    #[test]
    fn gate_requires_enabled_and_references() {
        assert!(!moa_enabled_for_session(&NomiBuildExtra::default()));

        let disabled = NomiBuildExtra {
            moa: Some(MoaSettings {
                enabled: false,
                ..settings(one_slot())
            }),
            ..Default::default()
        };
        assert!(!moa_enabled_for_session(&disabled));

        let empty_refs = NomiBuildExtra {
            moa: Some(settings(Vec::new())),
            ..Default::default()
        };
        assert!(!moa_enabled_for_session(&empty_refs));
    }
}
