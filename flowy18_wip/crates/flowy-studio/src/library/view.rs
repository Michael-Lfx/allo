use super::model::{
    ArtifactAvailability, Category, LibraryItem, LibraryModel, LibrarySort,
    LibraryUsage, LibraryView,
};
use super::preview_view::{PreviewEvent, PreviewPanel};
use super::test_control;
use crate::file_kind::FileKind;
use flowy_i18n::I18n;
use flowy_ui::{self as theme, UiContext};
use gpui::{
    Context, Entity, FocusHandle, IntoElement, ParentElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryEvent {
    PreviewRequested(String),
    ReadPreviewRequested {
        artifact_id: String,
        request_id: u64,
    },
    SaveAsRequested(String),
    OpenWithDefaultRequested(String),
    LocateSourceRequested {
        session_id: String,
        artifact_id: String,
    },
    SetFavoriteRequested {
        artifact_id: String,
        favorite: bool,
    },
    StartConversationRequested,
    ClearFiltersRequested,
    CleanViewCopiesRequested,
}

impl gpui::EventEmitter<LibraryEvent> for LibraryPanel {}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Popup {
    Sort,
    Item(String),
}

/// `IconPark` file-type artwork copied from the parent UI assets so library
/// documents use the same language as the chat deliverables cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibraryFileIcon {
    Code,
    Document,
    Java,
    Image,
    Audio,
    Video,
    Pdf,
    Presentation,
    Spreadsheet,
    Archive,
}

impl LibraryFileIcon {
    fn from_item(item: &LibraryItem) -> Option<Self> {
        let titled = crate::file_kind::from_path(item.title.as_str());
        let extension =
            item.extension.trim_start_matches('.').to_ascii_lowercase();
        let extended = if extension.is_empty() {
            None
        } else {
            crate::file_kind::from_path(&format!("file.{extension}"))
        };
        if let Some(kind) = titled.or(extended) {
            return Some(kind.into());
        }
        match item.category {
            Category::Document => Some(Self::Document),
            Category::Table => Some(Self::Spreadsheet),
            Category::Image => Some(Self::Image),
            Category::Slides => Some(Self::Presentation),
            Category::Video => Some(Self::Video),
            Category::Audio => Some(Self::Audio),
            Category::Other => Some(Self::Archive),
            Category::All
            | Category::Website
            | Category::Game
            | Category::MobileApp => None,
        }
    }

    fn icon(self) -> gpui::Img {
        FileKind::from(self).icon()
    }
}

/// Keeps the view's historical names readable while the classification and
/// the artwork live in [`FileKind`].
impl From<FileKind> for LibraryFileIcon {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::Code => Self::Code,
            FileKind::Document => Self::Document,
            FileKind::Java => Self::Java,
            FileKind::Image => Self::Image,
            FileKind::Audio => Self::Audio,
            FileKind::Video => Self::Video,
            FileKind::Pdf => Self::Pdf,
            FileKind::Presentation => Self::Presentation,
            FileKind::Spreadsheet => Self::Spreadsheet,
            FileKind::Archive => Self::Archive,
        }
    }
}

impl From<LibraryFileIcon> for FileKind {
    fn from(icon: LibraryFileIcon) -> Self {
        match icon {
            LibraryFileIcon::Code => Self::Code,
            LibraryFileIcon::Document => Self::Document,
            LibraryFileIcon::Java => Self::Java,
            LibraryFileIcon::Image => Self::Image,
            LibraryFileIcon::Audio => Self::Audio,
            LibraryFileIcon::Video => Self::Video,
            LibraryFileIcon::Pdf => Self::Pdf,
            LibraryFileIcon::Presentation => Self::Presentation,
            LibraryFileIcon::Spreadsheet => Self::Spreadsheet,
            LibraryFileIcon::Archive => Self::Archive,
        }
    }
}

pub fn artifact_file_icon(item: &LibraryItem) -> Option<gpui::Img> {
    LibraryFileIcon::from_item(item).map(LibraryFileIcon::icon)
}

const LIST_ROW_HEIGHT: f32 = 64.0;
/// Two-up at the 960px content cap (`3 * 320 + 2 * 16 > 960`) and one-up on
/// narrow windows. Keep in sync with the library reference notes.
const CARD_MIN_WIDTH: f32 = 320.0;

/// A resource library whose contents are supplied by its host.
pub struct LibraryPanel {
    pub model: LibraryModel,
    ui: UiContext,
    i18n: I18n,
    query: Option<Entity<gpui_kit::base::input::InputState>>,
    popup: Option<Popup>,
    preview: Option<String>,
    preview_view: Option<Entity<PreviewPanel>>,
    preview_subscription: Option<Subscription>,
    preview_observer: Option<Subscription>,
    return_focus: Option<FocusHandle>,
    status: Option<&'static str>,
    loading: bool,
    internal_previews: bool,
    preview_request_id: u64,
    focus: FocusHandle,
    scroll: gpui::ScrollHandle,
    usage: Option<LibraryUsage>,
    category_focus: Vec<FocusHandle>,
    item_focus: std::collections::HashMap<String, FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl std::fmt::Debug for LibraryPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryPanel")
            .field("model", &self.model)
            .field("popup", &self.popup)
            .field("preview", &self.preview)
            .finish_non_exhaustive()
    }
}

impl LibraryPanel {
    pub fn new(
        items: Vec<LibraryItem>,
        i18n: I18n,
        ui: UiContext,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        // `gpui_kit`'s `InputState` is created from a window, so the search
        // field is built on the first render (`ensure_query`) instead of here;
        // hosts construct the panel from windowless boot paths.
        let subscriptions = vec![
            cx.observe(&ui.theme, |_, _, cx| cx.notify()),
            cx.observe(&ui.overlays, |_, _, cx| cx.notify()),
            cx.subscribe(
                &ui.overlays,
                |panel, _, event: &theme::OverlayEvent, cx| {
                    let theme::OverlayEvent::CloseRequested { scope, .. } =
                        event;
                    if scope.starts_with("library-") {
                        if scope.as_ref() == "library-preview" {
                            panel.preview = None;
                        } else {
                            panel.popup = None;
                        }
                        cx.notify();
                    }
                },
            ),
        ];
        let item_focus = items
            .iter()
            .map(|item| (item.id.clone(), cx.focus_handle()))
            .collect();
        Self {
            model: LibraryModel::new(items),
            ui,
            i18n,
            query: None,
            popup: None,
            preview: None,
            preview_view: None,
            preview_subscription: None,
            preview_observer: None,
            return_focus: None,
            status: None,
            loading: false,
            internal_previews: false,
            preview_request_id: 0,
            focus: cx.focus_handle(),
            scroll: gpui::ScrollHandle::new(),
            usage: None,
            category_focus: Category::ALL
                .iter()
                .map(|_| cx.focus_handle())
                .collect(),
            item_focus,
            _subscriptions: subscriptions,
        }
    }

