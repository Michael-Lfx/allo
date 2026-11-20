//! Deliverables drawer: the files one turn produced, with a light preview.
//!
//! Kept apart from `starter` and `feed` so the drawer can be replaced without
//! touching the chat projection or the home stage.

use std::path::Path;
use std::sync::Arc;

use flowy_agent::FileEntry;
use flowy_ui as theme;
use gpui::{
    Context, Image, ImageFormat, IntoElement, SharedString, div, img, prelude::*,
    px,
};

use crate::panel::StudioPanel;

/// Reading more than this on the UI path would stall the frame.
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// What the drawer shows for the selected file.
pub(crate) enum PreviewBody {
    Text(String),
    Image(Arc<Image>),
    /// Locale key explaining why the file cannot be shown.
    Notice(&'static str),
}

/// Reads one file for preview, classifying it by extension and size.
pub(crate) fn load_preview(path: &str) -> PreviewBody {
    let path = Path::new(path);
    let Ok(meta) = std::fs::metadata(path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    if !meta.is_file() {
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
        .map(|ext| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => {
            PreviewBody::Image(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
        }
        Some("jpg" | "jpeg") => {
            PreviewBody::Image(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes)))
        }
        Some("webp") => {
            PreviewBody::Image(Arc::new(Image::from_bytes(ImageFormat::Webp, bytes)))
        }
        _ => {
            if bytes.contains(&0) {
                return PreviewBody::Notice("chat.preview_binary");
            }
            PreviewBody::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

impl StudioPanel {
    /// The whole right-hand drawer.
    pub(crate) fn deliverables_drawer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let files = self.deliverables.as_deref().unwrap_or(&[]);
        let title = self.t("chat.deliverables");
        let body = match (&self.preview_file, &self.preview_body) {
            (Some(path), Some(preview)) => {
                self.preview_view(path, preview, cx).into_any_element()
            }
            _ => self.deliverables_list(files, cx).into_any_element(),
        };
        div()
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
                    .child(div().flex_1().min_w_0().text_sm().child(title))
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
                                panel.close_deliverables(cx);
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
            )
    }

    fn deliverables_list(
        &self,
        files: &[FileEntry],
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut column = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(4.))
            .p(self.theme.space(8.));
        for (index, file) in files.iter().enumerate() {
            let name = file
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&file.path)
                .to_owned();
            let delta = if file.binary {
                "\u{2014}".to_owned()
            } else if file.removed == 0 {
                format!("+{}", file.added)
            } else {
                format!("+{} \u{2212}{}", file.added, file.removed)
            };
            let path = file.path.clone();
            column = column.child(
                theme::interactive(
                    ("deliverable", index as u64),
                    SharedString::from(name.clone()),
                    &self.theme,
                )
                .w_full()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .px(self.theme.space(8.))
                .py(self.theme.space(6.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_xs().child(name))
                        .child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .child(file.path.clone()),
                        ),
                )
                .child(div().text_xs().child(delta))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.set_preview_file(Some(path.clone()), cx);
                })),
            );
        }
        column
    }

    fn preview_view(
        &self,
        path: &str,
        preview: &PreviewBody,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(path)
            .to_owned();
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
                    .child(div().flex_1().min_w_0().text_xs().child(name)),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{load_preview, PreviewBody, MAX_PREVIEW_BYTES};

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "flowy-deliverables-{}-{name}",
            std::process::id()
        ))
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
