//! Bounded Markdown display for settled and streaming assistant text. The
//! source stays in the conversation record; rendering has no file or network
//! side effects.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use flowy_i18n::I18n;
use flowy_ui::{self as theme, ResolvedTheme};
use gpui::{
    AnyElement, Div, Font, Hsla, Rgba, StrikethroughStyle, StyledText, TextRun,
    UnderlineStyle, div, font, prelude::*, px,
};
use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag,
};
use url::Url;

const MAX_BYTES: usize = 64 * 1024;
const MAX_STREAMING_BYTES: usize = 16 * 1024;
const MAX_EVENTS: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_CACHED_DOCS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
enum OwnedCodeBlockKind {
    Indented,
    Fenced(String),
}

#[derive(Clone, Debug, PartialEq)]
enum OwnedTag {
    Paragraph,
    Heading { level: HeadingLevel },
    BlockQuote,
    CodeBlock(OwnedCodeBlockKind),
    List(Option<u64>),
    Item,
    Table(Vec<Alignment>),
    TableHead,
    TableRow,
    TableCell,
    Emphasis,
    Strong,
    Strikethrough,
    Link { dest_url: String },
    Image,
    Other,
}

impl OwnedTag {
    fn from_tag(tag: Tag<'_>) -> Self {
        match tag {
            Tag::Paragraph => Self::Paragraph,
            Tag::Heading { level, .. } => Self::Heading { level },
            Tag::BlockQuote(_) => Self::BlockQuote,
            Tag::CodeBlock(kind) => Self::CodeBlock(match kind {
                CodeBlockKind::Indented => OwnedCodeBlockKind::Indented,
                CodeBlockKind::Fenced(info) => {
                    OwnedCodeBlockKind::Fenced(info.into_string())
                }
            }),
            Tag::List(start) => Self::List(start),
            Tag::Item => Self::Item,
            Tag::Table(alignments) => Self::Table(alignments),
            Tag::TableHead => Self::TableHead,
            Tag::TableRow => Self::TableRow,
            Tag::TableCell => Self::TableCell,
            Tag::Emphasis => Self::Emphasis,
            Tag::Strong => Self::Strong,
            Tag::Strikethrough => Self::Strikethrough,
            Tag::Link { dest_url, .. } => Self::Link {
                dest_url: dest_url.into_string(),
            },
            Tag::Image { .. } => Self::Image,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum OwnedNode {
    Element(OwnedTag, Vec<Self>),
    Text(String),
    InlineCode(String),
    TaskMarker(bool),
    Break,
    Rule,
}

#[derive(Clone)]
struct CachedDoc {
    hash: u64,
    source: Arc<str>,
    ast: Option<Arc<[OwnedNode]>>,
}

#[derive(Default)]
struct AstCache {
    entries: VecDeque<CachedDoc>,
}

impl AstCache {
    fn hash_source(source: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut hasher);
        hasher.finish()
    }

    fn get_or_parse(&mut self, source: &str) -> Option<Arc<[OwnedNode]>> {
        if source.len() > MAX_BYTES {
            return None;
        }
        let hash = Self::hash_source(source);
        if let Some(pos) = self.entries.iter().position(|entry| {
            entry.hash == hash && entry.source.as_ref() == source
        }) {
            if pos != 0 {
                let entry = self.entries.remove(pos)?;
                self.entries.push_front(entry);
            }
            return self.entries.front()?.ast.clone();
        }
        let ast: Option<Arc<[OwnedNode]>> = parse(source).map(Arc::from);
        if self.entries.len() >= MAX_CACHED_DOCS {
            self.entries.pop_back();
        }
        self.entries.push_front(CachedDoc {
            hash,
            source: Arc::from(source),
            ast: ast.clone(),
        });
        ast
    }
}

thread_local! {
    static AST_CACHE: RefCell<AstCache> = RefCell::new(AstCache::default());
}

fn parse_cached(source: &str) -> Option<Arc<[OwnedNode]>> {
    AST_CACHE.with(|cache| cache.borrow_mut().get_or_parse(source))
}

fn parse(source: &str) -> Option<Vec<OwnedNode>> {
    if source.len() > MAX_BYTES {
        return None;
    }
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    parse_events(Parser::new_ext(source, options))
}

fn parse_events<'a>(
    events: impl Iterator<Item = Event<'a>>,
) -> Option<Vec<OwnedNode>> {
    let mut nodes = Vec::new();
    let mut stack: Vec<(OwnedTag, Vec<OwnedNode>)> = Vec::new();
    for (index, event) in events.enumerate() {
        if index >= MAX_EVENTS {
            return None;
        }
        let node = match event {
            Event::Start(tag) => {
                if stack.len() == MAX_DEPTH {
                    return None;
                }
                stack.push((OwnedTag::from_tag(tag), Vec::new()));
                continue;
            }
            Event::End(_) => {
                let (tag, children) = stack.pop()?;
                OwnedNode::Element(tag, children)
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                OwnedNode::Text(text.into_string())
            }
            Event::Code(code) => OwnedNode::InlineCode(code.into_string()),
            Event::SoftBreak => OwnedNode::Text(" ".to_owned()),
            Event::HardBreak => OwnedNode::Break,
            Event::Rule => OwnedNode::Rule,
            Event::TaskListMarker(checked) => OwnedNode::TaskMarker(checked),
            _ => continue,
        };
        stack
            .last_mut()
            .map_or(&mut nodes, |(_, inside)| inside)
            .push(node);
    }
    stack.is_empty().then_some(nodes)
}

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    strikethrough: bool,
}