    /// Builds the search input on the first render, where a window is
    /// available, and notifies on its events from then on.
    fn ensure_query(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<gpui_kit::base::input::InputState> {
        if let Some(query) = &self.query {
            return query.clone();
        }
        let query =
            cx.new(|cx| gpui_kit::base::input::InputState::new(window, cx));
        self._subscriptions.push(cx.subscribe(
            &query,
            |_, _, _: &gpui_kit::base::input::InputEvent, cx| cx.notify(),
        ));
        self.query = Some(query.clone());
        query
    }

    /// The search input, created by [`Self::ensure_query`] on the first render.
    fn query_state(&self) -> Entity<gpui_kit::base::input::InputState> {
        self.query
            .clone()
            .expect("the library search input is built on the first render")
    }

    pub fn set_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        if let Some(view) = &self.preview_view {
            view.update(cx, |view, cx| view.set_i18n(i18n.clone(), cx));
        }
        self.i18n = i18n;
        cx.notify();
    }

    pub fn set_items(
        &mut self,
        items: Vec<LibraryItem>,
        cx: &mut Context<'_, Self>,
    ) {
        self.item_focus
            .retain(|id, _| items.iter().any(|item| &item.id == id));
        for item in &items {
            self.item_focus
                .entry(item.id.clone())
                .or_insert_with(|| cx.focus_handle());
        }
        self.model.items = items;
        self.loading = false;
        cx.notify();
    }

    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<'_, Self>) {
        self.loading = loading;
        cx.notify();
    }

    pub fn set_usage(
        &mut self,
        usage: LibraryUsage,
        cx: &mut Context<'_, Self>,
    ) {
        self.usage = Some(usage);
        cx.notify();
    }

    /// Isolated examples may host the shared preview themselves. Production
    /// keeps this off and routes `PreviewRequested` to the application dialog.
    pub const fn use_internal_previews(&mut self) {
        self.internal_previews = true;
    }

    pub fn item_focus_handle(&self, id: &str) -> Option<FocusHandle> {
        self.item_focus.get(id).cloned()
    }

    pub fn artifact_focus(&self, id: &str) -> Option<FocusHandle> {
        self.item_focus_handle(id)
    }

    /// Complete an asynchronous host read. The request token prevents bytes
    /// from an earlier artifact or retry replacing the current preview.
    pub fn set_preview_bytes(
        &mut self,
        artifact_id: &str,
        request_id: u64,
        result: Result<Vec<u8>, &'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(preview) = self.preview_view.as_ref() else {
            return;
        };
        if self.preview.as_deref() != Some(artifact_id) {
            return;
        }
        preview.update(cx, |preview, cx| {
            preview.set_bytes(request_id, result, cx);
        });
    }

    pub fn set_status(
        &mut self,
        status: Option<&'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(view) = &self.preview_view {
            view.update(cx, |view, cx| view.set_status(status, cx));
        }
        self.status = status;
        cx.notify();
    }

    pub fn preview_item(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.internal_previews {
            cx.emit(LibraryEvent::PreviewRequested(id.to_owned()));
            return;
        }
        let Some(item) =
            self.model.items.iter().find(|item| item.id == id).cloned()
        else {
            return;
        };
        if let Some(old) = self.preview_view.take() {
            old.update(cx, |view, cx| view.release(window, cx));
        }
        self.return_focus = self
            .item_focus
            .get(id)
            .cloned()
            .or_else(|| window.focused(cx));
        self.preview_request_id = self.preview_request_id.wrapping_add(1);
        let request_id = self.preview_request_id;
        let view = cx.new(|cx| {
            PreviewPanel::new(
                item,
                request_id,
                self.i18n.clone(),
                self.ui.clone(),
                cx,
            )
        });
        self.preview_observer =
            Some(cx.observe_in(&view, window, |_, _, window, cx| {
                window.refresh();
                cx.notify();
            }));
        self.preview_subscription =
            Some(cx.subscribe(&view, |panel, _, event: &PreviewEvent, cx| {
                match event {
                    PreviewEvent::Close => panel.preview = None,
                    PreviewEvent::ReadRequested {
                        artifact_id,
                        request_id,
                    } => {
                        cx.emit(LibraryEvent::ReadPreviewRequested {
                            artifact_id: artifact_id.clone(),
                            request_id: *request_id,
                        });
                    }
                    PreviewEvent::Retry => {
                        panel.preview_request_id =
                            panel.preview_request_id.wrapping_add(1);
                        let request_id = panel.preview_request_id;
                        if let Some(preview) = panel.preview_view.as_ref() {
                            preview.update(cx, |preview, cx| {
                                preview.request_bytes(request_id, cx);
                            });
                        }
                    }
                    PreviewEvent::SaveAs(artifact_id) => {
                        cx.emit(LibraryEvent::SaveAsRequested(
                            artifact_id.clone(),
                        ));
                    }
                    PreviewEvent::LocateSource {
                        session_id,
                        artifact_id,
                    } => {
                        cx.emit(LibraryEvent::LocateSourceRequested {
                            session_id: session_id.clone(),
                            artifact_id: artifact_id.clone(),
                        });
                    }
                    PreviewEvent::OpenDefault => {
                        if let Some(id) = &panel.preview {
                            cx.emit(LibraryEvent::OpenWithDefaultRequested(
                                id.clone(),
                            ));
                        }
                    }
                }
                cx.notify();
            }));
        self.preview_view = Some(view);
        self.preview = Some(id.to_owned());
        if let Some(view) = self.preview_view.as_ref() {
            view.update(cx, |preview, cx| {
                preview.request_bytes(request_id, cx);
            });
        }
        self.popup = None;
        cx.notify();
    }

    pub fn modal_scope(&self) -> Option<SharedString> {
        self.preview.as_ref().map(|_| "library-preview".into())
    }

    pub fn popup_scope(&self) -> Option<SharedString> {
        self.popup.as_ref().map(|popup| match popup {
            Popup::Sort => "library-sort-menu".into(),
            Popup::Item(_) => "library-item-menu".into(),
        })
    }

    /// Standalone example hosts call this before drawing the panel. App hosts
    /// merge these scopes into their existing root overlay lifecycle instead.
    pub fn sync_overlays(
        &self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let modal = self.modal_scope();
        let popup = self.popup_scope();
        self.ui.overlays.update(cx, |host, cx| {
            host.begin_frame(window);
            host.set_fallback_focus(&self.focus);
            host.set_modal(modal, window, cx);
            host.set_popup(popup, window, cx);
        });
    }

    fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    fn theme(&self, cx: &Context<'_, Self>) -> theme::ResolvedTheme {
        self.ui.theme.read(cx).resolved()
    }

    fn toolbar(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let t = self.theme(cx);
        let sort = theme::IconButton::new(
            "library-sort",
            self.t("library.sort"),
            theme::icons::sort_desc().size(px(14.)),
        )
        .framed()
        .render(
            &t,
            cx.listener(|panel, _, _, cx| {
                panel.popup = if panel.popup == Some(Popup::Sort) {
                    None
                } else {
                    Some(Popup::Sort)
                };
                cx.notify();
            }),
        );
        let favorite = theme::Button::new(
            "library-favorites-only",
            self.t("library.favorite_only"),
        )
        .size(theme::ControlSize::Small)
        .variant(if self.model.favorites_only {
            theme::ControlVariant::Secondary
        } else {
            theme::ControlVariant::Ghost
        })
        .icon(theme::icons::star())
        .render(
            &t,
            cx.listener(|panel, _, _, cx| {
                panel.model.favorites_only = !panel.model.favorites_only;
                cx.notify();
            }),
        );
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(
                theme::PageHeader::new(self.t("dock.library"))
                    .trailing(theme::anchor_trigger(
                        div()
                            .id("library-sort-anchor")
                            .debug_selector(|| "library-sort".to_owned())
                            .child(sort),
                        "library-sort",
                        &self.ui,
                    ))
                    .trailing(self.view_switch(cx))
                    .render(&t, window),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(960.))
                    .mx_auto()
                    .min_w_0()
                    .px(px(24.))
                    .pb(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        // The wrapper owns the toolbar slot so the search field
                        // still grows to `max_w` inside it.
                        div()
                            .id("library-search")
                            .debug_selector(|| "library-search".to_owned())
                            .flex_1()
                            .max_w(px(440.))
                            .flex()
                            .child(
                                theme::ToolbarSearch::new(
                                    "library-search",
                                    self.t("library.search"),
                                    self.query_state(),
                                )
                                .render(&t)
                                .flex_1(),
                            ),
                    )
                    .child(test_control("library-favorites", favorite)),
            )
    }

    fn view_switch(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let t = self.theme(cx);
        theme::icon_segment_group(&t)
            .child(self.view_segment(LibraryView::Grid, cx))
            .child(self.view_segment(LibraryView::List, cx))
    }

    fn view_segment(
        &self,
        view: LibraryView,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let t = self.theme(cx);
        let (id, key, icon) = if view == LibraryView::List {
            ("library-list", "library.view.list", theme::icons::list())
        } else {
            (
                "library-grid",
                "library.view.grid",
                theme::icons::layout_grid(),
            )
        };
        let selector = if view == LibraryView::List {
            "library-view-list"
        } else {
            "library-view-grid"
        };
        // Segments size themselves against the group's fixed height, so the
        // test wrapper has to take that height rather than shrink-wrap it.
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .h_full()
            .child(theme::icon_segment(
                id,
                self.t(key),
                icon.size(px(14.)),
                self.model.view == view,
                &t,
                cx.listener(move |panel, _, _, cx| {
                    panel.model.view = view;
                    panel.popup = None;
                    cx.notify();
                }),
            ))
    }

    fn filters(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let t = self.theme(cx);
        let mut filters = theme::FilterTabs::new(
            "library-filters",
            self.t("library.categories"),
        );
        for (index, category) in Category::ALL.into_iter().enumerate() {
            let selected = self.model.category == category;
            filters = filters.child(
                theme::filter_chip(
                    SharedString::from(format!("library-filter-{index}")),
                    self.t(category.key()),
                    selected,
                    &t,
                )
                .debug_selector(move || {
                    format!("library-filter-{}", category.key())
                })
                .track_focus(&self.category_focus[index])
                .tab_stop(selected)
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.model.category = category;
                    panel.popup = None;
                    cx.notify();
                }))
                .on_key_down(cx.listener(
                    move |panel, event: &gpui::KeyDownEvent, window, cx| {
                        let next = match event.keystroke.key.as_str() {
                            "right" | "down" => {
                                Some((index + 1) % Category::ALL.len())
                            }
                            "left" | "up" => Some(
                                (index + Category::ALL.len() - 1)
                                    % Category::ALL.len(),
                            ),
                            "home" => Some(0),
                            "end" => Some(Category::ALL.len() - 1),
                            _ => None,
                        };
                        if let Some(next) = next {
                            panel.model.category = Category::ALL[next];
                            panel.category_focus[next].focus(window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    },
                )),
            );
        }
        filters.render()
    }

    fn content(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let t = self.theme(cx);
        let query = self.query_state().read(cx).value().to_string();
        let groups = self.model.groups(&query);
        let mut column = div().w_full().flex().flex_col().gap(px(24.));
        if groups.is_empty() {
            if self.status.is_some() && self.model.items.is_empty() {
                return column;
            }
            if self.loading {
                return column.child(
                    div().py(px(72.)).flex().justify_center().child(
                        theme::Text::new(
                            theme::TextRole::Body,
                            self.t("library.loading"),
                        )
                        .render(&t),
                    ),
                );
            }
            let is_true_empty = self.model.items.is_empty()
                && query.trim().is_empty()
                && self.model.category == Category::All
                && !self.model.favorites_only;
            let mut empty = div()
                .py(px(72.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(14.));
            empty = empty.child(
                theme::EmptyState::new(
                    self.t(if is_true_empty {
                        "library.empty.true.title"
                    } else {
                        "library.empty.search.title"
                    }),
                    self.t(if is_true_empty {
                        "library.empty.true.body"
                    } else {
                        "library.empty.search.body"
                    }),
                )
                .icon(if is_true_empty {
                    theme::icons::folder_open().size(px(28.))
                } else {
                    theme::icons::search().size(px(28.))
                })
                .render(&t),
            );
            if is_true_empty {
                empty = empty.child(test_control(
                    "library-start-conversation",
                    theme::Button::new(
                        "library-start-conversation",
                        self.t("library.start_conversation"),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .render(
                        &t,
                        cx.listener(|_, _, _, cx| {
                            cx.emit(LibraryEvent::StartConversationRequested);
                        }),
                    ),
                ));
            } else {
                empty = empty.child(test_control(
                    "library-clear-filters",
                    theme::Button::new(
                        "library-clear-filters",
                        self.t("library.clear_filters"),
                    )
                    .variant(theme::ControlVariant::Secondary)
                    .render(
                        &t,
                        cx.listener(|panel, _, window, cx| {
                            panel.query_state().update(cx, |state, cx| {
                                state.set_value("", window, cx);
                            });
                            panel.model.category = Category::All;
                            panel.model.favorites_only = false;
                            cx.emit(LibraryEvent::ClearFiltersRequested);
                            cx.notify();
                        }),
                    ),
                ));
            }
            return column.child(empty);
        }
        for group in groups {
            let body = self.group_body(&group.items, cx);
            column = column.child(if let Some(date) = group.date {
                theme::SectionHeader::new(self.date_label(&date))
                    .meta(format!("({})", group.items.len()))
                    .body(body)
                    .render(&t)
                    .into_any_element()
            } else {
                body.into_any_element()
            });
        }
        column
    }

    fn usage_status(&self, cx: &Context<'_, Self>) -> Option<gpui::AnyElement> {
        let usage = self.usage.as_ref()?;
        if usage.staging_bytes == 0 && usage.view_bytes == 0 {
            return None;
        }
        let t = self.theme(cx);
        let mut details = Vec::new();
        for (bytes, key) in [
            (usage.staging_bytes, "library.storage.staging"),
            (usage.view_bytes, "library.storage.views"),
        ] {
            if bytes > 0 {
                details
                    .push(self.i18n.tf(key, &[("size", &format_bytes(bytes))]));
            }
        }
        Some(
            div()
                .w_full()
                .max_w(px(960.))
                .mx_auto()
                .px(px(24.))
                .pb(px(8.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .when(!details.is_empty(), |line| {
                    line.child(
                        theme::Text::new(
                            theme::TextRole::Meta,
                            details.join(" · "),
                        )
                        .render(&t)
                        .whitespace_normal(),
                    )
                })
                .when(usage.view_bytes > 0, |line| {
                    line.child(test_control(
                        "library-clean-view-copies",
                        theme::Button::new(
                            "library-clean-view-copies",
                            self.t("library.storage.clean_views"),
                        )
                        .size(theme::ControlSize::Small)
                        .variant(theme::ControlVariant::Ghost)
                        .render(
                            &t,
                            cx.listener(|_, _, _, cx| {
                                cx.emit(LibraryEvent::CleanViewCopiesRequested);
                            }),
                        ),
                    ))
                })
                .into_any_element(),
        )
    }

    fn group_body(
        &self,
        entries: &[LibraryItem],
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        if self.model.view == LibraryView::List {
            let mut rows = div().w_full().flex().flex_col();
            for item in entries {
                rows = rows.child(self.row(item, cx));
            }
            rows.into_any_element()
        } else {
            let mut cards = div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_stretch()
                .gap(px(16.));
            for item in entries {
                cards = cards.child(self.card(item, cx));
            }
            cards.into_any_element()
        }
    }

    fn date_label(&self, date: &str) -> String {
        let parts: Vec<&str> = date.split('-').collect();
        if parts.len() != 3 {
            return date.to_string();
        }
        let month = parts[1].trim_start_matches('0');
        let day = parts[2].trim_start_matches('0');
        self.i18n
            .tf("library.date", &[("month", month), ("day", day)])
    }

    fn item_description(&self, item: &LibraryItem) -> String {
        let format = if item.extension.is_empty() {
            self.t(item.category.key())
        } else {
            item.extension.trim_start_matches('.').to_uppercase()
        };
        let mut details = match item.size_bytes {
            Some(bytes) => self.i18n.tf(
                "library.preview.file_info",
                &[
                    ("format", &format),
                    ("size", &format!("{:.1}", bytes as f64 / 1024.)),
                ],
            ),
            None => format,
        };
        if item.source_deleted {
            details.push_str(" · ");
            details.push_str(&self.t("library.source.deleted"));
        } else if item.source_unavailable {
            details.push_str(" · ");
            details.push_str(&self.t("library.source.unavailable"));
        } else if !item.source_title.is_empty() {
            details.push_str(" · ");
            details.push_str(&item.source_title);
        }
        match item.availability {
            ArtifactAvailability::Available => {}
            ArtifactAvailability::Missing => {
                details.push_str(" · ");
                details.push_str(&self.t("library.availability.missing"));
            }
            ArtifactAvailability::Corrupt => {
                details.push_str(" · ");
                details.push_str(&self.t("library.availability.corrupt"));
            }
        }
        details
    }

    fn row(
        &self,
        item: &LibraryItem,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let t = self.theme(cx);
        let id = item.id.clone();
        let more_id = SharedString::from(format!("library-more-{}", item.id));
        let menu_item = item.id.clone();
        let more = theme::IconButton::new(
            more_id.clone(),
            self.t("library.more"),
            theme::icons::ellipsis().size(px(12.)),
        )
        .size(theme::ControlSize::Small)
        .render(
            &t,
            cx.listener(move |panel, _, _, cx| {
                cx.stop_propagation();
                let popup = Popup::Item(menu_item.clone());
                panel.popup = if panel.popup.as_ref() == Some(&popup) {
                    None
                } else {
                    Some(popup)
                };
                cx.notify();
            }),
        );
        theme::interactive(
            SharedString::from(format!("library-row-{}", item.id)),
            item.title.clone(),
            &t,
        )
        .debug_selector({
            let selector = format!("library-row-{}", item.id);
            move || selector
        })
        .w_full()
        .h(px(LIST_ROW_HEIGHT))
        .border_0()
        .border_b_1()
        .border_color(t.library_line())
        .rounded(px(12.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(10.))
        .hover(move |style| style.bg(t.library_selected()).shadow_sm())
        .child(self.file_icon(item, 24., cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .child(
                            theme::Title::new(
                                theme::TitleLevel::Item,
                                item.title.clone(),
                            )
                            .truncate()
                            .render(&t)
                            .flex_1(),
                        )
                        .when(item.pending, |title| {
                            title.child(
                                theme::icons::loader_circle()
                                    .size(px(8.))
                                    .flex_shrink_0()
                                    .text_color(t.library_muted()),
                            )
                        }),
                )
                .child(
                    theme::Text::new(
                        theme::TextRole::Meta,
                        self.item_description(item),
                    )
                    .truncate()
                    .render(&t),
                ),
        )
        .child(
            theme::Text::new(
                theme::TextRole::Meta,
                if item.delivered_time.is_empty() {
                    item.time.clone()
                } else {
                    item.delivered_time.clone()
                },
            )
            .render(&t)
            .flex_shrink_0(),
        )
        .child(div().w(px(1.)).flex_shrink_0())
        .child(theme::anchor_trigger(
            div()
                .id(format!("{more_id}-anchor"))
                .debug_selector({
                    let selector = format!("library-more-{}", item.id);
                    move || selector
                })
                .child(more),
            more_id,
            &self.ui,
        ))
        .when_some(self.item_focus.get(&item.id), |row, focus| {
            row.track_focus(focus).tab_stop(true)
        })
        .on_click(cx.listener(move |panel, _, window, cx| {
            panel.preview_item(&id, window, cx);
        }))
    }

    fn file_icon(
        &self,
        item: &LibraryItem,
        size: f32,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let t = self.theme(cx);
        let mark = if let Some(icon) = LibraryFileIcon::from_item(item) {
            icon.icon().size(px(size)).into_any_element()
        } else {
            let color = if item.category == Category::Website {
                t.library_website()
            } else {
                t.library_app()
            };
            let glyph = if item.category == Category::Website {
                theme::icons::globe()
            } else {
                theme::icons::app_window()
            };
            div()
                .size(px(size))
                .bg(color)
                .rounded(px(size * 0.22))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    glyph.size(px(size * 0.58)).text_color(t.library_on_file()),
                )
                .into_any_element()
        };
        div()
            .w(px(size))
            .h(px(size))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(mark)
    }

    fn card(
        &self,
        item: &LibraryItem,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let t = self.theme(cx);
        let id = item.id.clone();
        let menu_item = item.id.clone();
        let more_id =
            SharedString::from(format!("library-card-more-{}", item.id));
        let more = theme::IconButton::new(
            more_id.clone(),
            self.t("library.more"),
            theme::icons::ellipsis().size(px(12.)),
        )
        .size(theme::ControlSize::Small)
        .render(
            &t,
            cx.listener(move |panel, _, _, cx| {
                cx.stop_propagation();
                let popup = Popup::Item(menu_item.clone());
                panel.popup = if panel.popup.as_ref() == Some(&popup) {
                    None
                } else {
                    Some(popup)
                };
                cx.notify();
            }),
        );
        theme::interactive(
            SharedString::from(format!("library-card-{}", item.id)),
            item.title.clone(),
            &t,
        )
        .debug_selector({
            let selector = format!("library-card-{}", item.id);
            move || selector
        })
        .relative()
        .min_w(px(CARD_MIN_WIDTH))
        .flex_1()
        .min_h(px(176.))
        .rounded(px(12.))
        .border_1()
        .border_color(t.library_line())
        .bg(t.card())
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .hover(move |style| style.bg(t.library_selected()).shadow_md())
        .child(
            div().absolute().top(px(10.)).right(px(10.)).child(
                theme::anchor_trigger(
                    div()
                        .id(format!("{more_id}-anchor"))
                        .debug_selector({
                            let selector = format!("library-more-{}", item.id);
                            move || selector
                        })
                        .child(more),
                    more_id,
                    &self.ui,
                ),
            ),
        )
        .child(self.file_icon(item, 28., cx))
        .child(
            theme::Title::new(theme::TitleLevel::Card, item.title.clone())
                .truncate()
                .render(&t),
        )
        .child(
            theme::Text::new(
                theme::TextRole::Body,
                self.item_description(item),
            )
            .render(&t)
            .whitespace_normal(),
        )
        .child(
            theme::Text::new(
                theme::TextRole::Meta,
                if item.delivered_time.is_empty() {
                    item.time.clone()
                } else {
                    item.delivered_time.clone()
                },
            )
            .render(&t),
        )
        .when_some(self.item_focus.get(&item.id), |card, focus| {
            card.track_focus(focus).tab_stop(true)
        })
        .on_click(cx.listener(move |panel, _, window, cx| {
            panel.preview_item(&id, window, cx);
        }))
    }

    fn action_menu_row(
        id: &'static str,
        label: String,
        icon: impl IntoElement,
        theme: theme::ResolvedTheme,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        theme::menu_item_with_icon(id, label, icon, false, &theme, on_click)
            .debug_selector(move || id.to_owned())
            .h(px(36.))
            .max_h(px(36.))
            .px(px(11.))
            .border_0()
            .rounded(px(6.))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(12.))
            .text_size(px(14.))
            .line_height(px(20.))
            .font_weight(gpui::FontWeight::NORMAL)
            .text_color(theme.ink())
            .bg(theme.card())
            .hover(move |style| style.bg(theme.soft()))
    }

    fn sort_menu_row(
        selector: &'static str,
        id: impl Into<gpui::ElementId>,
        label: String,
        mark: impl IntoElement,
        theme: theme::ResolvedTheme,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        let metrics = theme.control_metrics();
        let hover = theme.soft();
        theme::menu_item_with_icon(id, label, mark, false, &theme, on_click)
            .debug_selector(move || selector.to_owned())
            .h(px(32.))
            .max_h(px(32.))
            .px(px(8.))
            .border_0()
            .rounded(px(6.))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(metrics.size)
            .line_height(metrics.line_height)
            .font_weight(gpui::FontWeight::NORMAL)
            .text_color(theme.tab_selected())
            .hover(move |style| style.bg(hover))
    }

    fn popup(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> Option<gpui::AnyElement> {
        let popup = self.popup.as_ref()?;
        let t = self.theme(cx);
        let metrics = t.control_metrics();
        let mut menu = theme::menu(
            "library-menu",
            self.t("library.more"),
            &t,
            &self.ui,
            cx,
        )
        .p(px(4.))
        .text_size(metrics.size)
        .line_height(metrics.line_height)
        .font_weight(gpui::FontWeight::NORMAL);
        let (scope, anchor) = match popup {
            Popup::Sort => {
                for sort in [
                    LibrarySort::Newest,
                    LibrarySort::Oldest,
                    LibrarySort::Name,
                ] {
                    let label = self.t(sort.key());
                    let check = div()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(sort == self.model.sort, |mark| {
                            mark.child(theme::icons::check().size(px(14.)))
                        });
                    menu = menu.child(Self::sort_menu_row(
                        match sort {
                            LibrarySort::Newest => "library-sort-newest",
                            LibrarySort::Oldest => "library-sort-oldest",
                            LibrarySort::Name => "library-sort-name",
                        },
                        SharedString::from(format!(
                            "library-sort-{}",
                            sort.key()
                        )),
                        label,
                        check,
                        t,
                        cx.listener(move |panel, _, _, cx| {
                            panel.model.sort = sort;
                            panel.popup = None;
                            cx.notify();
                        }),
                    ));
                }
                ("library-sort-menu", SharedString::from("library-sort"))
            }
            Popup::Item(id) => {
                let favorite_label = if self.model.is_favorite(id) {
                    self.t("library.favorite.remove")
                } else {
                    self.t("library.favorite.add")
                };
                let item =
                    self.model.items.iter().find(|item| &item.id == id)?;
                menu = menu.w(px(220.)).rounded(t.modal_radius());
                let save_id = id.clone();
                let open_id = id.clone();
                let favorite_id = id.clone();
                let next_favorite = !item.favorite;
                menu = menu.child(Self::action_menu_row(
                    "library-menu-save-as",
                    self.t("library.save_as"),
                    theme::icons::download().size(px(16.)),
                    t,
                    cx.listener(move |panel, _, _, cx| {
                        cx.emit(LibraryEvent::SaveAsRequested(save_id.clone()));
                        panel.popup = None;
                        cx.notify();
                    }),
                ));
                if item.default_open_supported() {
                    menu = menu.child(Self::action_menu_row(
                        "library-menu-open-default",
                        self.t("library.open_default"),
                        theme::icons::external_link().size(px(16.)),
                        t,
                        cx.listener(move |panel, _, _, cx| {
                            cx.emit(LibraryEvent::OpenWithDefaultRequested(
                                open_id.clone(),
                            ));
                            panel.popup = None;
                            cx.notify();
                        }),
                    ));
                }
                if !item.source_deleted
                    && !item.source_unavailable
                    && !item.source_session_id.is_empty()
                {
                    let session_id = item.source_session_id.clone();
                    let artifact_id = id.clone();
                    menu = menu.child(Self::action_menu_row(
                        "library-menu-locate-source",
                        self.t("library.locate_task"),
                        theme::icons::external_link().size(px(16.)),
                        t,
                        cx.listener(move |panel, _, _, cx| {
                            cx.emit(LibraryEvent::LocateSourceRequested {
                                session_id: session_id.clone(),
                                artifact_id: artifact_id.clone(),
                            });
                            panel.popup = None;
                            cx.notify();
                        }),
                    ));
                }
                menu = menu.child(Self::action_menu_row(
                    "library-menu-favorite",
                    favorite_label,
                    theme::icons::star().size(px(16.)),
                    t,
                    cx.listener(move |panel, _, _, cx| {
                        cx.emit(LibraryEvent::SetFavoriteRequested {
                            artifact_id: favorite_id.clone(),
                            favorite: next_favorite,
                        });
                        panel.popup = None;
                        cx.notify();
                    }),
                ));
                (
                    "library-item-menu",
                    SharedString::from(format!("library-more-{id}")),
                )
            }
        };
        Some(theme::popover(
            scope,
            anchor,
            menu,
            &self.ui,
            window,
            cx,
            self.i18n.locale().is_rtl(),
        ))
    }

    fn preview_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> Option<gpui::AnyElement> {
        self.preview.as_ref()?;
        let view = self.preview_view.as_ref()?;
        Some(
            theme::modal_layer(
                "library-preview-layer",
                &self.theme(cx),
                &self.ui,
                view.clone(),
            )
            .into_any_element(),
        )
    }
}

impl Render for LibraryPanel {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        if self.preview.is_none()
            && let Some(view) = self.preview_view.take()
        {
            view.update(cx, |view, cx| view.release(window, cx));
            self.preview_subscription = None;
            self.preview_observer = None;
            if let Some(focus) = self.return_focus.take() {
                focus.focus(window, cx);
            }
        }
        let _ = self.ensure_query(window, cx);
        let t = self.theme(cx);
        let mut root = div()
            .id("library")
            .track_focus(&self.focus)
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(t.canvas())
            .text_color(t.library_ink())
            .flex()
            .flex_col()
            .child(self.toolbar(window, cx))
            .when_some(self.usage_status(cx), gpui::ParentElement::child)
            .when_some(self.status, |root, key| {
                root.child(
                    div()
                        .id("library-status")
                        .px(px(24.))
                        .py(px(8.))
                        .role(gpui::Role::Status)
                        .child(
                            theme::Text::new(
                                theme::TextRole::Body,
                                self.t(key),
                            )
                            .render(&t)
                            .whitespace_normal(),
                        ),
                )
            })
            .child(
                div()
                    .id("library-scroll")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        theme::PageColumn::new()
                            .child(self.filters(cx))
                            .child(self.content(cx))
                            .render(window),
                    ),
            );
        if let Some(popup) = self.popup(window, cx) {
            root = root.child(popup);
        }
        if let Some(preview) = self.preview_layer(cx) {
            root = root.child(preview);
        }
        root
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024. * 1024. * 1024.))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_i18n::Locale;
    use gpui::{Focusable, TestAppContext, VisualTestContext};

    struct LibraryHarness {
        panel: Entity<LibraryPanel>,
        _panel_sub: Subscription,
        visible: bool,
    }

    impl LibraryHarness {
        fn new(cx: &mut Context<'_, Self>) -> Self {
            let mut items: Vec<LibraryItem> = serde_json::from_str(
                include_str!("../../examples/fixtures/library.json"),
            )
            .expect("the isolated library fixture must parse");
            for (index, item) in items.iter_mut().enumerate() {
                item.delivered_at_ms = (100 - index as u64) * 60_000;
                item.source_session_id = "fixture-session".into();
                item.source_title = item.description.clone();
                item.extension = item.title.rsplit_once('.').map_or_else(
                    || {
                        if item.category == Category::Slides {
                            "pptx".into()
                        } else {
                            String::new()
                        }
                    },
                    |(_, ext)| ext.to_lowercase(),
                );
            }
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            let mut i18n = I18n::embedded("Flowy");
            i18n.set_locale(Locale::En);
            let panel = cx.new(|cx| {
                let mut panel = LibraryPanel::new(items, i18n, ui, cx);
                panel.use_internal_previews();
                panel
            });
            let panel_sub =
                cx.subscribe(&panel, |host, _, event: &LibraryEvent, cx| {
                    match event {
                        LibraryEvent::ReadPreviewRequested {
                            artifact_id,
                            request_id,
                        } => {
                            let bytes = include_bytes!(
                                "../../examples/fixtures/preview/sample.pdf"
                            )
                            .to_vec();
                            host.panel.update(cx, |panel, cx| {
                                panel.set_preview_bytes(
                                    artifact_id,
                                    *request_id,
                                    Ok(bytes),
                                    cx,
                                );
                            });
                        }
                        LibraryEvent::SetFavoriteRequested {
                            artifact_id,
                            favorite,
                        } => {
                            host.panel.update(cx, |panel, cx| {
                                panel
                                    .model
                                    .set_favorite(artifact_id, *favorite);
                                cx.notify();
                            });
                        }
                        _ => {}
                    }
                    cx.notify();
                });
            Self {
                panel,
                _panel_sub: panel_sub,
                visible: true,
            }
        }
    }

    impl Render for LibraryHarness {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            self.panel
                .update(cx, |panel, cx| panel.sync_overlays(window, cx));
            div()
                .size_full()
                .when(self.visible, |root| root.child(self.panel.clone()))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        // Overlay focus is deferred until paint, and popovers need the prior
        // trigger measurement. Drive the executor and native frame callbacks
        // together rather than reading the first transitional tree.
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    /// `VisualTestContext::debug_bounds` only accepts `&'static str`, so ids
    /// built from fixture data are leaked for the rest of the test process.
    fn static_selector(selector: &str) -> &'static str {
        Box::leak(selector.to_owned().into_boxed_str())
    }

    /// Whether the last frame painted the control a `.debug_selector` named.
    fn painted(cx: &mut VisualTestContext, selector: &str) -> bool {
        cx.debug_bounds(static_selector(selector)).is_some()
    }

    fn control_bounds(
        cx: &mut VisualTestContext,
        selector: &str,
    ) -> gpui::Bounds<gpui::Pixels> {
        cx.debug_bounds(static_selector(selector))
            .unwrap_or_else(|| panic!("missing painted control {selector:?}"))
    }

    /// Clicks the painted control named `selector`.
    ///
    /// `gpui_pre` exposes no accessibility tree to tests, so the controls are
    /// activated through the same pointer dispatch a platform click uses.
    fn activate(cx: &mut VisualTestContext, selector: &str) {
        draw(cx);
        let bounds = control_bounds(cx, selector);
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
        draw(cx);
    }

    fn press(cx: &mut VisualTestContext, key: &'static str) {
        let keystroke =
            gpui::Keystroke::parse(key).expect("valid test keystroke");
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        draw(cx);
    }

    #[test]
    fn file_icons_follow_the_parent_file_type_assets() {
        let records: Vec<LibraryItem> = serde_json::from_str(include_str!(
            "../../examples/fixtures/library.json"
        ))
        .expect("the isolated library fixture must parse");
        let expected = [
            ("1104-byd-presentation", LibraryFileIcon::Presentation),
            ("1104-dew-stone-hero", LibraryFileIcon::Image),
            ("1020-07-level-guide", LibraryFileIcon::Pdf),
            ("1020-06-balancing", LibraryFileIcon::Spreadsheet),
            ("1020-08-preview-video", LibraryFileIcon::Video),
            ("1020-09-game-audio", LibraryFileIcon::Audio),
            ("1020-11-sprite-package", LibraryFileIcon::Archive),
        ];
        for (id, icon) in expected {
            let item = records
                .iter()
                .find(|item| item.id == id)
                .expect("fixture item");
            assert_eq!(LibraryFileIcon::from_item(item), Some(icon));
        }
        let website = records
            .iter()
            .find(|item| item.category == Category::Website)
            .expect("website fixture item");
        assert_eq!(LibraryFileIcon::from_item(website), None);
    }

    #[gpui::test]
    fn file_controls_emit_host_actions_without_launching_or_bubbling(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let _subscription = host.update(cx, |_, cx| {
            cx.subscribe(&panel, move |_, _, event: &LibraryEvent, _| {
                captured.lock().unwrap().push(event.clone())
            })
        });
        let (id, _title) = cx.read_entity(&panel, |panel, _| {
            (
                panel.model.items[0].id.clone(),
                panel.model.items[0].title.clone(),
            )
        });
        draw(cx);
        // The library offers no upload control: a file row exposes its own
        // preview target and the "More actions" menu, nothing else.
        assert!(painted(cx, &format!("library-row-{id}")));
        assert!(painted(cx, &format!("library-more-{id}")));
        panel.update(cx, |panel, _| {
            panel.model.items[0].extension = "pdf".into();
        });
        activate(cx, &format!("library-more-{id}"));
        activate(cx, "library-menu-open-default");
        cx.read_entity(&panel, |panel, _| assert!(panel.preview.is_none()));
        activate(cx, &format!("library-row-{id}"));
        activate(cx, "preview-open-default");
        let captured = events.lock().unwrap();
        let open_events: Vec<_> = captured
            .iter()
            .filter(|event| {
                matches!(event, LibraryEvent::OpenWithDefaultRequested(_))
            })
            .collect();
        assert_eq!(open_events.len(), 2);
        for event in open_events {
            assert!(
                matches!(event, LibraryEvent::OpenWithDefaultRequested(value) if value == &id)
            );
        }
    }

    #[gpui::test]
    fn production_preview_routes_to_host_with_stable_artifact_id(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let _subscription = host.update(cx, |_, cx| {
            cx.subscribe(&panel, move |_, _, event: &LibraryEvent, _| {
                captured.lock().unwrap().push(event.clone());
            })
        });
        let (id, _title) = panel.update(cx, |panel, _| {
            panel.internal_previews = false;
            let item = &panel.model.items[0];
            (item.id.clone(), item.title.clone())
        });
        activate(cx, &format!("library-row-{id}"));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[LibraryEvent::PreviewRequested(id)]
        );
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.preview.is_none());
            assert!(panel.preview_view.is_none());
        });
    }

    #[gpui::test]
    fn toolbar_actions_remain_reachable_in_narrow_content(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        // A real 800px app window also contains the dock; exercise its smaller
        // content width instead of testing a full-width library in isolation.
        cx.simulate_resize(gpui::size(px(560.), px(600.)));
        draw(cx);
        for selector in [
            "library-search",
            "library-favorites",
            "library-sort",
            "library-view-grid",
            "library-view-list",
        ] {
            let bounds = control_bounds(cx, selector);
            assert!(
                bounds.left() >= px(0.) && bounds.right() <= px(560.),
                "{selector} exceeds narrow content: {bounds:?}"
            );
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(600.));
        }
        activate(cx, "library-favorites");
        cx.read_entity(&panel, |panel, _| assert!(panel.model.favorites_only));
    }

    #[gpui::test]
    fn leaving_and_returning_keeps_library_scroll_and_view_state(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        cx.simulate_resize(gpui::size(px(800.), px(600.)));
        draw(cx);
        panel.update(cx, |panel, cx| {
            panel.model.sort = LibrarySort::Oldest;
            panel.scroll.set_offset(gpui::point(px(0.), px(-180.)));
            cx.notify();
        });
        draw(cx);
        let before = cx.read_entity(&panel, |panel, _| panel.scroll.offset());
        assert!(before.y < px(0.));
        host.update(cx, |host, cx| {
            host.visible = false;
            cx.notify();
        });
        draw(cx);
        host.update(cx, |host, cx| {
            host.visible = true;
            cx.notify();
        });
        draw(cx);
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.scroll.offset(), before);
            assert_eq!(panel.model.sort, LibrarySort::Oldest);
        });
    }

    #[gpui::test]
    fn load_error_and_empty_library_have_distinct_empty_states(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        panel.update(cx, |panel, cx| {
            panel.set_items(Vec::new(), cx);
            panel.set_status(Some("library.error.catalog"), cx);
        });
        draw(cx);
        assert!(
            !painted(cx, "library-start-conversation"),
            "a catalog error must not offer the first-run empty state"
        );
        panel.update(cx, |panel, cx| panel.set_status(None, cx));
        draw(cx);
        assert!(
            painted(cx, "library-start-conversation"),
            "an empty library offers the first-run empty state"
        );
        panel.update(cx, |panel, cx| panel.set_loading(true, cx));
        draw(cx);
        assert!(
            !painted(cx, "library-start-conversation"),
            "a loading library must not offer the first-run empty state"
        );
    }

    #[gpui::test]
    fn viewing_copy_cleanup_is_requested_explicitly_without_touching_files(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let _subscription = host.update(cx, |_, cx| {
            cx.subscribe(&panel, move |_, _, event: &LibraryEvent, _| {
                captured.lock().unwrap().push(event.clone())
            })
        });
        let mut usage = LibraryUsage {
            staging_bytes: 0,
            view_bytes: 4096,
        };
        panel.update(cx, |panel, cx| panel.set_usage(usage.clone(), cx));
        activate(cx, "library-clean-view-copies");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[LibraryEvent::CleanViewCopiesRequested]
        );
        usage.view_bytes = 0;
        panel.update(cx, |panel, cx| panel.set_usage(usage, cx));
        draw(cx);
        assert!(
            !painted(cx, "library-clean-view-copies"),
            "the cleanup affordance disappears once no viewing copies remain"
        );
    }

    #[gpui::test]
    fn real_pdf_preview_opens_navigates_and_closes(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        let (id, _title) = cx.read_entity(&panel, |panel, _| {
            let item = &panel.model.items[0];
            (item.id.clone(), item.title.clone())
        });
        panel.update(cx, |panel, _| {
            panel.model.items[0].extension = "pdf".into();
        });
        activate(cx, &format!("library-row-{id}"));
        draw(cx);
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.preview.as_ref(), Some(&id))
        });
        activate(cx, "preview-next");
        activate(cx, "preview-previous");
        activate(cx, "preview-zoom-in");
        activate(cx, "preview-fit");
        press(cx, "escape");
        draw(cx);
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.preview.is_none());
            assert!(panel.preview_view.is_none());
        });
        let expected_focus =
            cx.read_entity(&panel, |panel, _| panel.item_focus[&id].clone());
        cx.update(|window, cx| {
            assert_eq!(window.focused(cx), Some(expected_focus))
        });
    }

    #[gpui::test]
    fn close_loading_preview_discards_late_completion(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let id = panel.model.items[0].id.clone();
                panel.model.items[0].extension = "pdf".into();
                panel.preview_item(&id, window, cx);
                panel.preview = None;
                cx.notify();
            })
        });
        draw(cx);
        cx.read_entity(&panel, |panel, _| {
            assert!(panel.preview.is_none());
            assert!(panel.preview_view.is_none());
        });
    }

    #[gpui::test]
    fn category_grid_toggle_and_typed_query_compose(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        let id =
            cx.read_entity(&panel, |panel, _| panel.model.items[0].id.clone());
        activate(cx, &format!("library-filter-{}", Category::Image.key()));
        activate(cx, "library-view-grid");
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.model.category, Category::Image);
            assert_eq!(panel.model.view, LibraryView::Grid);
        });
        cx.update(|window, cx| {
            let focus = panel.read(cx).query_state().read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("dew-stone");
        draw(cx);
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(
                panel.query_state().read(cx).value().to_string(),
                "dew-stone"
            );
            let query = panel.query_state().read(cx).value().to_string();
            let results = panel.model.filtered(&query);
            assert_eq!(results.len(), 3);
            assert!(
                results.iter().all(|item| item.category == Category::Image)
            );
        });
        activate(cx, "library-view-list");
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.model.view, LibraryView::List);
            assert_eq!(panel.model.category, Category::Image);
            assert_eq!(
                panel.query_state().read(cx).value().to_string(),
                "dew-stone"
            );
        });
        activate(cx, &format!("library-filter-{}", Category::Document.key()));
        cx.read_entity(&panel, |panel, cx| {
            let query = panel.query_state().read(cx).value().to_string();
            assert_eq!(panel.model.filtered(&query).len(), 0);
        });
        assert!(
            !painted(cx, &format!("library-more-{id}")),
            "an empty result must not retain file row actions"
        );
    }

    #[gpui::test]
    fn item_menu_actions_toggle_favorites_and_escape_closes_menu(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        // The fixture's first row; its menu drives the assertions below.
        let item_id = "1104-byd-presentation";
        let more = format!("library-more-{item_id}");
        activate(cx, "library-sort");
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.popup, Some(Popup::Sort));
        });
        activate(cx, "library-sort-oldest");
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.model.sort, LibrarySort::Oldest);
            assert!(panel.popup.is_none());
            let rows = panel.model.filtered("");
            assert!(rows.windows(2).all(|pair| pair[0].date <= pair[1].date));
        });
        activate(cx, "library-sort");
        activate(cx, "library-sort-newest");
        activate(cx, &more);
        // The row menu keeps one stable top-to-bottom order; `control_bounds`
        // also proves every documented action is reachable.
        let menu_rows = [
            "library-menu-save-as",
            "library-menu-open-default",
            "library-menu-locate-source",
            "library-menu-favorite",
        ];
        let order: Vec<_> = menu_rows
            .into_iter()
            .map(|selector| control_bounds(cx, selector).origin.y)
            .collect();
        assert!(
            order.windows(2).all(|pair| pair[0] < pair[1]),
            "row menu order changed: {order:?}"
        );
        cx.read_entity(&panel, |panel, _| {
            assert!(matches!(panel.popup, Some(Popup::Item(_))));
        });
        activate(cx, "library-menu-favorite");
        cx.read_entity(&panel, |panel, cx| {
            assert!(panel.model.is_favorite(item_id));
            assert!(panel.popup.is_none());
            assert!(!panel.ui.overlays.read(cx).modal_active());
        });
        activate(cx, &more);
        activate(cx, "library-menu-favorite");
        cx.read_entity(&panel, |panel, cx| {
            assert!(!panel.model.is_favorite(item_id));
            assert!(panel.popup.is_none());
            assert!(!panel.ui.overlays.read(cx).modal_active());
        });
        activate(cx, "library-sort");
        press(cx, "escape");
        cx.read_entity(&panel, |panel, _| assert!(panel.popup.is_none()));
    }

    #[gpui::test]
    fn panel_owns_filter_sort_view_and_localized_labels(
        cx: &mut gpui::TestAppContext,
    ) {
        let panel = cx.new(|cx| {
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            LibraryPanel::new(Vec::new(), I18n::embedded("Flowy"), ui, cx)
        });
        panel.update(cx, |panel, cx| {
            assert_eq!(panel.model.category, Category::All);
            assert_eq!(panel.model.view, LibraryView::List);
            assert_eq!(panel.model.sort, LibrarySort::Newest);
            panel.model.category = Category::Image;
            panel.model.view = LibraryView::Grid;
            panel.model.sort = LibrarySort::Name;
            let file = LibraryItem {
                id: "existing".to_owned(),
                title: "File without an extension".to_owned(),
                original_name: "source-file".to_owned(),
                description: "/private/folder".to_owned(),
                category: Category::Image,
                date: String::new(),
                time: String::new(),
                pending: false,
                preview: None,
                size_bytes: Some(2048),
                extension: String::new(),
                media_type: None,
                delivered_at_ms: 0,
                delivered_date: String::new(),
                delivered_time: String::new(),
                source_session_id: String::new(),
                source_title: String::new(),
                source_deleted: false,
                source_unavailable: false,
                favorite: false,
                availability: ArtifactAvailability::Available,
            };
            let mut english = I18n::embedded("Flowy");
            english.set_locale(Locale::En);
            panel.set_i18n(english, cx);
            assert_eq!(panel.t("library.search"), "Search files");
            assert_eq!(panel.item_description(&file), "Images · 2.0 KiB");
            let mut chinese = I18n::embedded("Flowy");
            chinese.set_locale(Locale::ZhCn);
            panel.set_i18n(chinese, cx);
            assert_eq!(panel.t("library.search"), "搜索文件");
            assert_eq!(panel.item_description(&file), "图片 · 2.0 KiB");
            assert_eq!(panel.model.category, Category::Image);
            assert_eq!(panel.model.view, LibraryView::Grid);
            assert_eq!(panel.model.sort, LibrarySort::Name);
        });
    }
}
