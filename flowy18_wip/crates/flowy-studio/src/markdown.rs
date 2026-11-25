//! Bounded Markdown display for settled assistant text. The source stays
//! in the conversation record; rendering has no file or network side effects.

use std::cell::Cell;

use flowy_i18n::I18n;
use flowy_ui::{self as theme, ResolvedTheme};
use gpui::{
    AnyElement, Div, Font, Hsla, Rgba, StyledText, TextRun, UnderlineStyle,
    div, font, prelude::*, px,
};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag};
use url::Url;

const MAX_BYTES: usize = 64 * 1024;
const MAX_EVENTS: usize = 4096;
const MAX_DEPTH: usize = 32;

#[derive(Debug)]
enum Node<'a> {
    Element(Tag<'a>, Vec<Self>),
    Text(CowStr<'a>),
    InlineCode(CowStr<'a>),
    Break,
    Rule,
}

fn parse(source: &str) -> Option<Vec<Node<'_>>> {
    if source.len() > MAX_BYTES {
        return None;
    }
    let mut nodes = Vec::new();
    let mut stack: Vec<(Tag<'_>, Vec<Node<'_>>)> = Vec::new();
    for (index, event) in
        Parser::new_ext(source, Options::ENABLE_TABLES).enumerate()
    {
        if index >= MAX_EVENTS {
            return None;
        }
        let node = match event {
            Event::Start(tag) => {
                if stack.len() == MAX_DEPTH {
                    return None;
                }
                stack.push((tag, Vec::new()));
                continue;
            }
            Event::End(_) => {
                let (tag, children) = stack.pop()?;
                Node::Element(tag, children)
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                Node::Text(text)
            }
            Event::Code(code) => Node::InlineCode(code),
            Event::SoftBreak => Node::Text(" ".into()),
            Event::HardBreak => Node::Break,
            Event::Rule => Node::Rule,
            _ => return None,
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

fn plain_text(nodes: &[Node<'_>], text: &mut String) {
    for node in nodes {
        match node {
            Node::Text(part) | Node::InlineCode(part) => text.push_str(part),
            Node::Element(_, children) => plain_text(children, text),
            Node::Break => text.push('\n'),
            Node::Rule => (),
        }
    }
}

fn inline<'a>(
    nodes: &'a [Node<'_>],
    style: Style,
    links_enabled: bool,
    output: &mut Vec<Inline<'a>>,
) {
    for node in nodes {
        match node {
            Node::Text(text) => output.push(Inline::Text(text, style)),
            Node::InlineCode(text) => output.push(Inline::Text(
                text,
                Style {
                    code: true,
                    ..style
                },
            )),
            Node::Break => output.push(Inline::Break),
            Node::Element(Tag::Strong, children) => inline(
                children,
                Style {
                    bold: true,
                    ..style
                },
                links_enabled,
                output,
            ),
            Node::Element(Tag::Emphasis, children) => inline(
                children,
                Style {
                    italic: true,
                    ..style
                },
                links_enabled,
                output,
            ),
            Node::Element(Tag::Link { dest_url, .. }, children) => {
                if links_enabled && let Some(url) = web_address(dest_url) {
                    let mut label = String::new();
                    plain_text(children, &mut label);
                    if !label.is_empty() {
                        output.push(Inline::Link { label, url });
                    }
                } else {
                    inline(children, style, links_enabled, output);
                }
            }
            Node::Element(Tag::Image { .. }, children) => {
                // Images only contribute their alternative text.
                inline(children, style, false, output);
            }
            Node::Element(_, children) => {
                inline(children, style, links_enabled, output);
            }
            Node::Rule => (),
        }
    }
}

struct Renderer<'a> {
    theme: &'a ResolvedTheme,
    i18n: &'a I18n,
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

    fn prose(&self, nodes: &[Node<'_>], body_font: &Font, color: Rgba) -> Div {
        let mut parts = Vec::new();
        inline(nodes, Style::default(), true, &mut parts);
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
                        ..Default::default()
                    });
                    text.push_str(content);
                }
                Inline::Link { label, url } => {
                    result = append_text(
                        result,
                        &mut text,
                        &mut runs,
                        self.control(),
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
                    );
                    result = result.child(div().w_full().h(px(0.)));
                }
            }
        }
        append_text(result, &mut text, &mut runs, self.control())
    }

    fn blocks(&self, nodes: &[Node<'_>]) -> Div {
        let mut result =
            div().flex().flex_col().min_w_0().gap(self.theme.space(8.));
        let mut pending = None;
        for (index, node) in nodes.iter().enumerate() {
            match node {
                Node::Text(_)
                | Node::InlineCode(_)
                | Node::Break
                | Node::Element(
                    Tag::Strong
                    | Tag::Emphasis
                    | Tag::Link { .. }
                    | Tag::Image { .. },
                    _,
                ) => {
                    pending.get_or_insert(index);
                    continue;
                }
                _ => (),
            }
            if let Some(first) = pending.take() {
                result = result.child(self.prose(
                    &nodes[first..index],
                    &Font::default(),
                    self.theme.ink(),
                ));
            }
            result = result.child(self.block(node));
        }
        if let Some(first) = pending {
            result = result.child(self.prose(
                &nodes[first..],
                &Font::default(),
                self.theme.ink(),
            ));
        }
        result
    }

    fn block(&self, node: &Node<'_>) -> AnyElement {
        let Node::Element(tag, children) = node else {
            return div()
                .h(px(1.))
                .w_full()
                .bg(self.theme.line())
                .into_any_element();
        };
        match tag {
            Tag::Heading { level, .. } => {
                let mut label = String::new();
                plain_text(children, &mut label);
                self.prose(children, &Font::default().bold(), self.theme.ink())
                    .id(("markdown-heading", self.control()))
                    .role(gpui::Role::Heading)
                    .aria_level(*level as usize)
                    .aria_label(label)
                    .text_size(px(if *level as u8 <= 2 { 20. } else { 16. }))
                    .into_any_element()
            }
            Tag::BlockQuote(_) => self
                .blocks(children)
                .border_l_2()
                .border_color(self.theme.line())
                .pl(self.theme.space(12.))
                .into_any_element(),
            Tag::List(start) => div()
                .flex()
                .flex_col()
                .gap(self.theme.space(4.))
                .children(children.iter().enumerate().filter_map(
                    |(index, item)| {
                        let Node::Element(Tag::Item, inside) = item else {
                            return None;
                        };
                        let marker = start.map_or_else(
                            || "•".into(),
                            |n| format!("{}.", n.saturating_add(index as u64)),
                        );
                        Some(
                            div()
                                .flex()
                                .gap(self.theme.space(8.))
                                .child(marker)
                                .child(self.blocks(inside)),
                        )
                    },
                ))
                .into_any_element(),
            Tag::CodeBlock(kind) => {
                let mut code = String::new();
                plain_text(children, &mut code);
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or_default()
                    }
                    CodeBlockKind::Indented => "",
                };
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .rounded_md()
                    .bg(self.theme.soft())
                    .p(self.theme.space(8.))
                    .child(
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
                            .child(
                                self.copy("markdown.copy_code", code.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id(("markdown-code", self.control()))
                            .role(gpui::Role::Code)
                            .aria_label(code.clone())
                            .overflow_x_scroll()
                            .w_full()
                            .whitespace_nowrap()
                            .font_family(mono_font().family)
                            .child(code),
                    )
                    .into_any_element()
            }
            Tag::Table(_) => div()
                .id(("markdown-table", self.control()))
                .w_full()
                .overflow_x_scroll()
                .child(div().flex().flex_col().children(
                    children.iter().filter_map(|row| {
                        let Node::Element(
                            row_tag @ (Tag::TableHead | Tag::TableRow),
                            cells,
                        ) = row
                        else {
                            return None;
                        };
                        let header = matches!(row_tag, Tag::TableHead);
                        let face = if header {
                            Font::default().bold()
                        } else {
                            Font::default()
                        };
                        Some(div().flex().children(cells.iter().filter_map(
                            |node| {
                                let Node::Element(Tag::TableCell, cell) = node
                                else {
                                    return None;
                                };
                                Some(
                                    self.prose(cell, &face, self.theme.ink())
                                        .w(px(160.))
                                        .flex_none()
                                        .px(self.theme.space(8.))
                                        .py(self.theme.space(4.))
                                        .border_b_1()
                                        .border_color(self.theme.line())
                                        .when(header, |cell| {
                                            cell.bg(self.theme.soft())
                                        }),
                                )
                            },
                        )))
                    }),
                ))
                .into_any_element(),
            _ => self
                .prose(children, &Font::default(), self.theme.ink())
                .into_any_element(),
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
) -> Div {
    if text.is_empty() {
        return parent;
    }
    let text = std::mem::take(text);
    parent.child(
        div()
            .id(("markdown-text", id))
            .role(gpui::Role::Label)
            .aria_label(text.clone())
            .max_w_full()
            .min_w_0()
            .child(StyledText::new(text).with_runs(std::mem::take(runs))),
    )
}

pub fn render(source: &str, theme: ResolvedTheme, i18n: &I18n) -> Div {
    let renderer = Renderer {
        theme: &theme,
        i18n,
        next_control: Cell::new(0),
    };
    let body = if let Some(nodes) = parse(source) {
        renderer.blocks(&nodes)
    } else {
        div().whitespace_normal().child(source.to_owned())
    };
    div().flex().flex_col().min_w_0().text_sm().child(body)
}

#[cfg(test)]
mod tests {
    use super::{Inline, MAX_BYTES, Node, Style, inline, parse, plain_text};
    use pulldown_cmark::Tag;

    #[test]
    fn native_subset_preserves_code_and_resolves_late_reference() {
        let source = "# 中文 🌱\n\n[site][s]\n\n```rust\nlet x = `literal`;\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n[s]: https://example.com/";
        let nodes = parse(source).unwrap();
        assert!(matches!(&nodes[0], Node::Element(Tag::Heading { .. }, _)));
        assert!(matches!(&nodes[3], Node::Element(Tag::Table(_), _)));
        let Node::Element(Tag::CodeBlock(_), code) = &nodes[2] else {
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
}
