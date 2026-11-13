use super::FlowyApp;
use flowy_ui as theme;
use gpui::{ColorExt, Context, SharedString, div, prelude::*, px, rgb};

const PAGE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Video,
    App,
    Site,
    Image,
    Slides,
}

impl Kind {
    const fn key(self) -> &'static str {
        match self {
            Self::Video => "gallery.kind_video",
            Self::App => "gallery.kind_app",
            Self::Site => "gallery.kind_site",
            Self::Image => "gallery.kind_image",
            Self::Slides => "gallery.kind_slides",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Filter {
    All,
    Kind(Kind),
}

impl Filter {
    const ALL: [Self; 6] = [
        Self::All,
        Self::Kind(Kind::Video),
        Self::Kind(Kind::App),
        Self::Kind(Kind::Site),
        Self::Kind(Kind::Image),
        Self::Kind(Kind::Slides),
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::All => "gallery.all",
            Self::Kind(kind) => kind.key(),
        }
    }

    fn matches(self, kind: Kind) -> bool {
        match self {
            Self::All => true,
            Self::Kind(wanted) => wanted == kind,
        }
    }
}

struct Item {
    id: &'static str,
    key: &'static str,
    body: &'static str,
    kind: Kind,
    tint: u32,
    strip: bool,
}

const GALLERY: &[Item] = &[
    Item {
        id: "documentary",
        key: "gallery.documentary",
        body: "gallery.documentary_body",
        kind: Kind::Video,
        tint: 0x009b_b0b8,
        strip: true,
    },
    Item {
        id: "budget",
        key: "gallery.budget",
        body: "gallery.budget_body",
        kind: Kind::App,
        tint: 0x0024_3044,
        strip: true,
    },
    Item {
        id: "fashion",
        key: "gallery.fashion",
        body: "gallery.fashion_body",
        kind: Kind::Image,
        tint: 0x00d7_b48a,
        strip: true,
    },
    Item {
        id: "cafe",
        key: "gallery.cafe",
        body: "gallery.cafe_body",
        kind: Kind::Image,
        tint: 0x00e6_d2b8,
        strip: true,
    },
    Item {
        id: "pyramid",
        key: "gallery.pyramid",
        body: "gallery.pyramid_body",
        kind: Kind::Video,
        tint: 0x00c4_8a4a,
        strip: true,
    },
    Item {
        id: "anime",
        key: "gallery.anime",
        body: "gallery.anime_body",
        kind: Kind::Video,
        tint: 0x00e7_a15a,
        strip: false,
    },
    Item {
        id: "bedtime",
        key: "gallery.bedtime",
        body: "gallery.bedtime_body",
        kind: Kind::Video,
        tint: 0x00f0_c27a,
        strip: false,
    },
    Item {
        id: "house",
        key: "gallery.house",
        body: "gallery.house_body",
        kind: Kind::Video,
        tint: 0x008e_a8c8,
        strip: false,
    },
    Item {
        id: "bee",
        key: "gallery.bee",
        body: "gallery.bee_body",
        kind: Kind::Video,
        tint: 0x00e0_b000,
        strip: false,
    },
    Item {
        id: "fashion-film",
        key: "gallery.fashion_film",
        body: "gallery.fashion_film_body",
        kind: Kind::Video,
        tint: 0x00a0_4030,
        strip: false,
    },
    Item {
        id: "neon",
        key: "gallery.neon",
        body: "gallery.neon_body",
        kind: Kind::Video,
        tint: 0x0030_20a0,
        strip: false,
    },
    Item {
        id: "esports",
        key: "gallery.esports",
        body: "gallery.esports_body",
        kind: Kind::Video,
        tint: 0x0080_1020,
        strip: false,
    },
    Item {
        id: "motion",
        key: "gallery.motion",
        body: "gallery.motion_body",
        kind: Kind::Video,
        tint: 0x00e0_5010,
        strip: false,
    },
    Item {
        id: "cinema",
        key: "gallery.cinema",
        body: "gallery.cinema_body",
        kind: Kind::Video,
        tint: 0x0020_2840,
        strip: false,
    },
    Item {
        id: "bike",
        key: "gallery.bike",
        body: "gallery.bike_body",
        kind: Kind::Video,
        tint: 0x0060_6870,
        strip: false,
    },
    Item {
        id: "soda",
        key: "gallery.soda",
        body: "gallery.soda_body",
        kind: Kind::Video,
        tint: 0x00b0_d070,
        strip: false,
    },
    Item {
        id: "trip",
        key: "gallery.trip",
        body: "gallery.trip_body",
        kind: Kind::Site,
        tint: 0x0060_90c0,
        strip: false,
    },
    Item {
        id: "slides",
        key: "gallery.slides",
        body: "gallery.slides_body",
        kind: Kind::Slides,
        tint: 0x00c0_8060,
        strip: false,
    },
];

fn strip_items() -> impl Iterator<Item = &'static Item> {
    GALLERY.iter().filter(|item| item.strip)
}