enum Inline<'a> {
    Text(&'a str, Style),
    Link { label: String, url: Url },
    Break,
}

fn web_address(raw: &str) -> Option<Url> {
    if !raw.starts_with("https://") && !raw.starts_with("http://") {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

fn plain_text(nodes: &[OwnedNode], text: &mut String) {
    for node in nodes {
        match node {
            OwnedNode::Text(part) | OwnedNode::InlineCode(part) => {
                text.push_str(part);
            }
            OwnedNode::Element(_, children) => plain_text(children, text),
            OwnedNode::Break => text.push('\n'),
            OwnedNode::TaskMarker(_) | OwnedNode::Rule => (),
        }
    }
}

const fn char_display_width(ch: char) -> usize {
    match ch as u32 {
        0x1100..=0x115F
        | 0x2329..=0x232A
        | 0x2E80..=0x303E
        | 0x3040..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF
        | 0x20000..=0x3FFFF => 2,
        _ => 1,
    }
}

fn text_display_width(text: &str) -> usize {
    text.lines()
        .map(|line| line.chars().map(char_display_width).sum::<usize>())
        .max()
        .unwrap_or(0)
}

fn table_col_widths(children: &[OwnedNode], min_cols: usize) -> Vec<f32> {
    let mut max_units = vec![0usize; min_cols];
    let mut buf = String::new();
    for row in children {
        let OwnedNode::Element(OwnedTag::TableHead | OwnedTag::TableRow, cells) =
            row
        else {
            continue;
        };
        let mut col_idx = 0usize;
        for cell in cells {
            let OwnedNode::Element(OwnedTag::TableCell, content) = cell else {
                continue;
            };
            buf.clear();
            plain_text(content, &mut buf);
            let width = text_display_width(&buf);
            if col_idx >= max_units.len() {
                max_units.resize(col_idx + 1, 0);
            }
            max_units[col_idx] = max_units[col_idx].max(width);
            col_idx += 1;
        }
    }
    max_units
        .into_iter()
        .map(|units| (units as f32).mul_add(8., 24.).clamp(72., 280.))
        .collect()
}

fn leading_task_marker(nodes: &[OwnedNode]) -> Option<bool> {
    match nodes.first()? {
        OwnedNode::TaskMarker(checked) => Some(*checked),
        OwnedNode::Element(OwnedTag::Paragraph, children) => {
            match children.first()? {
                OwnedNode::TaskMarker(checked) => Some(*checked),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
fn inline<'a>(
    nodes: &'a [OwnedNode],
    style: Style,
    links_enabled: bool,
    output: &mut Vec<Inline<'a>>,
) {
    let mut skip_task = false;
    inline_inner(nodes, style, links_enabled, &mut skip_task, output);
}

fn inline_inner<'a>(
    nodes: &'a [OwnedNode],
    style: Style,
    links_enabled: bool,
    skip_task: &mut bool,
    output: &mut Vec<Inline<'a>>,
) {
    for node in nodes {
        match node {
            OwnedNode::Text(text) => output.push(Inline::Text(text, style)),
            OwnedNode::InlineCode(text) => output.push(Inline::Text(
                text,
                Style {
                    code: true,
                    ..style
                },
            )),
            OwnedNode::TaskMarker(checked) => {
                if *skip_task {
                    *skip_task = false;
                } else {
                    output.push(Inline::Text(
                        if *checked { "☑ " } else { "☐ " },
                        style,
                    ));
                }
            }
            OwnedNode::Break => output.push(Inline::Break),
            OwnedNode::Element(OwnedTag::Strong, children) => inline_inner(
                children,
                Style {
                    bold: true,
                    ..style
                },
                links_enabled,
                skip_task,
                output,
            ),
            OwnedNode::Element(OwnedTag::Emphasis, children) => inline_inner(
                children,
                Style {
                    italic: true,
                    ..style
                },
                links_enabled,
                skip_task,
                output,
            ),
            OwnedNode::Element(OwnedTag::Strikethrough, children) => {
                inline_inner(
                    children,
                    Style {
                        strikethrough: true,
                        ..style
                    },
                    links_enabled,
                    skip_task,
                    output,
                );
            }
            OwnedNode::Element(OwnedTag::Link { dest_url }, children) => {
                if links_enabled && let Some(url) = web_address(dest_url) {
                    let mut label = String::new();
                    plain_text(children, &mut label);
                    if !label.is_empty() {
                        output.push(Inline::Link { label, url });
                    }
                } else {
                    inline_inner(
                        children,
                        style,
                        links_enabled,
                        skip_task,
                        output,
                    );
                }
            }
            OwnedNode::Element(OwnedTag::Image, children) => {
                // Images only contribute their alternative text.
                inline_inner(children, style, false, skip_task, output);
            }
            OwnedNode::Element(_, children) => {
                inline_inner(children, style, links_enabled, skip_task, output);
            }
            OwnedNode::Rule => (),
        }
    }
}

struct Renderer<'a> {
    theme: &'a ResolvedTheme,
    i18n: &'a I18n,
    interactive: bool,
    next_control: Cell<usize>,
}

impl Renderer<'_> {
    fn control(&self) -> usize {
        let id = self.next_control.get();
        self.next_control.set(id + 1);
        id
    }

    fn copy(&self, key: &str, text: String) -> impl IntoElement {
        theme::IconButton::new(
            ("markdown-copy", self.control()),
            self.i18n.t(key),
            theme::icons::copy().size(px(13.)),
        )
        .size(theme::ControlSize::Small)
        .variant(theme::ControlVariant::Ghost)
        .render(self.theme, move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                text.clone(),
            ));
        })
    }

    fn task_checkbox(&self, checked: bool) -> AnyElement {
        let box_el = div()
            .w(px(14.))
            .h(px(14.))
            .mt(px(3.))
            .flex_none()
            .rounded(px(3.))
            .border_1()
            .flex()
            .items_center()
            .justify_center();
        if checked {
            box_el
                .border_color(self.theme.accent())
                .bg(self.theme.accent())
                .child(
                    theme::icons::check()
                        .size(px(10.))
                        .text_color(self.theme.on_accent()),
                )
                .into_any_element()
        } else {
            box_el.border_color(self.theme.muted()).into_any_element()
        }
    }

    fn prose(&self, nodes: &[OwnedNode], body_font: &Font, color: Rgba) -> Div {
        let mut skip_task = false;
        self.prose_inner(nodes, body_font, color, &mut skip_task)
    }

    fn prose_inner(
        &self,
        nodes: &[OwnedNode],
        body_font: &Font,
        color: Rgba,
        skip_task: &mut bool,
    ) -> Div {
        let mut parts = Vec::new();
        inline_inner(nodes, Style::default(), true, skip_task, &mut parts);
        let mut result = div().min_w_0().whitespace_normal().flex().flex_wrap();
        let mut text = String::new();
        let mut runs = Vec::new();
        for part in parts {
            match part {
                Inline::Text(content, style) => {
                    let mut face = if style.code {
                        mono_font()
                    } else {
                        body_font.clone()
                    };
                    if style.bold {
                        face = face.bold();
                    }
                    if style.italic {
                        face = face.italic();
                    }
                    runs.push(TextRun {
                        len: content.len(),
                        font: face,
                        color: Hsla::from(color),
                        background_color: style
                            .code
                            .then(|| Hsla::from(self.theme.soft())),
                        strikethrough: style.strikethrough.then(|| {
                            StrikethroughStyle {
                                thickness: px(1.),
                                color: None,
                            }
                        }),
                        ..Default::default()
                    });
                    text.push_str(content);
                }
                Inline::Link { label, url } => {
                    if !self.interactive {
                        runs.push(TextRun {
                            len: label.len(),
                            font: body_font.clone(),
                            color: Hsla::from(self.theme.accent()),
                            underline: Some(UnderlineStyle::default()),
                            ..Default::default()
                        });
                        text.push_str(&label);
                        continue;
                    }
                    result = append_text(
                        result,
                        &mut text,
                        &mut runs,
                        self.control(),
                        self.interactive,
                    );
                    let styled =
                        StyledText::new(label.clone()).with_runs(vec![
                            TextRun {
                                len: label.len(),
                                font: body_font.clone(),
                                color: Hsla::from(self.theme.accent()),
                                underline: Some(UnderlineStyle::default()),
                                ..Default::default()
                            },
                        ]);
                    result = result.child(
                        theme::interactive(
                            ("markdown-link", self.control()),
                            label,
                            self.theme,
                        )
                        .role(gpui::Role::Link)
                        .max_w_full()
                        .cursor_pointer()
                        .child(styled)
                        .on_click(move |_, _, cx| cx.open_url(url.as_str())),
                    );
                }
                Inline::Break => {
                    result = append_text(
                        result,
                        &mut text,
                        &mut runs,
                        self.control(),
                        self.interactive,
                    );
                    result = result.child(div().w_full().h(px(0.)));
                }
            }
        }
        append_text(
            result,
            &mut text,
            &mut runs,
            self.control(),
            self.interactive,
        )
    }

    fn blocks(&self, nodes: &[OwnedNode]) -> Div {
        self.blocks_inner(nodes, false)
    }

    fn blocks_inner(&self, nodes: &[OwnedNode], mut skip_task: bool) -> Div {
        let mut result =
            div().flex().flex_col().min_w_0().gap(self.theme.space(8.));
        let mut pending = None;
        for (index, node) in nodes.iter().enumerate() {
            match node {
                OwnedNode::Text(_)
                | OwnedNode::InlineCode(_)
                | OwnedNode::Break
                | OwnedNode::TaskMarker(_)
                | OwnedNode::Element(
                    OwnedTag::Strong
                    | OwnedTag::Emphasis
                    | OwnedTag::Strikethrough
                    | OwnedTag::Link { .. }
                    | OwnedTag::Image,
                    _,
                ) => {
                    pending.get_or_insert(index);
                    continue;
                }
                _ => (),
            }
            if let Some(first) = pending.take() {
                result = result.child(self.prose_inner(
                    &nodes[first..index],
                    &Font::default(),
                    self.theme.ink(),
                    &mut skip_task,
                ));
            }
            result = result.child(self.block_inner(node, &mut skip_task));
        }
        if let Some(first) = pending {
            result = result.child(self.prose_inner(
                &nodes[first..],
                &Font::default(),
                self.theme.ink(),
                &mut skip_task,
            ));
        }
        result
    }

    fn block_inner(
        &self,
        node: &OwnedNode,
        skip_task: &mut bool,
    ) -> AnyElement {
        let OwnedNode::Element(tag, children) = node else {
            return div()
                .h(px(1.))
                .w_full()
                .bg(self.theme.line())
                .into_any_element();
        };
        match tag {
            OwnedTag::Paragraph => self
                .prose_inner(
                    children,
                    &Font::default(),
                    self.theme.ink(),
                    skip_task,
                )
                .into_any_element(),
            OwnedTag::Heading { level } => {
                let heading = self
                    .prose(children, &Font::default().bold(), self.theme.ink())
                    .text_size(px(if *level as u8 <= 2 { 20. } else { 16. }));
                if self.interactive {
                    let mut label = String::new();
                    plain_text(children, &mut label);
                    heading
                        .id(("markdown-heading", self.control()))
                        .role(gpui::Role::Heading)
                        .aria_level(*level as usize)
                        .aria_label(label)
                        .into_any_element()
                } else {
                    heading.into_any_element()
                }
            }
            OwnedTag::BlockQuote => self
                .blocks(children)
                .border_l_2()
                .border_color(self.theme.line())
                .pl(self.theme.space(12.))
                .into_any_element(),
            OwnedTag::List(start) => div()
                .flex()
                .flex_col()
                .gap(self.theme.space(4.))
                .children(children.iter().enumerate().filter_map(
                    |(index, item)| {
                        let OwnedNode::Element(OwnedTag::Item, inside) = item
                        else {
                            return None;
                        };
                        let task = leading_task_marker(inside);
                        let marker: AnyElement = if let Some(checked) = task {
                            self.task_checkbox(checked)
                        } else {
                            let label = start.map_or_else(
                                || "•".to_owned(),
                                |n| {
                                    format!(
                                        "{}.",
                                        n.saturating_add(index as u64)
                                    )
                                },
                            );
                            div().flex_none().child(label).into_any_element()
                        };
                        Some(
                            div()
                                .flex()
                                .items_start()
                                .gap(self.theme.space(8.))
                                .child(marker)
                                .child(
                                    self.blocks_inner(inside, task.is_some()),
                                ),
                        )
                    },
                ))
                .into_any_element(),
            OwnedTag::CodeBlock(kind) => {
                let mut code = String::new();
                plain_text(children, &mut code);
                let display =
                    code.strip_suffix('\n').unwrap_or(&code).to_owned();
                let language = match kind {
                    OwnedCodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or_default()
                    }
                    OwnedCodeBlockKind::Indented => "",
                };
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .rounded_md()
                    .bg(self.theme.soft())
                    .p(self.theme.space(8.))
                    .when(self.interactive || !language.is_empty(), |block| {
                        block.child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .child(language.to_owned()),
                                )
                                .when(self.interactive, |row| {
                                    row.child(self.copy(
                                        "markdown.copy_code",
                                        code.clone(),
                                    ))
                                }),
                        )
                    })
                    .child(if self.interactive {
                        div()
                            .id(("markdown-code", self.control()))
                            .role(gpui::Role::Code)
                            .aria_label(display.clone())
                            .overflow_x_scroll()
                            .w_full()
                            .whitespace_nowrap()
                            .font_family(mono_font().family)
                            .child(display)
                            .into_any_element()
                    } else {
                        div()
                            .w_full()
                            .overflow_hidden()
                            .whitespace_normal()
                            .font_family(mono_font().family)
                            .child(display)
                            .into_any_element()
                    })
                    .into_any_element()
            }
            OwnedTag::Table(alignments) => {
                let col_widths = table_col_widths(children, alignments.len());
                let rows: Vec<_> = children
                    .iter()
                    .filter_map(|row| {
                        let OwnedNode::Element(
                            row_tag
                            @ (OwnedTag::TableHead | OwnedTag::TableRow),
                            cells,
                        ) = row
                        else {
                            return None;
                        };
                        Some((matches!(row_tag, OwnedTag::TableHead), cells))
                    })
                    .collect();
                let row_count = rows.len();
                let table_body = div().flex().flex_col().children(
                    rows.into_iter().enumerate().map(
                        |(row_idx, (header, cells))| {
                            let face = if header {
                                Font::default().bold()
                            } else {
                                Font::default()
                            };
                            let has_next = row_idx + 1 < row_count;
                            div().flex().children(
                                cells
                                    .iter()
                                    .filter_map(|node| {
                                        let OwnedNode::Element(
                                            OwnedTag::TableCell,
                                            cell,
                                        ) = node
                                        else {
                                            return None;
                                        };
                                        Some(cell)
                                    })
                                    .enumerate()
                                    .map(|(col_idx, cell)| {
                                        let width = col_widths
                                            .get(col_idx)
                                            .copied()
                                            .unwrap_or(160.);
                                        let alignment = alignments
                                            .get(col_idx)
                                            .copied()
                                            .unwrap_or(Alignment::None);
                                        let el = self
                                            .prose(
                                                cell,
                                                &face,
                                                self.theme.ink(),
                                            )
                                            .w(px(width))
                                            .flex_none()
                                            .px(self.theme.space(8.))
                                            .py(self.theme.space(4.))
                                            .when(has_next, |cell| {
                                                cell.border_b_1().border_color(
                                                    self.theme.line(),
                                                )
                                            })
                                            .when(header, |cell| {
                                                cell.bg(self.theme.soft())
                                            });
                                        match alignment {
                                            Alignment::Center => el
                                                .justify_center()
                                                .text_center(),
                                            Alignment::Right => {
                                                el.justify_end().text_right()
                                            }
                                            Alignment::Left
                                            | Alignment::None => {
                                                el.justify_start().text_left()
                                            }
                                        }
                                    }),
                            )
                        },
                    ),
                );
                let inner = if self.interactive {
                    div()
                        .id(("markdown-table", self.control()))
                        .w_full()
                        .overflow_x_scroll()
                        .child(table_body)
                        .into_any_element()
                } else {
                    div()
                        .w_full()
                        .overflow_hidden()
                        .child(table_body)
                        .into_any_element()
                };
                div()
                    .w_full()
                    .border_1()
                    .border_color(self.theme.line())
                    .rounded_md()
                    .overflow_hidden()
                    .child(inner)
                    .into_any_element()
            }
            _ => self.blocks(children).into_any_element(),
        }
    }
}

