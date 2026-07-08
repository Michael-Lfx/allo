//! Resolve config/agent `model` strings against `GET /model/availableListClaw` entries only.

use super::ClawModelEntry;

/// Map a candidate to a catalog list `id` (`AIPC-<tb_model.name>`).
///
/// Does not invent or guess upstream model names — only exact matches against
/// entries returned by `availableListClaw`.
pub fn resolve_model_in_catalog(candidate: &str, catalog: &[ClawModelEntry]) -> Option<String> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }
    catalog
        .iter()
        .find(|entry| entry.matches_model_candidate(candidate))
        .map(|entry| entry.api_model_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seedance_entry() -> ClawModelEntry {
        ClawModelEntry {
            id: "AIPC-doubao-seedance-2-0-fast-260128".into(),
            name: "Doubao Seedance 2.0 Fast".into(),
            extra: String::new(),
            endpoint: String::new(),
            anthropic_endpoint: String::new(),
            icon: String::new(),
            category: 4,
        }
    }

    fn z_image_entry() -> ClawModelEntry {
        ClawModelEntry {
            id: "AIPC-z-image-turbo".into(),
            name: "Z-Image Turbo".into(),
            extra: String::new(),
            endpoint: String::new(),
            anthropic_endpoint: String::new(),
            icon: String::new(),
            category: 6,
        }
    }

    #[test]
    fn tb_model_name_and_flowy_equivalent() {
        let entry = z_image_entry();
        assert_eq!(entry.tb_model_name(), "z-image-turbo");
        assert_eq!(entry.flowy_model_id(), "flowy/z-image-turbo");
        assert_eq!(entry.api_model_id(), "AIPC-z-image-turbo");
    }

    #[test]
    fn resolve_list_id() {
        let catalog = vec![seedance_entry()];
        assert_eq!(
            resolve_model_in_catalog("AIPC-doubao-seedance-2-0-fast-260128", &catalog).as_deref(),
            Some("AIPC-doubao-seedance-2-0-fast-260128")
        );
    }

    #[test]
    fn resolve_flowy_equivalent() {
        let catalog = vec![z_image_entry()];
        assert_eq!(
            resolve_model_in_catalog("flowy/z-image-turbo", &catalog).as_deref(),
            Some("AIPC-z-image-turbo")
        );
    }

    #[test]
    fn rejects_display_name_and_guessed_ids() {
        let catalog = vec![seedance_entry()];
        assert!(resolve_model_in_catalog("Doubao Seedance 2.0 Fast", &catalog).is_none());
        assert!(resolve_model_in_catalog("AIPC-Doubao-Seedance-2-0-fast", &catalog).is_none());
        assert!(resolve_model_in_catalog("doubao-seedance-2-0-fast-260128", &catalog).is_none());
    }
}
