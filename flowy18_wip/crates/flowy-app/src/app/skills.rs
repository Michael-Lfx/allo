use super::FlowyApp;
use crate::prefs::Plan;
use flowy_ui as theme;
use gpui::{
    Context, Pixels, SharedString, Size, Window, div, prelude::*, px, rgb,
};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
const MAX_UNSCROLLABLE_FOLLOWS: u8 = 2;
const MAX_EMPTY_FOLLOWS: u8 = 2;
const MAX_LAYOUT_POLLS: u8 = 8;
const FOLLOW_THRESHOLD_PX: f32 = 320.;
const SHELF_SUMMARY_LINES: usize = 3;
/// Market tabs, in shelf order. Labels live in the locale files.
const MARKET_CATEGORIES: &[(&str, &str)] = &[
    ("integrations", "skills.cat_integrations"),
    ("automation", "skills.cat_automation"),
    ("research", "skills.cat_research"),
    ("development", "skills.cat_development"),
    ("productivity", "skills.cat_productivity"),
    ("communication", "skills.cat_communication"),
    ("creative", "skills.cat_creative"),
    ("knowledge", "skills.cat_knowledge"),
    ("agents", "skills.cat_agents"),
    ("operations", "skills.cat_operations"),
    ("security", "skills.cat_security"),
    ("finance", "skills.cat_finance"),
    ("lifestyle", "skills.cat_lifestyle"),
    ("other", "skills.cat_other"),
];

