use super::FlowyApp;
use flowy_ui as ui;
use gpui::{Context, FontWeight, div, prelude::*, px};

pub(super) fn card(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    let theme = app.theme;
    let title = app.t("more.import_card");
    let description = app.t("more.import_card_body");
    ui::interactive("import-memory-card", title.clone(), &theme)
        .aria_description(description.clone())
        .w_full()
        .min_h(px(72.))
        .px_4()
        .py_3()
        .rounded_xl()
        .bg(theme.tab_selected_bg())
        .hover(move |style| style.bg(theme.soft()))
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .size(px(40.))
                .flex_shrink_0()
                .rounded_lg()
                .bg(theme.card())
                .flex()
                .items_center()
                .justify_center()
                .child(memory_icon(px(20.))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    ui::Text::new(ui::TextRole::Body, title)
                        .render(&theme)
                        .font_weight(FontWeight::MEDIUM),
                )
                .child(
                    ui::Text::new(ui::TextRole::Meta, description)
                        .render(&theme),
                ),
        )
        .child(
            ui::icons::chevron_right()
                .size(px(16.))
                .flex_shrink_0()
                .text_color(theme.muted()),
        )
        .on_click(cx.listener(|app, _, _, cx| app.open_import(cx)))
}

pub(super) fn footer(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    div().flex().child(
        ui::Button::new("open-import-memory", app.t("more.import"))
            .variant(ui::ControlVariant::Secondary)
            .framed()
            .icon(memory_icon(px(16.)))
            .render(
                &app.theme,
                cx.listener(|app, _, _, cx| app.open_import(cx)),
            ),
    )
}

fn memory_icon(size: gpui::Pixels) -> ui::icons::Icon {
    ui::icons::layout_grid().size(size)
}
