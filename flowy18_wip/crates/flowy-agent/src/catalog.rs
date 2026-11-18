use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub context_window: u64,
    pub max_tokens: u64,
    pub credit_rate: f64,
    /// Catalog-declared plan gate (`minTierLevel`); `0` means unrestricted.
    pub min_tier_level: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderCatalog {
    pub provider: String,
    pub models: Vec<CatalogModel>,
    pub selected: Option<String>,
}

impl Default for ProviderCatalog {
    fn default() -> Self {
        Self {
            provider: "flowy".into(),
            models: Vec::new(),
            selected: None,
        }
    }
}

impl ProviderCatalog {
    #[must_use]
    pub fn new() -> Self {
        Self {
            provider: "flowy".to_owned(),
            ..Self::default()
        }
    }

    pub fn replace(&mut self, models: Vec<CatalogModel>) {
        self.models = models;
        self.selected = self.selected_model_id().map(str::to_owned);
    }

    pub fn refresh<E>(
        &mut self,
        result: Result<Vec<CatalogModel>, E>,
    ) -> Result<(), E> {
        self.replace(result?);
        Ok(())
    }

    pub fn select(
        &mut self,
        requested: impl Into<String>,
    ) -> Option<&CatalogModel> {
        let requested = requested.into();
        self.selected = self
            .models
            .iter()
            .find(|model| model.id == requested)
            .map(|model| model.id.clone())
            .or_else(|| self.models.first().map(|model| model.id.clone()));
        self.selected_model()
    }

    #[must_use]
    pub fn selected_model(&self) -> Option<&CatalogModel> {
        let selected = self.selected.as_deref();
        selected
            .and_then(|id| self.models.iter().find(|model| model.id == id))
            .or_else(|| self.models.first())
    }

    fn selected_model_id(&self) -> Option<&str> {
        self.selected_model().map(|model| model.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogModel, ProviderCatalog};

    fn models() -> Vec<CatalogModel> {
        vec![
            CatalogModel {
                id: "first".into(),
                name: "First".into(),
                ..Default::default()
            },
            CatalogModel {
                id: "second".into(),
                name: "Second".into(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn refresh_preserves_valid_selection_and_empty_success_clears_it() {
        let mut catalog = ProviderCatalog::new();
        catalog.replace(models());
        catalog.select("second");
        catalog.refresh::<()>(Ok(models())).unwrap();
        assert_eq!(catalog.selected.as_deref(), Some("second"));
        catalog.refresh::<()>(Ok(Vec::new())).unwrap();
        assert!(catalog.selected.is_none());
        assert!(catalog.selected_model().is_none());
        assert!(catalog.select("second").is_none());
    }

    #[test]
    fn invalid_selection_falls_back_to_first() {
        let mut catalog = ProviderCatalog::new();
        catalog.replace(models());
        assert_eq!(catalog.select("missing").unwrap().id, "first");
    }

    #[test]
    fn failed_refresh_keeps_previous_snapshot() {
        let mut catalog = ProviderCatalog::new();
        catalog.replace(models());
        assert!(catalog.refresh(Err("offline")).is_err());
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.selected.as_deref(), Some("first"));
    }
}
