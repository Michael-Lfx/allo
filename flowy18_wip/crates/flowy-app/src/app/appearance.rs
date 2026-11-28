use super::FlowyApp;
use super::settings::Picker;
use crate::prefs::{Accent, Density, ThemeMode};
use flowy_ui as theme;
use gpui::{Context, SharedString, div, prelude::*, px};

impl FlowyApp {
    pub(super) fn touch_font_size(&mut self, cx: &mut Context<'_, Self>) {
        let raw = self.appearance.font_size_field.read(cx).value().to_string();
        let digits: String =
            raw.chars().filter(char::is_ascii_digit).take(2).collect();
        if digits != raw {
            // `set_input_text` queues the value for the next render: installing
            // it here would need a `&mut Window`, which this handler has none of.
            self.set_input_text(
                &self.appearance.font_size_field,
                digits.as_str(),
                cx,
            );
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
            self.set_input_text(
                &self.appearance.font_size_field,
                clamped.to_string(),
                cx,
            );
        }
        if self.prefs.font_size == clamped {
            return;
        }
        self.prefs.font_size = clamped;
        self.prefs.save(self.store.root());
        let options = Self::theme_options(&self.prefs);
        let dark = self.theme.dark;
        self.ui
            .theme
            .update(cx, |theme, cx| theme.update(options, dark, cx));
        self.theme = self.ui.theme.read(cx).resolved();
        cx.notify();
    }

    pub(super) fn appearance_body(&self, cx: &Context<'_, Self>) -> gpui::Div {
        let gap = self.theme.space(20.);
        let mut page = div().relative().flex_1().min_h_0().flex().flex_col();
        page = page.child(
            theme::scroll_area("appearance", &self.theme)
                .a11y_label(self.t("a11y.scroll_appearance"))
                .flex_1()
                .min_h_0()
                .viewport_style(move |s| {
                    s.px_8().pt_2().pb_8().flex().flex_col().gap(gap)
                })
                .child(self.theme_block(cx))
                .child(self.accent_block(cx))
                .child(self.font_block(cx))
                .child(self.density_block(cx))
                .child(self.activity_block(cx)),
        );

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
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .shadow_md()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t("look.activity")),
            )
            .child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("look.idle")),
            )
    }

    fn theme_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading(&self.theme, "look.theme", "look.theme_body", self))
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
        // Omit `Role::RadioButton` and `.aria_toggled(...)` so
        // `accesskit_consumer::is_invocable()` keeps Windows UIA InvokePattern.
        theme::interactive(id, self.t(mode.key()), &self.theme)
            .w(px(108.))
            .h(px(76.))
            .rounded_xl()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .bg(self.theme.card())
            .border_1()
            .border_color(if selected {
                self.theme.ink()
            } else {
                self.theme.line()
            })
            .when(selected, gpui::Styled::border_2)
            .cursor_pointer()
            .hover(|card| card.bg(self.theme.soft()))
            .child(theme_icon(mode))
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .child(self.t(mode.key())),
            )
            .on_click(cx.listener(move |app, _event, window, cx| {
                app.prefs.theme = mode;
                app.settings.picker = Picker::None;
                app.prefs.save(app.store.root());
                app.apply_appearance(window, cx);
                cx.notify();
            }))
    }

    fn accent_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(
            self.theme,
            heading(&self.theme, "look.accent", "look.accent_body", self),
        )
        .child(chip(
            self,
            "accent-button",
            self.t(self.prefs.accent.key()),
            Some(self.prefs.accent),
            Picker::Accent,
            cx,
        ))
    }

    fn font_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(
            self.theme,
            heading(&self.theme, "look.font", "look.font_body", self),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(self.font_field(cx))
                .child(
                    div()
                        .text_size(self.theme.body_size())
                        .text_color(self.theme.muted())
                        .child(self.t("look.unit")),
                ),
        )
    }

    fn font_field(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::TextField::new(
            "font-size",
            self.t("look.font"),
            self.appearance.font_size_field.clone(),
        )
        .render(&self.theme, cx)
        .w(px(76.))
    }

    fn density_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(
            self.theme,
            heading(&self.theme, "look.density", "look.density_body", self),
        )
        .child(chip(
            self,
            "density-button",
            self.t(self.prefs.density.key()),
            None,
            Picker::Density,
            cx,
        ))
    }

    fn activity_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        split_row(
            self.theme,
            heading(&self.theme, "look.activity", "look.activity_body", self),
        )
        .child(
            theme::Switch::new(
                "pref-activity",
                self.t("look.activity"),
                self.prefs.activity,
            )
            .render(
                &self.theme,
                cx.listener(|app, _event, window, cx| {
                    app.prefs.activity = !app.prefs.activity;
                    app.prefs.save(app.store.root());
                    app.apply_appearance(window, cx);
                    cx.notify();
                }),
            ),
        )
    }

    pub(super) fn accent_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut menu = theme::menu(
            "accent-menu",
            self.t("look.accent"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(240.));
        for accent in Accent::ALL {
            let selected = self.prefs.accent == accent;
            let id = SharedString::from(format!("accent-{}", accent.key()));
            menu = menu.child(
                // Omit `Role::MenuItemRadio` and `.aria_toggled(...)` so
                // `accesskit_consumer::is_invocable()` keeps Windows UIA InvokePattern.
                theme::interactive(id, self.t(accent.key()), &self.theme)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(self.theme.body_size())
                    .cursor_pointer()
                    .bg(if selected {
                        self.theme.soft()
                    } else {
                        self.theme.card()
                    })
                    .hover(|row| row.bg(self.theme.soft()))
                    .child(dot(&self.theme, accent))
                    .child(self.t(accent.key()))
                    .on_click(cx.listener(move |app, _event, window, cx| {
                        app.prefs.accent = accent;
                        app.settings.picker = Picker::None;
                        app.prefs.save(app.store.root());
                        app.apply_appearance(window, cx);
                        cx.notify();
                    })),
            );
        }
        menu
    }

    pub(super) fn density_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut menu = theme::menu(
            "density-menu",
            self.t("look.density"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(240.));
        for density in Density::ALL {
            let selected = self.prefs.density == density;
            let id = SharedString::from(format!("density-{}", density.key()));
            menu = menu.child(
                // Omit `Role::MenuItemRadio` and `.aria_toggled(...)` so
                // `accesskit_consumer::is_invocable()` keeps Windows UIA InvokePattern.
                theme::interactive(id, self.t(density.key()), &self.theme)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .text_size(self.theme.body_size())
                    .cursor_pointer()
                    .bg(if selected {
                        self.theme.soft()
                    } else {
                        self.theme.card()
                    })
                    .hover(|row| row.bg(self.theme.soft()))
                    .child(self.t(density.key()))
                    .on_click(cx.listener(move |app, _event, window, cx| {
                        app.prefs.density = density;
                        app.settings.picker = Picker::None;
                        app.prefs.save(app.store.root());
                        app.apply_appearance(window, cx);
                        cx.notify();
                    })),
            );
        }
        menu
    }
}