fn strip_slots() -> usize {
    strip_items().count().saturating_add(1)
}

fn page_count() -> usize {
    strip_slots().div_ceil(PAGE)
}

fn item_by_id(id: &str) -> Option<&'static Item> {
    GALLERY.iter().find(|item| item.id == id)
}

fn similar(item: &Item) -> Vec<&'static Item> {
    let mut picked = Vec::new();
    for other in GALLERY {
        if other.id != item.id && other.kind == item.kind {
            picked.push(other);
            if picked.len() == 4 {
                return picked;
            }
        }
    }
    for other in GALLERY {
        if other.id != item.id && !picked.iter().any(|kept| kept.id == other.id)
        {
            picked.push(other);
            if picked.len() == 4 {
                break;
            }
        }
    }
    picked
}

impl FlowyApp {
    pub fn gallery_row(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let pages = page_count();
        let page = self.gallery.page.min(pages.saturating_sub(1));
        let start = page.saturating_mul(PAGE);
        let mut row = div().flex().gap_4();
        for index in start..start.saturating_add(PAGE).min(strip_slots()) {
            row = if index + 1 == strip_slots() {
                row.child(self.see_all_card(cx))
            } else {
                let item = strip_items().nth(index).expect("strip slot");
                row.child(self.gallery_card(item, cx))
            };
        }
        div()
            .relative()
            .child(row)
            .when(page > 0, |strip| {
                strip.child(gallery_arrow("gallery-prev", true, cx))
            })
            .when(page + 1 < pages, |strip| {
                strip.child(gallery_arrow("gallery-next", false, cx))
            })
    }
}

fn gallery_arrow(
    id: &'static str,
    previous: bool,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    let mut button = div()
        .id(id)
        .absolute()
        .top(px(58.))
        .size(px(32.))
        .rounded_full()
        .bg(theme::card())
        .border_1()
        .border_color(theme::line())
        .shadow_md()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|arrow| arrow.bg(theme::soft()))
        .child(if previous { "‹" } else { "›" })
        .on_click(cx.listener(move |app, _event, _window, cx| {
            cx.stop_propagation();
            if previous {
                app.gallery.page = app.gallery.page.saturating_sub(1);
            } else {
                let last = page_count().saturating_sub(1);
                app.gallery.page = app.gallery.page.saturating_add(1).min(last);
            }
            cx.notify();
        }));
    button = if previous {
        button.left(px(8.))
    } else {
        button.right(px(8.))
    };
    button
}