/// One skill card on the market shelf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ShelfCard {
    pub(super) owner: String,
    pub(super) slug: String,
    pub(super) version: String,
    pub(super) title: String,
    pub(super) summary: String,
    pub(super) categories: Vec<String>,
    pub(super) official: bool,
    pub(super) downloads: Option<u64>,
    pub(super) installs: Option<u64>,
    pub(super) updated_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum InstallPhase {
    Verifying,
    Downloading,
    Installing,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InstallUi {
    pub(super) key: String,
    pub(super) phase: InstallPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Category {
    Coding,
    Data,
    Automation,
    Business,
    Design,
    Media,
    Content,
    App,
}

impl Category {
    const fn key(self) -> &'static str {
        match self {
            Self::Coding => "skills.cat_coding",
            Self::Data => "skills.cat_data",
            Self::Automation => "skills.cat_automation",
            Self::Business => "skills.cat_business",
            Self::Design => "skills.cat_design",
            Self::Media => "skills.cat_media",
            Self::Content => "skills.cat_content",
            Self::App => "skills.cat_app",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Author {
    Official,
    Community,
}

impl Author {
    const fn key(self) -> &'static str {
        match self {
            Self::Official => "skills.author_official",
            Self::Community => "skills.author_community",
        }
    }
}

struct Skill {
    id: &'static str,
    /// Slash-command name without the leading `/`.
    command: &'static str,
    title: &'static str,
    body: &'static str,
    summary: &'static str,
    categories: &'static [Category],
    author: Author,
    uses: &'static str,
    date: &'static str,
    tint: u32,
    mark: &'static str,
    prompts: &'static [&'static str],
    required_plan: Plan,
    details_url: Option<&'static str>,
}

const SKILLS: &[Skill] = &[
    Skill {
        id: "excel",
        command: "excel",
        title: "skills.excel",
        body: "skills.excel_body",
        summary: "skills.excel_summary",
        categories: &[Category::Data, Category::App],
        author: Author::Official,
        uses: "9.8k",
        date: "skills.excel_date",
        tint: 0x001e_7a46,
        mark: "X",
        prompts: &["skills.excel_p1", "skills.excel_p2", "skills.excel_p3"],
        required_plan: Plan::Free,
        details_url: None,
    },
    Skill {
        id: "traffic",
        command: "traffic",
        title: "skills.traffic",
        body: "skills.traffic_body",
        summary: "skills.traffic_summary",
        categories: &[Category::Data],
        author: Author::Official,
        uses: "4.8k",
        date: "skills.traffic_date",
        tint: 0x001a_2744,
        mark: "TR",
        prompts: &[
            "skills.traffic_p1",
            "skills.traffic_p2",
            "skills.traffic_p3",
        ],
        required_plan: Plan::Free,
        details_url: None,
    },
    Skill {
        id: "slides",
        command: "slides",
        title: "skills.slides",
        body: "skills.slides_body",
        summary: "skills.slides_summary",
        categories: &[Category::Content],
        author: Author::Official,
        uses: "13.6k",
        date: "skills.slides_date",
        tint: 0x00c4_5c26,
        mark: "SD",
        prompts: &["skills.slides_p1", "skills.slides_p2", "skills.slides_p3"],
        required_plan: Plan::Free,
        details_url: None,
    },
    Skill {
        id: "research",
        command: "research",
        title: "skills.research",
        body: "skills.research_body",
        summary: "skills.research_summary",
        categories: &[Category::Content],
        author: Author::Official,
        uses: "4.7k",
        date: "skills.research_date",
        tint: 0x00e8_e4dc,
        mark: "Aa",
        prompts: &[
            "skills.research_p1",
            "skills.research_p2",
            "skills.research_p3",
        ],
        required_plan: Plan::Free,
        details_url: None,
    },
    Skill {
        id: "nano",
        command: "nano-banana",
        title: "skills.nano",
        body: "skills.nano_body",
        summary: "skills.nano_summary",
        categories: &[Category::Design],
        author: Author::Community,
        uses: "3.3k",
        date: "skills.nano_date",
        tint: 0x00f2_c94c,
        mark: "NB",
        prompts: &["skills.nano_p1", "skills.nano_p2", "skills.nano_p3"],
        required_plan: Plan::Pro,
        details_url: None,
    },
    Skill {
        id: "flux",
        command: "flux",
        title: "skills.flux",
        body: "skills.flux_body",
        summary: "skills.flux_summary",
        categories: &[Category::Design],
        author: Author::Community,
        uses: "2.8k",
        date: "skills.flux_date",
        tint: 0x00d4_5a2a,
        mark: "FX",
        prompts: &["skills.flux_p1", "skills.flux_p2", "skills.flux_p3"],
        required_plan: Plan::Pro,
        details_url: None,
    },
    Skill {
        id: "script",
        command: "script",
        title: "skills.script",
        body: "skills.script_body",
        summary: "skills.script_summary",
        categories: &[Category::Coding, Category::Automation],
        author: Author::Official,
        uses: "6.1k",
        date: "skills.script_date",
        tint: 0x0024_3b55,
        mark: "</>",
        prompts: &["skills.script_p1", "skills.script_p2", "skills.script_p3"],
        required_plan: Plan::Free,
        details_url: None,
    },
    Skill {
        id: "pipeline",
        command: "ops-pipeline",
        title: "skills.pipeline",
        body: "skills.pipeline_body",
        summary: "skills.pipeline_summary",
        categories: &[Category::Automation, Category::Business],
        author: Author::Community,
        uses: "2.1k",
        date: "skills.pipeline_date",
        tint: 0x003a_2f5c,
        mark: ">>",
        prompts: &[
            "skills.pipeline_p1",
            "skills.pipeline_p2",
            "skills.pipeline_p3",
        ],
        required_plan: Plan::Pro,
        details_url: None,
    },
    Skill {
        id: "clip",
        command: "highlight-clip",
        title: "skills.clip",
        body: "skills.clip_body",
        summary: "skills.clip_summary",
        categories: &[Category::Media],
        author: Author::Official,
        uses: "5.4k",
        date: "skills.clip_date",
        tint: 0x0011_1a28,
        mark: "AV",
        prompts: &["skills.clip_p1", "skills.clip_p2", "skills.clip_p3"],
        required_plan: Plan::Ultra,
        details_url: None,
    },
];

fn skill_by_id(id: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|skill| skill.id == id)
}

fn installed_skill_chips(
    installed: &[flowy_skills::InstalledSkill],
) -> Vec<flowy_studio::SkillChip> {
    let mut seen = HashSet::new();
    let mut chips = Vec::with_capacity(installed.len());
    for skill in installed {
        let invocation_id =
            format!("{}/{}", skill.id.owner_handle, skill.id.slug);
        if seen.insert(invocation_id.clone()) {
            let trimmed_name = skill.name.trim();
            let title = if trimmed_name.is_empty() {
                skill.id.slug.clone()
            } else {
                trimmed_name.to_owned()
            };
            chips.push(flowy_studio::SkillChip {
                id: invocation_id,
                title,
                summary: skill.description.trim().to_owned(),
                official: matches!(
                    skill.origin,
                    flowy_skills::InstallOrigin::Official
                ),
            });
        }
    }
    let mut title_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for chip in &chips {
        *title_counts.entry(chip.title.clone()).or_insert(0) += 1;
    }
    for chip in &mut chips {
        if title_counts.get(&chip.title).copied().unwrap_or(0) > 1 {
            chip.title = format!("{} ({})", chip.title, chip.id);
        }
    }
    chips
}

impl FlowyApp {
    /// Publishes only locally installed skills (`self.skills.installed`) into
    /// `StudioPanel` using real `/skill:<owner>/<slug>` invocation identities.
    pub(super) fn sync_studio_skills(&self, cx: &mut Context<'_, Self>) {
        let chips = installed_skill_chips(&self.skills.installed);
        self.studio
            .update(cx, |panel, cx| panel.set_installed_skills(chips, cx));
    }

    pub(super) fn skills_canvas(
        &self,
        window: &mut Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        self.arm_skills_follow(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.skills_header(window, cx))
            .child(
                div()
                    .id("skills-scroll")
                    .track_scroll(&self.skills.scroll)
                    .on_scroll(cx.listener(|app, _event, window, cx| {
                        // The built-in scroller updates the offset before this
                        // listener, so the bottom can be checked in this event.
                        if app.skills_follow_ready(cx) {
                            app.continue_skills(cx);
                        } else {
                            app.arm_skills_follow(window, cx);
                        }
                    }))
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div().w_full().min_h_full().min_w_0().child(
                            theme::PageColumn::new()
                                .child(self.skills_filters(cx))
                                .child(self.skills_grid(cx))
                                .render(window),
                        ),
                    ),
            )
    }

    fn skills_header(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::PageHeader::new(self.t("more.skills"))
            .trailing(
                theme::ToolbarSearch::new(
                    "skills-search",
                    self.t("skills.search"),
                    self.skills.query.clone(),
                )
                .render(&self.theme),
            )
            .trailing(self.skills_mine_button(cx))
            .trailing(self.skills_create_button(cx))
            .render(&self.theme, window)
    }

    fn skills_mine_button(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::Button::new("skills-mine", self.t("skills.mine"))
            .framed()
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.open_skill("mine", cx);
                }),
            )
    }

    fn skills_create_button(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        theme::Button::new("skills-create", self.t("skills.create"))
            .framed()
            .trailing_icon(theme::icons::chevron_down().size(px(14.)))
            .render(
                &self.theme,
                cx.listener(|app, _event, _window, cx| {
                    cx.stop_propagation();
                    app.open_skill("create", cx);
                }),
            )
    }

    pub(super) fn touch_skills_query(&mut self, cx: &mut Context<'_, Self>) {
        cx.notify();
        if cfg!(test) {
            return;
        }
        if self.section != super::Section::More
            || self.persona.page != super::more::Page::Skills
        {
            return;
        }
        self.skills.search_task = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let _ = weak.update(cx, |app, cx| {
                if app.section == super::Section::More
                    && app.persona.page == super::more::Page::Skills
                {
                    app.refresh_skills(cx);
                }
            });
        }));
    }

    pub(super) fn refresh_skills(&mut self, cx: &mut Context<'_, Self>) {
        if cfg!(test) {
            return;
        }
        let query = self.skills.query.read(cx).value().trim().to_owned();
        let category = self.skills.category.clone();
        let root = self.store.root().to_path_buf();
        self.skills.cards.clear();
        self.skills.cursor = None;
        self.skills.exhausted = false;
        self.skills.auto_pages = 0;
        self.skills.empty_follows = 0;
        self.skills.follow_armed.set(false);
        self.skills.scroll.set_offset(gpui::point(px(0.), px(0.)));
        tracing::debug!(
            target: "flowy.skills",
            query = %query,
            category = category.as_deref().unwrap_or(""),
            "refresh skill shelf"
        );
        self.watch_skill_shelf(cx, false, false, move |notes| {
            publish_skill_shelf(&root, &query, category.as_deref(), &notes);
        });
    }

    fn continue_skills(&mut self, cx: &mut Context<'_, Self>) {
        if cfg!(test) || self.skills.loading || !self.skills_can_resume(cx) {
            return;
        }
        let query = self.skills.query.read(cx).value().trim().to_owned();
        let category = self.skills.category.clone();
        let root = self.store.root().to_path_buf();
        let filling_short_viewport = self.skills_viewport_is_short();
        self.skills.follow_mark = self.skills.cards.len();
        if let Some(cursor) = self.skills.cursor.clone() {
            self.watch_skill_shelf(
                cx,
                true,
                filling_short_viewport,
                move |notes| {
                    let settled = load_more_skill_shelf(
                        &root,
                        &query,
                        category.as_deref(),
                        &cursor,
                    );
                    push_note(&notes, ShelfDelivery::Done(settled));
                },
            );
            return;
        }
        // The visible shelf has no next-page marker, so ask for the first
        // page again without discarding the cards already on screen.
        self.watch_skill_shelf(
            cx,
            true,
            filling_short_viewport,
            move |notes| {
                publish_skill_shelf(&root, &query, category.as_deref(), &notes);
            },
        );
    }

    fn watch_skill_shelf(
        &mut self,
        cx: &mut Context<'_, Self>,
        is_follow: bool,
        filling_short_viewport: bool,
        work: impl FnOnce(Arc<Mutex<VecDeque<ShelfDelivery>>>) + Send + 'static,
    ) {
        self.skills.generation = self.skills.generation.wrapping_add(1);
        let generation = self.skills.generation;
        self.skills.loading = true;
        self.skills.unavailable = false;
        let notes = Arc::new(Mutex::new(VecDeque::new()));
        let writer = Arc::clone(&notes);
        std::thread::spawn(move || {
            let panicked =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    work(Arc::clone(&writer));
                }))
                .is_err();
            if panicked {
                push_note(
                    &writer,
                    ShelfDelivery::Done(Err(
                        flowy_skills::MarketError::Unavailable,
                    )),
                );
            }
        });
        self.skills.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let batch = notes
                    .lock()
                    .ok()
                    .map(|mut guard| guard.drain(..).collect::<Vec<_>>());
                let Some(batch) = batch else {
                    continue;
                };
                if batch.is_empty() {
                    continue;
                }
                let mut finished = false;
                let _ = weak.update(cx, |app, cx| {
                    if generation != app.skills.generation {
                        finished = true;
                        return;
                    }
                    for note in batch {
                        match note {
                            ShelfDelivery::Preview(shelf) => {
                                app.apply_skill_shelf(shelf, is_follow);
                            }
                            ShelfDelivery::Done(result) => {
                                app.finish_skill_shelf(
                                    result,
                                    is_follow,
                                    filling_short_viewport,
                                );
                                finished = true;
                            }
                        }
                    }
                    cx.notify();
                });
                if finished {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn finish_skill_shelf(
        &mut self,
        result: Result<SkillShelf, flowy_skills::MarketError>,
        is_follow: bool,
        filling_short_viewport: bool,
    ) {
        self.skills.loading = false;
        match result {
            Ok(shelf) => {
                let applied = self.apply_skill_shelf(shelf, is_follow);
                if !is_follow {
                    return;
                }
                let grew = applied > self.skills.follow_mark;
                if filling_short_viewport {
                    self.skills.auto_pages =
                        self.skills.auto_pages.saturating_add(1);
                }
                if grew {
                    self.skills.empty_follows = 0;
                } else {
                    self.note_empty_skill_follow();
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "flowy.skills",
                    error = %error,
                    "skill shelf refresh failed"
                );
                if self.skills.cards.is_empty() {
                    self.skills.cursor = None;
                    self.skills.exhausted = true;
                    self.skills.unavailable = true;
                } else if is_follow {
                    self.note_empty_skill_follow();
                }
            }
        }
    }

    fn apply_skill_shelf(
        &mut self,
        shelf: SkillShelf,
        merge_existing: bool,
    ) -> usize {
        self.skills.cards = if merge_existing {
            merge_shelf(shelf.cards, std::mem::take(&mut self.skills.cards))
        } else {
            shelf.cards
        };
        self.skills.cursor = shelf.next_cursor;
        self.skills.exhausted = shelf.complete;
        self.skills.unavailable = false;
        self.skills.cards.len()
    }

    const fn note_empty_skill_follow(&mut self) {
        self.skills.empty_follows = self.skills.empty_follows.saturating_add(1);
    }

    /// A shelf can still grow until it is complete or repeated follows make no
    /// progress. The latter also covers an old cache without a cursor marker.
    fn skills_can_resume(&self, cx: &Context<'_, Self>) -> bool {
        if self.skills.cards.is_empty()
            || self.skills.cards.len() >= self.skills_card_cap(cx)
            || self.skills.exhausted
        {
            return false;
        }
        self.skills.empty_follows < MAX_EMPTY_FOLLOWS
    }

    fn skills_card_cap(&self, cx: &Context<'_, Self>) -> usize {
        let query = self.skills.query.read(cx).value();
        match shelf_kind(query.as_ref(), self.skills.category.as_deref()) {
            ShelfKind::Trending => flowy_skills::TRENDING_CARD_CAP,
            ShelfKind::Category(_) => flowy_skills::CATEGORY_CARD_CAP,
            ShelfKind::Search => flowy_skills::SEARCH_CARD_CAP,
        }
    }

    fn skills_viewport_is_short(&self) -> bool {
        self.skills.scroll.bounds().size.height.as_f32() > 1.
            && self.skills.scroll.max_offset().y.as_f32() <= 1.
    }

    fn skills_follow_ready(&self, cx: &Context<'_, Self>) -> bool {
        follow_ready(&FollowProbe {
            cards: self.skills.cards.len(),
            cap: self.skills_card_cap(cx),
            viewport_height: self.skills.scroll.bounds().size.height.as_f32(),
            max_offset: self.skills.scroll.max_offset().y.as_f32(),
            offset_y: self.skills.scroll.offset().y.as_f32(),
            auto_pages: self.skills.auto_pages,
        })
    }

    fn arm_skills_follow(&self, window: &mut Window, cx: &Context<'_, Self>) {
        self.arm_skills_follow_after(window, cx, MAX_LAYOUT_POLLS);
    }

    /// One check after the next layout. Repeats only until the scroll view has
    /// bounds, so an idle skills page does not schedule a frame forever.
    fn arm_skills_follow_after(
        &self,
        window: &mut Window,
        cx: &Context<'_, Self>,
        polls_left: u8,
    ) {
        if polls_left == 0
            || cfg!(test)
            || self.skills.follow_armed.get()
            || self.skills.loading
            || !self.skills_can_resume(cx)
            || self.section != super::Section::More
            || self.persona.page != super::more::Page::Skills
        {
            return;
        }
        self.skills.follow_armed.set(true);
        cx.on_next_frame(window, move |app, window, cx| {
            app.skills.follow_armed.set(false);
            if app.section != super::Section::More
                || app.persona.page != super::more::Page::Skills
                || app.skills.loading
                || !app.skills_can_resume(cx)
            {
                return;
            }
            // No marker means the page on screen cannot ask for the next one.
            // Recover it immediately, instead of waiting for a scroll that
            // would otherwise do nothing.
            if app.skills.cursor.is_none() || app.skills_follow_ready(cx) {
                app.continue_skills(cx);
                return;
            }
            // Scrolling updates the offset and redraws. Keep checking for a
            // few frames so the bottom is noticed after the wheel event, not
            // only when the scroll view still has no bounds.
            if polls_left > 1 {
                app.arm_skills_follow_after(window, cx, polls_left - 1);
            }
        });
    }

    fn skills_filters(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row =
            theme::FilterTabs::new("skills-tabs", self.t("library.categories"));
        let all_selected = self.skills.category.is_none();
        row = row.child(
            theme::filter_chip(
                "skill-filter-all",
                self.t("skills.cat_all"),
                all_selected,
                &self.theme,
            )
            .on_click(cx.listener(|app, _event, _window, cx| {
                cx.stop_propagation();
                app.skills.category = None;
                app.refresh_skills(cx);
            })),
        );
        for (slug, key) in MARKET_CATEGORIES {
            let slug = (*slug).to_owned();
            let selected =
                self.skills.category.as_deref() == Some(slug.as_str());
            let id = SharedString::from(format!("skill-filter-{slug}"));
            let label = self.t(key);
            row = row.child(
                theme::filter_chip(id, label, selected, &self.theme).on_click(
                    cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.skills.category = Some(slug.clone());
                        app.refresh_skills(cx);
                    }),
                ),
            );
        }
        row.render()
    }

    fn skills_grid(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut column = div().flex().flex_col().gap_4();
        if self.skills.loading && self.skills.cards.is_empty() {
            column = column.child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("ui.loading")),
            );
        }
        if self.skills.cards.is_empty() && !self.skills.loading {
            let message = if self.skills.unavailable {
                self.t("skills.unavailable")
            } else {
                self.t("skills.empty")
            };
            column = column.child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(message),
            );
            return column;
        }
        for chunk in self.skills.cards.chunks(3) {
            let mut row = div().flex().gap_4().items_stretch();
            for skill in chunk {
                row = row.child(self.shelf_card(skill, cx));
            }
            for _ in chunk.len()..3 {
                row = row.child(div().flex_1().min_w_0());
            }
            column = column.child(row);
        }
        if self.skills.loading && !self.skills.cards.is_empty() {
            column = column.child(
                div()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("ui.loading")),
            );
        }
        column
    }

    fn shelf_card(
        &self,
        skill: &ShelfCard,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let card = skill.clone();
        let element_id = SharedString::from(format!(
            "skill-card-{}-{}",
            skill.owner, skill.slug
        ));
        let title = skill.title.clone();
        let summary = skill.summary.replace(['\n', '\r'], " ");
        theme::interactive(element_id, title.clone(), &self.theme)
            .flex_1()
            .min_w_0()
            .h(self.shelf_card_height())
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .overflow_hidden()
            .hover(|card| card.border_color(self.theme.muted()))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.open_shelf_card(card.clone(), cx);
            }))
            .child(
                div()
                    .h_full()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .text_size(self.theme.body_size())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex_1()
                            .min_h_0()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .line_clamp(SHELF_SUMMARY_LINES)
                            .text_ellipsis()
                            .child(summary),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .max_h(self.shelf_tag_row_height())
                            .overflow_hidden()
                            .child(self.shelf_tags(skill)),
                    ),
            )
    }

    /// Title, three summary lines, and one tag row, including the card padding.
    fn shelf_card_height(&self) -> Pixels {
        let rem = px(f32::from(self.prefs.font_size) / 0.875);
        let line = std::f32::consts::GOLDEN_RATIO;
        let title = self.theme.body_size() * line;
        let caption = self.theme.caption_size() * line;
        let gap = rem * 0.5;
        rem * 2.
            + title
            + gap
            + caption * (SHELF_SUMMARY_LINES as f32)
            + gap
            + self.shelf_tag_row_height()
    }

    fn shelf_tag_row_height(&self) -> Pixels {
        self.theme.caption_size() * std::f32::consts::GOLDEN_RATIO + px(4.)
    }

    fn shelf_tags(&self, skill: &ShelfCard) -> impl IntoElement {
        let mut row = div().flex().gap_1().flex_wrap().items_center();
        let author = if skill.official {
            self.t("skills.author_official")
        } else {
            skill.owner.clone()
        };
        row = row.child(self.skill_text_tag(author));
        if skill.version != flowy_skills::UNVERSIONED {
            row = row.child(self.skill_text_tag(skill.version.clone()));
        }
        for category in &skill.categories {
            row = row.child(
                self.skill_text_tag(self.market_category_label(category)),
            );
        }
        row
    }

    fn market_category_label(&self, slug: &str) -> String {
        MARKET_CATEGORIES
            .iter()
            .find(|(id, _)| *id == slug)
            .map_or_else(|| slug.to_owned(), |(_, key)| self.t(key))
    }

    pub(super) fn open_shelf_card(
        &mut self,
        card: ShelfCard,
        cx: &mut Context<'_, Self>,
    ) {
        self.skills.opened = Some(card);
        self.activate_dialog(super::state::ActiveDialog::RemoteSkill, cx);
    }

    pub(super) fn skill_shelf_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let Some(skill) = self.skills.opened.clone() else {
            return div().into_any_element();
        };
        let frame = self.theme.modal_size(window.viewport_size());
        let has_body = !skill.summary.trim().is_empty();
        theme::modal_layer(
            "skill-shelf-layer",
            &self.theme,
            &self.ui,
            theme::dialog(
                "skill-shelf-dialog",
                skill.title.clone(),
                &self.theme,
                &self.ui,
                cx,
            )
            .w(px(560.))
            .max_w(frame.width)
            .max_h(frame.height)
            .rounded(self.theme.modal_radius())
            .bg(self.theme.card())
            .shadow_md()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(self.shelf_dialog_header(&skill, cx))
            .when(has_body, |dialog| {
                dialog.child(self.shelf_dialog_body(&skill))
            }),
        )
        .into_any_element()
    }

    fn shelf_dialog_header(
        &self,
        skill: &ShelfCard,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .px_5()
            .pt_4()
            .pb_4()
            .flex()
            .flex_col()
            .gap_4()
            .flex_shrink_0()
            .child(self.shelf_dialog_tools(skill, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(self.theme.heading_size())
                            .line_height(self.theme.heading_size() * 1.25)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .whitespace_normal()
                            .child(skill.title.clone()),
                    )
                    .child(self.shelf_dialog_meta(skill)),
            )
            .child(self.shelf_dialog_actions(skill, cx))
    }

    fn shelf_dialog_actions(
        &self,
        skill: &ShelfCard,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let static_id = shelf_static_id(skill);
        let saved_key = shelf_saved_key(skill);
        let installed =
            self.installed_skill_for(&skill.owner, &skill.slug).cloned();
        let needs_update = installed.as_ref().is_some_and(|installed| {
            installed
                .latest_version
                .as_ref()
                .is_some_and(|latest| latest != &installed.id.version)
        });
        let install_label = self
            .skills
            .install
            .as_ref()
            .filter(|install| install.key == saved_key)
            .map_or_else(
                || {
                    if needs_update {
                        self.t("skills.update")
                    } else if installed.is_some() {
                        self.t("skills.try_out")
                    } else {
                        self.t("skills.install")
                    }
                },
                |install| self.install_phase_label(&install.phase),
            );
        let slug = skill.slug.clone();
        let version = skill.version.clone();
        let target_version = installed
            .as_ref()
            .and_then(|installed| installed.latest_version.clone())
            .unwrap_or_else(|| version.clone());
        let page = shelf_page_url(skill);
        let owner = skill.owner.clone();
        let install_key = saved_key.clone();
        let mut primary = theme::Button::new("skill-shelf-add", install_label)
            .variant(theme::ControlVariant::Primary);
        if installed.is_none() || needs_update {
            primary = primary.icon(theme::icons::plus());
        }
        let mut actions =
            div().flex().flex_wrap().items_center().gap_2().child(
                primary.render(
                    &self.theme,
                    cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        if needs_update {
                            app.install_shelf_skill(
                                &owner,
                                &slug,
                                &target_version,
                                &install_key,
                                cx,
                            );
                        } else if let Some(skill) =
                            app.installed_skill_for(&owner, &slug).cloned()
                        {
                            app.try_installed_skill(&skill, cx);
                        } else {
                            app.install_shelf_skill(
                                &owner,
                                &slug,
                                &version,
                                &install_key,
                                cx,
                            );
                        }
                    }),
                ),
            );
        let save_label = if self.shelf_is_saved(&saved_key) {
            self.t("skills.saved")
        } else {
            self.t("skills.save")
        };
        let save_key = saved_key;
        actions = actions.child(
            theme::Button::new("skill-shelf-save", save_label)
                .icon(theme::icons::book())
                .render(
                    &self.theme,
                    cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.toggle_saved_skill(save_key.clone(), cx);
                    }),
                ),
        );
        if static_id.is_some() || page.is_some() {
            actions = actions.child(
                theme::Button::new(
                    "skill-shelf-details",
                    self.t("skills.details"),
                )
                .icon(theme::icons::book())
                .render(
                    &self.theme,
                    cx.listener(move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        if let Some(id) = static_id {
                            app.open_skill_details(id, cx);
                        } else if let Some(page) = page.clone() {
                            cx.open_url(&page);
                        }
                    }),
                ),
            );
        }
        actions
    }

    fn shelf_dialog_tools(
        &self,
        skill: &ShelfCard,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut tools = div().flex().items_center().gap_1();
        if let Some(page) = shelf_page_url(skill) {
            tools = tools.child(
                theme::IconButton::new(
                    "skill-shelf-share",
                    self.t("skills.share"),
                    theme::icons::arrow_up_right(),
                )
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(move |_app, _event, _window, cx| {
                        cx.stop_propagation();
                        cx.open_url(&page);
                    }),
                ),
            );
        }
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap_3()
            .child(self.shelf_header_chips(skill))
            .child(tools.flex_shrink_0().child(self.skill_icon_hit(
                "skill-shelf-close",
                self.t("ui.close"),
                theme::icons::x(),
                cx,
            )))
    }

    fn shelf_header_chips(&self, skill: &ShelfCard) -> impl IntoElement {
        let mut row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .flex_wrap()
            .items_center();
        for category in &skill.categories {
            row = row.child(
                div()
                    .px_2()
                    .py(px(3.))
                    .rounded_md()
                    .bg(self.theme.soft())
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.ink())
                    .child(self.market_category_label(category)),
            );
        }
        row
    }

    fn shelf_dialog_meta(&self, skill: &ShelfCard) -> impl IntoElement {
        let author = if skill.official {
            self.t("skills.author_official")
        } else {
            skill.owner.clone()
        };
        let mut facts = vec![author];
        if skill.version != flowy_skills::UNVERSIONED {
            facts.push(skill.version.clone());
        }
        if let Some(date) = skill.updated_at_ms.and_then(format_unix_date) {
            facts.push(self.tf("skills.updated", &[("date", &date)]));
        }
        if let Some(label) = self.shelf_count_label(skill) {
            facts.push(label);
        }
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .text_size(self.theme.caption_size())
            .text_color(self.theme.muted())
            .child(
                theme::icons::user()
                    .size(px(12.))
                    .text_color(self.theme.toolbar_icon()),
            );
        for (index, fact) in facts.into_iter().enumerate() {
            if index > 0 {
                row = row.child(
                    div().text_color(self.theme.toolbar_icon()).child("·"),
                );
            }
            row = row.child(fact);
        }
        row
    }

    fn shelf_count_label(&self, skill: &ShelfCard) -> Option<String> {
        if let Some(count) = skill.downloads {
            let count = format_count(count);
            return Some(self.tf("skills.downloads", &[("count", &count)]));
        }
        let count = format_count(skill.installs?);
        Some(self.tf("skills.installs", &[("count", &count)]))
    }

    fn shelf_dialog_body(&self, skill: &ShelfCard) -> impl IntoElement {
        div()
            .id("skill-shelf-body")
            .min_h_0()
            .max_h(px(240.))
            .overflow_y_scroll()
            .px_5()
            .pt_4()
            .pb_5()
            .border_t_1()
            .border_color(self.theme.line())
            .text_size(self.theme.body_size())
            .line_height(self.theme.body_size() * 1.55)
            .text_color(self.theme.ink())
            .whitespace_normal()
            .child(skill.summary.clone())
    }

    fn skill_tag(&self, key: &'static str) -> impl IntoElement {
        self.skill_text_tag(self.t(key))
    }

    fn skill_text_tag(
        &self,
        label: impl Into<SharedString>,
    ) -> impl IntoElement {
        div()
            .px_2()
            .py(px(2.))
            .rounded_md()
            .bg(self.theme.soft())
            .text_size(self.theme.caption_size())
            .text_color(self.theme.muted())
            .child(label.into())
    }

    pub(super) fn open_skill(
        &mut self,
        id: &'static str,
        cx: &mut Context<'_, Self>,
    ) {
        self.activate_dialog(super::state::ActiveDialog::Skill(id), cx);
    }

    pub(super) fn skill_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let id = if matches!(
            self.active_dialog,
            super::state::ActiveDialog::SkillScriptConfirm
        ) {
            "mine"
        } else {
            self.active_dialog.skill().unwrap_or("")
        };
        let content = if id == "mine" {
            self.mine_skills_dialog(frame, cx).into_any_element()
        } else if let Some(skill) = skill_by_id(id) {
            self.skill_dialog(skill, frame, cx).into_any_element()
        } else {
            self.skill_stub_dialog(id, cx).into_any_element()
        };
        theme::modal_layer("skill-layer", &self.theme, &self.ui, content)
    }

    fn skill_dialog(
        &self,
        skill: &Skill,
        frame: Size<Pixels>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::dialog(
            "skill-dialog",
            self.t(skill.title),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(560.))
        .max_w(frame.width)
        .max_h(frame.height)
        .rounded(self.theme.modal_radius())
        .bg(self.theme.card())
        .shadow_md()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(self.skill_dialog_header(skill, cx))
        .child(self.skill_dialog_body(skill, cx))
    }

    fn mine_skills_dialog(
        &self,
        frame: Size<Pixels>,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut body = div().flex().flex_col().gap_2().px_5().pb_5();
        if self.skills.installed.is_empty() {
            body = body.child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .child(self.t("skills.no_installed")),
            );
        }
        for skill in self.skills.installed.iter().cloned() {
            let title = if skill.description.is_empty() {
                skill.name.clone()
            } else {
                format!("{} · {}", skill.name, skill.description)
            };
            let auto_label = if skill.auto_invoke {
                self.t("skills.auto_on")
            } else {
                self.t("skills.auto_off")
            };
            let scripts_label = if skill.scripts_enabled {
                self.t("skills.scripts_on")
            } else {
                self.t("skills.scripts_off")
            };
            let origin_label =
                if skill.origin == flowy_skills::InstallOrigin::Official {
                    self.t("skills.built_in")
                } else {
                    self.t("skills.community")
                };
            let update = skill
                .latest_version
                .as_ref()
                .is_some_and(|latest| latest != &skill.id.version);
            let owner = skill.id.owner_handle.clone();
            let slug = skill.id.slug.clone();
            let latest = skill.latest_version.clone().unwrap_or_default();
            let key = format!("{owner}/{slug}");
            let auto_id = skill.id.clone();
            let auto_invoke = skill.auto_invoke;
            let scripts_id = skill.id.clone();
            let scripts_enabled = skill.scripts_enabled;
            let uninstall_id = skill.id.clone();
            let row = div()
                .w_full()
                .rounded_lg()
                .border_1()
                .border_color(self.theme.line())
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_size(self.theme.body_size())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .truncate()
                                .child(title),
                        )
                        .child(self.skill_text_tag(format!(
                            "{origin_label} · {}",
                            skill.id.version
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            theme::Button::new("skill-auto-toggle", auto_label)
                                .render(
                                    &self.theme,
                                    cx.listener(move |app, _event, _window, cx| {
                                        cx.stop_propagation();
                                        app.set_auto_invoke(
                                            auto_id.clone(),
                                            !auto_invoke,
                                            cx,
                                        );
                                    }),
                                ),
                        )
                        .child(
                            theme::Button::new("skill-scripts-toggle", scripts_label)
                                .render(
                                    &self.theme,
                                    cx.listener(move |app, _event, _window, cx| {
                                        cx.stop_propagation();
                                        if scripts_enabled {
                                            app.set_scripts_enabled(
                                                scripts_id.clone(),
                                                false,
                                                cx,
                                            );
                                        } else {
                                            app.skills.script_confirm =
                                                Some(scripts_id.clone());
                                            app.activate_dialog(
                                                super::state::ActiveDialog::SkillScriptConfirm,
                                                cx,
                                            );
                                        }
                                    }),
                                ),
                        )
                        .when(update, |row| {
                            row.child(
                                theme::Button::new("skill-update", self.t("skills.update"))
                                    .render(
                                        &self.theme,
                                        cx.listener(move |app, _event, _window, cx| {
                                            cx.stop_propagation();
                                            app.install_shelf_skill(
                                                &owner,
                                                &slug,
                                                &latest,
                                                &key,
                                                cx,
                                            );
                                        }),
                                    ),
                            )
                        })
                        .when(
                            skill.origin == flowy_skills::InstallOrigin::Community,
                            |row| {
                                row.child(
                                    theme::Button::new(
                                        "skill-uninstall",
                                        self.t("skills.uninstall"),
                                    )
                                    .render(
                                        &self.theme,
                                        cx.listener(move |app, _event, _window, cx| {
                                            cx.stop_propagation();
                                            app.uninstall_skill(uninstall_id.clone(), cx);
                                        }),
                                    ),
                                )
                            },
                        ),
                );
            body = body.child(row);
        }
        if !self.prefs.skills.is_empty() {
            body = body.child(
                div()
                    .pt_3()
                    .text_size(self.theme.caption_size())
                    .text_color(self.theme.muted())
                    .child(self.t("skills.saved")),
            );
            for saved in self.prefs.skills.iter().cloned() {
                let remove = saved.clone();
                let install = saved.clone();
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .text_size(self.theme.caption_size())
                        .text_color(self.theme.ink())
                        .child(saved)
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    theme::Button::new(
                                        "skill-install-saved",
                                        self.t("skills.install"),
                                    )
                                    .render(
                                        &self.theme,
                                        cx.listener(
                                            move |app, _event, _window, cx| {
                                                cx.stop_propagation();
                                                if let Some((owner, slug)) =
                                                    install.split_once('/')
                                                {
                                                    app.install_shelf_skill(
                                                    owner,
                                                    slug,
                                                    flowy_skills::UNVERSIONED,
                                                    &install,
                                                    cx,
                                                );
                                                }
                                            },
                                        ),
                                    ),
                                )
                                .child(
                                    theme::Button::new(
                                        "skill-remove-saved",
                                        self.t("skills.remove_saved"),
                                    )
                                    .render(
                                        &self.theme,
                                        cx.listener(
                                            move |app, _event, _window, cx| {
                                                cx.stop_propagation();
                                                app.toggle_saved_skill(
                                                    remove.clone(),
                                                    cx,
                                                );
                                            },
                                        ),
                                    ),
                                ),
                        ),
                );
            }
        }
        theme::dialog(
            "skill-mine-dialog",
            self.t("skills.mine"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(640.))
        .max_w(frame.width)
        .max_h(frame.height)
        .rounded(self.theme.modal_radius())
        .bg(self.theme.card())
        .shadow_md()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .px_5()
                .pt_4()
                .pb_3()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(self.theme.heading_size())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(self.t("skills.mine")),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            theme::Button::new(
                                "skills-check-updates",
                                self.t("skills.check_updates"),
                            )
                            .render(
                                &self.theme,
                                cx.listener(|app, _event, _window, cx| {
                                    cx.stop_propagation();
                                    app.check_skill_updates(cx);
                                }),
                            ),
                        )
                        .child(self.skill_icon_hit(
                            "skill-stub-close",
                            self.t("ui.close"),
                            theme::icons::x(),
                            cx,
                        )),
                ),
        )
        .child(
            div()
                .id("skill-mine-body")
                .min_h_0()
                .overflow_y_scroll()
                .border_t_1()
                .border_color(self.theme.line())
                .pt_4()
                .child(body),
        )
    }

    pub(super) fn skill_script_confirm_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        if self.active_dialog != super::state::ActiveDialog::SkillScriptConfirm
        {
            return div().into_any_element();
        }
        let Some(id) = self.skills.script_confirm.as_ref() else {
            return div().into_any_element();
        };
        let Some(skill) = self
            .skills
            .installed
            .iter()
            .find(|skill| &skill.id == id)
            .cloned()
        else {
            return div().into_any_element();
        };
        let confirm_id = id.clone();
        let frame = self.theme.modal_size(window.viewport_size());
        theme::modal_layer(
            "skill-script-confirm-layer",
            &self.theme,
            &self.ui,
            theme::dialog(
                "skill-script-confirm",
                self.t("skills.scripts_confirm_title"),
                &self.theme,
                &self.ui,
                cx,
            )
            .w(px(420.))
            .max_w(frame.width)
            .bg(self.theme.card())
            .shadow_md()
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .on_click(cx.listener(|_app, _event, _window, cx| {
                cx.stop_propagation();
            }))
            .child(
                div()
                    .text_size(self.theme.heading_size())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.t("skills.scripts_confirm_title")),
            )
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.tf(
                        "skills.scripts_confirm_body",
                        &[("name", &skill.name)],
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        theme::Button::new(
                            "skill-script-cancel",
                            self.t("more.cancel"),
                        )
                        .variant(theme::ControlVariant::Secondary)
                        .full_width()
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.skills.script_confirm = None;
                                app.active_dialog =
                                    super::state::ActiveDialog::Skill("mine");
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        theme::Button::new(
                            "skill-script-confirm",
                            self.t("skills.enable"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .full_width()
                        .render(
                            &self.theme,
                            cx.listener(move |app, _event, _window, cx| {
                                app.set_scripts_enabled(
                                    confirm_id.clone(),
                                    true,
                                    cx,
                                );
                                app.skills.script_confirm = None;
                                app.active_dialog =
                                    super::state::ActiveDialog::Skill("mine");
                                cx.notify();
                            }),
                        ),
                    ),
            ),
        )
        .into_any_element()
    }

    fn skill_stub_dialog(
        &self,
        id: &str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = match id {
            "mine" => self.t("skills.mine"),
            "create" => self.t("skills.create"),
            _ => self.t("more.skills"),
        };
        theme::dialog(
            "skill-stub-dialog",
            title.clone(),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(420.))
        .rounded(self.theme.modal_radius())
        .bg(self.theme.card())
        .shadow_md()
        .p_5()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .text_size(self.theme.heading_size())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(self.skill_icon_hit(
                    "skill-stub-close",
                    self.t("ui.close"),
                    theme::icons::x(),
                    cx,
                )),
        )
        .child(
            div()
                .text_size(self.theme.body_size())
                .text_color(self.theme.muted())
                .whitespace_normal()
                .child(self.t("skills.placeholder_body")),
        )
    }

    fn skill_dialog_header(
        &self,
        skill: &Skill,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .px_6()
            .pt_5()
            .pb_5()
            .rounded_t(self.theme.modal_radius())
            .bg(self.theme.soft())
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(self.skill_dialog_cats(skill))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(self.skill_icon_hit(
                                "skill-share",
                                self.t("skills.share"),
                                theme::icons::arrow_up_right(),
                                cx,
                            ))
                            .child(self.skill_icon_hit(
                                "skill-export",
                                self.t("skills.export"),
                                theme::icons::file_plus(),
                                cx,
                            ))
                            .child(self.skill_icon_hit(
                                "skill-close",
                                self.t("ui.close"),
                                theme::icons::x(),
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(self.theme.heading_size())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(self.t(skill.title)),
                            )
                            .child(self.skill_dialog_meta(skill))
                            .child(self.skill_dialog_actions(skill, cx)),
                    )
                    .child(skill_hero(skill, &self.theme)),
            )
    }

    fn skill_dialog_cats(&self, skill: &Skill) -> impl IntoElement {
        let mut row = div().flex().gap_1().flex_wrap();
        for category in skill.categories {
            row = row.child(self.skill_tag(category.key()));
        }
        row
    }

    fn skill_dialog_meta(&self, skill: &Skill) -> impl IntoElement {
        let date = self.t(skill.date);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .text_size(self.theme.caption_size())
            .text_color(self.theme.muted())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        theme::icons::sparkles()
                            .size(px(12.))
                            .text_color(self.theme.muted()),
                    )
                    .child(self.t(skill.author.key())),
            )
            .child(self.tf("skills.updated", &[("date", &date)]))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        theme::icons::user()
                            .size(px(12.))
                            .text_color(self.theme.muted()),
                    )
                    .child(
                        self.tf("skills.used_short", &[("count", skill.uses)]),
                    ),
            )
    }

    fn skill_dialog_actions(
        &self,
        skill: &Skill,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = skill.id;
        let added = self.skill_is_added(id);
        let primary_label = if added {
            self.t("skills.try_out")
        } else {
            self.t("skills.add")
        };
        let mut primary = theme::Button::new("skill-add", primary_label)
            .variant(theme::ControlVariant::Primary);
        if !added {
            primary = primary.icon(theme::icons::plus());
        }
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(primary.render(
                &self.theme,
                cx.listener(move |app, _event, _window, cx| {
                    cx.stop_propagation();
                    if app.skill_is_added(id) {
                        app.try_skill(id, cx);
                    } else {
                        app.add_skill(id, cx);
                    }
                }),
            ))
            .child(
                theme::Button::new("skill-details", self.t("skills.details"))
                    .icon(theme::icons::book())
                    .render(
                        &self.theme,
                        cx.listener(move |app, _event, _window, cx| {
                            cx.stop_propagation();
                            app.open_skill_details(id, cx);
                        }),
                    ),
            )
    }

    fn skill_is_added(&self, id: &str) -> bool {
        self.skills
            .installed
            .iter()
            .any(|skill| skill.id.slug == id || skill.name == id)
    }

    fn shelf_is_saved(&self, key: &str) -> bool {
        self.prefs.skills.iter().any(|skill| skill == key)
    }

    fn installed_skill_for(
        &self,
        owner: &str,
        slug: &str,
    ) -> Option<&flowy_agent::InstalledSkill> {
        self.skills.installed.iter().find(|skill| {
            skill.id.owner_handle == owner && skill.id.slug == slug
        })
    }

    fn toggle_saved_skill(&mut self, key: String, cx: &mut Context<'_, Self>) {
        if key.is_empty() {
            return;
        }
        if let Some(index) =
            self.prefs.skills.iter().position(|skill| skill == &key)
        {
            self.prefs.skills.remove(index);
        } else {
            self.prefs.skills.push(key);
        }
        let _ = self.prefs.save_checked(self.store.root());
        cx.notify();
    }

    fn install_phase_label(&self, phase: &InstallPhase) -> String {
        match phase {
            InstallPhase::Verifying => self.t("skills.verifying"),
            InstallPhase::Downloading => self.t("skills.downloading"),
            InstallPhase::Installing => self.t("skills.installing"),
            InstallPhase::Failed(_) => self.t("skills.install_failed"),
        }
    }

    fn try_installed_skill(
        &mut self,
        skill: &flowy_agent::InstalledSkill,
        cx: &mut Context<'_, Self>,
    ) {
        self.active_dialog = super::state::ActiveDialog::None;
        self.studio.update(cx, flowy_studio::StudioPanel::new_task);
        let duplicates = self
            .skills
            .installed
            .iter()
            .filter(|item| item.name == skill.name)
            .count();
        let command = if duplicates > 1 {
            format!("{}/{}", skill.id.owner_handle, skill.id.slug)
        } else {
            skill.name.clone()
        };
        self.fill_prompt(&format!("/skill:{command} "), cx);
        self.push_section(super::Section::Studio, cx);
    }

    fn add_skill(&mut self, id: &'static str, cx: &mut Context<'_, Self>) {
        let Some(skill) = skill_by_id(id) else {
            return;
        };
        if skill.required_plan.level() > self.prefs.plan.level() {
            self.activate_dialog(
                super::state::ActiveDialog::SkillUnlock(id),
                cx,
            );
            return;
        }
        if let Some(installed) = self
            .skills
            .installed
            .iter()
            .find(|item| item.id.owner_handle == "flowy" && item.id.slug == id)
            .cloned()
        {
            self.try_installed_skill(&installed, cx);
        }
    }

    fn try_skill(&mut self, id: &'static str, cx: &mut Context<'_, Self>) {
        if let Some(installed) = self
            .skills
            .installed
            .iter()
            .find(|item| item.id.owner_handle == "flowy" && item.id.slug == id)
            .cloned()
        {
            self.try_installed_skill(&installed, cx);
        }
    }

    fn open_skill_details(
        &mut self,
        id: &'static str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(skill) = skill_by_id(id) else {
            return;
        };
        if let Some(url) = skill.details_url {
            cx.open_url(url);
            return;
        }
        self.activate_dialog(super::state::ActiveDialog::SkillDocs(id), cx);
    }

    fn close_skill_unlock(&mut self, cx: &mut Context<'_, Self>) {
        let id = self.active_dialog.skill().unwrap_or("");
        if id.is_empty() {
            self.active_dialog = super::state::ActiveDialog::None;
        } else {
            self.active_dialog = super::state::ActiveDialog::Skill(id);
        }
        cx.notify();
    }

    fn close_skill_docs(&mut self, cx: &mut Context<'_, Self>) {
        self.close_skill_unlock(cx);
    }

    pub(super) fn skill_unlock_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        theme::modal_layer(
            "skill-unlock-layer",
            &self.theme,
            &self.ui,
            theme::dialog(
                "skill-unlock-card",
                self.t("skills.unlock_title"),
                &self.theme,
                &self.ui,
                cx,
            )
            .w(px(360.))
            .max_w(frame.width)
            .bg(self.theme.canvas())
            .shadow_md()
            .px_5()
            .py_4()
            .flex()
            .flex_col()
            .gap_4()
            .on_click(
                cx.listener(|_app, _event, _window, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(self.theme.heading_size())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t("skills.unlock_title")),
                    )
                    .child(
                        theme::IconButton::new(
                            "close-skill-unlock",
                            self.t("ui.close"),
                            theme::icons::x(),
                        )
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_skill_unlock(cx);
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(self.t("skills.unlock_body")),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_3()
                    .child(
                        theme::Button::new(
                            "cancel-skill-unlock",
                            self.t("more.cancel"),
                        )
                        .variant(theme::ControlVariant::Secondary)
                        .full_width()
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_skill_unlock(cx);
                            }),
                        ),
                    )
                    .child(
                        theme::Button::new(
                            "upgrade-skill-unlock",
                            self.t("model.upgrade"),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .full_width()
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_skill_unlock(cx);
                                app.note("notice.upgrade_later", cx);
                            }),
                        ),
                    ),
            ),
        )
    }

    pub(super) fn skill_docs_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let id = self.active_dialog.skill().unwrap_or("");
        let skill = skill_by_id(id);
        let title = skill.map_or_else(
            || self.t("skills.details"),
            |skill| self.t(skill.title),
        );
        let body = skill.map_or_else(String::new, |skill| {
            let command = skill.command;
            let detail = self.t(skill.body);
            let hint = self.tf("skills.details_body", &[("command", command)]);
            format!("{detail}\n\n{hint}")
        });
        theme::modal_layer(
            "skill-docs-layer",
            &self.theme,
            &self.ui,
            theme::dialog(
                "skill-docs-card",
                title.clone(),
                &self.theme,
                &self.ui,
                cx,
            )
            .w(px(420.))
            .max_w(frame.width)
            .max_h(frame.height)
            .bg(self.theme.canvas())
            .shadow_md()
            .px_5()
            .py_4()
            .flex()
            .flex_col()
            .gap_4()
            .on_click(
                cx.listener(|_app, _event, _window, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_size(self.theme.heading_size())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        theme::IconButton::new(
                            "close-skill-docs",
                            self.t("ui.close"),
                            theme::icons::x(),
                        )
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|app, _event, _window, cx| {
                                app.close_skill_docs(cx);
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.muted())
                    .whitespace_normal()
                    .child(body),
            ),
        )
    }

    fn skill_dialog_body(
        &self,
        skill: &Skill,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let mut body = div()
            .id("skill-dialog-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .pt_5()
            .pb_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_size(self.theme.body_size())
                    .text_color(self.theme.ink())
                    .whitespace_normal()
                    .child(self.t(skill.summary)),
            );
        for (index, prompt) in skill.prompts.iter().enumerate() {
            body = body.child(self.skill_prompt_row(index, prompt, cx));
        }
        body
    }

    fn skill_prompt_row(
        &self,
        index: usize,
        prompt: &'static str,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = SharedString::from(format!("skill-prompt-{index}"));
        theme::interactive(id, self.t(prompt), &self.theme)
            .w_full()
            .px_4()
            .py_3()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .hover(|row| row.bg(self.theme.soft()))
            .flex()
            .items_center()
            .gap_3()
            .child(
                theme::icons::square_pen()
                    .size(px(16.))
                    .text_color(self.theme.muted()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(self.theme.body_size())
                    .whitespace_normal()
                    .child(self.t(prompt)),
            )
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.active_dialog = super::state::ActiveDialog::None;
                app.section = super::Section::Studio;
                app.fill_prompt_key(prompt, cx);
            }))
    }

    fn skill_icon_hit(
        &self,
        id: &'static str,
        label: String,
        icon: theme::icons::Icon,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        theme::IconButton::new(id, label, icon)
            .size(theme::ControlSize::Small)
            .render(
                &self.theme,
                cx.listener(move |app, _event, _window, cx| {
                    cx.stop_propagation();
                    if id == "skill-close"
                        || id == "skill-stub-close"
                        || id == "skill-shelf-close"
                    {
                        app.active_dialog = super::state::ActiveDialog::None;
                        cx.notify();
                    }
                }),
            )
    }
}