fn mono_font() -> Font {
    font(if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "monospace"
    })
}

fn append_text(
    parent: Div,
    text: &mut String,
    runs: &mut Vec<TextRun>,
    id: usize,
    interactive: bool,
) -> Div {
    if text.is_empty() {
        return parent;
    }
    let text = std::mem::take(text);
    let runs = std::mem::take(runs);
    if interactive {
        let styled = StyledText::new(text.clone()).with_runs(runs);
        parent.child(
            div()
                .id(("markdown-text", id))
                .role(gpui::Role::Label)
                .aria_label(text)
                .max_w_full()
                .min_w_0()
                .child(styled),
        )
    } else {
        let styled = StyledText::new(text).with_runs(runs);
        parent.child(div().max_w_full().min_w_0().child(styled))
    }
}

/// Closes an unclosed `CommonMark` fenced code block (` ``` ` or `~~~`, up to 3
/// leading spaces) on an owned copy so streaming code blocks do not flash as
/// plain text until the closing fence arrives.
fn close_unclosed_fence(source: &str) -> Cow<'_, str> {
    let mut open: Option<(u8, usize)> = None;
    for line in source.lines() {
        let bytes = line.as_bytes();
        let indent = bytes.iter().take_while(|&&b| b == b' ').count();
        if indent > 3 {
            continue;
        }
        let rest = &bytes[indent..];
        let Some(&marker) = rest.first() else {
            continue;
        };
        if marker != b'`' && marker != b'~' {
            continue;
        }
        let run_len = rest.iter().take_while(|&&b| b == marker).count();
        if run_len < 3 {
            continue;
        }
        let after = &rest[run_len..];
        match open {
            None => {
                // CommonMark forbids backticks in a backtick fence info string
                // (e.g. single-line inline code ```` ```foo``` ````).
                if marker == b'`' && after.contains(&b'`') {
                    continue;
                }
                open = Some((marker, run_len));
            }
            Some((open_marker, open_len)) => {
                if marker == open_marker
                    && run_len >= open_len
                    && after.iter().all(u8::is_ascii_whitespace)
                {
                    open = None;
                }
            }
        }
    }
    if let Some((marker, len)) = open {
        let mut closed = String::with_capacity(source.len() + len + 2);
        closed.push_str(source);
        if !closed.ends_with('\n') {
            closed.push('\n');
        }
        for _ in 0..len {
            closed.push(char::from(marker));
        }
        Cow::Owned(closed)
    } else {
        Cow::Borrowed(source)
    }
}

