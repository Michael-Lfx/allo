use super::{FlowyApp, Tab};
use flowy_ui as ui;
use gpui::{Context, FontWeight, Window, div, prelude::*, px};

pub(super) fn render(
    app: &FlowyApp,
    window: &mut Window,
    cx: &mut Context<'_, FlowyApp>,
) -> gpui::Div {
    let theme = app.theme;
    let help = ui::tooltip_trigger(
        "personal-knowledge-help-tooltip",
        ui::interactive(
            "personal-knowledge-help",
            app.t("more.knowledge"),
            &theme,
        )
        .size(px(24.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .child(
            ui::icons::circle_help()
                .size(px(16.))
                .text_color(theme.muted()),
        ),
        app.t("more.knowledge_help"),
        &app.ui,
        window,
        cx,
        app.i18n.locale().is_rtl(),
    );
    div()
        .w_full()
        .flex()
        .items_start()
        .gap(px(3.))
        .border_b_1()
        .border_color(theme.chrome_line())
        .child(
            ui::FilterTabs::new("personal-tabs", app.t("more.personal"))
                .child(
                    tab(
                        app,
                        cx,
                        Tab::Profile,
                        "more-tab-profile",
                        "more.profile",
                    )
                    .mr_5(),
                )
                .child(tab(
                    app,
                    cx,
                    Tab::Knowledge,
                    "more-tab-knowledge",
                    "more.knowledge",
                ))
                .render()
                .w_auto(),
        )
        .child(help.mt(px(2.)))
}

fn tab(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
    tab: Tab,
    id: &'static str,
    label_key: &'static str,
) -> gpui::Stateful<gpui::Div> {
    let selected = app.persona.tab == tab;
    let label = app.t(label_key);
    let theme = app.theme;
    ui::interactive(id, label.clone(), &theme)
        .role(gpui::Role::Tab)
        .aria_selected(selected)
        .border_0()
        .border_b_2()
        .border_color(if selected {
            theme.ink()
        } else {
            theme.canvas()
        })
        .pb_2()
        .text_size(theme.body_size() + px(2.))
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(if selected { theme.ink() } else { theme.muted() })
        .hover(move |style| style.text_color(theme.ink()))
        .on_click(cx.listener(move |app, _, _, cx| {
            app.persona.tab = tab;
            cx.notify();
        }))
        .child(label)
}