fn skill_hero(skill: &Skill, theme: &theme::ResolvedTheme) -> impl IntoElement {
    div()
        .size(px(112.))
        .rounded_2xl()
        .bg(rgb(skill.tint))
        .shadow_md()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(skill_mark(skill, theme, 28.))
}

fn skill_mark(
    skill: &Skill,
    theme: &theme::ResolvedTheme,
    size: f32,
) -> impl IntoElement {
    let light = skill.tint == 0x00e8_e4dc || skill.tint == 0x00f2_c94c;
    div()
        .text_size(px(size))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(if light { theme.ink() } else { rgb(0x00ff_ffff) })
        .child(skill.mark.to_string())
}

struct SkillShelf {
    cards: Vec<ShelfCard>,
    next_cursor: Option<String>,
    complete: bool,
}

enum ShelfDelivery {
    Preview(SkillShelf),
    Done(Result<SkillShelf, flowy_skills::MarketError>),
}

struct FollowProbe {
    cards: usize,
    cap: usize,
    viewport_height: f32,
    max_offset: f32,
    offset_y: f32,
    auto_pages: u8,
}

fn follow_ready(probe: &FollowProbe) -> bool {
    if probe.cards == 0 || probe.cards >= probe.cap {
        return false;
    }
    if probe.viewport_height <= 1. {
        return false;
    }
    if probe.max_offset <= 1. {
        return probe.auto_pages < MAX_UNSCROLLABLE_FOLLOWS;
    }
    probe.max_offset + probe.offset_y < FOLLOW_THRESHOLD_PX
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShelfKind<'a> {
    Trending,
    Category(&'a str),
    Search,
}

fn shelf_kind<'a>(query: &str, category: Option<&'a str>) -> ShelfKind<'a> {
    if !query.trim().is_empty() {
        ShelfKind::Search
    } else if let Some(slug) = category.filter(|slug| !slug.trim().is_empty()) {
        ShelfKind::Category(slug)
    } else {
        ShelfKind::Trending
    }
}

