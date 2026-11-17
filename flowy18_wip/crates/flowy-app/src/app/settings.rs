use super::FlowyApp;
use crate::i18n::Locale;
use crate::prefs::SendMode;
use flowy_ui as theme;
use gpui::{Context, Pixels, SharedString, Size, Window, div, prelude::*, px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Account,
    Billing,
    General,
    Appearance,
    Shortcuts,
    Personalization,
    Connectors,
    Skills,
    Mail,
    Computer,
    Mailbox,
    Phone,
    Wallet,
    Data,
    Integrations,
    Developer,
    Browser,
    Team,
    About,
    Help,
}

impl Page {
    const fn key(self) -> &'static str {
        match self {
            Self::Account => "settings.account",
            Self::Billing => "settings.billing",
            Self::General => "settings.general",
            Self::Appearance => "settings.appearance",
            Self::Shortcuts => "settings.shortcuts",
            Self::Personalization => "settings.personalization",
            Self::Connectors => "settings.connectors",
            Self::Skills => "settings.skills",
            Self::Mail => "settings.mail",
            Self::Computer => "settings.computer",
            Self::Mailbox => "settings.mailbox",
            Self::Phone => "settings.phone",
            Self::Wallet => "settings.wallet",
            Self::Data => "settings.data",
            Self::Integrations => "settings.integrations",
            Self::Developer => "settings.developer",
            Self::Browser => "settings.browser",
            Self::Team => "settings.create_team",
            Self::About => "settings.about",
            Self::Help => "settings.help",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Picker {
    None,
    Language,
    Send,
    Accent,
    Density,
}

const TOP: &[Page] = &[Page::Account, Page::Billing];

const GROUPS: &[(&str, &[Page])] = &[
    (
        "settings.group_settings",
        &[Page::General, Page::Appearance, Page::Shortcuts],
    ),
    (
        "settings.group_features",
        &[
            Page::Personalization,
            Page::Connectors,
            Page::Skills,
            Page::Mail,
            Page::Computer,
        ],
    ),
    (
        "settings.group_assets",
        &[Page::Mailbox, Page::Phone, Page::Wallet],
    ),
    (
        "settings.group_data",
        &[
            Page::Data,
            Page::Integrations,
            Page::Developer,
            Page::Browser,
        ],
    ),
];

const BOTTOM: &[Page] = &[Page::Team, Page::About, Page::Help];

impl FlowyApp {
    pub(super) fn open_settings(&mut self, cx: &mut Context<'_, Self>) {
        self.open_settings_page(Page::General, cx);
    }

    pub(super) fn open_settings_page(
        &mut self,
        page: Page,
        cx: &mut Context<'_, Self>,
    ) {
        self.activate_dialog(super::state::ActiveDialog::Settings, cx);
        self.settings.page = page;
        self.settings.picker = Picker::None;
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<'_, Self>) {
        self.active_dialog = super::state::ActiveDialog::None;
        self.settings.picker = Picker::None;
        cx.notify();
    }

    fn choose_locale(&mut self, locale: Locale, cx: &mut Context<'_, Self>) {
        self.prefs.locale = locale;
        self.i18n.set_locale(locale);
        let i18n = self.i18n.clone();
        self.studio.update(cx, |panel, cx| {
            panel.sync_i18n(i18n, cx);
        });
        self.settings.picker = Picker::None;
        self.prefs.save(self.store.root());
        cx.notify();
    }

    fn choose_send(&mut self, mode: SendMode, cx: &mut Context<'_, Self>) {
        self.prefs.send_mode = mode;
        self.settings.picker = Picker::None;
        self.prefs.save(self.store.root());
        cx.notify();
    }

    fn toggle_pref(&mut self, which: PrefToggle, cx: &mut Context<'_, Self>) {
        match which {
            PrefToggle::Notify => {
                self.prefs.notify_browser = !self.prefs.notify_browser;
            }
            PrefToggle::Sound => self.prefs.sound = !self.prefs.sound,
            PrefToggle::Early => {
                self.prefs.early_access = !self.prefs.early_access;
            }
            PrefToggle::Telemetry => {
                self.prefs.telemetry_opt_out = !self.prefs.telemetry_opt_out;
                if let Some(telemetry) = crate::telemetry() {
                    telemetry.set_opt_out(self.prefs.telemetry_opt_out);
                }
            }
            PrefToggle::LaunchAtLogin => {
                self.prefs.launch_at_login = !self.prefs.launch_at_login;
                let _ = flowy_shell::set_autostart(self.prefs.launch_at_login);
            }
        }
        self.prefs.save(self.store.root());
        cx.notify();
    }

    pub(super) fn settings_layer(
        &self,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        theme::modal_layer(
            "settings-layer",
            &self.theme,
            &self.ui,
            self.settings_panel(frame, cx),
        )
    }

    fn settings_panel(
        &self,
        frame: Size<Pixels>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::dialog(
            "settings-panel",
            self.t("settings.general"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(frame.width)
        .h(frame.height)
        .rounded(self.theme.modal_radius())
        .bg(self.theme.card())
        .shadow_md()
        .overflow_hidden()
        .flex()
        .text_color(self.theme.ink())
        .on_click(cx.listener(|_app, _event, _window, cx| {
            cx.stop_propagation();
        }))
        .child(self.settings_nav(cx))
        .child(self.settings_main(cx))
    }

    fn settings_nav(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let query = self.settings.query.read(cx).as_str().trim().to_string();
        let name = self.auth.session.as_ref().map_or_else(
            || self.t("dock.user_fallback"),
            |session| session.display_name.clone(),
        );
        let initial = name.chars().next().unwrap_or('F').to_string();
        div()
            .w(px(260.))
            .h_full()
            .flex()
            .flex_col()
            .rounded_l(self.theme.modal_radius())
            .overflow_hidden()
            .pb(self.theme.modal_radius())
            .border_r_1()
            .border_color(self.theme.line())
            .bg(self.theme.dock())
            .child(self.profile_block(&name, &initial))
            .child(self.settings_search(cx))
            .child(self.settings_links(&query, cx))
    }

    fn profile_block(&self, name: &str, initial: &str) -> impl IntoElement {
        div()
            .px_4()
            .pt_4()
            .pb_2()
            .flex()
            .items_center()
            .gap_2()
            .child(initial_mark(&self.theme, initial))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .child(name.to_string()),
                    )
                    .child(
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .child(self.t("settings.personal_plan")),
                    ),
            )
    }

    fn settings_search(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div().px_3().pb_2().child(
            theme::TextField::new(
                "settings-search",
                self.t("settings.search"),
                self.settings.query.clone(),
            )
            .placeholder(self.t("settings.search"))
            .render(&self.theme, cx),
        )
    }

    fn settings_links(
        &self,
        query: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut column = div()
            .id("settings-nav")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .pb_3();
        column = column.child(self.link_list(TOP, query, cx));
        for (title, pages) in GROUPS {
            if !group_visible(self, pages, query) {
                continue;
            }
            column = column
                .child(group_label(&self.theme, &self.t(title)))
                .child(self.link_list(pages, query, cx));
        }
        column
            .child(
                div()
                    .mt_4()
                    .pt_2()
                    .border_t_1()
                    .border_color(self.theme.line()),
            )
            .child(self.link_list(BOTTOM, query, cx))
    }

    fn link_list(
        &self,
        pages: &[Page],
        query: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut list = div().flex().flex_col();
        for page in pages {
            let label = self.t(page.key());
            if !matches_query(&label, query) {
                continue;
            }
            let selected = self.settings.page == *page;
            list = list.child(self.nav_link(*page, label, selected, cx));
        }
        list
    }

    fn nav_link(
        &self,
        page: Page,
        label: String,
        selected: bool,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = SharedString::from(format!("settings-{}", page.key()));
        theme::interactive(id, label.clone(), &self.theme)
            .role(gpui::Role::MenuItemRadio)
            .aria_toggled(if selected {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            })
            .mx_2()
            .px(self.theme.space(8.))
            .py(self.theme.space(4.))
            .rounded_md()
            .text_size(self.theme.body_size())
            .cursor_pointer()
            .bg(if selected {
                self.theme.soft()
            } else {
                self.theme.dock()
            })
            .hover(|row| row.bg(self.theme.soft()))
            .child(label)
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.settings.page = page;
                app.settings.picker = Picker::None;
                cx.notify();
            }))
    }

    fn settings_main(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let title = self.t(self.settings.page.key());
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .rounded_r(self.theme.modal_radius())
            .overflow_hidden()
            .pb(self.theme.modal_radius())
            .bg(self.theme.card())
            .child(
                div()
                    .h(px(48.))
                    .px_8()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        theme::IconButton::new(
                            "close-settings",
                            self.t("ui.close"),
                            theme::icons::x().size(px(16.)),
                        )
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_settings(cx);
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .px_8()
                    .pb_2()
                    .text_size(self.theme.heading_size())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(self.settings_body(cx))
    }

    fn settings_body(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        if self.settings.page == Page::Appearance {
            return self.appearance_body(cx);
        }
        if self.settings.page == Page::About {
            return self.about_body(cx);
        }
        if self.settings.page == Page::Data {
            return self.data_body(cx);
        }
        if self.settings.page != Page::General {
            let title = self.t(self.settings.page.key());
            return div()
                .px_8()
                .pt_4()
                .text_size(self.theme.body_size())
                .text_color(self.theme.muted())
                .child(self.tf("settings.later", &[("title", &title)]));
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .px_8()
            .pt_4()
            .pb_8()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.language_block(cx))
            .child(self.send_block(cx))
            .child(self.toggle_row(
                "pref-notify",
                "settings.notify",
                self.prefs.notify_browser,
                PrefToggle::Notify,
                cx,
            ))
            .child(self.toggle_row(
                "pref-sound",
                "settings.sound",
                self.prefs.sound,
                PrefToggle::Sound,
                cx,
            ))
            .child(self.toggle_row(
                "pref-early",
                "settings.early_access",
                self.prefs.early_access,
                PrefToggle::Early,
                cx,
            ))
            .child(self.toggle_row(
                "pref-launch",
                "settings.launch_at_login",
                self.prefs.launch_at_login,
                PrefToggle::LaunchAtLogin,
                cx,
            ))
    }

    fn language_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .child(self.t("settings.language")),
            )
            .child(self.menu_button(
                "language-button",
                Locale::endonym(self.i18n.locale()).to_string(),
                Picker::Language,
                cx,
            ))
    }

    fn send_block(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_6()
            .border_t_1()
            .border_color(self.theme.line())
            .pt_4()
            .child(
                div()
                    .flex_1()
                    .text_size(self.theme.body_size())
                    .child(self.t("settings.send_label")),
            )
            .child(self.menu_button(
                "send-button",
                self.t(self.prefs.send_mode.key()),
                Picker::Send,
                cx,
            ))
    }

    fn menu_button(
        &self,
        id: &'static str,
        label: String,
        picker: Picker,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::anchor_trigger(
            theme::select_trigger(
                id,
                label.clone(),
                self.settings.picker == picker,
                &self.theme,
            ),
            id,
            &self.ui,
        )
        .min_w(px(220.))
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(self.theme.soft())
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .cursor_pointer()
        .hover(|style| style.bg(self.theme.line()))
        .child(div().text_size(self.theme.body_size()).child(label))
        .child(
            theme::icons::chevron_down()
                .size(px(14.))
                .text_color(self.theme.muted()),
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

    pub(super) fn language_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut menu = theme::menu(
            "language-menu",
            self.t("settings.language"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(240.))
        .max_h(px(320.))
        .overflow_y_scroll()
        .w(px(260.));
        for locale in Locale::ALL {
            let selected = self.i18n.locale() == locale;
            let id = SharedString::from(format!("locale-{}", locale.code()));
            menu = menu.child(
                theme::interactive(id, locale.endonym(), &self.theme)
                    .role(gpui::Role::MenuItemRadio)
                    .aria_toggled(if selected {
                        gpui::accesskit::Toggled::True
                    } else {
                        gpui::accesskit::Toggled::False
                    })
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .text_size(self.theme.body_size())
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .bg(if selected {
                        self.theme.soft()
                    } else {
                        self.theme.card()
                    })
                    .hover(|row| row.bg(self.theme.soft()))
                    .child(div().flex_1().min_w_0().child(locale.endonym()))
                    .when(selected, |row| {
                        row.child(
                            theme::icons::check()
                                .size(px(14.))
                                .text_color(self.theme.accent()),
                        )
                    })
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        app.choose_locale(locale, cx);
                    })),
            );
        }
        menu
    }

    pub(super) fn send_menu(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut menu = theme::menu(
            "send-menu",
            self.t("settings.send_label"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(240.))
        .max_h(px(320.))
        .overflow_y_scroll();
        for mode in [SendMode::Quiet, SendMode::Queue, SendMode::Interrupt] {
            let selected = self.prefs.send_mode == mode;
            let label = self.t(mode.key());
            let id = SharedString::from(format!("send-{}", mode.key()));
            menu = menu.child(
                theme::interactive(id, label.clone(), &self.theme)
                    .role(gpui::Role::MenuItemRadio)
                    .aria_toggled(if selected {
                        gpui::accesskit::Toggled::True
                    } else {
                        gpui::accesskit::Toggled::False
                    })
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
                    .child(label)
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        app.choose_send(mode, cx);
                    })),
            );
        }
        menu
    }

    fn toggle_row(
        &self,
        id: &'static str,
        key: &'static str,
        on: bool,
        which: PrefToggle,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_6()
            .border_t_1()
            .border_color(self.theme.line())
            .pt_4()
            .child(
                div()
                    .flex_1()
                    .text_size(self.theme.body_size())
                    .child(self.t(key)),
            )
            .child(theme::Switch::new(id, self.t(key), on).render(
                &self.theme,
                cx.listener(move |app, _event, _window, cx| {
                    app.toggle_pref(which, cx)
                }),
            ))
    }
}