fn heading(
    theme: &flowy_ui::ResolvedTheme,
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
                .text_size(theme.body_size())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(app.t(title)),
        )
        .child(
            div()
                .text_size(theme.body_size())
                .text_color(theme.muted())
                .whitespace_normal()
                .child(app.t(body)),
        )
}

fn split_row(
    theme: flowy_ui::ResolvedTheme,
    label: impl IntoElement,
) -> gpui::Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap_6()
        .border_t_1()
        .border_color(theme.line())
        .pt(theme.space(16.))
        .child(div().flex_1().child(label))
}

fn chip(
    app: &FlowyApp,
    id: &'static str,
    label: String,
    accent: Option<Accent>,
    picker: Picker,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    let theme = &app.theme;
    theme::anchor_trigger(
        theme::select_trigger(
            id,
            label.clone(),
            app.settings.picker == picker,
            theme,
        ),
        id,
        &app.ui,
    )
    .px_3()
    .py_1()
    .rounded_lg()
    .bg(theme.soft())
    .flex()
    .items_center()
    .gap_2()
    .cursor_pointer()
    .hover(|style| style.bg(theme.line()))
    .children(accent.map(|accent| dot(theme, accent)))
    .child(div().text_size(app.theme.body_size()).child(label))
    .child(
        theme::icons::chevron_down()
            .size(px(14.))
            .text_color(theme.muted()),
    )
    .on_click(cx.listener(move |app, _event, _window, cx| {
        app.settings.picker = if app.settings.picker == picker {
            Picker::None
        } else {
            picker
        };
        cx.notify();
    }))
}

fn dot(theme: &flowy_ui::ResolvedTheme, accent: Accent) -> impl IntoElement {
    div().size(px(8.)).rounded_full().bg(theme.swatch(accent))
}

fn theme_icon(mode: ThemeMode) -> gpui::AnyElement {
    match mode {
        ThemeMode::Light => theme::icons::sun().into_any_element(),
        ThemeMode::Dark => theme::icons::moon().into_any_element(),
        ThemeMode::Auto => theme::icons::monitor().into_any_element(),
    }
}