fn publish_skill_shelf(
    root: &Path,
    query: &str,
    category: Option<&str>,
    notes: &Arc<Mutex<VecDeque<ShelfDelivery>>>,
) {
    let mut market = match open_market(root) {
        Ok(market) => market,
        Err(error) => {
            push_note(notes, ShelfDelivery::Done(Err(error)));
            return;
        }
    };
    let now = unix_now();
    let kind = shelf_kind(query, category);
    let cached = match cached_window(&market, kind, query, category, now) {
        Ok(cached) => cached,
        Err(error) => {
            push_note(notes, ShelfDelivery::Done(Err(error)));
            return;
        }
    };
    let official = official_cards(&market);
    if let Some(hit) = cached.as_ref().filter(|hit| cache_reusable(kind, hit)) {
        push_note(
            notes,
            ShelfDelivery::Done(Ok(present_shelf(
                kind,
                hit.window.clone(),
                &official,
            ))),
        );
        return;
    }
    if let Some(hit) = cached {
        push_note(
            notes,
            ShelfDelivery::Preview(present_shelf(kind, hit.window, &official)),
        );
    }
    let settled = match refresh_window(&mut market, kind, query, category, now)
    {
        Ok(window) => Ok(present_shelf(kind, window, &official)),
        Err(error)
            if matches!(kind, ShelfKind::Trending) && !official.is_empty() =>
        {
            tracing::warn!(
                target: "flowy.skills",
                error = %error,
                "trending shelf failed; showing official skills"
            );
            Ok(SkillShelf {
                cards: official,
                next_cursor: None,
                complete: true,
            })
        }
        Err(error) => Err(error),
    };
    push_note(notes, ShelfDelivery::Done(settled));
}

