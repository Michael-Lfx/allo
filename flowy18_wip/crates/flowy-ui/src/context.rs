//! Window-owned UI services. Never store these entities in preferences.

use crate::{OverlayHost, ResolvedTheme, ThemeOptions, ThemeState};
use gpui::{App, AppContext, Entity};

#[derive(Clone, Debug)]
pub struct UiContext {
    pub theme: Entity<ThemeState>,
    pub overlays: Entity<OverlayHost>,
}

impl UiContext {
    pub fn new(options: ThemeOptions, system_dark: bool, cx: &mut App) -> Self {
        let theme = cx.new(|_| ThemeState::new(options, system_dark));
        let overlays = cx.new(OverlayHost::new);
        OverlayHost::install(&overlays, cx);
        Self { theme, overlays }
    }

    pub fn snapshot(&self, cx: &App) -> ResolvedTheme {
        self.theme.read(cx).resolved()
    }
}