impl FlowyApp {
    fn gallery_card(
        &self,
        item: &Item,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = item.id;
        div()
            .id(item.id)
            .flex_1()
            .min_w_0()
            .cursor_pointer()
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.open_idea(id, cx);
            }))
            .child(thumb(item.tint, self.t(item.kind.key())))
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .whitespace_normal()
                    .child(self.t(item.key)),
            )
    }

    fn see_all_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let count = GALLERY.len().to_string();
        div()
            .id("gallery-see-all")
            .flex_1()
            .min_w_0()
            .cursor_pointer()
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.gallery.ideas_open = true;
                app.gallery.idea_filter = Filter::All;
                cx.notify();
            }))
            .child(
                div()
                    .h(px(148.))
                    .rounded_xl()
                    .bg(theme::soft())
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(grid_mark())
                    .child(div().text_sm().child(self.t("gallery.see_all")))
                    .child(
                        div().text_xs().text_color(theme::muted()).child(
                            self.tf("gallery.count", &[("count", &count)]),
                        ),
                    ),
            )
    }

    pub(super) fn ideas_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("ideas-layer")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::ink().opacity(0.28))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.gallery.ideas_open = false;
                cx.notify();
            }))
            .child(self.ideas_card(cx))
    }

    fn ideas_card(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .id("ideas-card")
            .w(px(860.))
            .h(px(640.))
            .max_w_full()
            .rounded_xl()
            .bg(theme::card())
            .shadow_md()
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .on_click(cx.listener(|_app, _event, _window, cx| {
                cx.stop_propagation();
            }))
            .child(self.ideas_heading(cx))
            .child(self.idea_filters(cx))
            .child(self.idea_grid(cx))
    }

    fn ideas_heading(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t("gallery.browse")),
            )
            .child(
                div()
                    .id("ideas-close")
                    .size(px(28.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|mark| mark.bg(theme::soft()))
                    .child("×")
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.gallery.ideas_open = false;
                        cx.notify();
                    })),
            )
    }

    fn idea_filters(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div().flex().gap_2().flex_wrap();
        for filter in Filter::ALL {
            let selected = self.gallery.idea_filter == filter;
            let id =
                SharedString::from(format!("idea-filter-{}", filter.key()));
            row = row.child(
                div()
                    .id(id)
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .text_sm()
                    .cursor_pointer()
                    .bg(if selected {
                        theme::ink()
                    } else {
                        theme::card()
                    })
                    .text_color(if selected {
                        theme::on_ink()
                    } else {
                        theme::ink()
                    })
                    .border_1()
                    .border_color(if selected {
                        theme::ink()
                    } else {
                        theme::line()
                    })
                    .child(self.t(filter.key()))
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.gallery.idea_filter = filter;
                        cx.notify();
                    })),
            );
        }
        row
    }

    fn idea_grid(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut grid = div()
            .id("idea-grid")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_wrap()
            .gap_3()
            .content_start();
        for item in GALLERY {
            if !self.gallery.idea_filter.matches(item.kind) {
                continue;
            }
            grid = grid.child(self.idea_tile(item, cx));
        }
        grid
    }

    fn idea_tile(
        &self,
        item: &Item,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let idea = item.id;
        let id = SharedString::from(format!("idea-{}", item.id));
        div()
            .id(id)
            .w(px(260.))
            .cursor_pointer()
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.open_idea(idea, cx);
            }))
            .child(thumb(item.tint, self.t(item.kind.key())))
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .whitespace_normal()
                    .child(self.t(item.key)),
            )
    }

    fn open_idea(&mut self, id: &'static str, cx: &mut Context<'_, Self>) {
        self.gallery.idea_id = Some(id);
        self.gallery.idea_note = false;
        self.gallery.ideas_open = false;
        cx.notify();
    }

    pub(super) fn idea_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let item = self.gallery.idea_id.and_then(item_by_id);
        let mut layer = div()
            .id("idea-layer")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::ink().opacity(0.28))
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.gallery.idea_id = None;
                app.gallery.idea_note = false;
                cx.notify();
            }));
        if let Some(item) = item {
            layer = layer.child(self.idea_card(item, cx));
        }
        layer
    }

    fn idea_card(
        &self,
        item: &Item,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let body = item.body;
        div()
            .id("idea-card")
            .w(px(880.))
            .max_w_full()
            .rounded_xl()
            .bg(theme::card())
            .shadow_md()
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .on_click(cx.listener(|_app, _event, _window, cx| {
                cx.stop_propagation();
            }))
            .child(self.idea_heading(item, cx))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(self.t(item.body)),
            )
            .child(idea_stage(item, cx))
            .when(self.gallery.idea_note && item.kind == Kind::Video, |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(theme::muted())
                        .child(self.t("gallery.play_later")),
                )
            })
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("idea-use")
                        .px_4()
                        .py_2()
                        .rounded_full()
                        .bg(theme::fill())
                        .text_color(theme::on_fill())
                        .text_sm()
                        .cursor_pointer()
                        .child(self.t("gallery.use"))
                        .on_click(cx.listener(
                            move |app, _event, _window, cx| {
                                cx.stop_propagation();
                                app.gallery.idea_id = None;
                                app.gallery.idea_note = false;
                                app.fill_prompt_key(body, cx);
                            },
                        )),
                ),
            )
            .child(self.similar_row(item, cx))
    }

    fn idea_heading(
        &self,
        item: &Item,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .px_2()
                    .py(px(2.))
                    .rounded_md()
                    .bg(theme::soft())
                    .text_xs()
                    .child(self.t(item.kind.key())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t(item.key)),
            )
            .child(
                div()
                    .id("idea-close")
                    .size(px(28.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|mark| mark.bg(theme::soft()))
                    .child("×")
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.gallery.idea_id = None;
                        app.gallery.idea_note = false;
                        cx.notify();
                    })),
            )
    }

    fn similar_row(
        &self,
        item: &Item,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut row = div().flex().gap_3();
        for other in similar(item) {
            let id = other.id;
            let element_id = SharedString::from(format!("similar-{id}"));
            row = row.child(
                div()
                    .id(element_id)
                    .w(px(168.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.open_idea(id, cx);
                    }))
                    .child(poster(other.tint, 84.))
                    .child(
                        div()
                            .mt_1()
                            .text_xs()
                            .whitespace_normal()
                            .child(self.t(other.key)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.similar_heading(cx))
            .child(row)
    }

    fn similar_heading(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(self.t("gallery.similar")),
            )
            .child(
                div()
                    .id("idea-detail-all")
                    .text_sm()
                    .cursor_pointer()
                    .child(format!("{}  ›", self.t("gallery.see_all")))
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.gallery.idea_id = None;
                        app.gallery.idea_note = false;
                        app.gallery.ideas_open = true;
                        app.gallery.idea_filter = Filter::All;
                        cx.notify();
                    })),
            )
    }
}

