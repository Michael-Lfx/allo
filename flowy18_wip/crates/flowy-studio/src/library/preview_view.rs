use super::{ArtifactAvailability, LibraryItem, preview};
use flowy_i18n::I18n;
use flowy_ui::{self as ui, UiContext};
use gpui::{
    Context, Entity, EventEmitter, Focusable, Render, RenderImage, Task,
    Window, div, prelude::*, px,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum PreviewEvent {
    Close,
    ReadRequested {
        artifact_id: String,
        request_id: u64,
    },
    Retry,
    SaveAs(String),
    OpenDefault,
    LocateSource {
        session_id: String,
        artifact_id: String,
    },
}

pub struct PreviewPanel {
    item: LibraryItem,
    ui: UiContext,
    i18n: I18n,
    document: Option<Arc<preview::Document>>,
    error: Option<&'static str>,
    status: Option<&'static str>,
    page: usize,
    page_input: Option<Entity<gpui_kit::base::input::InputState>>,
    /// Page number waiting for the next render, which is the first point with a
    /// `Window` available to build or update the page field.
    page_pending: Option<String>,
    zoom: Option<f32>,
    effective_zoom: f32,
    cache: BTreeMap<(usize, u32), Arc<RenderImage>>,
    failed_prefetch: BTreeSet<usize>,
    pending_drops: Vec<Arc<RenderImage>>,
    requested: Option<(usize, u32)>,
    load_task: Option<Task<()>>,
    render_task: Option<Task<()>>,
    generation: u64,
    request_id: u64,
    scroll: gpui::ScrollHandle,
    focus: gpui::FocusHandle,
}

impl std::fmt::Debug for PreviewPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewPanel")
            .field("item", &self.item)
            .field("page", &self.page)
            .field("error", &self.error)
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<PreviewEvent> for PreviewPanel {}
impl Focusable for PreviewPanel {
    fn focus_handle(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.ui
            .overlays
            .read(cx)
            .modal_focus()
            .unwrap_or_else(|| self.focus.clone())
    }
}

impl PreviewPanel {
    pub fn new(
        item: LibraryItem,
        request_id: u64,
        i18n: I18n,
        ui: UiContext,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let page_input = None;
        Self {
            item,
            ui,
            i18n,
            document: None,
            error: None,
            status: None,
            page: 0,
            page_input,
            page_pending: Some("1".to_owned()),
            zoom: None,
            effective_zoom: 1.,
            cache: BTreeMap::new(),
            failed_prefetch: BTreeSet::new(),
            pending_drops: Vec::new(),
            requested: None,
            load_task: None,
            render_task: None,
            generation: 0,
            request_id,
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
        }
    }
    pub fn set_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
        cx.notify();
    }
    pub fn set_status(
        &mut self,
        status: Option<&'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        self.status = status;
        cx.notify();
    }
    pub fn set_item(&mut self, item: LibraryItem, cx: &mut Context<'_, Self>) {
        if item.id == self.item.id {
            if item.availability != ArtifactAvailability::Available {
                self.generation = self.generation.wrapping_add(1);
                self.request_id = self.request_id.wrapping_add(1);
                self.load_task = None;
                self.render_task = None;
                self.document = None;
                self.requested = None;
                self.pending_drops
                    .extend(std::mem::take(&mut self.cache).into_values());
                self.error = Some(match item.availability {
                    ArtifactAvailability::Missing => {
                        "library.availability.missing"
                    }
                    ArtifactAvailability::Corrupt => {
                        "library.availability.corrupt"
                    }
                    ArtifactAvailability::Available => unreachable!(),
                });
            }
            self.item = item;
            cx.notify();
        }
    }
    fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    pub fn request_bytes(
        &mut self,
        request_id: u64,
        cx: &mut Context<'_, Self>,
    ) {
        self.request_id = request_id;
        self.generation = self.generation.wrapping_add(1);
        self.load_task = None;
        self.render_task = None;
        self.requested = None;
        self.failed_prefetch.clear();
        self.error = None;
        self.document = None;
        self.page = 0;
        self.zoom = None;
        self.page_pending = Some("1".to_owned());
        cx.notify();
        self.pending_drops
            .extend(std::mem::take(&mut self.cache).into_values());
        if !self.item.inline_preview_supported()
            && self.item.availability == ArtifactAvailability::Available
        {
            self.error = Some("library.error.unsupported");
            cx.notify();
            return;
        }
        cx.emit(PreviewEvent::ReadRequested {
            artifact_id: self.item.id.clone(),
            request_id,
        });
    }

    pub fn set_bytes(
        &mut self,
        request_id: u64,
        result: Result<Vec<u8>, &'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        if request_id != self.request_id {
            return;
        }
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error = Some(error);
                self.status = None;
                cx.notify();
                return;
            }
        };
        let generation = self.generation;
        let extension = self.item.extension.clone();
        let job = cx
            .background_executor()
            .spawn(async move { preview::load(bytes, &extension) });
        self.load_task = Some(cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(doc) => this.document = Some(doc),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
    }

    pub fn release(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.request_id = self.request_id.wrapping_add(1);
        self.load_task = None;
        self.render_task = None;
        self.document = None;
        self.pending_drops
            .extend(std::mem::take(&mut self.cache).into_values());
        for image in self.pending_drops.drain(..) {
            cx.drop_image(image, Some(window));
        }
    }

    fn go(&mut self, page: usize, cx: &mut Context<'_, Self>) {
        let Some(doc) = &self.document else {
            return;
        };
        if page >= doc.dims.len() {
            return;
        }
        self.page = page;
        self.requested = None;
        self.error = None;
        self.page_pending = Some((page + 1).to_string());
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    fn go_input(&mut self, cx: &mut Context<'_, Self>) {
        let typed = self
            .page_input
            .as_ref()
            .map_or_default(|input| input.read(cx).value());
        if let Ok(page) = typed.parse::<usize>()
            && page > 0
        {
            self.go(page - 1, cx);
        }
        self.page_pending = Some((self.page + 1).to_string());
        cx.notify();
    }

    /// Build or update the page field.
    ///
    /// `InputState` needs a `Window` to shape its text, and the panel is
    /// constructed from `cx.new` callbacks that have none, so the field is
    /// created on the first render and updated there from `page_pending`.
    fn page_field(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<gpui_kit::base::input::InputState> {
        if let Some(input) = self.page_input.clone() {
            if let Some(text) = self.page_pending.take() {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
            return input;
        }
        let text = self.page_pending.take().unwrap_or_else(|| "1".to_owned());
        let input = cx.new(|cx| {
            gpui_kit::base::input::InputState::new(window, cx)
                .default_value(text)
        });
        self.page_input = Some(input.clone());
        input
    }

    fn change_zoom(&mut self, multiplier: f32, cx: &mut Context<'_, Self>) {
        self.set_zoom(
            Some((self.effective_zoom * multiplier).clamp(0.1, 4.)),
            cx,
        );
    }

    fn set_zoom(&mut self, zoom: Option<f32>, cx: &mut Context<'_, Self>) {
        self.zoom = zoom;
        self.requested = None;
        if self.document.is_some() {
            self.error = None;
        }
        cx.notify();
    }

    fn request_render(
        &mut self,
        scale: f32,
        cache_budget: usize,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let scale_key = if document.is_image() {
            100
        } else {
            (scale * 100.).round().clamp(1., 1600.) as u32
        };
        let page = self.page;
        let key = (page, scale_key);
        if self.requested != Some(key) {
            self.requested = Some(key);
            self.failed_prefetch.clear();
        }
        // Keep one job in flight even when the page or zoom changes. Admit the
        // next result only after reserving room alongside all retained rasters.
        if self.render_task.is_some() || self.error.is_some() {
            return;
        }
        let current_cached = self.cache.contains_key(&key);
        self.cache.retain(|(index, zoom), image| {
            let keep = (*zoom == scale_key && index.abs_diff(page) <= 1)
                || (!current_cached && *index == page);
            if !keep {
                cx.drop_image(image.clone(), Some(window));
            }
            keep
        });
        let available = cache_budget
            - self
                .cache
                .values()
                .map(|image| raster_bytes(image))
                .sum::<usize>();
        let scale = scale_key as f32 / 100.;
        let (index, reserved) = if current_cached {
            let next = [Some(page + 1), page.checked_sub(1)]
                .into_iter()
                .flatten()
                .filter(|index| {
                    *index < document.dims.len()
                        && !self.cache.contains_key(&(*index, scale_key))
                        && !self.failed_prefetch.contains(index)
                })
                .find_map(|index| {
                    let bytes = document.raster_bytes(index, scale).ok()?;
                    (bytes <= available).then_some((index, bytes))
                });
            let Some(next) = next else { return };
            next
        } else {
            let reserved = match document.raster_bytes(page, scale) {
                Ok(bytes) if bytes <= cache_budget => bytes,
                Ok(_) => {
                    self.error = Some("library.error.large");
                    return;
                }
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            if reserved > available {
                for image in std::mem::take(&mut self.cache).into_values() {
                    cx.drop_image(image, Some(window));
                }
            }
            (page, reserved)
        };
        let generation = self.generation;
        let job = cx.background_executor().spawn(async move {
            preview::render(&document, index, scale).and_then(|image| {
                (raster_bytes(&image) <= reserved)
                    .then_some(image)
                    .ok_or("library.error.render")
            })
        });
        self.render_task = Some(cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.render_task = None;
                if this.requested == Some(key) {
                    match result {
                        Ok(image) => {
                            this.cache.insert((index, scale_key), image);
                        }
                        Err(error) if index == page => this.error = Some(error),
                        Err(_) => {
                            this.failed_prefetch.insert(index);
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    fn controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> gpui::Div {
        let t = self.ui.theme.read(cx).resolved();
        let count = self.document.as_ref().map_or(0, |doc| doc.dims.len());
        let previous = super::test_control(
            "preview-previous",
            ui::IconButton::new(
                "preview-previous",
                self.t("library.preview.previous"),
                ui::icons::chevron_left(),
            )
            .disabled(self.page == 0)
            .render(
                &t,
                cx.listener(|this, _, _, cx| {
                    this.go(this.page.saturating_sub(1), cx);
                }),
            ),
        );
        let next = super::test_control(
            "preview-next",
            ui::IconButton::new(
                "preview-next",
                self.t("library.preview.next"),
                ui::icons::chevron_right(),
            )
            .disabled(self.page + 1 >= count)
            .render(
                &t,
                cx.listener(|this, _, _, cx| this.go(this.page + 1, cx)),
            ),
        );
        let page = div()
            .w(px(60.))
            .child(
                ui::TextField::new(
                    "preview-page",
                    self.t("library.preview.page"),
                    self.page_field(window, cx),
                )
                .render(&t, cx),
            )
            .on_action(cx.listener(
                |this, _: &gpui_kit::base::input::Enter, _, cx| {
                    this.go_input(cx);
                },
            ));
        let go = ui::Button::new("preview-go", self.t("library.preview.go"))
            .size(ui::ControlSize::Small)
            .render(&t, cx.listener(|this, _, _, cx| this.go_input(cx)));
        let out = ui::IconButton::new(
            "preview-zoom-out",
            self.t("library.preview.zoom_out"),
            ui::icons::minus(),
        )
        .render(&t, cx.listener(|this, _, _, cx| this.change_zoom(0.8, cx)));
        let zoom_in = super::test_control(
            "preview-zoom-in",
            ui::IconButton::new(
                "preview-zoom-in",
                self.t("library.preview.zoom_in"),
                ui::icons::plus(),
            )
            .render(
                &t,
                cx.listener(|this, _, _, cx| this.change_zoom(1.25, cx)),
            ),
        );
        let fit = super::test_control(
            "preview-fit",
            ui::Button::new("preview-fit", self.t("library.preview.fit"))
                .size(ui::ControlSize::Small)
                .variant(ui::ControlVariant::Ghost)
                .render(
                    &t,
                    cx.listener(|this, _, _, cx| this.set_zoom(None, cx)),
                ),
        );
        let actual =
            ui::Button::new("preview-actual", self.t("library.preview.actual"))
                .size(ui::ControlSize::Small)
                .variant(ui::ControlVariant::Ghost)
                .render(
                    &t,
                    cx.listener(|this, _, _, cx| this.set_zoom(Some(1.), cx)),
                );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .px(px(16.))
            .py(px(10.))
            .rounded_b(t.modal_radius())
            .bg(t.card())
            .child(div().flex().items_center().gap(px(6.)).when(
                count > 1,
                |row| {
                    row.child(previous)
                        .child(next)
                        .child(page)
                        .child(format!("/ {count}"))
                        .child(go)
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(out)
                    .child(format!("{:.0}%", self.effective_zoom * 100.))
                    .child(zoom_in)
                    .child(fit)
                    .child(actual),
            )
    }
}

impl Render for PreviewPanel {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        for image in self.pending_drops.drain(..) {
            cx.drop_image(image, Some(window));
        }
        let t = self.ui.theme.read(cx).resolved();
        let frame = t.modal_size(window.viewport_size());
        let stage = (
            f32::from(frame.width).max(200.) - 32.,
            (f32::from(frame.height) - 180.).max(100.),
        );
        let dims = self
            .document
            .as_ref()
            .and_then(|doc| doc.dims.get(self.page))
            .copied();
        if let Some((w, h)) = dims {
            self.effective_zoom =
                self.zoom.unwrap_or_else(|| (stage.0 / w).min(stage.1 / h));
            self.request_render(
                self.effective_zoom * window.scale_factor(),
                preview::CACHE_BYTES,
                window,
                cx,
            );
        }
        let open = super::test_control(
            "preview-open-default",
            ui::Button::new(
                "preview-open-default",
                self.t("library.open_default"),
            )
            .size(ui::ControlSize::Small)
            .icon(ui::icons::external_link())
            .disabled(!self.item.default_open_supported())
            .render(
                &t,
                cx.listener(|_, _, _, cx| cx.emit(PreviewEvent::OpenDefault)),
            ),
        );
        let save_as =
            ui::Button::new("preview-save-as", self.t("library.save_as"))
                .size(ui::ControlSize::Small)
                .icon(ui::icons::download())
                .disabled(
                    self.item.availability != ArtifactAvailability::Available,
                )
                .render(
                    &t,
                    cx.listener(|this, _, _, cx| {
                        cx.emit(PreviewEvent::SaveAs(this.item.id.clone()));
                    }),
                );
        let close = ui::IconButton::new(
            "library-close-preview",
            self.t("library.close"),
            ui::icons::x(),
        )
        .render(&t, cx.listener(|_, _, _, cx| cx.emit(PreviewEvent::Close)));
        let format = self.item.extension.to_uppercase();
        let file_info = self
            .document
            .as_ref()
            .map(|doc| doc.file_size)
            .or(self.item.size_bytes)
            .map(|bytes| {
                self.i18n.tf(
                    "library.preview.file_info",
                    &[
                        ("format", &format),
                        ("size", &format!("{:.1}", bytes as f64 / 1024.)),
                    ],
                )
            });
        let source_label = if self.item.source_deleted {
            self.t("library.source.deleted")
        } else if self.item.source_unavailable {
            self.t("library.source.unavailable")
        } else if self.item.source_title.is_empty() {
            self.t("library.source.label")
        } else {
            self.item.source_title.clone()
        };
        let mut header = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .py(px(12.))
            .rounded_t(t.modal_radius())
            .bg(t.card())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        ui::Title::new(
                            ui::TitleLevel::Card,
                            self.item.title.clone(),
                        )
                        .truncate()
                        .render(&t),
                    )
                    .child(
                        ui::Text::new(ui::TextRole::Meta, source_label)
                            .truncate()
                            .render(&t),
                    )
                    .when_some(file_info, |col, info| {
                        col.child(
                            ui::Text::new(ui::TextRole::Meta, info).render(&t),
                        )
                    }),
            )
            .child(save_as);
        if self.item.default_open_supported() {
            header = header.child(open);
        }
        if !self.item.source_deleted
            && !self.item.source_unavailable
            && !self.item.source_session_id.is_empty()
        {
            header = header.child(
                ui::IconButton::new(
                    "preview-locate-source",
                    self.t("library.locate_task"),
                    ui::icons::external_link(),
                )
                .render(
                    &t,
                    cx.listener(|this, _, _, cx| {
                        cx.emit(PreviewEvent::LocateSource {
                            session_id: this.item.source_session_id.clone(),
                            artifact_id: this.item.id.clone(),
                        });
                    }),
                ),
            );
        }
        header = header.child(close);
        let mut body = div()
            .id("preview-scroll")
            .track_scroll(&self.scroll)
            .overflow_scroll()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .bg(t.soft());
        if self.document.is_none() {
            body = body.rounded_b(t.modal_radius());
        }
        if let Some(error) = self.error {
            body = body.child(
                div()
                    .size_full()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(14.))
                    .child(
                        ui::icons::file_fold()
                            .size(px(36.))
                            .text_color(t.muted()),
                    )
                    .child(
                        ui::Text::new(ui::TextRole::Body, self.t(error))
                            .render(&t)
                            .whitespace_normal(),
                    )
                    .when(
                        !matches!(
                            error,
                            "library.error.unsupported" | "library.error.large"
                        ),
                        |body| {
                            body.child(
                                ui::Button::new(
                                    "preview-retry",
                                    self.t("library.preview.retry"),
                                )
                                .icon(ui::icons::refresh_cw())
                                .render(
                                    &t,
                                    cx.listener(|_, _, _, cx| {
                                        cx.emit(PreviewEvent::Retry);
                                    }),
                                ),
                            )
                        },
                    ),
            );
        } else if let Some(((w, h), image)) = dims.zip(
            self.requested
                .and_then(|key| self.cache.get(&key))
                .or_else(|| {
                    self.cache
                        .range((self.page, 0)..=(self.page, u32::MAX))
                        .next()
                        .map(|(_, image)| image)
                })
                .cloned(),
        ) {
            let (w, h) = (w * self.effective_zoom, h * self.effective_zoom);
            body = body.child(
                div()
                    .w(px(stage.0.max(w + 32.)))
                    .h(px(stage.1.max(h + 32.)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(16.))
                    .child(
                        gpui::img(image)
                            .w(px(w))
                            .h(px(h))
                            .object_fit(gpui::ObjectFit::Contain),
                    ),
            );
        } else {
            body = body.child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .child(
                        div().w(px(160.)).h(px(5.)).rounded_full().bg(t.line()),
                    )
                    .child(
                        ui::Text::new(
                            ui::TextRole::Body,
                            self.t("library.preview.loading"),
                        )
                        .render(&t),
                    ),
            );
        }
        let warning = self.document.as_ref().is_some_and(|doc| doc.warning);
        ui::dialog(
            "library-preview-dialog",
            self.item.title.clone(),
            &t,
            &self.ui,
            cx,
        )
        .w(frame.width)
        .h(frame.height)
        .flex()
        .flex_col()
        .overflow_hidden()
        .on_key_down(cx.listener(
            |this, event: &gpui::KeyDownEvent, window, cx| {
                let page_focus = this
                    .page_input
                    .as_ref()
                    .map(|input| input.read(cx).focus_handle(cx));
                if page_focus.as_ref().is_some_and(|focus| {
                    window.focused(cx).as_ref() == Some(focus)
                }) {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "left" | "pageup" => {
                        this.go(this.page.saturating_sub(1), cx);
                    }
                    "right" | "pagedown" => this.go(this.page + 1, cx),
                    _ => return,
                }
                cx.stop_propagation();
            },
        ))
        .child(header)
        .when_some(self.status, |dialog, key| {
            dialog.child(
                div().px(px(16.)).py(px(8.)).child(
                    ui::Text::new(ui::TextRole::Body, self.t(key))
                        .render(&t)
                        .whitespace_normal(),
                ),
            )
        })
        .when(warning, |dialog| {
            dialog.child(
                div().px(px(16.)).py(px(6.)).bg(t.soft()).child(
                    ui::Text::new(
                        ui::TextRole::Meta,
                        self.t("library.preview.approximate"),
                    )
                    .render(&t)
                    .whitespace_normal(),
                ),
            )
        })
        .child(body)
        .when(self.document.is_some(), |dialog| {
            dialog.child(self.controls(window, cx))
        })
    }
}

fn raster_bytes(image: &RenderImage) -> usize {
    image.as_bytes(0).map_or(0, <[u8]>::len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn load_preview(cx: &mut VisualTestContext) -> Entity<PreviewPanel> {
        // Keep the test window empty so only explicit render requests drive the
        // scheduler. The existing three-page fixture exercises real rasters.
        let panel = cx.update(|_, cx| {
            let ui = UiContext::new(ui::ThemeOptions::default(), false, cx);
            cx.new(|cx| {
                PreviewPanel::new(
                    LibraryItem {
                        id: "preview-test".into(),
                        title: "Preview test".into(),
                        original_name: "preview-test.pdf".into(),
                        description: String::new(),
                        category: super::super::Category::Document,
                        date: String::new(),
                        time: String::new(),
                        pending: false,
                        preview: None,
                        size_bytes: None,
                        extension: "pdf".into(),
                        media_type: None,
                        delivered_at_ms: 0,
                        delivered_date: String::new(),
                        delivered_time: String::new(),
                        source_session_id: "session".into(),
                        source_title: "Preview test".into(),
                        source_deleted: false,
                        source_unavailable: false,
                        favorite: false,
                        availability: ArtifactAvailability::Available,
                    },
                    1,
                    I18n::embedded("Flowy"),
                    ui,
                    cx,
                )
            })
        });
        panel.update(cx, |panel, cx| {
            panel.set_bytes(
                1,
                Ok(include_bytes!(
                    "../../examples/fixtures/preview/sample.pdf"
                )
                .to_vec()),
                cx,
            );
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| assert!(panel.document.is_some()));
        panel
    }

    fn request(
        cx: &mut VisualTestContext,
        panel: &Entity<PreviewPanel>,
        scale: f32,
        budget: usize,
    ) {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.request_render(scale, budget, window, cx);
            });
        });
    }

    #[gpui::test]
    fn unavailable_item_invalidates_preview_and_late_host_bytes(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let panel = load_preview(cx);
        request(cx, &panel, 0.01, preview::CACHE_BYTES);
        cx.run_until_parked();
        panel.update(cx, |panel, cx| {
            assert!(panel.document.is_some());
            let mut missing = panel.item.clone();
            missing.availability = ArtifactAvailability::Missing;
            panel.set_item(missing, cx);
            assert!(panel.document.is_none());
            assert!(panel.cache.is_empty());
            assert_eq!(panel.error, Some("library.availability.missing"));
            panel.set_bytes(
                1,
                Ok(include_bytes!(
                    "../../examples/fixtures/preview/sample.pdf"
                )
                .to_vec()),
                cx,
            );
            assert!(panel.load_task.is_none());
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| assert!(panel.document.is_none()));
    }

    #[gpui::test]
    fn newer_host_read_ignores_previous_payload(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let panel = load_preview(cx);
        panel.update(cx, |panel, cx| {
            panel.request_bytes(2, cx);
            panel.set_bytes(
                1,
                Ok(include_bytes!(
                    "../../examples/fixtures/preview/sample.pdf"
                )
                .to_vec()),
                cx,
            );
            assert!(panel.load_task.is_none());
            panel.set_bytes(
                2,
                Ok(include_bytes!(
                    "../../examples/fixtures/preview/sample.pdf"
                )
                .to_vec()),
                cx,
            );
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.document.as_ref().unwrap().dims.len(), 3);
        });
    }

    #[gpui::test]
    fn current_raster_has_priority_over_prefetch_and_zoom_fallback(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let panel = load_preview(cx);
        let budget = 800;
        request(cx, &panel, 0.01, budget);
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(
                panel.cache.keys().copied().collect::<Vec<_>>(),
                [(0, 1)]
            );
        });
        request(cx, &panel, 0.01, budget);
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.cache.contains_key(&(1, 1)));
        });

        panel.update(cx, |panel, cx| panel.set_zoom(Some(0.02), cx));
        request(cx, &panel, 0.02, budget);
        cx.read_entity(&panel, |panel, _| {
            // The old current-page fallback plus this output would exceed
            // the budget, so release the fallback before starting the job.
            assert!(panel.cache.is_empty());
            assert!(panel.render_task.is_some());
        });
        cx.run_until_parked();
        request(cx, &panel, 0.02, budget);
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.render_task.is_none());
            assert_eq!(
                panel.cache.keys().copied().collect::<Vec<_>>(),
                [(0, 2)]
            );
            assert!(raster_bytes(&panel.cache[&(0, 2)]) <= budget);
        });
    }

    #[gpui::test]
    fn page_and_zoom_changes_discard_the_in_flight_raster(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let panel = load_preview(cx);
        let budget = 800;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.request_render(0.01, budget, window, cx);
                panel.go(2, cx);
                panel.set_zoom(Some(0.02), cx);
                panel.request_render(0.02, budget, window, cx);
                assert!(panel.render_task.is_some());
                assert_eq!(panel.requested, Some((2, 2)));
            });
        });
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.render_task.is_none());
            assert!(panel.cache.is_empty());
            assert!(panel.error.is_none());
        });
        request(cx, &panel, 0.02, budget);
        cx.run_until_parked();
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(
                panel.cache.keys().copied().collect::<Vec<_>>(),
                [(2, 2)]
            );
        });
    }
}
