use super::FlowyApp;
use flowy_ui as ui;
use gpui::{Context, Entity, Pixels, div, prelude::*, px};
use gpui_kit::base::input::{InputState, TextareaState};

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
        ))
        .child(field(
            app,
            cx,
            "more-occupation",
            "more.occupation",
            "more.occupation_placeholder",
            &app.persona.occupation,
        ));

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(identity)
        .child(area(
            app,
            cx,
            "more-about",
            "more.about",
            "more.about_placeholder",
            &app.persona.about,
        ))
        .child(
            ui::Text::new(ui::TextRole::Meta, app.t("more.about_hint"))
                .render(&app.theme),
        )
        .child(div().border_b_1().border_color(app.theme.chrome_line()))
        .child(div().pt_2().child(area(
            app,
            cx,
            "more-instructions",
            "more.instructions",
            "more.instructions_placeholder",
            &app.persona.instructions,
        )))
}

fn field(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
    id: &'static str,
    label_key: &'static str,
    placeholder_key: &'static str,
    state: &Entity<InputState>,
) -> impl IntoElement {
    let label = app.t(label_key);
    let input = ui::TextField::new(id, label.clone(), state.clone())
        .placeholder(app.t(placeholder_key))
        .quiet();

    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(ui::Text::new(ui::TextRole::Body, label).render(&app.theme))
        .child(input.render(&app.theme, cx))
}

/// The multi-line twin of [`field`]: same shell, fixed editor height.
fn area(
    app: &FlowyApp,
    cx: &Context<'_, FlowyApp>,
    id: &'static str,
    label_key: &'static str,
    placeholder_key: &'static str,
    state: &Entity<TextareaState>,
) -> impl IntoElement {
    let label = app.t(label_key);
    let input = ui::TextAreaField::new(id, label.clone(), state.clone())
        .placeholder(app.t(placeholder_key))
        .quiet()
        .height(px(146.))
        .render(&app.theme, cx)
        // Plain Enter is bound to the composer's submit action for every
        // `Input`-context editor; keep it a line break in this field.
        .on_action(cx.listener({
            let state = state.clone();
            move |app, _: &flowy_studio::SubmitComposer, window, cx| {
                app.insert_line_break(&state, window, cx);
            }
        }));

    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(ui::Text::new(ui::TextRole::Body, label).render(&app.theme))
        .child(input)
}
