use super::FlowyApp;
use flowy_ui as ui;
use gpui::{Context, Entity, Pixels, div, prelude::*, px};
use gpui_ce_elements::editable_text::EditableTextState;

pub(super) fn form(
    app: &FlowyApp,
    width: Pixels,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    let identity = div()
        .flex()
        .when(width < px(480.), Styled::flex_col)
        .gap_4()
        .child(field(
            app,
            cx,
            "more-nickname",
            "more.nickname",
            "more.nickname_placeholder",
            &app.persona.nickname,
            false,
        ))
        .child(field(
            app,
            cx,
            "more-occupation",
            "more.occupation",
            "more.occupation_placeholder",
            &app.persona.occupation,
            false,
        ));

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(identity)
        .child(field(
            app,
            cx,
            "more-about",
            "more.about",
            "more.about_placeholder",
            &app.persona.about,
            true,
        ))
        .child(
            ui::Text::new(ui::TextRole::Meta, app.t("more.about_hint"))
                .render(&app.theme),
        )
        .child(div().border_b_1().border_color(app.theme.chrome_line()))
        .child(div().pt_2().child(field(
            app,
            cx,
            "more-instructions",
            "more.instructions",
            "more.instructions_placeholder",
            &app.persona.instructions,
            true,
        )))
}

fn field(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
    id: &'static str,
    label_key: &'static str,
    placeholder_key: &'static str,
    state: &Entity<EditableTextState>,
    multiline: bool,
) -> impl IntoElement {
    let label = app.t(label_key);
    let input = ui::TextField::new(id, label.clone(), state.clone())
        .placeholder(app.t(placeholder_key))
        .quiet();
    let input = if multiline {
        input.multiline_height(px(146.))
    } else {
        input
    };

    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(ui::Text::new(ui::TextRole::Body, label).render(&app.theme))
        .child(input.render(&app.theme, cx))
}