pub fn render(source: &str, theme: ResolvedTheme, i18n: &I18n) -> Div {
    let renderer = Renderer {
        theme: &theme,
        i18n,
        interactive: true,
        next_control: Cell::new(0),
    };
    let body = if let Some(nodes) = parse_cached(source) {
        renderer.blocks(&nodes)
    } else {
        div().whitespace_normal().child(source.to_owned())
    };
    div().flex().flex_col().min_w_0().text_sm().child(body)
}

/// Lightweight streaming Markdown renderer: avoids raw `**` / backtick leakage
/// during live tokens while keeping interactive/a11y nodes off until the turn
/// settles, closing partial code fences, and falling back cleanly past 16 KiB.
pub fn render_streaming(
    source: &str,
    theme: ResolvedTheme,
    i18n: &I18n,
) -> Div {
    if source.len() > MAX_STREAMING_BYTES {
        return div()
            .flex()
            .flex_col()
            .min_w_0()
            .text_sm()
            .whitespace_normal()
            .child(source.to_owned());
    }
    let normalized = close_unclosed_fence(source);
    let renderer = Renderer {
        theme: &theme,
        i18n,
        interactive: false,
        next_control: Cell::new(0),
    };
    let body = if let Some(nodes) = parse(&normalized) {
        renderer.blocks(&nodes)
    } else {
        div().whitespace_normal().child(source.to_owned())
    };
    div().flex().flex_col().min_w_0().text_sm().child(body)
}

