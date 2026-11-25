//! Home starter browsing.
//!
//! Quiet intent category pills plus a crisp two-column grid of calm starter
//! prompts directly below the composer. Picking an example fills the composer
//! so the prompt can be previewed and refined before sending.

pub mod catalog;

use crate::panel::StudioPanel;
use catalog::{Category, Starter};
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, Context, FontWeight, IntoElement, SharedString,
    Window, div, prelude::*, px,
};

impl StudioPanel {
    /// Category chips and the current category's clean two-column prompt cards.
    pub fn starter_section(
        &self,
        _window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let category = self.starter_category();
        let examples = category.examples();
        let mut grid = div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_stretch()
            .gap(self.theme.space(8.));
        for (index, starter) in examples.iter().enumerate() {
            grid = grid.child(
                // 300px floor yields a clean 2x2 grid up to `STAGE_MAX_W` and
                // wraps to a single column on narrow viewports (<608px).
                div().flex_1().min_w(px(300.)).child(self.example_card(
                    starter,
                    index as u64,
                    category,
                    cx,
                )),
            );
        }
        div()
            .id("home-starters")
            // `w_full` collapses inside a centred flex row; `flex_1` takes the
            // width from the parent, which is what makes the row responsive.
            .flex_1()
            .min_w_0()
            .max_w(px(crate::STAGE_MAX_W))
            .flex()
            .flex_col()
            .items_center()
            .gap(self.theme.space(12.))
            .child(self.category_chips(cx))
            .child(grid)
    }

    fn category_chips(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div()
            .w_full()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(self.theme.space(6.));
        for category in Category::ALL {
            row = row.child(self.category_chip(category, cx));
        }
        row
    }

    /// Geometry stays put across hover and focus; only colours change.
    fn category_chip(
        &self,
        category: Category,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = self.t(category.title_key());
        let selected = category == self.starter_category();
        let mut chip = theme::interactive(
            ("home-starter-chip", category as u32 as u64),
            SharedString::from(title.clone()),
            &self.theme,
        )
        .px(self.theme.space(12.))
        .py(self.theme.space(4.))
        .rounded_full()
        .border_1()
        .cursor_pointer()
        .child(
            // Same weight for every chip: a bold selected label changes the
            // row's width and shifts the centred stage.
            div().text_xs().child(title),
        );
        let soft = self.theme.soft();
        chip = if selected {
            chip.bg(self.theme.card())
                .border_color(self.theme.line())
                .shadow_sm()
                .text_color(self.theme.starter_ink())
        } else {
            chip.border_color(self.theme.canvas())
                .text_color(self.theme.muted())
                .hover(move |style| style.bg(soft))
        };
        chip.on_click(cx.listener(move |panel, _, _, cx| {
            panel.set_starter_category(category, cx);
        }))
    }

    fn example_card(
        &self,
        starter: &Starter,
        index: u64,
        category: Category,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = self.t(starter.title_key);
        let prompt = self.t(starter.prompt_key);
        let glimpse =
            crate::history::clip_words(&prompt, 46).unwrap_or_default();
        let fill_hint = self.t("home.fill_hint");
        let suggested_id = category.suggested_skill_id();
        let suggested_skill = self
            .installed_skills
            .iter()
            .find(|chip| chip.id == suggested_id)
            .cloned();
        let skill_badge_title =
            suggested_skill.as_ref().map(|chip| chip.title.clone());
        let skill_pick_id = suggested_skill.map(|chip| chip.id);
        let scale = crate::motion::travel_scale(self.theme.motion_policy());
        let delay = crate::motion::stagger(index, 28, crate::motion::MODERATE);
        let soft = self.theme.soft();
        let card = theme::interactive(
            ("home-starter-example", index),
            SharedString::from(title.clone()),
            &self.theme,
        )
        .flex_1()
        .min_w_0()
        .px(self.theme.space(12.))
        .py(self.theme.space(10.))
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .hover(move |style| style.bg(soft))
        .cursor_pointer()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap(self.theme.space(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(title),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .when_some(skill_badge_title, |row, skill_name| {
                            row.child(
                                div()
                                    .max_w(px(144.))
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded_full()
                                    .bg(self.theme.soft())
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(skill_name),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .child(fill_hint),
                        ),
                ),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_color(self.theme.muted())
                .overflow_hidden()
                .whitespace_nowrap()
                .truncate()
                .child(glimpse),
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            // Picking previews the prompt (and mounts the suggested skill); it
            // never sends automatically.
            panel.request_fill_prompt_with_skill(
                prompt.clone(),
                skill_pick_id.as_deref(),
                cx,
            );
        }));
        if scale <= 0.0 {
            return card.into_any_element();
        }
        card.with_animation(
            // Keyed by category so a switcher replays the stagger.
            ("home-starter-card", category as u32 as u64 * 8 + index),
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

    #[gpui::test]
    fn switching_category_keeps_the_stage_in_sync(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "switch");
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.starter_category(), Category::Website);
        });
        panel.update(cx, |panel, cx| {
            panel.set_starter_category(Category::Docs, cx);
        });
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.starter_category(), Category::Docs);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }
}
