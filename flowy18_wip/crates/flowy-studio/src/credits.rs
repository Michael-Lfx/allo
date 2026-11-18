//! Explicit account-data states. Unknown is never represented as a zero balance.
use flowy_i18n::I18n;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CreditsViewState {
    Loading,
    Ready {
        total: u64,
        free: Option<u64>,
        daily: Option<u64>,
    },
    #[default]
    Unavailable,
    Error,
}

impl CreditsViewState {
    pub fn label(&self, i18n: &I18n) -> String {
        match self {
            Self::Loading => i18n.t("credits.loading"),
            Self::Ready { total, .. } => grouped(*total),
            Self::Unavailable => i18n.t("credits.unavailable"),
            Self::Error => i18n.t("credits.error"),
        }
    }
}

#[must_use]
pub fn grouped(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::new();
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index.is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_i18n::Locale;

    #[test]
    fn unavailable_error_loading_and_real_zero_are_distinct_in_all_locales() {
        for locale in Locale::ALL {
            let mut i18n = I18n::embedded("Flowy");
            i18n.set_locale(locale);
            let states = [
                CreditsViewState::Unavailable,
                CreditsViewState::Error,
                CreditsViewState::Loading,
                CreditsViewState::Ready {
                    total: 0,
                    free: Some(0),
                    daily: None,
                },
            ];
            let labels: Vec<_> =
                states.iter().map(|state| state.label(&i18n)).collect();
            assert_eq!(labels[3], "0");
            for (i, label) in labels.iter().enumerate() {
                assert!(!label.starts_with("credits."), "{}", locale.code());
                assert!(
                    labels
                        .iter()
                        .enumerate()
                        .all(|(j, other)| i == j || label != other)
                );
            }
        }
    }
}
