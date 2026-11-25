//! Home (Fresh Desk) stage rendering.
//!
//! Extracted from `studio.rs` so the Home / Spaces experience can evolve
//! independently from the active Router chat timeline.

use gpui::{Context, Window, div, prelude::*, px};

impl crate::panel::StudioPanel {
    pub(crate) fn studio(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        // The width is left to flex: this column takes whatever the drawer
        // leaves, so an open drawer shrinks it without any pixel math.
        div()
            .id("studio-scroll")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .pt(self.theme.space(96.))
                    .pb(self.theme.space(32.))
                    .gap(self.theme.space(12.))
                    // Each block centres itself; a parent `items_center`
                    // would collapse `w_full` children to their content.
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(self.starter_section(window, cx)),
                    )
                    .child(
                        div().w_full().flex().justify_center().child(
                            div()
                                .w_full()
                                .max_w(px(880.))
                                .child(self.composer.clone()),
                        ),
                    ),
            )
    }
}
