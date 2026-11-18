use super::model::{
    Category, LibraryItem, LibraryModel, LibrarySort, LibraryView,
};
use flowy_i18n::I18n;
use flowy_ui::{self as theme, UiContext};
use gpui::{
    Context, Entity, FocusHandle, IntoElement, ParentElement, Render, Role,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Popup {
    Sort,
    Item(String),
}

/// IconPark file-type artwork copied from the parent UI assets so library
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
        let extension = item
            .title
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        let by_extension = match extension.as_str() {
            "java" => Some(Self::Java),
            "c" | "cc" | "cpp" | "cs" | "css" | "go" | "h" | "hpp" | "htm"
            | "html" | "js" | "json" | "jsx" | "kt" | "kts" | "log" | "md"
            | "markdown" | "php" | "ps1" | "py" | "rb" | "rs" | "scss"
            | "sh" | "swift" | "toml" | "ts" | "tsx" | "txt" | "xml"
            | "yaml" | "yml" => Some(Self::Code),
            "doc" | "docx" | "odt" | "rtf" => Some(Self::Document),
            "bmp" | "gif" | "jpeg" | "jpg" | "png" | "svg" | "webp"
            | "avif" => Some(Self::Image),
            "aac" | "flac" | "m4a" | "mp3" | "ogg" | "wav" => Some(Self::Audio),
            "avi" | "mkv" | "mov" | "mp4" | "webm" => Some(Self::Video),
            "pdf" => Some(Self::Pdf),
            "odp" | "ppt" | "pptx" => Some(Self::Presentation),
            "csv" | "ods" | "xls" | "xlsx" => Some(Self::Spreadsheet),
            "7z" | "bz2" | "gz" | "rar" | "tar" | "tgz" | "xz" | "zip" => {
                Some(Self::Archive)
            }
            _ => None,
        };
        by_extension.or(match item.category {
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
        })
    }

    fn icon(self) -> gpui::Img {
        match self {
            Self::Code => theme::icons::code_file(),
            Self::Document => theme::icons::docx_file(),
            Self::Java => theme::icons::java_file(),
            Self::Image => theme::icons::jpg_file(),
            Self::Audio => theme::icons::mp3_file(),
            Self::Video => theme::icons::mp4_file(),
            Self::Pdf => theme::icons::pdf_file(),
            Self::Presentation => theme::icons::ppt_file(),
            Self::Spreadsheet => theme::icons::xls_file(),
            Self::Archive => theme::icons::zip_file(),
        }
    }
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
    query: Entity<EditableTextState>,
    rename: Entity<EditableTextState>,
    popup: Option<Popup>,
    preview: Option<String>,
    renaming: Option<String>,
    focus: FocusHandle,
    category_focus: Vec<FocusHandle>,
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
        let query =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let rename =
            cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        let subscriptions = vec![
            cx.subscribe(&query, |_, _, _: &TextChanged, cx| cx.notify()),
            cx.subscribe(&rename, |_, _, _: &TextChanged, cx| cx.notify()),
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
                        } else if scope.as_ref() == "library-rename" {
                            panel.renaming = None;
                        } else {
                            panel.popup = None;
                        }
                        cx.notify();
                    }
                },
            ),
        ];
        Self {
            model: LibraryModel::new(items),
            ui,
            i18n,
            query,
            rename,
            popup: None,
            preview: None,
            renaming: None,
            focus: cx.focus_handle(),
            category_focus: Category::ALL
                .iter()
                .map(|_| cx.focus_handle())
                .collect(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_i18n(&mut self, i18n: I18n, cx: &mut Context<'_, Self>) {
        self.i18n = i18n;
        cx.notify();
    }

    pub fn modal_scope(&self) -> Option<SharedString> {
        if self.renaming.is_some() {
            Some("library-rename".into())
        } else {
            self.preview.as_ref().map(|_| "library-preview".into())
        }
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
        theme::PageHeader::new(self.t("dock.library"))
            .trailing(
                theme::ToolbarSearch::new(
                    "library-search",
                    self.t("library.search"),
                    self.query.clone(),
                )
                .render(&t),
            )
            .trailing(theme::anchor_trigger(sort, "library-sort", &self.ui))
            .trailing(self.view_switch(cx))
            .render(&t, window)
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
        theme::icon_segment(
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
        )
    }

    fn filters(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let t = self.theme(cx);
        let mut filters = div()
            .id("library-filters")
            .role(Role::TabList)
            .aria_label(self.t("library.categories"))
            .flex()
            .gap(px(4.))
            .items_center()
            .flex_wrap();
        for (index, category) in Category::ALL.into_iter().enumerate() {
            let selected = self.model.category == category;
            filters = filters.child(
                theme::filter_chip(
                    SharedString::from(format!("library-filter-{index}")),
                    self.t(category.key()),
                    selected,
                    &t,
                )
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
        filters
    }

    fn content(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let t = self.theme(cx);
        let groups = self.model.groups(self.query.read(cx).as_str());
        let mut column = div().w_full().flex().flex_col().gap(px(24.));
        if groups.is_empty() {
            return column.child(
                div().py(px(72.)).child(
                    theme::EmptyState::new(
                        self.t("library.empty.title"),
                        self.t("library.empty.body"),
                    )
                    .icon(theme::icons::search().size(px(28.)))
                    .render(&t),
                ),
            );
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
                        item.description.clone(),
                    )
                    .truncate()
                    .render(&t),
                ),
        )
        .child(
            theme::Text::new(theme::TextRole::Meta, item.time.clone())
                .render(&t)
                .flex_shrink_0(),
        )
        .child(div().w(px(1.)).flex_shrink_0())
        .child(theme::anchor_trigger(more, more_id, &self.ui))
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.preview = Some(id.clone());
            panel.popup = None;
            cx.notify();
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
            div()
                .absolute()
                .top(px(10.))
                .right(px(10.))
                .child(theme::anchor_trigger(more, more_id, &self.ui)),
        )
        .child(self.file_icon(item, 28., cx))
        .child(
            theme::Title::new(theme::TitleLevel::Card, item.title.clone())
                .truncate()
                .render(&t),
        )
        .child(
            theme::Text::new(theme::TextRole::Body, item.description.clone())
                .render(&t)
                .whitespace_normal(),
        )
        .child(
            theme::Text::new(theme::TextRole::Meta, item.time.clone())
                .render(&t),
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.preview = Some(id.clone());
            panel.popup = None;
            cx.notify();
        }))
    }

    /// Same 14px text and 32px row as the view-options button menu.
    fn menu_row(
        item: gpui::Stateful<gpui::Div>,
        theme: &theme::ResolvedTheme,
    ) -> gpui::Stateful<gpui::Div> {
        let metrics = theme.control_metrics();
        let hover = theme.soft();
        item.h(px(32.))
            .max_h(px(32.))
            .px(px(8.))
            .border_0()
            .rounded(px(6.))
            .overflow_hidden()
            .flex()
            .items_center()
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
                    menu = menu.child(
                        Self::menu_row(
                            theme::menu_item(
                                SharedString::from(format!(
                                    "library-sort-{}",
                                    sort.key()
                                )),
                                self.t(sort.key()),
                                false,
                                &t,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.model.sort = sort;
                                    panel.popup = None;
                                    cx.notify();
                                }),
                            ),
                            &t,
                        )
                        .gap(px(8.))
                        .child(div().w(px(16.)).when(
                            sort == self.model.sort,
                            |mark| {
                                mark.child(theme::icons::check().size(px(14.)))
                            },
                        ))
                        .child(self.t(sort.key())),
                    );
                }
                ("library-sort-menu", SharedString::from("library-sort"))
            }
            Popup::Item(id) => {
                let preview_id = id.clone();
                let rename_id = id.clone();
                let remove_id = id.clone();
                menu = menu
                    .child(
                        Self::menu_row(
                            theme::menu_item(
                                "library-menu-preview",
                                self.t("library.preview"),
                                false,
                                &t,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.preview = Some(preview_id.clone());
                                    panel.popup = None;
                                    cx.notify();
                                }),
                            ),
                            &t,
                        )
                        .child(self.t("library.preview")),
                    )
                    .child(
                        Self::menu_row(
                            theme::menu_item(
                                "library-menu-rename",
                                self.t("library.rename"),
                                false,
                                &t,
                                cx.listener(move |panel, _, _, cx| {
                                    if let Some(item) = panel
                                        .model
                                        .items
                                        .iter()
                                        .find(|item| item.id == rename_id)
                                    {
                                        let title = item.title.clone();
                                        panel.rename.update(cx, |state, cx| {
                                            state.emplace(&title, cx);
                                        });
                                        panel.renaming =
                                            Some(rename_id.clone());
                                    }
                                    panel.popup = None;
                                    cx.notify();
                                }),
                            ),
                            &t,
                        )
                        .child(self.t("library.rename")),
                    )
                    .child(
                        Self::menu_row(
                            theme::menu_item(
                                "library-menu-remove",
                                self.t("library.remove"),
                                false,
                                &t,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.model.remove(&remove_id);
                                    panel.popup = None;
                                    cx.notify();
                                }),
                            ),
                            &t,
                        )
                        .text_color(t.danger())
                        .child(self.t("library.remove")),
                    );
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
        let id = self.preview.as_ref()?;
        let item = self.model.items.iter().find(|item| &item.id == id)?;
        let t = self.theme(cx);
        let close = theme::IconButton::new(
            "library-close-preview",
            self.t("library.close"),
            theme::icons::x(),
        )
        .render(
            &t,
            cx.listener(|panel, _, _, cx| {
                panel.preview = None;
                cx.notify();
            }),
        );
        let body = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(self.file_icon(item, 28., cx))
            .child(
                theme::Title::new(
                    theme::TitleLevel::Section,
                    item.title.clone(),
                )
                .render(&t),
            )
            .child(
                theme::Text::new(
                    theme::TextRole::Body,
                    item.description.clone(),
                )
                .render(&t)
                .whitespace_normal(),
            )
            .child(
                theme::Text::new(
                    theme::TextRole::Meta,
                    format!("{} · {}", self.date_label(&item.date), item.time),
                )
                .render(&t),
            )
            .child(
                theme::Text::new(
                    theme::TextRole::Body,
                    item.preview.clone().unwrap_or_else(|| {
                        self.t("library.preview.unavailable")
                    }),
                )
                .render(&t)
                .whitespace_normal(),
            );
        let dialog = theme::dialog(
            "library-preview-dialog",
            item.title.clone(),
            &t,
            &self.ui,
            cx,
        )
        .w(px(620.))
        .max_w(gpui::relative(0.9))
        .max_h(gpui::relative(0.9))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .rounded_t(t.modal_radius())
                .p(px(12.))
                .flex()
                .items_center()
                .child(
                    theme::Text::new(
                        theme::TextRole::Meta,
                        self.t("library.preview"),
                    )
                    .render(&t)
                    .flex_1(),
                )
                .child(close),
        )
        .child(body);
        Some(
            theme::modal_layer("library-preview-layer", &t, &self.ui, dialog)
                .into_any_element(),
        )
    }

    fn rename_layer(&self, cx: &Context<'_, Self>) -> Option<gpui::AnyElement> {
        let id = self.renaming.as_ref()?.clone();
        let t = self.theme(cx);
        let title = self.rename.read(cx).as_str().trim().to_string();
        let dialog = theme::dialog(
            "library-rename-dialog",
            self.t("library.rename"),
            &t,
            &self.ui,
            cx,
        )
        .w(px(380.))
        .max_w(gpui::relative(0.9))
        .p(px(24.))
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(
            theme::Title::new(
                theme::TitleLevel::Section,
                self.t("library.rename"),
            )
            .render(&t),
        )
        .child(
            theme::TextField::new(
                "library-new-name",
                self.t("library.name"),
                self.rename.clone(),
            )
            .render(&t, cx),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(
                    theme::Button::new(
                        "library-rename-cancel",
                        self.t("library.cancel"),
                    )
                    .render(
                        &t,
                        cx.listener(|panel, _, _, cx| {
                            panel.renaming = None;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    theme::Button::new(
                        "library-rename-save",
                        self.t("library.save"),
                    )
                    .variant(theme::ControlVariant::Primary)
                    .disabled(title.is_empty())
                    .render(
                        &t,
                        cx.listener(move |panel, _, _, cx| {
                            let title = panel
                                .rename
                                .read(cx)
                                .as_str()
                                .trim()
                                .to_string();
                            if !title.is_empty() {
                                panel.model.rename(&id, &title);
                                panel.renaming = None;
                                cx.notify();
                            }
                        }),
                    ),
                ),
        );
        Some(
            theme::modal_layer("library-rename-layer", &t, &self.ui, dialog)
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
        let t = self.theme(cx);
        let mut root = div()
            .id("library")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(t.library_canvas())
            .text_color(t.library_ink())
            .flex()
            .flex_col()
            .child(self.toolbar(window, cx))
            .child(
                div()
                    .id("library-scroll")
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
        if let Some(rename) = self.rename_layer(cx) {
            root = root.child(rename);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_i18n::Locale;
    use gpui::{Focusable, TestAppContext, VisualTestContext, accesskit};

    struct LibraryHarness {
        panel: Entity<LibraryPanel>,
        _panel_sub: Subscription,
    }

    impl LibraryHarness {
        fn new(cx: &mut Context<'_, Self>) -> Self {
            let items = serde_json::from_str(include_str!(
                "../../examples/fixtures/library.json"
            ))
            .expect("the isolated library fixture must parse");
            let ui = UiContext::new(theme::ThemeOptions::default(), false, cx);
            let mut i18n = I18n::embedded("Flowy");
            i18n.set_locale(Locale::En);
            let panel = cx.new(|cx| LibraryPanel::new(items, i18n, ui, cx));
            let panel_sub = cx.observe(&panel, |_, _, cx| cx.notify());
            Self {
                panel,
                _panel_sub: panel_sub,
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
            div().size_full().child(self.panel.clone())
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        // Overlay focus is deferred until paint, and popovers need the prior
        // trigger measurement. Drive the executor and native frame callbacks
        // together rather than reading the first transitional tree.
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.set_a11y_forced(true);
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        }
        cx.run_until_parked();
    }

    fn activate(
        cx: &mut VisualTestContext,
        label: &str,
        role: accesskit::Role,
    ) {
        draw(cx);
        let id = cx.update(|window, _| {
            let tree = window.a11y_tree().expect("library accessibility tree");
            let (id, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label) && node.role() == role)
                .unwrap_or_else(|| {
                    let labels: Vec<_> = tree.nodes.iter()
                        .filter_map(|(_, node)| node.label().map(|label| (node.role(), label)))
                        .collect();
                    panic!("missing {role:?} labeled {label:?}; tree labels: {labels:?}")
                });
            assert!(node.supports_action(accesskit::Action::Click));
            *id
        });
        cx.update(|window, cx| {
            window.perform_a11y_action(id, accesskit::Action::Click, None, cx);
        });
        draw(cx);
    }

    fn press(cx: &mut VisualTestContext, key: &str) {
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
    fn category_grid_toggle_and_typed_query_compose(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        activate(cx, "Images", accesskit::Role::Tab);
        activate(cx, "Grid view", accesskit::Role::Button);
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.model.category, Category::Image);
            assert_eq!(panel.model.view, LibraryView::Grid);
        });
        cx.update(|window, cx| {
            let focus = panel.read(cx).query.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("dew-stone");
        draw(cx);
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.query.read(cx).as_str(), "dew-stone");
            let results = panel.model.filtered(panel.query.read(cx).as_str());
            assert_eq!(results.len(), 3);
            assert!(
                results.iter().all(|item| item.category == Category::Image)
            );
        });
        activate(cx, "List view", accesskit::Role::Button);
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.model.view, LibraryView::List);
            assert_eq!(panel.model.category, Category::Image);
            assert_eq!(panel.query.read(cx).as_str(), "dew-stone");
        });
        activate(cx, "Text and PDF", accesskit::Role::Tab);
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(
                panel.model.filtered(panel.query.read(cx).as_str()).len(),
                0
            );
        });
        cx.update(|window, _| {
            let tree = window.a11y_tree().expect("library accessibility tree");
            assert!(
                !tree.nodes.iter().any(|(_, node)| {
                    node.role() == accesskit::Role::Button
                        && node.label() == Some("More actions")
                }),
                "an empty result must not retain file row actions"
            );
        });
    }

    #[gpui::test]
    fn sort_and_item_menu_preview_close_through_escape(
        cx: &mut TestAppContext,
    ) {
        let (host, cx) = cx.add_window_view(|_, cx| LibraryHarness::new(cx));
        let panel = cx.read_entity(&host, |host, _| host.panel.clone());
        activate(cx, "Sort", accesskit::Role::Button);
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.popup, Some(Popup::Sort));
        });
        activate(cx, "Oldest first", accesskit::Role::MenuItem);
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.model.sort, LibrarySort::Oldest);
            assert!(panel.popup.is_none());
            let rows = panel.model.filtered("");
            assert!(rows.windows(2).all(|pair| pair[0].date <= pair[1].date));
        });
        activate(cx, "Sort", accesskit::Role::Button);
        activate(cx, "Newest first", accesskit::Role::MenuItem);
        activate(cx, "More actions", accesskit::Role::Button);
        cx.read_entity(&panel, |panel, _| {
            assert!(matches!(panel.popup, Some(Popup::Item(_))));
        });
        activate(cx, "Preview", accesskit::Role::MenuItem);
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.preview.as_deref(), Some("1104-byd-presentation"));
            assert!(panel.popup.is_none());
            assert!(panel.ui.overlays.read(cx).modal_active());
        });
        press(cx, "escape");
        cx.read_entity(&panel, |panel, cx| {
            assert!(panel.preview.is_none());
            assert!(!panel.ui.overlays.read(cx).modal_active());
        });
        activate(cx, "Sort", accesskit::Role::Button);
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
            let mut english = I18n::embedded("Flowy");
            english.set_locale(Locale::En);
            panel.set_i18n(english, cx);
            assert_eq!(panel.t("library.search"), "Search files");
            let mut chinese = I18n::embedded("Flowy");
            chinese.set_locale(Locale::ZhCn);
            panel.set_i18n(chinese, cx);
            assert_eq!(panel.t("library.search"), "搜索文件");
            assert_eq!(panel.model.category, Category::Image);
            assert_eq!(panel.model.view, LibraryView::Grid);
            assert_eq!(panel.model.sort, LibrarySort::Name);
        });
    }
}
