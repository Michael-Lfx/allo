//! Deliverables drawer: the files one turn produced, with a light preview.
//!
//! Kept apart from `starter` and `feed` so the drawer can be replaced without
//! touching the chat projection or the home stage.

use std::path::Path;
use std::sync::Arc;

use flowy_agent::{ChatLine, FileEntry};
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, FontWeight, Image,
    ImageFormat, IntoElement, SharedString, div, img, prelude::*, px,
};

use crate::panel::StudioPanel;

/// Truncated parent directory path, keeping the tail legible in a narrow drawer.
fn short_parent_path(path: &str) -> String {
    let parent = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default();
    const MAX: usize = 42;
    let count = parent.chars().count();
    if count <= MAX {
        return parent.to_owned();
    }
    let tail: String = parent.chars().skip(count - (MAX - 1)).collect();
    format!("\u{2026}{tail}")
}

fn status_pill(
    bg: gpui::Rgba,
    fg: gpui::Rgba,
    label: String,
    leading: Option<AnyElement>,
) -> AnyElement {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(bg)
        .flex()
        .items_center()
        .gap(px(5.))
        .children(leading)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(label),
        )
        .into_any_element()
}

/// Reading more than this on the UI path would stall the frame.
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;

/// What the right-hand inspector sheet shows when opened from the background
/// works popover (Plan §3.2 #2: single-work step trace without interrupting the
/// active main conversation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectedWork {
    Session { id: String, title: String },
    Receipt(flowy_agent::AcceptedMessage),
}