fn push_note(notes: &Arc<Mutex<VecDeque<ShelfDelivery>>>, note: ShelfDelivery) {
    if let Ok(mut guard) = notes.lock() {
        guard.push_back(note);
    }
}

fn load_more_skill_shelf(
    root: &Path,
    query: &str,
    category: Option<&str>,
    cursor: &str,
) -> Result<SkillShelf, flowy_skills::MarketError> {
    let mut market = open_market(root)?;
    let now = unix_now();
    let kind = shelf_kind(query, category);
    let window = match kind {
        ShelfKind::Search => {
            market.continue_search(query, category, cursor, now)?
        }
        ShelfKind::Category(slug) => {
            market.continue_category(slug, cursor, now)?
        }
        ShelfKind::Trending => market.continue_trending(cursor, now)?,
    };
    let official = match kind {
        ShelfKind::Trending => official_cards(&market),
        ShelfKind::Category(_) | ShelfKind::Search => Vec::new(),
    };
    Ok(present_shelf(kind, window, &official))
}

fn open_market(
    root: &Path,
) -> Result<
    flowy_skills::SkillMarket<flowy_skills::NetTransport>,
    flowy_skills::MarketError,
> {
    let transport = flowy_skills::NetTransport::new()
        .map_err(flowy_skills::MarketError::Transport)?;
    let store =
        flowy_skills::Store::open(&root.join("skills").join("market.sqlite3"))?;
    Ok(flowy_skills::SkillMarket::new(transport, store))
}

