//! Compiles and applies `assets/schemas/checkpoints/checkpoint.schema.json`.

use jsonschema::Validator;
use std::sync::OnceLock;

use crate::error::{MontageError, MontageResult};

fn validator() -> MontageResult<&'static Validator> {
    static CELL: OnceLock<Result<Validator, String>> = OnceLock::new();
    let result = CELL.get_or_init(|| {
        let path = crate::assets_root()
            .join("schemas")
            .join("checkpoints")
            .join("checkpoint.schema.json");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let schema: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parsing checkpoint schema: {e}"))?;
        jsonschema::options()
            .build(&schema)
            .map_err(|e| format!("compiling checkpoint schema: {e}"))
    });
    result.as_ref().map_err(|e| MontageError::msg(e.clone()))
}

/// Validate a checkpoint JSON value against the canonical checkpoint schema.
pub fn validate_checkpoint_value(value: &serde_json::Value) -> MontageResult<()> {
    let v = validator()?;
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("at {}: {e}", e.instance_path()))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(MontageError::CheckpointInvalid(errors.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_embedded_schema() {
        validator().expect("checkpoint schema compiles");
    }
}
