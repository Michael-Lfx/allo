use crate::error::AuthError;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudModel {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub context_window: u64,
    #[serde(default)]
    pub max_tokens: u64,
    #[serde(default)]
    pub credit_rate: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AvailableModels {
    pub cloud: Vec<CloudModel>,
    pub effective_model_tier: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAvailableModels {
    cloud: Vec<RawCloudModel>,
    #[serde(default)]
    effective_model_tier: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCloudModel {
    #[serde(flatten)]
    model: CloudModel,
    #[serde(default)]
    extra: Value,
}

pub fn parse_available_models(value: &Value) -> Result<AvailableModels, AuthError> {
    let raw: RawAvailableModels = serde_json::from_value(value.clone())
        .map_err(|_| AuthError::Server("model catalog decode failed".into()))?;
    let cloud = raw
        .cloud
        .into_iter()
        .map(|raw| {
            let mut model = raw.model;
            if let Value::String(extra) = raw.extra {
                if let Ok(value) = serde_json::from_str::<Value>(&extra) {
                    apply_extra(&mut model, &value);
                }
            } else {
                apply_extra(&mut model, &raw.extra);
            }
            model
        })
        .filter(|model| !model.id.trim().is_empty())
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    let cloud = cloud.into_iter().filter(|model| seen.insert(model.id.clone())).collect();
    Ok(AvailableModels {
        cloud,
        effective_model_tier: raw.effective_model_tier,
    })
}

fn apply_extra(model: &mut CloudModel, extra: &Value) {
    if let Some(value) = extra.get("context_window").and_then(Value::as_u64) {
        model.context_window = value;
    }
    if let Some(value) = extra.get("max_tokens").and_then(Value::as_u64) {
        model.max_tokens = value;
    }
    if let Some(value) = extra.get("credit_rate").and_then(Value::as_f64) {
        model.credit_rate = value;
    }
}

#[cfg(test)]
mod tests {
    use super::parse_available_models;
    use serde_json::json;

    // Structural fixture: only the first entry is from the documented cloud sample.
    // Remaining IDs are synthetic and never used in production.
    fn fixture() -> serde_json::Value {
        serde_json::from_str(r#"{
            "cloud": [
                {"id":"AIPC-deepseek-v4-flash","name":"Deepseek-v4-flash","endpoint":"https://server.flowyaipc.com/claw/v1","extra":"{\"context_window\":1000000,\"max_tokens\":384000,\"credit_rate\":0.5}"},
                {"id":"fixture-2"}, {"id":"fixture-3"}, {"id":"fixture-4"},
                {"id":"fixture-5"}, {"id":"fixture-6"}, {"id":"fixture-7"},
                {"id":"fixture-8"}, {"id":"fixture-9"}, {"id":"fixture-10"},
                {"id":"fixture-11"}, {"id":"fixture-12"}
            ], "auto": [{"id":"AIPC-auto-fixture"}], "effectiveModelTier": 0
        }"#).unwrap()
    }

    #[test]
    fn parses_cloud_models_and_extra_fields() {
        let catalog = parse_available_models(&fixture()).unwrap();
        assert_eq!(catalog.cloud.len(), 12);
        assert_eq!(catalog.cloud[0].id, "AIPC-deepseek-v4-flash");
        assert_eq!(catalog.cloud[0].context_window, 1_000_000);
        assert_eq!(catalog.cloud[0].max_tokens, 384_000);
        assert_eq!(catalog.cloud[0].credit_rate, 0.5);
    }

    #[test]
    fn ignores_auto_and_missing_ids_without_panicking() {
        let value = json!({"auto": [{"id":"AIPC-auto-test"}], "cloud": [{"name":"missing"}, {"id":"ok"}]});
        let catalog = parse_available_models(&value).unwrap();
        assert_eq!(catalog.cloud.len(), 1);
        assert_eq!(catalog.cloud[0].id, "ok");
    }

    #[test]
    fn missing_cloud_is_not_a_successful_empty_snapshot() {
        assert!(parse_available_models(&json!({"auto": []})).is_err());
        assert!(parse_available_models(&json!({"cloud": null})).is_err());
        assert!(parse_available_models(&json!({"cloud": []})).unwrap().cloud.is_empty());
    }

    #[test]
    fn object_extra_and_duplicate_ids_preserve_first_server_entry() {
        let value = json!({"cloud":[
            {"id":"one","name":"first","extra":{"context_window":42,"max_tokens":12,"credit_rate":0.25}},
            {"id":"one","name":"duplicate"}
        ]});
        let catalog = parse_available_models(&value).unwrap();
        assert_eq!(catalog.cloud.len(), 1);
        assert_eq!(catalog.cloud[0].name, "first");
        assert_eq!(catalog.cloud[0].context_window, 42);
        assert_eq!(catalog.cloud[0].max_tokens, 12);
        assert_eq!(catalog.cloud[0].credit_rate, 0.25);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let catalog = parse_available_models(&json!({"cloud":[{}]})).unwrap();
        assert!(catalog.cloud.is_empty());
    }
}
