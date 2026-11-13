use super::settings::{Picker, toggle_switch};
use super::{FlowyApp, place};
use crate::prefs::{Accent, Density, ThemeMode};
use flowy_ui as theme;
use gpui::{Context, SharedString, div, prelude::*, px};
use gpui_ce_elements::editable_text::text_input;

impl FlowyApp {
    pub(super) fn touch_font_size(&mut self, cx: &mut Context<'_, Self>) {
        let raw = self
            .appearance
            .font_size_field
            .read(cx)
            .as_str()
            .to_string();
        let digits: String =
            raw.chars().filter(char::is_ascii_digit).take(2).collect();
        if digits != raw {
            place(&self.appearance.font_size_field, &digits, cx);
            return;
        }
        let Ok(value) = digits.parse::<u8>() else {
            return;
        };
        if digits.len() < 2 && value < 12 {
            return;
        }
        let clamped = value.clamp(12, 16);
        if clamped != value {
            place(&self.appearance.font_size_field, &clamped.to_string(), cx);
        }
        if self.prefs.font_size == clamped {
            return;
        }
        self.prefs.font_size = clamped;
        self.prefs.save(self.store.root());
        cx.notify();
    }

    pub(super) fn appearance_body(&self, cx: &Context<'_, Self>) -> gpui::Div {
        let mut page = div().relative().flex_1().min_h_0();
        page = page.child(
            div()
                .id("appearance")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_8()
                .pt_2()
                .pb_8()
                .flex()
                .flex_col()
                .gap(theme::space(20.))
                .child(self.theme_block(cx))
                .child(self.accent_block(cx))
                .child(self.font_block())
                .child(self.density_block(cx))
                .child(self.activity_block(cx)),
        );
        if self.settings.picker == Picker::Accent {
            page =
                page.child(self.accent_menu(cx).top(px(188.)).right(px(32.)));
        }
        if self.settings.picker == Picker::Density {
            page =
                page.child(self.density_menu(cx).top(px(312.)).right(px(32.)));
        }
        page
    }

    pub(super) fn activity_floater(&self) -> impl IntoElement {
        div()
            .id("activity-floater")
            .absolute()
            .bottom(px(24.))
            .right(px(24.))
            .w(px(240.))
            .p_3()
            .rounded_xl()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t("look.activity")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t("look.idle")),
            )
    }

    fn theme_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading("look.theme", "look.theme_body", self))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(self.theme_card(ThemeMode::Light, cx))
                    .child(self.theme_card(ThemeMode::Dark, cx))
                    .child(self.theme_card(ThemeMode::Auto, cx)),
            )
    }

    fn theme_card(
        &self,
        mode: ThemeMode,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let selected = self.prefs.theme == mode;
        let id = SharedString::from(format!("theme-{}", mode.key()));
        div()
            .id(id)
            .w(px(108.))
            .h(px(76.))
            .rounded_xl()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .bg(theme::card())
            .border_1()
            .border_color(if selected {
                theme::ink()
            } else {
                theme::line()
            })
            .when(selected, gpui::Styled::border_2)
            .cursor_pointer()
            .hover(|card| card.bg(theme::soft()))
            .child(theme_icon(mode))
            .child(div().text_sm().child(self.t(mode.key())))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.prefs.theme = mode;
                app.settings.picker = Picker::None;
                app.prefs.save(app.store.root());
                cx.notify();
            }))
    }

    fn accent_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(heading("look.accent", "look.accent_body", self)).child(chip(
            "accent-button",
            self.t(self.prefs.accent.key()),
            Some(self.prefs.accent),
            Picker::Accent,
            cx,
        ))
    }

    fn font_block(&self) -> impl IntoElement {
        split_row(heading("look.font", "look.font_body", self)).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(self.font_field())
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::muted())
                        .child(self.t("look.unit")),
                ),
        )
    }

    fn font_field(&self) -> impl IntoElement {
        text_input("font-size")
            .state(self.appearance.font_size_field.downgrade())
            .caret_color(gpui::rgb_to_hsla(theme::ink()))
            .caret_blink_interval_500ms()
            .w(px(48.))
            .min_h_auto()
            .whitespace_nowrap()
            .px_3()
            .py_1()
            .rounded_lg()
            .bg(theme::card())
            .border_1()
            .border_color(theme::line())
            .text_sm()
            .text_color(theme::ink())
    }

    fn density_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(heading("look.density", "look.density_body", self)).child(
            chip(
                "density-button",
                self.t(self.prefs.density.key()),
                None,
                Picker::Density,
                cx,
            ),
        )
    }

    fn activity_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(heading("look.activity", "look.activity_body", self)).child(
            toggle_switch("pref-activity", self.prefs.activity).on_click(
                cx.listener(|app, _event, _window, cx| {
                    app.prefs.activity = !app.prefs.activity;
                    app.prefs.save(app.store.root());
                    cx.notify();
                }),
            ),
        )
    }

    fn accent_menu(&self, cx: &Context<'_, Self>) -> gpui::Stateful<gpui::Div> {
        let mut menu = super::settings::menu_shell("accent-menu");
        for accent in Accent::ALL {
            let selected = self.prefs.accent == accent;
            let id = SharedString::from(format!("accent-{}", accent.key()));
            menu = menu.child(
                div()
                    .id(id)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .cursor_pointer()
                    .bg(if selected {
                        theme::soft()
                    } else {
                        theme::card()
                    })
                    .hover(|row| row.bg(theme::soft()))
                    .child(dot(accent))
                    .child(self.t(accent.key()))
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        app.prefs.accent = accent;
                        app.settings.picker = Picker::None;
                        app.prefs.save(app.store.root());
                        cx.notify();
                    })),
            );
        }
        menu
    }

    fn density_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut menu = super::settings::menu_shell("density-menu");
        for density in Density::ALL {
            let selected = self.prefs.density == density;
            let id = SharedString::from(format!("density-{}", density.key()));
            menu = menu.child(
                div()
                    .id(id)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .bg(if selected {
                        theme::soft()
                    } else {
                        theme::card()
                    })
                    .hover(|row| row.bg(theme::soft()))
                    .child(self.t(density.key()))
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        app.prefs.density = density;
                        app.settings.picker = Picker::None;
                        app.prefs.save(app.store.root());
                        cx.notify();
                    })),
            );
        }
        menu
    }
}