#[cfg(test)]
mod tests {
    use super::{
        AstCache, Inline, MAX_BYTES, MAX_CACHED_DOCS, OwnedNode, OwnedTag,
        Style, inline, leading_task_marker, parse, parse_cached, parse_events,
        plain_text, render, render_streaming, table_col_widths,
    };
    use flowy_i18n::{I18n, Locale};
    use flowy_ui::{ResolvedTheme, ThemeMode, ThemeOptions};
    use pulldown_cmark::{Alignment, Options, Parser};
    use std::sync::Arc;

    #[test]
    fn native_subset_preserves_code_and_resolves_late_reference() {
        let source = "# 中文 🌱\n\n[site][s]\n\n```rust\nlet x = `literal`;\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n[s]: https://example.com/";
        let nodes = parse(source).unwrap();
        assert!(matches!(
            &nodes[0],
            OwnedNode::Element(OwnedTag::Heading { .. }, _)
        ));
        assert!(matches!(
            &nodes[3],
            OwnedNode::Element(OwnedTag::Table(_), _)
        ));
        let OwnedNode::Element(OwnedTag::CodeBlock(_), code) = &nodes[2] else {
            panic!()
        };
        let mut copied = String::new();
        plain_text(code, &mut copied);
        assert_eq!(copied, "let x = `literal`;\n");
        let mut parts = Vec::new();
        inline(&nodes, Style::default(), true, &mut parts);
        assert!(parts.iter().any(|part| matches!(part, Inline::Link { url, .. } if url.host_str() == Some("example.com"))));
        assert!(parse(&"界".repeat(MAX_BYTES)).is_none());
    }

