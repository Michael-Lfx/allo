//! Home starter browsing.
//!
//! One selector plus one content row. A centred slogan anchors the stage, a
//! row of intent chips picks the category, and the four examples below swap in
//! place. Picking an example overwrites the composer so the prompt can be
//! previewed without sending it.

pub(crate) mod catalog;



use crate::panel::StudioPanel;
use catalog::{Category, Starter};
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, Context, IntoElement, ObjectFit, SharedString,
    Window, div, prelude::*, px,
};

/// Width of the whole stage, shared by the slogan, the chips and the cards.
const STAGE_W: f32 = 880.;
/// Card height. One value: alternating heights made the row look misaligned.
const CARD_H: f32 = 170.;
/// Card width. Fixed, so a narrow window scrolls the row instead of squeezing
/// the cards into unreadable slivers.
const CARD_W: f32 = 220.;

impl StudioPanel {
    /// Slogan, category chips and the current category's four examples.
    pub fn starter_section(
        &self,
        _window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let gap = self.theme.space(12.);
        let category = self.starter_category();
        let title = self.t("home.title");
        // One row, never wrapping: the row keeps its natural width and the host
        // above scrolls, so a narrow window shows fewer cards instead of
        // stacking them.
        let mut row = div().flex().gap(gap).flex_none();
        for (index, starter) in category.examples().iter().enumerate() {
            row = row.child(self.example_card(
                starter,
                index as u64,
                category,
                cx,
            ));
        }
        // One row, never wrapping. The offset is owned by the panel rather than
        // a `ScrollHandle`: GPUI leaves `max_offset` at zero until the node has
        // been laid out, and `set_offset` then clamps every press back to zero.
        let total =
            category.examples().len() as f32 * (CARD_W + 12.) - 12.;
        let max_scroll = (total - STAGE_W).max(0.);
        let offset = self.starter_offset.clamp(-max_scroll, 0.);
        let strip = div()
            .id("home-starters-row")
            .w_full()
            .overflow_hidden()
            .child(row.relative().left(px(offset)));
        let show_prev = offset < -0.5;
        let show_next = offset > -max_scroll + 0.5;
        let mut rail = div()
            .w_full()
            .flex()
            .items_center()
            .gap(self.theme.space(8.));
        if show_prev {
            rail = rail.child(self.carousel_arrow(-1, max_scroll, cx));
        }
        rail = rail.child(div().flex_1().min_w_0().child(strip));
        if show_next {
            rail = rail.child(self.carousel_arrow(1, max_scroll, cx));
        }
        div()
            .id("home-starters")
            // `w_full` collapses inside a centred flex row; `flex_1` takes the
            // width from the parent, which is what makes the row responsive.
            .flex_1()
            .min_w_0()
            .max_w(px(STAGE_W))
            .flex()
            .flex_col()
            .items_center()
            .gap(self.theme.space(18.))
            .child(
                div()
                    .id("home-goal")
                    .role(gpui::Role::Heading)
                    .aria_level(1)
                    .aria_label(title.clone())
                    .w_full()
                    .text_2xl()
                    .text_center()
                    .child(title),
            )
            .child(self.category_chips(cx))
            .child(rail)
    }

    /// One circular carousel arrow. Callers only render it when it can act.
    fn carousel_arrow(
        &self,
        direction: i32,
        max_scroll: f32,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let icon = if direction < 0 {
            theme::icons::chevron_left()
        } else {
            theme::icons::chevron_right()
        };
        theme::interactive(
            ("home-carousel", if direction < 0 { 0u64 } else { 1 }),
            SharedString::from(if direction < 0 { "previous" } else { "next" }),
            &self.theme,
        )
        .w(px(30.))
        .h(px(30.))
        .rounded_full()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(icon)
        .on_click(cx.listener(move |panel, _, _, cx| {
            // Offset grows negative when moving right; clamp against the known
            // content width so the row never scrolls past its own end.
            let step = if direction < 0 { 660. } else { -660. };
            panel.starter_offset =
                (panel.starter_offset + step).clamp(-max_scroll, 0.);
            cx.notify();
        }))
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
        .py(self.theme.space(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .child(
            // Same weight for every chip: a bold selected label changes the
            // row's width and shifts the centred slogan.
            div().text_sm().child(title),
        );
        let soft = self.theme.soft();
        chip = if selected {
            // Teal marks the selected intent. It is used as ink, not as a
            // background: white on teal fails the contrast check in flowy-ui.
            chip.bg(soft).text_color(self.theme.starter_ink())
        } else {
            chip.text_color(self.theme.muted())
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
        let cover = starter.cover();
        // Uniform height: alternating heights read as a misaligned row.
        let height = CARD_H;
        // Cards arrive one after another: each holds its curve slightly longer
        // than the previous one, so the row reads as a sequence.
        let delay = crate::motion::stagger(
            index,
            24,
            crate::motion::MODERATE,
        );
        theme::interactive(
            ("home-starter-example", index),
            SharedString::from(title.clone()),
            &self.theme,
        )
        .w(px(CARD_W))
        .h(px(height))
        // Below this the card becomes unreadable, so wrap instead of squeezing.
        .min_w(px(180.))
        .flex()
        .flex_col()
        .rounded_xl()
        .bg(self.theme.soft())
        .overflow_hidden()
        .child(
            // The cover takes the remaining height; the band below darkens
            // the image edge and carries the title.
            div()
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(cover.w_full().h_full().object_fit(ObjectFit::Cover)),
        )
        .child(
            div()
                .w_full()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .text_sm()
                .bg(self.theme.scrim())
                .child(title),
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            // Picking previews the prompt; it never sends.
            panel.request_fill_prompt(prompt.clone(), cx);
        }))
        .with_animation(
            // Keyed by category so a switcher replays the stagger.
            (
                "home-starter-card",
                category as u32 as u64 * 8 + index,
            ),
            Animation::new(crate::motion::MODERATE).with_easing(
                crate::motion::delayed(delay, crate::motion::entrance()),
            ),
            move |card, delta| {
                card.opacity(delta)
                    .relative()
                    .top(px(crate::motion::ENTER_OFFSET * (1.0 - delta)))
            },
        )
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
        cx: &TestAppContext,
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
        let panel = cx.update(|cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            cx.new(|cx| {
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
                    cx,
                )
            })
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
            assert_eq!(panel.prompt.read(cx).as_str(), "starter copy");
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