fn heading(
    title: &'static str,
    body: &'static str,
    app: &FlowyApp,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(app.t(title)),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme::muted())
                .whitespace_normal()
                .child(app.t(body)),
        )
}

fn split_row(label: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap_6()
        .border_t_1()
        .border_color(theme::line())
        .pt(theme::space(16.))
        .child(div().flex_1().child(label))
}

fn chip(
    id: &'static str,
    label: String,
    accent: Option<Accent>,
    picker: Picker,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .bg(theme::soft())
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(|style| style.bg(theme::line()))
        .children(accent.map(dot))
        .child(div().text_sm().child(label))
        .child(div().text_xs().text_color(theme::muted()).child("▾"))
        .on_click(cx.listener(move |app, _event, _window, cx| {
            app.settings.picker = if app.settings.picker == picker {
                Picker::None
            } else {
                picker
            };
            cx.notify();
        }))
}

fn dot(accent: Accent) -> impl IntoElement {
    div().size(px(8.)).rounded_full().bg(theme::swatch(accent))
}

fn theme_icon(mode: ThemeMode) -> gpui::AnyElement {
    match mode {
        ThemeMode::Light => sun_icon().into_any_element(),
        ThemeMode::Dark => moon_icon().into_any_element(),
        ThemeMode::Auto => monitor_icon().into_any_element(),
    }
}

fn sun_icon() -> impl IntoElement {
    div()
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(8.))
                .rounded_full()
                .border_1()
                .border_color(theme::ink()),
        )
}

fn moon_icon() -> impl IntoElement {
    div()
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(8.))
                .rounded_full()
                .border_1()
                .border_color(theme::ink()),
        )
        .child(
            div()
                .ml(px(-5.))
                .size(px(8.))
                .rounded_full()
                .bg(theme::card()),
        )
}

fn monitor_icon() -> impl IntoElement {
    div()
        .size(px(16.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(1.))
        .child(
            div()
                .w(px(12.))
                .h(px(8.))
                .rounded(px(2.))
                .border_1()
                .border_color(theme::ink()),
        )
        .child(div().w(px(6.)).h(px(2.)).bg(theme::ink()))
}