    #[test]
    fn raw_html_images_and_non_web_links_stay_text() {
        let source = "<b>raw</b> ![alt](https://example.com/image) [local](file:///notes.md) [relative](notes.md) [action](app:send) [web](https://example.com/path)";
        let nodes = parse(source).unwrap();
        let mut parts = Vec::new();
        inline(&nodes, Style::default(), true, &mut parts);
        let mut text = String::new();
        let mut links = Vec::new();
        for part in parts {
            match part {
                Inline::Text(part, _) => text.push_str(part),
                Inline::Link { label, .. } => links.push(label),
                Inline::Break => (),
            }
        }
        assert_eq!(text.trim(), "<b>raw</b> alt local relative action");
        assert_eq!(links, ["web"]);
    }

    #[test]
    fn streaming_fence_helper_closes_partial_blocks_without_mutating_closed_ones()
     {
        use super::close_unclosed_fence;
        use std::borrow::Cow;

        let partial = "说明如下：\n```rust\nfn main() { println!(\"你好\"); }";
        let closed = close_unclosed_fence(partial);
        assert!(matches!(closed, Cow::Owned(_)));
        let nodes = parse(&closed).unwrap();
        assert!(nodes.iter().any(|n| matches!(
            n,
            OwnedNode::Element(OwnedTag::CodeBlock(_), _)
        )));

        let complete = "```rust\nlet x = 1;\n```";
        assert!(matches!(close_unclosed_fence(complete), Cow::Borrowed(_)));

        // Inline triple-backtick code, 4-space indented lines, and nested
        // fences inside a 4-backtick outer fence must follow CommonMark rules.
        let inline_code = "```foo``` and normal text";
        assert!(matches!(
            close_unclosed_fence(inline_code),
            Cow::Borrowed(_)
        ));
        let indented = "    ```not a fence";
        assert!(matches!(close_unclosed_fence(indented), Cow::Borrowed(_)));
        let nested_open = "````markdown\n```rust\nlet a = 1;\n```";
        let nested_closed = close_unclosed_fence(nested_open);
        assert!(matches!(nested_closed, Cow::Owned(_)));
        assert!(nested_closed.ends_with("\n````"));
    }

