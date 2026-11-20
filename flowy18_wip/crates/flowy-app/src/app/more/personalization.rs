use super::{FlowyApp, Tab};
use flowy_ui as ui;
use gpui::{Context, Pixels, Window, div, prelude::*, px};

mod memory;
mod profile;
mod tabs;

const CONTENT_WIDTH: f32 = 768.;

impl FlowyApp {
    pub(super) fn personal_canvas(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let width = window.viewport_size().width
            - if self.dock_visible {
                px(ui::DOCK_WIDTH)
            } else {
                px(0.)
            };
        let inset = ui::page_inset(width);
        let selected = self.persona.tab;
        let tabs = tabs::render(self, window, cx);

        let panel = div()
            .id(match selected {
                Tab::Profile => "personal-profile-panel",
                Tab::Knowledge => "personal-knowledge-panel",
            })
            .role(gpui::Role::TabPanel)
            .pt_6();
        let panel = match selected {
            Tab::Profile => panel
                .aria_label(self.t("more.profile"))
                .flex()
                .flex_col()
                .gap_6()
                .child(memory::card(self, cx))
                .child(profile::form(self, width - inset * 2., cx)),
            Tab::Knowledge => panel
                .aria_label(self.t("more.knowledge"))
                .child(self.knowledge_pane(cx)),
        };

        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                ui::PageHeader::new(self.t("more.personal"))
                    .render(&self.theme, window),
            )
            .child(
                div()
                    .id("more-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        content_column(inset).pb_6().child(tabs).child(panel),
                    ),
            )
            .when(selected == Tab::Profile, |page| {
                page.child(
                    content_column(inset)
                        .flex_shrink_0()
                        .pb_6()
                        .child(memory::footer(self, cx)),
                )
            })
    }
}

fn content_column(inset: Pixels) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .max_w(px(CONTENT_WIDTH) + inset * 2.)
        .mx_auto()
        .px(inset)
}