/// What the drawer shows for the selected file.
pub enum PreviewBody {
    Text(String),
    Image(Arc<Image>),
    /// Locale key explaining why the file cannot be shown.
    Notice(&'static str),
}

/// Reads one file for preview, classifying it by extension and size.
pub fn load_preview(path: &str) -> PreviewBody {
    let path = Path::new(path);
    if flowy_agent::has_reserved_device_component(path) {
        return PreviewBody::Notice("chat.preview_missing");
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || flowy_agent::is_windows_reparse_point(&meta)
    {
        return PreviewBody::Notice("chat.preview_missing");
    }
    if meta.len() > MAX_PREVIEW_BYTES {
        return PreviewBody::Notice("chat.preview_too_large");
    }
    let Ok(bytes) = std::fs::read(path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => PreviewBody::Image(Arc::new(Image::from_bytes(
            ImageFormat::Png,
            bytes,
        ))),
        Some("jpg" | "jpeg") => PreviewBody::Image(Arc::new(
            Image::from_bytes(ImageFormat::Jpeg, bytes),
        )),
        Some("webp") => PreviewBody::Image(Arc::new(Image::from_bytes(
            ImageFormat::Webp,
            bytes,
        ))),
        _ => {
            if bytes.contains(&0) {
                return PreviewBody::Notice("chat.preview_binary");
            }
            PreviewBody::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

impl StudioPanel {
    /// Unified right-hand sheet drawer: either the Single-Work Step Inspector
    /// (opened from the background-works popover) or Space Deliverables & file
    /// preview (opened from the top bar or inline receipt).
    pub(crate) fn right_sheet_drawer(
        &self,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let (title, badge_count, body) =
            if let Some(work) = &self.inspected_work {
                (
                    self.t("drawer.work_title"),
                    None,
                    self.work_inspector_view(work, cx),
                )
            } else {
                let files = self.deliverables.as_deref().unwrap_or(&[]);
                let count = (!files.is_empty()).then_some(files.len());
                let body = match (&self.preview_file, &self.preview_body) {
                    (Some(path), Some(preview)) => {
                        self.preview_view(path, preview, cx).into_any_element()
                    }
                    _ => self.deliverables_list(files, cx),
                };
                (self.t("chat.deliverables"), count, body)
            };

        let sheet = div()
            .id("deliverables-drawer")
            .w(px(360.))
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(10.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(self.theme.space(6.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(self.theme.ink())
                                    .child(title),
                            )
                            .when_some(badge_count, |row, count| {
                                row.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .rounded_full()
                                        .bg(self.theme.soft())
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .child(count.to_string()),
                                )
                            }),
                    )
                    .child(
                        theme::Button::new(
                            "deliverables-close",
                            self.t("chat.close"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.close_right_sheet(cx);
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .id("deliverables-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            );

        let scale = crate::motion::travel_scale(self.theme.motion_policy());
        if scale <= 0.0 {
            sheet.into_any_element()
        } else {
            sheet
                .with_animation(
                    // The drawer travels in from the edge. Closing removes it
                    // at once, which reads as a dismiss rather than a broken
                    // exit.
                    "deliverables-drawer",
                    Animation::new(crate::motion::SLOW)
                        .with_easing(crate::motion::out_quint()),
                    move |drawer, delta| {
                        drawer
                            .opacity(delta)
                            .relative()
                            .left(px(16. * scale * (1.0 - delta)))
                    },
                )
                .into_any_element()
        }
    }

    fn session_steps(&self, session_id: &str) -> Vec<String> {
        let lines = self.agent.session_lines(session_id);
        let last_user = lines
            .iter()
            .rposition(|line| matches!(line, ChatLine::User { .. }))
            .unwrap_or(0);
        let mut steps = Vec::new();
        for line in &lines[last_user..] {
            match line {
                ChatLine::ToolCall {
                    name,
                    target,
                    running,
                    ..
                } => {
                    let prefix =
                        if *running { "\u{25cf} " } else { "\u{2713} " };
                    let heading = if *running {
                        self.tf("chat.tool_live", &[("name", name)])
                    } else {
                        name.clone()
                    };
                    if target.trim().is_empty() {
                        steps.push(format!("{prefix}{heading}"));
                    } else {
                        let short: String = target.chars().take(48).collect();
                        steps.push(format!(
                            "{prefix}{heading} \u{00b7} {short}"
                        ));
                    }
                }
                ChatLine::Reasoning {
                    duration_secs,
                    live,
                    ..
                } => {
                    steps.push(if *live {
                        format!("\u{25cf} {}", self.t("chat.reasoning_live"))
                    } else {
                        let dur = self.format_duration_secs(*duration_secs);
                        format!(
                            "\u{2713} {}",
                            self.tf("chat.thought_for", &[("duration", &dur)])
                        )
                    });
                }
                ChatLine::Verification {
                    artifact_path,
                    ok,
                    override_used,
                    ..
                } => {
                    let name = artifact_path
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(artifact_path);
                    let status = if *ok {
                        self.t("verify.card.passed")
                    } else if *override_used {
                        self.t("verify.override.badge")
                    } else {
                        self.t("verify.card.failed")
                    };
                    steps.push(format!("{status} \u{00b7} {name}"));
                }
                ChatLine::ToolResult { text, is_error } if *is_error => {
                    let short: String = text.chars().take(56).collect();
                    steps.push(format!("\u{2717} {short}"));
                }
                _ => {}
            }
        }
        steps
    }

    fn work_inspector_view(
        &self,
        work: &InspectedWork,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        match work {
            InspectedWork::Session { id, title } => {
                let is_running = self.agent.busy_session_ids().contains(id);
                let needs_attention = self.agent.session_needs_attention(id);
                let steps = self.session_steps(id);
                let open_id = id.clone();
                let stop_id = id.clone();

                let status_badge = if is_running {
                    status_pill(
                        self.theme.status_active_bg(),
                        self.theme.status_active(),
                        self.t("chat.bg_task_running"),
                        Some(crate::motion::live_dot(
                            "inspector-running-dot",
                            self.theme.status_active(),
                            self.theme.motion_policy(),
                        )),
                    )
                } else if needs_attention {
                    status_pill(
                        self.theme.status_attention_bg(),
                        self.theme.status_attention(),
                        self.t("dock.attention_badge"),
                        None,
                    )
                } else {
                    status_pill(
                        self.theme.status_verified_bg(),
                        self.theme.status_verified(),
                        self.t("verify.card.passed"),
                        None,
                    )
                };

                let mut steps_col =
                    div().w_full().flex().flex_col().gap(self.theme.space(6.));
                if steps.is_empty() {
                    steps_col = steps_col.child(
                        div()
                            .w_full()
                            .px(self.theme.space(10.))
                            .py(self.theme.space(12.))
                            .rounded_lg()
                            .bg(self.theme.soft().opacity(0.45))
                            .text_xs()
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(self.t("drawer.work_no_steps")),
                    );
                } else {
                    for (step_ix, step_text) in steps.into_iter().enumerate() {
                        steps_col = steps_col.child(
                            div()
                                .id(("work-step", step_ix as u64))
                                .w_full()
                                .px(self.theme.space(10.))
                                .py(self.theme.space(8.))
                                .rounded_lg()
                                .border_1()
                                .border_color(self.theme.line())
                                .bg(self.theme.canvas())
                                .flex()
                                .items_start()
                                .gap(self.theme.space(8.))
                                .child(
                                    div()
                                        .size(px(18.))
                                        .flex_none()
                                        .rounded_full()
                                        .bg(self.theme.soft())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(self.theme.muted())
                                        .child((step_ix + 1).to_string()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(self.theme.ink())
                                        .whitespace_normal()
                                        .child(step_text),
                                ),
                        );
                    }
                }

                div()
                    .w_full()
                    .p(self.theme.space(12.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(12.))
                    .child(
                        div()
                            .w_full()
                            .p(self.theme.space(12.))
                            .rounded_xl()
                            .border_1()
                            .border_color(self.theme.line())
                            .bg(self.theme.canvas())
                            .flex()
                            .flex_col()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(self.theme.space(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(self.theme.ink())
                                            .whitespace_normal()
                                            .child(title.clone()),
                                    )
                                    .child(status_badge),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(self.theme.space(8.))
                                    .child(
                                        theme::Button::new(
                                            "work-inspector-open",
                                            self.t("drawer.work_open_session"),
                                        )
                                        .variant(
                                            theme::ControlVariant::Secondary,
                                        )
                                        .size(theme::ControlSize::Small)
                                        .render(
                                            &self.theme,
                                            cx.listener(
                                                move |panel, _, _, cx| {
                                                    panel.open_session(
                                                        &open_id, cx,
                                                    );
                                                    panel.close_right_sheet(cx);
                                                },
                                            ),
                                        ),
                                    )
                                    .when(is_running, |row| {
                                        row.child(
                                            theme::Button::new(
                                                "work-inspector-stop",
                                                self.t("drawer.work_stop"),
                                            )
                                            .variant(
                                                theme::ControlVariant::Ghost,
                                            )
                                            .size(theme::ControlSize::Small)
                                            .render(
                                                &self.theme,
                                                cx.listener(
                                                    move |panel, _, _, cx| {
                                                        panel
                                                            .stop_session_turn(
                                                                &stop_id, cx,
                                                            );
                                                    },
                                                ),
                                            ),
                                        )
                                    }),
                            ),
                    )
                    .child(steps_col)
                    .into_any_element()
            }
            InspectedWork::Receipt(receipt) => {
                let owner = self.receipt_owner(receipt);
                let hint = receipt.session_hint.clone();
                div()
                    .w_full()
                    .p(self.theme.space(12.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(12.))
                    .child(
                        div()
                            .w_full()
                            .p(self.theme.space(12.))
                            .rounded_xl()
                            .border_1()
                            .border_color(self.theme.line())
                            .bg(self.theme.canvas())
                            .flex()
                            .flex_col()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(self.theme.space(8.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(self.theme.muted())
                                            .child(owner),
                                    )
                                    .child(status_pill(
                                        self.theme.soft(),
                                        self.theme.muted(),
                                        self.t("chat.bg_task_queued"),
                                        None,
                                    )),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .p(self.theme.space(10.))
                                    .rounded_lg()
                                    .bg(self.theme.soft().opacity(0.45))
                                    .text_xs()
                                    .text_color(self.theme.ink())
                                    .whitespace_normal()
                                    .child(receipt.text.clone()),
                            )
                            .when_some(hint, |card, session_id| {
                                card.child(
                                    theme::Button::new(
                                        "receipt-inspector-open",
                                        self.t("drawer.work_open_session"),
                                    )
                                    .variant(theme::ControlVariant::Secondary)
                                    .size(theme::ControlSize::Small)
                                    .render(
                                        &self.theme,
                                        cx.listener(move |panel, _, _, cx| {
                                            panel.open_session(&session_id, cx);
                                            panel.close_right_sheet(cx);
                                        }),
                                    ),
                                )
                            }),
                    )
                    .into_any_element()
            }
        }
    }

    fn deliverables_list(
        &self,
        files: &[FileEntry],
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        if files.is_empty() {
            return div()
                .w_full()
                .px(self.theme.space(16.))
                .py(self.theme.space(32.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(self.theme.space(8.))
                .child(
                    theme::icons::document_file()
                        .size(px(24.))
                        .text_color(self.theme.muted()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .text_center()
                        .child(self.t("deliverables.empty")),
                )
                .into_any_element();
        }
        let mut column = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .p(self.theme.space(10.));
        for (index, file) in files.iter().enumerate() {
            let name = file
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&file.path)
                .to_owned();
            let parent = short_parent_path(&file.path);
            let file_kind = crate::file_kind::from_path(&file.path);
            let icon = file_kind.map_or_else(
                || {
                    theme::icons::document_file()
                        .size(px(18.))
                        .text_color(self.theme.muted())
                        .into_any_element()
                },
                |kind| kind.icon().size(px(20.)).into_any_element(),
            );
            let kind_label =
                self.t(crate::file_kind::kind_label_key(&file.path));
            let path = file.path.clone();
            column = column.child(
                theme::interactive(
                    ("deliverable", index as u64),
                    SharedString::from(name.clone()),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap(self.theme.space(10.))
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.canvas())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div()
                        .flex_none()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(name),
                        )
                        .when(!parent.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(parent),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .px(self.theme.space(6.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(self.theme.soft())
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(kind_label),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.set_preview_file(Some(path.clone()), cx);
                    },
                )),
            );
        }
        column.into_any_element()
    }

    fn preview_view(
        &self,
        path: &str,
        preview: &PreviewBody,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned();
        let external_path = path.to_owned();
        let body = match preview {
            PreviewBody::Text(text) => div()
                .p(self.theme.space(12.))
                .text_xs()
                .child(text.clone())
                .into_any_element(),
            PreviewBody::Image(image) => div()
                .p(self.theme.space(12.))
                .child(img(image.clone()).w_full())
                .into_any_element(),
            PreviewBody::Notice(key) => div()
                .p(self.theme.space(12.))
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t(key))
                .into_any_element(),
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .child(
                        theme::Button::new("preview-back", self.t("chat.back"))
                            .variant(theme::ControlVariant::Ghost)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(|panel, _, _, cx| {
                                    panel.set_preview_file(None, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(name),
                    )
                    .child(
                        theme::Button::new(
                            "preview-open-external",
                            self.t("deliverables.open_external"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                if open::that(&external_path).is_err() {
                                    panel.emit_notice(
                                        Some("library.error.open"),
                                        cx,
                                    );
                                }
                            }),
                        ),
                    ),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PREVIEW_BYTES, PreviewBody, load_preview};

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("flowy-deliverables-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_missing_file_reports_a_notice() {
        let preview = load_preview("flowy-definitely-absent-98765.txt");
        assert!(matches!(
            preview,
            PreviewBody::Notice("chat.preview_missing")
        ));
    }

    #[test]
    fn binary_content_reports_a_notice() {
        let path = scratch("binary.dat");
        std::fs::write(&path, [0u8, 1, 2, 3]).expect("write");
        assert!(matches!(
            load_preview(path.to_str().expect("utf8")),
            PreviewBody::Notice("chat.preview_binary")
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn text_content_is_returned_as_text() {
        let path = scratch("note.txt");
        std::fs::write(&path, "hello").expect("write");
        match load_preview(path.to_str().expect("utf8")) {
            PreviewBody::Text(text) => assert_eq!(text, "hello"),
            _ => panic!("expected a text preview"),
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_content_reports_a_notice() {
        let path = scratch("big.txt");
        let data = vec![b'a'; (MAX_PREVIEW_BYTES + 1) as usize];
        std::fs::write(&path, &data).expect("write");
        assert!(matches!(
            load_preview(path.to_str().expect("utf8")),
            PreviewBody::Notice("chat.preview_too_large")
        ));
        let _ = std::fs::remove_file(path);
    }
}