    #[test]
    fn ast_cache_hits_evicts_lru_and_isolates_theme_and_locale() {
        let mut cache = AstCache::default();
        let docs: Vec<String> = (0..MAX_CACHED_DOCS)
            .map(|i| format!("# Heading {i}"))
            .collect();
        let first = cache.get_or_parse(&docs[0]).unwrap();
        let first_again = cache.get_or_parse(&docs[0]).unwrap();
        assert!(Arc::ptr_eq(&first, &first_again));

        for doc in &docs[1..] {
            assert!(cache.get_or_parse(doc).is_some());
        }
        // Promote `docs[0]` to MRU before overflowing capacity.
        let promoted = cache.get_or_parse(&docs[0]).unwrap();
        assert!(Arc::ptr_eq(&first, &promoted));

        assert!(cache.get_or_parse("# Heading overflow").is_some());
        assert_eq!(cache.entries.len(), MAX_CACHED_DOCS);

        // `docs[0]` stays cached while `docs[1]` was evicted as LRU.
        let still_hit = cache.get_or_parse(&docs[0]).unwrap();
        assert!(Arc::ptr_eq(&first, &still_hit));
        let second_miss_before = cache
            .entries
            .iter()
            .any(|entry| entry.source.as_ref() == docs[1]);
        assert!(!second_miss_before);

        // Hash collision guard: even with identical hash values, distinct sources never collide.
        cache.entries[0].hash = AstCache::hash_source("# Distinct source");
        let distinct = cache.get_or_parse("# Distinct source").unwrap();
        assert!(!Arc::ptr_eq(&still_hit, &distinct));

        // Dual-theme / dual-locale rendering shares AST without mutating colors or text.
        let md =
            "~~strike~~\n\n```rust\nlet v = 1;\n```\n\n- [x] Done\n- [ ] Todo";
        let ast_a = parse_cached(md).unwrap();
        let ast_b = parse_cached(md).unwrap();
        assert!(Arc::ptr_eq(&ast_a, &ast_b));

        let light = ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Light,
                ..ThemeOptions::default()
            },
            false,
        );
        let dark = ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Dark,
                ..ThemeOptions::default()
            },
            true,
        );
        let mut en = I18n::embedded("Flowy");
        en.set_locale(Locale::En);
        let mut zh = I18n::embedded("Flowy");
        zh.set_locale(Locale::ZhCn);
        let _ = render(md, light, &en);
        let _ = render(md, dark, &zh);
        let _ = render_streaming("```\ncode\n```", dark, &zh);
    }

    #[test]
    fn strikethrough_tasklists_and_unhandled_events_are_supported() {
        let md = "~~old~~ and **~~both~~**\n\n- [x] Done\n- [ ] Pending\n\n* [x] Loose\n\n  paragraph";
        let nodes = parse(md).unwrap();
        let mut parts = Vec::new();
        inline(&nodes, Style::default(), true, &mut parts);
        assert!(
            parts.iter().any(
                |p| matches!(p, Inline::Text("old", s) if s.strikethrough)
            )
        );
        assert!(parts.iter().any(
            |p| matches!(p, Inline::Text("both", s) if s.strikethrough && s.bold)
        ));

        let OwnedNode::Element(OwnedTag::List(_), tight_items) = &nodes[1]
        else {
            panic!("expected tight task list");
        };
        let OwnedNode::Element(OwnedTag::Item, first_item) = &tight_items[0]
        else {
            panic!("expected item");
        };
        assert!(matches!(&first_item[0], OwnedNode::TaskMarker(true)));
        assert_eq!(leading_task_marker(first_item), Some(true));
        let OwnedNode::Element(OwnedTag::Item, second_item) = &tight_items[1]
        else {
            panic!("expected second item");
        };
        assert_eq!(leading_task_marker(second_item), Some(false));

        let OwnedNode::Element(OwnedTag::List(_), loose_items) = &nodes[2]
        else {
            panic!("expected loose task list");
        };
        let OwnedNode::Element(OwnedTag::Item, loose_item) = &loose_items[0]
        else {
            panic!("expected loose item");
        };
        assert!(matches!(
            &loose_item[0],
            OwnedNode::Element(OwnedTag::Paragraph, _)
        ));
        assert_eq!(leading_task_marker(loose_item), Some(true));

        // Unhandled extension events (`Options::all()` emits footnotes, math, superscript)
        // must be skipped cleanly without discarding the whole document.
        let rich = "---\ntitle: demo\n---\n\nBefore $x^2$ and [^fn1] and ^sup^.\n\n[^fn1]: note text\n\nAfter.";
        let parsed = parse_events(Parser::new_ext(rich, Options::all()))
            .expect("unhandled events must not fail parsing");
        let mut text = String::new();
        plain_text(&parsed, &mut text);
        assert!(text.contains("Before"), "{text}");
        assert!(text.contains("After."), "{text}");
    }

    #[test]
    fn table_widths_alignments_and_code_trailing_newline_trim() {
        let md = "| A | 中文宽列测试 | Long description column exceeding max width limit |\n| :--- | :---: | ---: |\n| 1 | 2 | short |";
        let nodes = parse(md).unwrap();
        let OwnedNode::Element(OwnedTag::Table(alignments), rows) = &nodes[0]
        else {
            panic!("expected table");
        };
        assert_eq!(
            alignments,
            &[Alignment::Left, Alignment::Center, Alignment::Right]
        );
        let widths = table_col_widths(rows, alignments.len());
        assert_eq!(widths.len(), 3);
        assert!(widths.iter().all(|&w| (72.0..=280.0).contains(&w)));
        assert_eq!(widths[0], 72.0);
        assert!(widths[1] > widths[0] && widths[1] < widths[2]);
        assert_eq!(widths[2], 280.0);

        let code_md = "```\nline1\nline2\n```";
        let code_nodes = parse(code_md).unwrap();
        let OwnedNode::Element(OwnedTag::CodeBlock(_), code) = &code_nodes[0]
        else {
            panic!("expected code block");
        };
        let mut raw = String::new();
        plain_text(code, &mut raw);
        assert_eq!(raw, "line1\nline2\n");
        assert_eq!(raw.strip_suffix('\n').unwrap_or(&raw), "line1\nline2");
    }
}
