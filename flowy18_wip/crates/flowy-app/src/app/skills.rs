use super::FlowyApp;
use flowy_ui as theme;
use gpui::{
    Context, Pixels, SharedString, Size, Window, div, prelude::*, px, rgb,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Category {
    All,
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
    const ALL: [Self; 9] = [
        Self::All,
        Self::Coding,
        Self::Data,
        Self::Automation,
        Self::Business,
        Self::Design,
        Self::Media,
        Self::Content,
        Self::App,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::All => "skills.cat_all",
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

    fn matches(self, categories: &[Self]) -> bool {
        match self {
            Self::All => true,
            other => categories.contains(&other),
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
}

const SKILLS: &[Skill] = &[
    Skill {
        id: "excel",
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
    },
    Skill {
        id: "traffic",
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
    },
    Skill {
        id: "slides",
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
    },
    Skill {
        id: "research",
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
    },
    Skill {
        id: "nano",
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
    },
    Skill {
        id: "flux",
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
    },
    Skill {
        id: "script",
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
    },
    Skill {
        id: "pipeline",
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
    },
    Skill {
        id: "clip",
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
    },
];

fn skill_by_id(id: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|skill| skill.id == id)
}

impl FlowyApp {
    pub(super) fn skills_canvas(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.skills_header(window, cx))
            .child(
                div()
                    .id("skills-scroll")
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

    fn skills_filters(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row =
            theme::FilterTabs::new("skills-tabs", self.t("library.categories"));
        for category in Category::ALL {
            let selected = self.skills.filter == category;
            let id =
                SharedString::from(format!("skill-filter-{}", category.key()));
            row = row.child(
                theme::filter_chip(
                    id,
                    self.t(category.key()),
                    selected,
                    &self.theme,
                )
                .on_click(cx.listener(
                    move |app, _event, _window, cx| {
                        cx.stop_propagation();
                        app.skills.filter = category;
                        cx.notify();
                    },
                )),
            );
        }
        row.render()
    }

    fn skills_grid(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let query = self.skills.query.read(cx).as_str().trim().to_lowercase();
        let filtered: Vec<&'static Skill> = SKILLS
            .iter()
            .filter(|skill| self.skills.filter.matches(skill.categories))
            .filter(|skill| {
                if query.is_empty() {
                    return true;
                }
                let title = self.t(skill.title).to_lowercase();
                let body = self.t(skill.body).to_lowercase();
                title.contains(&query) || body.contains(&query)
            })
            .collect();

        let mut column = div().flex().flex_col().gap_4();
        for chunk in filtered.chunks(3) {
            let mut row = div().flex().gap_4().items_stretch();
            for skill in chunk {
                row = row.child(self.skill_card(skill, cx));
            }
            for _ in chunk.len()..3 {
                row = row.child(div().flex_1().min_w_0());
            }
            column = column.child(row);
        }
        column
    }

    fn skill_card(
        &self,
        skill: &Skill,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let id = skill.id;
        let element_id = SharedString::from(format!("skill-card-{id}"));
        theme::interactive(element_id, self.t(skill.title), &self.theme)
            .flex_1()
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .overflow_hidden()
            .hover(|card| card.border_color(self.theme.muted()))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                cx.stop_propagation();
                app.open_skill(id, cx);
            }))
            .child(skill_cover(skill, &self.theme))
            .child(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(self.theme.body_size())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.t(skill.title)),
                    )
                    .child(
                        div()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .whitespace_normal()
                            .child(self.t(skill.body)),
                    )
                    .child(self.skill_tags(skill))
                    .child(
                        div()
                            .mt_1()
                            .text_size(self.theme.caption_size())
                            .text_color(self.theme.muted())
                            .child(
                                self.tf(
                                    "skills.used",
                                    &[("count", skill.uses)],
                                ),
                            ),
                    ),
            )
    }

    fn skill_tags(&self, skill: &Skill) -> impl IntoElement {
        let mut row = div().flex().gap_1().flex_wrap().items_center();
        row = row.child(self.author_tag(skill.author));
        for category in skill.categories {
            row = row.child(self.skill_tag(category.key()));
        }
        row
    }

    fn author_tag(&self, author: Author) -> impl IntoElement {
        let icon = match author {
            Author::Official => theme::icons::sparkles()
                .size(px(12.))
                .text_color(self.theme.muted()),
            Author::Community => theme::icons::user()
                .size(px(12.))
                .text_color(self.theme.muted()),
        };
        div()
            .px_2()
            .py(px(2.))
            .rounded_md()
            .bg(self.theme.soft())
            .text_size(self.theme.caption_size())
            .text_color(self.theme.muted())
            .flex()
            .items_center()
            .gap_1()
            .child(icon)
            .child(self.t(author.key()))
    }

    fn skill_tag(&self, key: &'static str) -> impl IntoElement {
        div()
            .px_2()
            .py(px(2.))
            .rounded_md()
            .bg(self.theme.soft())
            .text_size(self.theme.caption_size())
            .text_color(self.theme.muted())
            .child(self.t(key))
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
        let id = self.active_dialog.skill().unwrap_or("");
        let content = if let Some(skill) = skill_by_id(id) {
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
                            .child(self.skill_dialog_actions(cx)),
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

    fn skill_dialog_actions(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                theme::Button::new("skill-add", self.t("skills.add"))
                    .variant(theme::ControlVariant::Primary)
                    .icon(theme::icons::plus())
                    .render(
                        &self.theme,
                        cx.listener(|app, _event, _window, cx| {
                            cx.stop_propagation();
                            app.note("skills.add_later", cx);
                        }),
                    ),
            )
            .child(
                theme::Button::new("skill-details", self.t("skills.details"))
                    .icon(theme::icons::book())
                    .render(
                        &self.theme,
                        cx.listener(|app, _event, _window, cx| {
                            cx.stop_propagation();
                            app.note("skills.details_later", cx);
                        }),
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
                    if id == "skill-close" || id == "skill-stub-close" {
                        app.active_dialog = super::state::ActiveDialog::None;
                        cx.notify();
                    }
                }),
            )
    }
}

fn skill_cover(
    skill: &Skill,
    theme: &theme::ResolvedTheme,
) -> impl IntoElement {
    div()
        .h(px(148.))
        .w_full()
        .rounded_t_xl()
        .bg(rgb(skill.tint))
        .flex()
        .items_center()
        .justify_center()
        .child(skill_mark(skill, theme, 24.))
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