fn idea_stage(item: &Item, cx: &Context<'_, FlowyApp>) -> impl IntoElement {
    let mut stage = div()
        .relative()
        .h(px(280.))
        .rounded_xl()
        .overflow_hidden()
        .bg(rgb(item.tint));
    if item.kind == Kind::Video {
        stage = stage.child(transport(cx));
    }
    stage
}

fn transport(cx: &Context<'_, FlowyApp>) -> impl IntoElement {
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(36.))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .bg(rgb(0x0000_0000).opacity(0.55))
                .child(transport_hit("idea-play", "▶", cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x00ff_ffff))
                        .child("0:00 / 0:12"),
                )
                .child(div().flex_1())
                .child(transport_hit("idea-volume", volume_mark(), cx))
                .child(transport_hit("idea-more", "···", cx))
                .child(transport_hit("idea-full", full_mark(), cx)),
        )
}

fn transport_hit(
    id: &'static str,
    mark: impl IntoElement,
    cx: &Context<'_, FlowyApp>,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(24.))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(0x00ff_ffff))
        .text_xs()
        .cursor_pointer()
        .child(mark)
        .on_click(cx.listener(|app, _event, _window, cx| {
            cx.stop_propagation();
            app.gallery.idea_note = true;
            cx.notify();
        }))
}

fn volume_mark() -> impl IntoElement {
    div()
        .flex()
        .items_end()
        .gap(px(2.))
        .child(div().w(px(2.)).h(px(6.)).bg(rgb(0x00ff_ffff)))
        .child(div().w(px(2.)).h(px(9.)).bg(rgb(0x00ff_ffff)))
        .child(div().w(px(2.)).h(px(12.)).bg(rgb(0x00ff_ffff)))
}

fn full_mark() -> impl IntoElement {
    div()
        .size(px(11.))
        .border_1()
        .border_color(rgb(0x00ff_ffff))
}

fn poster(tint: u32, height: f32) -> impl IntoElement {
    div()
        .h(px(height))
        .rounded_lg()
        .bg(rgb(tint))
        .border_1()
        .border_color(theme::line())
}

fn thumb(tint: u32, kind: String) -> impl IntoElement {
    div()
        .relative()
        .h(px(148.))
        .rounded_xl()
        .bg(rgb(tint))
        .border_1()
        .border_color(theme::line())
        .child(
            div()
                .absolute()
                .top(px(8.))
                .left(px(8.))
                .px_2()
                .py(px(2.))
                .rounded_md()
                .bg(rgb(0x001c_1c1c).opacity(0.72))
                .text_color(rgb(0x00ff_ffff))
                .text_xs()
                .child(kind),
        )
}

fn grid_mark() -> impl IntoElement {
    let mut mark = div().flex().flex_col().gap(px(3.));
    for _row in 0..2 {
        let mut line = div().flex().gap(px(3.));
        for _column in 0..2 {
            line =
                line.child(div().size(px(6.)).rounded(px(1.)).bg(theme::ink()));
        }
        mark = mark.child(line);
    }
    mark
}

#[cfg(test)]
mod tests {
    use super::{GALLERY, page_count, similar, strip_slots};

    #[test]
    fn strip_is_six_slots_on_two_pages() {
        assert_eq!(strip_slots(), 6);
        assert_eq!(page_count(), 2);
    }

    #[test]
    fn similar_ideas_leave_out_the_open_item() {
        let anime = GALLERY.iter().find(|item| item.id == "anime").unwrap();
        let row = similar(anime);
        assert!(row.len() <= 4);
        assert!(row.iter().all(|item| item.id != "anime"));
        assert!(row.iter().all(|item| item.kind == super::Kind::Video));
    }
}
