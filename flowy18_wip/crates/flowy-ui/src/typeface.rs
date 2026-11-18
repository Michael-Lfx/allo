//! Page type. Screens choose a level; size, line height, weight, and color stay here.
use gpui::{
    FontFallbacks, FontWeight, Pixels, Rgba, Window, div, prelude::*, px,
};

use crate::ResolvedTheme;

const NARROW_WIDTH: f32 = 768.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleLevel {
    Page,
    Hero,
    Section,
    Card,
    Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRole {
    Body,
    Meta,
    Caption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeMetrics {
    pub size: Pixels,
    pub line_height: Pixels,
    pub weight: FontWeight,
}

#[must_use]
pub fn ui_font() -> gpui::Font {
    let mut font = gpui::font("Segoe UI");
    font.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Microsoft YaHei".into(),
        "PingFang SC".into(),
        "Noto Sans CJK SC".into(),
    ]));
    font
}

pub fn page_inset(viewport_width: Pixels) -> Pixels {
    if viewport_width < px(NARROW_WIDTH) {
        px(20.)
    } else {
        px(32.)
    }
}

impl ResolvedTheme {
    fn type_delta(self) -> f32 {
        f32::from(i16::from(self.options.font_size) - 14)
    }

    fn scale(self, base: f32) -> Pixels {
        px((base + self.type_delta()).max(11.))
    }

    #[must_use]
    pub fn title_metrics(
        self,
        level: TitleLevel,
        viewport_width: Pixels,
    ) -> TypeMetrics {
        let (size, line_height) = match level {
            TitleLevel::Page if viewport_width < px(NARROW_WIDTH) => (18., 24.),
            TitleLevel::Page => (20., 28.),
            TitleLevel::Hero => (32., 40.),
            TitleLevel::Section | TitleLevel::Card => (16., 24.),
            TitleLevel::Item => (14., 20.),
        };
        TypeMetrics {
            size: self.scale(size),
            line_height: self.scale(line_height),
            weight: match level {
                TitleLevel::Item => FontWeight::MEDIUM,
                TitleLevel::Page
                | TitleLevel::Hero
                | TitleLevel::Section
                | TitleLevel::Card => FontWeight::SEMIBOLD,
            },
        }
    }

    #[must_use]
    pub fn text_metrics(self, role: TextRole) -> TypeMetrics {
        let (size, line_height) = match role {
            TextRole::Body => (14., 20.),
            TextRole::Meta => (12., 18.),
            TextRole::Caption => (11., 16.),
        };
        TypeMetrics {
            size: self.scale(size),
            line_height: self.scale(line_height),
            weight: FontWeight::NORMAL,
        }
    }

    /// Search fields, filters, buttons, and navigation share one control scale.
    #[must_use]
    pub fn control_metrics(self) -> TypeMetrics {
        TypeMetrics {
            size: self.scale(14.),
            line_height: self.scale(20.),
            weight: FontWeight::MEDIUM,
        }
    }
}

fn styled_text(
    metrics: TypeMetrics,
    color: Rgba,
    text: impl Into<gpui::SharedString>,
    truncate: bool,
) -> gpui::Div {
    div()
        .font(ui_font())
        .text_size(metrics.size)
        .line_height(metrics.line_height)
        .font_weight(metrics.weight)
        .text_color(color)
        .when(truncate, |text| {
            text.min_w_0().w_full().whitespace_nowrap().truncate()
        })
        .child(text.into())
}

#[derive(Clone, Debug)]
#[must_use]
pub struct Title {
    level: TitleLevel,
    text: gpui::SharedString,
    viewport_width: Pixels,
    truncate: bool,
}

impl Title {
    pub fn new(level: TitleLevel, text: impl Into<gpui::SharedString>) -> Self {
        Self {
            level,
            text: text.into(),
            viewport_width: px(1280.),
            truncate: false,
        }
    }

    pub const fn viewport(mut self, width: Pixels) -> Self {
        self.viewport_width = width;
        self
    }

    pub const fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }

    pub fn render(self, theme: &ResolvedTheme) -> gpui::Div {
        let metrics = theme.title_metrics(self.level, self.viewport_width);
        styled_text(metrics, theme.ink(), self.text, self.truncate)
    }
}

#[derive(Clone, Debug)]
#[must_use]
pub struct Text {
    role: TextRole,
    content: gpui::SharedString,
    truncate: bool,
}

impl Text {
    pub fn new(role: TextRole, text: impl Into<gpui::SharedString>) -> Self {
        Self {
            role,
            content: text.into(),
            truncate: false,
        }
    }

    pub const fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }

    pub fn render(self, theme: &ResolvedTheme) -> gpui::Div {
        let color = match self.role {
            TextRole::Body => theme.ink(),
            TextRole::Meta | TextRole::Caption => theme.muted(),
        };
        styled_text(
            theme.text_metrics(self.role),
            color,
            self.content,
            self.truncate,
        )
    }
}