#[derive(Clone, Copy)]
enum PrefToggle {
    Notify,
    Sound,
    Early,
    Telemetry,
    LaunchAtLogin,
}

impl FlowyApp {
    fn data_body(&self, cx: &Context<'_, Self>) -> gpui::Div {
        div()
            .flex_1()
            .min_h_0()
            .px_8()
            .pt_4()
            .pb_8()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .child(self.t("settings.telemetry_help")),
            )
            .child(self.toggle_row(
                "pref-telemetry",
                "settings.telemetry",
                !self.prefs.telemetry_opt_out,
                PrefToggle::Telemetry,
                cx,
            ))
    }
}

fn group_visible(app: &FlowyApp, pages: &[Page], query: &str) -> bool {
    pages
        .iter()
        .any(|page| matches_query(&app.t(page.key()), query))
}

fn matches_query(label: &str, query: &str) -> bool {
    query.is_empty() || label.to_lowercase().contains(&query.to_lowercase())
}

fn group_label(
    theme: &flowy_ui::ResolvedTheme,
    text: &str,
) -> impl IntoElement {
    div()
        .px_4()
        .pt_4()
        .pb_1()
        .text_size(theme.caption_size())
        .text_color(theme.muted())
        .child(text.to_string())
}

fn initial_mark(
    theme: &flowy_ui::ResolvedTheme,
    initial: &str,
) -> impl IntoElement {
    div()
        .size(px(32.))
        .rounded_full()
        .bg(theme.fill())
        .text_color(theme.on_fill())
        .flex()
        .items_center()
        .justify_center()
        .text_size(theme.body_size())
        .child(initial.to_string())
}