const fn cache_reusable(
    kind: ShelfKind<'_>,
    cache: &flowy_skills::CachedShelf,
) -> bool {
    match kind {
        ShelfKind::Search => cache.fresh,
        ShelfKind::Trending => flowy_skills::trending_cache_reusable(cache),
        ShelfKind::Category(_) => flowy_skills::category_cache_reusable(cache),
    }
}

fn cached_window(
    market: &flowy_skills::SkillMarket<flowy_skills::NetTransport>,
    kind: ShelfKind<'_>,
    query: &str,
    category: Option<&str>,
    now: i64,
) -> Result<Option<flowy_skills::CachedShelf>, flowy_skills::MarketError> {
    match kind {
        ShelfKind::Search => market.cached_search(query, category, now),
        ShelfKind::Category(slug) => market.cached_category(slug, now),
        ShelfKind::Trending => market.cached_trending(now),
    }
}

fn refresh_window(
    market: &mut flowy_skills::SkillMarket<flowy_skills::NetTransport>,
    kind: ShelfKind<'_>,
    query: &str,
    category: Option<&str>,
    now: i64,
) -> Result<flowy_skills::ShelfWindow, flowy_skills::MarketError> {
    match kind {
        ShelfKind::Search => market.search(query, category, now),
        ShelfKind::Category(slug) => market.refresh_category(slug, now),
        ShelfKind::Trending => market.refresh_trending(now),
    }
}