#[must_use]
pub struct PageHeader {
    title: gpui::SharedString,
    trailing: Vec<gpui::AnyElement>,
}

impl PageHeader {
    pub fn new(title: impl Into<gpui::SharedString>) -> Self {
        Self {
            title: title.into(),
            trailing: Vec::new(),
        }
    }

    pub fn trailing(mut self, element: impl gpui::IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    pub fn render(self, theme: &ResolvedTheme, window: &Window) -> gpui::Div {
        let width = window.viewport_size().width;
        div()
            .h(px(56.))
            .flex_shrink_0()
            .px(page_inset(width))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                Title::new(TitleLevel::Page, self.title)
                    .viewport(width)
                    .truncate()
                    .render(theme),
            )
            .child(div().flex_1())
            .children(self.trailing)
    }
}

impl std::fmt::Debug for PageHeader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PageHeader")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub struct SectionHeader {
    title: gpui::SharedString,
    meta: Option<gpui::SharedString>,
    body: Option<gpui::AnyElement>,
}

impl SectionHeader {
    pub fn new(title: impl Into<gpui::SharedString>) -> Self {
        Self {
            title: title.into(),
            meta: None,
            body: None,
        }
    }

    pub fn meta(mut self, meta: impl Into<gpui::SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn body(mut self, body: impl gpui::IntoElement) -> Self {
        self.body = Some(body.into_any_element());
        self
    }

    pub fn render(self, theme: &ResolvedTheme) -> gpui::Div {
        let mut header = div()
            .min_h(px(28.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(Title::new(TitleLevel::Section, self.title).render(theme));
        if let Some(meta) = self.meta {
            header =
                header.child(Text::new(TextRole::Meta, meta).render(theme));
        }
        let mut section =
            div().w_full().flex().flex_col().gap(px(12.)).child(header);
        if let Some(body) = self.body {
            section = section.child(body);
        }
        section
    }
}

impl std::fmt::Debug for SectionHeader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SectionHeader")
            .field("title", &self.title)
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub struct PageColumn {
    children: Vec<gpui::AnyElement>,
}

impl PageColumn {
    pub const fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: impl gpui::IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn render(self, window: &Window) -> gpui::Div {
        let inset = page_inset(window.viewport_size().width);
        div().w_full().px(inset).pt(px(16.)).pb(inset).child(
            div()
                .w_full()
                .max_w(px(960.))
                .mx_auto()
                .flex()
                .flex_col()
                .gap(px(24.))
                .children(self.children),
        )
    }
}

impl Default for PageColumn {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PageColumn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PageColumn").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{TextRole, TitleLevel};
    use crate::{ResolvedTheme, ThemeMode, ThemeOptions};
    use gpui::{FontWeight, px};

    fn theme(font_size: u8) -> ResolvedTheme {
        ResolvedTheme::resolve(
            ThemeOptions {
                mode: ThemeMode::Light,
                font_size,
                ..ThemeOptions::default()
            },
            false,
        )
    }

    #[test]
    fn type_scale_follows_font_size_and_narrow_page_title() {
        let base = theme(14);
        let page = base.title_metrics(TitleLevel::Page, px(1280.));
        assert_eq!(page.size, px(20.));
        assert_eq!(page.line_height, px(28.));
        assert_eq!(page.weight, FontWeight::SEMIBOLD);
        let narrow = base.title_metrics(TitleLevel::Page, px(700.));
        assert_eq!(narrow.size, px(18.));
        assert_eq!(narrow.line_height, px(24.));
        assert_eq!(
            base.title_metrics(TitleLevel::Section, px(1280.)).size,
            px(16.)
        );
        assert_eq!(
            base.title_metrics(TitleLevel::Item, px(1280.)).weight,
            FontWeight::MEDIUM
        );
        assert_eq!(base.text_metrics(TextRole::Body).size, px(14.));
        assert_eq!(base.text_metrics(TextRole::Meta).size, px(12.));
        assert_eq!(base.text_metrics(TextRole::Meta).line_height, px(18.));
        assert_eq!(base.text_metrics(TextRole::Caption).size, px(11.));
        assert_eq!(base.control_metrics().weight, FontWeight::MEDIUM);
        assert_eq!(
            theme(12).title_metrics(TitleLevel::Page, px(1280.)).size,
            px(18.)
        );
        assert_eq!(theme(12).text_metrics(TextRole::Caption).size, px(11.));
        assert_eq!(
            theme(16).title_metrics(TitleLevel::Page, px(1280.)).size,
            px(22.)
        );
        assert_eq!(theme(16).text_metrics(TextRole::Meta).size, px(14.));
    }
}
