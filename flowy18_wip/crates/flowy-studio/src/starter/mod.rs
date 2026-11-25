//! Home starter suggestions.
//!
//! Four quiet one-click prompt pills below the composer. Picking a pill fills
//! the composer so the prompt can be reviewed or adjusted before sending.

pub mod catalog;

use crate::panel::StudioPanel;
use catalog::{CURATED, Starter};
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, Context, IntoElement, SharedString, Window, div,
    prelude::*, px,
};

impl StudioPanel {
    /// Four calm one-click prompt pills directly below the Home composer.
    pub fn starter_section(
        &self,
        _window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut row = div()
            .id("home-starters")
            // `w_full` collapses inside a centred flex row; `flex_1` takes the
            // width from the parent, which is what makes the row responsive.
            .flex_1()
            .min_w_0()
            .max_w(px(crate::STAGE_MAX_W))
            .flex()
            .flex_wrap()
            .justify_center()
            .items_center()
            .gap(self.theme.space(8.));
        for (index, starter) in CURATED.iter().enumerate() {
            row = row.child(self.starter_pill(starter, index as u64, cx));
        }
        row
    }

    fn starter_pill(
        &self,
        starter: &Starter,
        index: u64,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = self.t(starter.title_key);
        let prompt = self.t(starter.prompt_key);
        let scale = crate::motion::travel_scale(self.theme.motion_policy());
        let delay = crate::motion::stagger(index, 24, crate::motion::MODERATE);
        let soft = self.theme.soft();
        let ink = self.theme.ink();
        let pill = theme::interactive(
            ("home-starter-pill", index),
            SharedString::from(title.clone()),
            &self.theme,
        )
        .px(self.theme.space(12.))
        .py(px(6.))
        .rounded_full()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .text_xs()
        .text_color(self.theme.muted())
        .cursor_pointer()
        .hover(move |style| style.bg(soft).text_color(ink))
        .child(title)
        .on_click(cx.listener(move |panel, _, _, cx| {
            // Picking fills the prompt for review; it never sends automatically.
            panel.request_fill_prompt(prompt.clone(), cx);
        }));
        if scale <= 0.0 {
            return pill.into_any_element();
        }
        pill.with_animation(
            ("home-starter-pill-enter", index),
            Animation::new(crate::motion::MODERATE).with_easing(
                crate::motion::delayed(delay, crate::motion::out_quint()),
            ),
            move |el, delta| {
                el.opacity(delta).relative().top(px(
                    crate::motion::ENTER_OFFSET * scale * (1.0 - delta),
                ))
            },
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{StudioConfig, StudioPanel};
    use flowy_agent::{LlmConfig, Model, Plan};
    use flowy_i18n::I18n;
    use flowy_ui::UiContext;
    use gpui::{Entity, TestAppContext};
    use std::path::PathBuf;

    fn test_panel(
        cx: &mut TestAppContext,
        name: &str,
    ) -> (Entity<StudioPanel>, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-starter-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (panel, _) = cx.add_window_view(|window, cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            StudioPanel::new(
                StudioConfig {
                    product: "Flowy",
                    i18n: I18n::embedded("Flowy"),
                    model: Model::Lite,
                    plan: Plan::Free,
                    model_info_version: String::new(),
                    model_info_open: false,
                    store_root: root.clone(),
                    llm: LlmConfig {
                        api_key: None,
                        base_url: String::new(),
                        model: String::new(),
                        provider: String::new(),
                        ..LlmConfig::default()
                    },
                },
                ui,
                window,
                cx,
            )
        });
        (panel, root)
    }

    #[gpui::test]
    fn picking_an_example_overwrites_the_draft(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "overwrite");
        panel.update(cx, |panel, cx| {
            panel.fill_prompt("my draft", cx);
            panel.request_fill_prompt("starter copy".into(), cx);
        });
        cx.read_entity(&panel, |panel, cx| {
            // The click means "show me this prompt", so the draft is replaced
            // without asking.
            assert_eq!(panel.prompt.read(cx).value(), "starter copy");
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn home_limits_starters_to_four_calm_pills() {
        assert_eq!(CURATED.len(), 4);
    }
}