fn official_cards(
    market: &flowy_skills::SkillMarket<flowy_skills::NetTransport>,
) -> Vec<ShelfCard> {
    market.official().map_or_else(
        |_| Vec::new(),
        |skills| skills.into_iter().map(ShelfCard::from_official).collect(),
    )
}

fn present_shelf(
    kind: ShelfKind<'_>,
    window: flowy_skills::ShelfWindow,
    official: &[ShelfCard],
) -> SkillShelf {
    let next_cursor = window.next_cursor;
    let complete = window.complete;
    let remote = window
        .cards
        .into_iter()
        .map(ShelfCard::from_remote)
        .collect();
    let cards = match kind {
        ShelfKind::Trending => merge_shelf(official.to_vec(), remote),
        ShelfKind::Category(_) | ShelfKind::Search => remote,
    };
    SkillShelf {
        cards,
        next_cursor,
        complete,
    }
}

pub(super) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

impl ShelfCard {
    fn from_official(skill: flowy_skills::OfficialSkill) -> Self {
        Self {
            owner: skill.id.owner_handle,
            slug: skill.id.slug,
            version: skill.id.version,
            title: skill.title,
            summary: skill.summary,
            categories: Vec::new(),
            official: true,
            downloads: None,
            installs: None,
            updated_at_ms: None,
        }
    }

    fn from_remote(card: flowy_skills::Card) -> Self {
        Self {
            owner: card.id.owner_handle,
            title: {
                let title = card.title.trim();
                if title.is_empty() {
                    display_slug(&card.id.slug)
                } else {
                    title.to_owned()
                }
            },
            slug: card.id.slug,
            version: card.id.version,
            summary: card.summary,
            categories: card.categories,
            official: false,
            downloads: card.downloads,
            installs: card.installs,
            updated_at_ms: card.updated_at_ms,
        }
    }
}

fn display_slug(slug: &str) -> String {
    let title = slug.replace('-', " ");
    if title.trim().is_empty() {
        slug.to_owned()
    } else {
        title
    }
}

fn shelf_saved_key(skill: &ShelfCard) -> String {
    format!("{}/{}", skill.owner, skill.slug)
}

fn shelf_static_id(skill: &ShelfCard) -> Option<&'static str> {
    if !skill.official {
        return None;
    }
    SKILLS
        .iter()
        .find(|item| item.id == skill.slug)
        .map(|item| item.id)
}

fn shelf_public_url(skill: &ShelfCard) -> Option<String> {
    if skill.owner.is_empty() || skill.slug.is_empty() {
        return None;
    }
    Some(format!(
        "https://clawhub.ai/{}/skills/{}",
        flowy_skills::percent_encode(&skill.owner),
        flowy_skills::percent_encode(&skill.slug),
    ))
}

fn shelf_page_url(skill: &ShelfCard) -> Option<String> {
    if skill.official {
        return None;
    }
    shelf_public_url(skill)
}

fn format_unix_date(ms: i64) -> Option<String> {
    if ms < 0 {
        return None;
    }
    chrono::DateTime::from_timestamp(ms.div_euclid(1000), 0)
        .map(|time| time.format("%Y-%m-%d").to_string())
}

fn format_count(count: u64) -> String {
    const THOUSAND: u64 = 1_000;
    const MILLION: u64 = 1_000_000;
    const BILLION: u64 = 1_000_000_000;
    if count >= BILLION {
        format_scaled(count, BILLION, 'b')
    } else if count >= MILLION {
        format_scaled(count, MILLION, 'm')
    } else if count >= THOUSAND {
        format_scaled(count, THOUSAND, 'k')
    } else {
        count.to_string()
    }
}

fn format_scaled(count: u64, unit: u64, suffix: char) -> String {
    let whole = count / unit;
    let tenth = (count % unit) / (unit / 10);
    if tenth == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{tenth}{suffix}")
    }
}

fn merge_shelf(
    official: Vec<ShelfCard>,
    remote: Vec<ShelfCard>,
) -> Vec<ShelfCard> {
    let mut seen = HashSet::new();
    let mut cards = Vec::new();
    for card in official.into_iter().chain(remote) {
        if seen.insert(format!("{}\u{0}{}", card.owner, card.slug)) {
            cards.push(card);
        }
    }
    cards
}

#[cfg(test)]
mod shelf_tests {
    use super::{
        FollowProbe, ShelfCard, ShelfKind, follow_ready, merge_shelf,
        shelf_kind,
    };

    fn card(owner: &str, slug: &str, official: bool) -> ShelfCard {
        ShelfCard {
            owner: owner.to_owned(),
            slug: slug.to_owned(),
            version: "1.0.0".to_owned(),
            title: slug.to_owned(),
            summary: String::new(),
            categories: Vec::new(),
            official,
            downloads: None,
            installs: None,
            updated_at_ms: None,
        }
    }

    #[test]
    fn trending_keeps_official_ahead_of_remote_duplicates() {
        let merged = merge_shelf(
            vec![card("flowy", "excel", true)],
            vec![card("flowy", "excel", false), card("awspace", "pdf", false)],
        );
        assert_eq!(merged.len(), 2);
        assert!(merged[0].official);
        assert_eq!(merged[0].slug, "excel");
        assert_eq!(merged[1].slug, "pdf");
    }

    #[test]
    fn query_selects_search_even_when_a_category_is_set() {
        assert_eq!(shelf_kind("  ", None), ShelfKind::Trending);
        assert_eq!(shelf_kind("", Some("")), ShelfKind::Trending);
        assert_eq!(shelf_kind("", Some("tools")), ShelfKind::Category("tools"));
        assert_eq!(shelf_kind("pdf", Some("tools")), ShelfKind::Search);
    }

    #[test]
    fn counts_and_dates_format_for_the_detail_meta() {
        assert_eq!(super::format_count(693), "693");
        assert_eq!(super::format_count(1_327), "1.3k");
        assert_eq!(super::format_count(25_400), "25.4k");
        assert_eq!(super::format_count(495_021), "495k");
        assert_eq!(super::format_count(1_500_000), "1.5m");
        assert_eq!(
            super::format_unix_date(1_767_632_598_365).as_deref(),
            Some("2026-01-05")
        );
        assert_eq!(super::format_unix_date(-1), None);
    }

    #[test]
    fn community_cards_link_to_the_public_page() {
        let mut card = card("clawcall-dev", "clawcall-dev", false);
        assert_eq!(
            super::shelf_page_url(&card).as_deref(),
            Some("https://clawhub.ai/clawcall-dev/skills/clawcall-dev")
        );
        card.official = true;
        assert_eq!(super::shelf_page_url(&card), None);
    }

    fn probe(max_offset: f32, offset_y: f32, auto_pages: u8) -> FollowProbe {
        FollowProbe {
            cards: 24,
            cap: 120,
            viewport_height: 640.,
            max_offset,
            offset_y,
            auto_pages,
        }
    }

    #[test]
    fn follow_waits_for_layout_and_then_the_bottom() {
        let mut hidden = probe(0., 0., 0);
        hidden.viewport_height = 0.;
        assert!(!follow_ready(&hidden));
        let mut empty = probe(0., 0., 0);
        empty.cards = 0;
        assert!(!follow_ready(&empty));
        assert!(follow_ready(&probe(0., 0., 0)));
        assert!(!follow_ready(&probe(0., 0., 2)));
        assert!(!follow_ready(&probe(800., 0., 0)));
        assert!(follow_ready(&probe(800., -500., 2)));
        let mut capped = probe(800., -500., 0);
        capped.cards = capped.cap;
        assert!(!follow_ready(&capped));
    }

    #[test]
    fn installed_skill_chips_only_include_real_installed_skills_with_canonical_ids()
     {
        assert_eq!(super::installed_skill_chips(&[]), Vec::new());

        let official = flowy_skills::InstalledSkill {
            id: flowy_skills::SkillId {
                owner_handle: "flowy".into(),
                slug: "xlsx-summary".into(),
                version: "1.0.0".into(),
            },
            name: "Excel Summary".into(),
            description: "Summarize spreadsheets cleanly.".into(),
            root: std::path::PathBuf::from("."),
            skill_path: std::path::PathBuf::from("SKILL.md"),
            content: String::new(),
            auto_invoke: true,
            scripts_enabled: false,
            origin: flowy_skills::InstallOrigin::Official,
            latest_version: None,
        };
        let community = flowy_skills::InstalledSkill {
            id: flowy_skills::SkillId {
                owner_handle: "alice".into(),
                slug: "notes-digest".into(),
                version: "0.2.0".into(),
            },
            name: String::new(),
            description: "Digest notes".into(),
            root: std::path::PathBuf::from("."),
            skill_path: std::path::PathBuf::from("SKILL.md"),
            content: String::new(),
            auto_invoke: true,
            scripts_enabled: false,
            origin: flowy_skills::InstallOrigin::Community,
            latest_version: None,
        };

        let chips = super::installed_skill_chips(&[
            official.clone(),
            official,
            community,
        ]);
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].id, "flowy/xlsx-summary");
        assert_eq!(chips[0].title, "Excel Summary");
        assert!(chips[0].official);
        assert_eq!(chips[1].id, "alice/notes-digest");
        assert_eq!(chips[1].title, "notes-digest");
        assert!(!chips[1].official);
    }
}
