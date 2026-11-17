//! Window-scoped, immutable design tokens. A render never changes theme state.
use crate::{Accent, Density};
use gpui::{ColorExt, Context, Pixels, Rgba, Size, px, rgb, size};
use serde::{Deserialize, Serialize};

pub const DOCK_WIDTH: f32 = 248.0;
pub const MODAL_INSET: f32 = 0.08;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    Auto,
}
impl ThemeMode {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Light => "look.light",
            Self::Dark => "look.dark",
            Self::Auto => "look.auto",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeOptions {
    pub mode: ThemeMode,
    pub accent: Accent,
    pub density: Density,
    pub font_size: u8,
}
impl Default for ThemeOptions {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Auto,
            accent: Accent::Default,
            density: Density::Compact,
            font_size: 14,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionPolicy {
    #[default]
    Reduced,
    Full,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlSize {
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlVariant {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

#[derive(Clone, Copy, Debug)]
pub struct ControlColors {
    pub background: Rgba,
    pub foreground: Rgba,
    pub border: Rgba,
    pub hover: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTheme {
    pub options: ThemeOptions,
    pub dark: bool,
}
impl Default for ResolvedTheme {
    fn default() -> Self {
        Self::resolve(ThemeOptions::default(), false)
    }
}
impl ResolvedTheme {
    pub const fn resolve(options: ThemeOptions, system_dark: bool) -> Self {
        let dark = match options.mode {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::Auto => system_dark,
        };
        Self { options, dark }
    }
    /// Small secondary copy stays legible when the user reduces the base size.
    pub fn caption_size(self) -> Pixels {
        px(f32::from(self.options.font_size.saturating_sub(2).max(12)))
    }
    pub fn body_size(self) -> Pixels {
        px(f32::from(self.options.font_size.max(12)))
    }
    pub fn label_size(self) -> Pixels {
        self.body_size()
    }
    pub fn heading_size(self) -> Pixels {
        self.body_size() * 1.5
    }
    pub fn display_size(self) -> Pixels {
        self.body_size() * 4.
    }
    pub fn canvas(self) -> Rgba {
        rgb(if self.dark { 0x001c_1c1a } else { 0x00f4_f3ef })
    }
    pub fn dock(self) -> Rgba {
        rgb(if self.dark { 0x0024_2422 } else { 0x00f7_f6f3 })
    }
    pub fn card(self) -> Rgba {
        rgb(if self.dark { 0x002c_2c2a } else { 0x00ff_ffff })
    }
    pub fn ink(self) -> Rgba {
        rgb(if self.dark { 0x00f4_f3ef } else { 0x001c_1c1c })
    }
    pub fn muted(self) -> Rgba {
        rgb(if self.dark { 0x00a8_a49c } else { 0x006b_6861 })
    }
    pub fn line(self) -> Rgba {
        rgb(if self.dark { 0x003e_3e3a } else { 0x00e6_e2da })
    }
    pub fn border_strong(self) -> Rgba {
        rgb(if self.dark { 0x0096_938c } else { 0x0085_8178 })
    }
    pub fn soft(self) -> Rgba {
        rgb(if self.dark { 0x0034_3432 } else { 0x00ef_eee9 })
    }
    pub fn composer(self) -> Rgba {
        rgb(if self.dark { 0x0034_3432 } else { 0x002c_2c2a })
    }
    pub fn on_composer(self) -> Rgba {
        rgb(0x00f4_f3ef)
    }
    pub fn composer_muted(self) -> Rgba {
        rgb(0x00a8_a49c)
    }
    pub fn danger(self) -> Rgba {
        rgb(if self.dark { 0x00e0_7070 } else { 0x009f_2d2d })
    }
    pub fn on_ink(self) -> Rgba {
        rgb(if self.dark { 0x001c_1c1a } else { 0x00ff_ffff })
    }
    pub fn accent(self) -> Rgba {
        match self.options.accent {
            Accent::Default | Accent::Blue => rgb(0x002e_68f5),
            other => self.swatch(other),
        }
    }
    pub fn on_accent(self) -> Rgba {
        match self.options.accent {
            Accent::Green | Accent::Orange => rgb(0),
            _ => rgb(0x00ff_ffff),
        }
    }
    pub fn fill(self) -> Rgba {
        match self.options.accent {
            Accent::Default => self.ink(),
            Accent::Blue => rgb(0x002e_68f5),
            other => self.swatch(other),
        }
    }
    pub fn on_fill(self) -> Rgba {
        match self.options.accent {
            Accent::Default => self.on_ink(),
            Accent::Green | Accent::Orange => rgb(0),
            _ => rgb(0x00ff_ffff),
        }
    }
    /// Focus is a high-contrast interaction color, independent of decorative accent swatches.
    pub fn focus_ring(self) -> Rgba {
        rgb(if self.dark { 0x00a4_c2ff } else { 0x0017_4bb8 })
    }
    pub fn swatch(self, accent: Accent) -> Rgba {
        match accent {
            Accent::Default => self.muted(),
            Accent::Blue => rgb(0x002f_6bff),
            Accent::Green => rgb(0x001f_8a4c),
            Accent::Orange => rgb(0x00d4_6b08),
            Accent::Red => rgb(0x00c4_3737),
            Accent::Purple => rgb(0x006b_46c1),
        }
    }
    /// Discrete spacing recipe. Arbitrary values are snapped to the nearest defined step.
    pub fn space(self, base: f32) -> Pixels {
        const STEPS: [f32; 9] = [2., 4., 8., 12., 16., 24., 32., 48., 64.];
        let value = STEPS
            .into_iter()
            .min_by(|a, b| (a - base).abs().total_cmp(&(b - base).abs()))
            .unwrap_or(8.);
        let adjustment = match self.options.density {
            Density::Compact => 0.,
            Density::Regular => 2.,
            Density::Comfortable => 4.,
        };
        px(value + if value >= 8. { adjustment } else { 0. })
    }
    pub fn control_height(self, size: ControlSize) -> Pixels {
        let base = match size {
            ControlSize::Small => 28.,
            ControlSize::Medium => 32.,
            ControlSize::Large => 40.,
        };
        let density = match self.options.density {
            Density::Compact => 0.,
            Density::Regular => 4.,
            Density::Comfortable => 8.,
        };
        px(f32::from(self.options.font_size.saturating_sub(14))
            .mul_add(2., base + density))
    }
    pub fn control_padding(self, size: ControlSize) -> Pixels {
        self.space(match size {
            ControlSize::Small => 8.,
            ControlSize::Medium => 12.,
            ControlSize::Large => 16.,
        })
    }
    pub fn scrim(self) -> Rgba {
        rgb(0).opacity(if self.dark { 0.48 } else { 0.18 })
    }
    pub const fn scrim_blur(self) -> Pixels {
        px(4.)
    }
    pub const fn modal_radius(self) -> Pixels {
        px(20.)
    }
    pub fn modal_size(self, viewport: Size<Pixels>) -> Size<Pixels> {
        let scale = 2.0f32.mul_add(-MODAL_INSET, 1.);
        size(viewport.width * scale, viewport.height * scale)
    }
    pub fn control_colors(self, variant: ControlVariant) -> ControlColors {
        match variant {
            ControlVariant::Primary => ControlColors {
                background: self.fill(),
                foreground: self.on_fill(),
                border: self.fill(),
                hover: self.fill().opacity(0.9),
            },
            ControlVariant::Secondary => ControlColors {
                background: self.soft(),
                foreground: self.ink(),
                border: self.border_strong(),
                hover: self.line(),
            },
            ControlVariant::Ghost => ControlColors {
                background: self.card().opacity(0.),
                foreground: self.ink(),
                border: self.line().opacity(0.),
                hover: self.soft(),
            },
            ControlVariant::Danger => ControlColors {
                background: self.danger(),
                foreground: rgb(if self.dark { 0 } else { 0x00ff_ffff }),
                border: self.danger(),
                hover: self.danger().opacity(0.9),
            },
        }
    }
    pub fn auth_accent(self) -> Rgba {
        rgb(if self.dark { 0x0072_d8cb } else { 0x000f_766e })
    }
    pub fn auth_on_accent(self) -> Rgba {
        rgb(if self.dark { 0x0012_2724 } else { 0x00ff_ffff })
    }
    pub fn auth_brand_canvas(self) -> Rgba {
        rgb(if self.dark { 0x0024_2422 } else { 0x00e8_e6e1 })
    }
}

#[derive(Debug)]
pub struct ThemeState {
    resolved: ResolvedTheme,
}
impl ThemeState {
    pub const fn new(options: ThemeOptions, system_dark: bool) -> Self {
        Self {
            resolved: ResolvedTheme::resolve(options, system_dark),
        }
    }
    pub const fn resolved(&self) -> ResolvedTheme {
        self.resolved
    }
    pub fn update(
        &mut self,
        options: ThemeOptions,
        system_dark: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let next = ResolvedTheme::resolve(options, system_dark);
        if self.resolved != next {
            self.resolved = next;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contrast(a: Rgba, b: Rgba) -> f32 {
        fn luminance(color: Rgba) -> f32 {
            fn linear(v: f32) -> f32 {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            0.0722f32.mul_add(
                linear(color.color.blue),
                0.7152f32.mul_add(
                    linear(color.color.green),
                    0.2126 * linear(color.color.red),
                ),
            )
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn actual_color_pairs_are_readable_in_all_modes_and_accents() {
        for dark in [false, true] {
            for accent in Accent::ALL {
                let t = ResolvedTheme::resolve(
                    ThemeOptions {
                        accent,
                        ..ThemeOptions::default()
                    },
                    dark,
                );
                for v in [
                    ControlVariant::Primary,
                    ControlVariant::Secondary,
                    ControlVariant::Ghost,
                    ControlVariant::Danger,
                ] {
                    let c = t.control_colors(v);
                    let bg = if v == ControlVariant::Ghost {
                        t.card()
                    } else {
                        c.background
                    };
                    assert!(
                        contrast(c.foreground, bg) >= 4.5,
                        "{v:?} {accent:?} dark={dark}"
                    );
                }
                assert!(
                    contrast(t.accent(), t.on_accent()) >= 4.5,
                    "accent {accent:?}"
                );
                for bg in [t.card(), t.canvas(), t.dock(), t.soft()] {
                    assert!(contrast(t.muted(), bg) >= 4.5);
                    assert!(contrast(t.focus_ring(), bg) >= 3.);
                    assert!(contrast(t.border_strong(), bg) >= 3.);
                }
                assert!(contrast(t.composer_muted(), t.composer()) >= 4.5);
            }
        }
    }
    #[test]
    fn primary_and_danger_controls_have_visible_hover_feedback() {
        for variant in [ControlVariant::Primary, ControlVariant::Danger] {
            let colors = ResolvedTheme::default().control_colors(variant);
            assert_ne!(colors.hover, colors.background);
        }
    }

    #[test]
    fn window_snapshots_do_not_share_mutable_state() {
        let light = ThemeState::new(
            ThemeOptions {
                mode: ThemeMode::Light,
                ..ThemeOptions::default()
            },
            true,
        );
        let dark = ThemeState::new(
            ThemeOptions {
                mode: ThemeMode::Dark,
                accent: Accent::Orange,
                ..ThemeOptions::default()
            },
            false,
        );
        assert!(!light.resolved().dark);
        assert!(dark.resolved().dark);
        assert_ne!(light.resolved().ink(), dark.resolved().ink());
    }
    #[test]
    fn auto_and_explicit_modes_resolve_before_render() {
        for system in [false, true] {
            assert_eq!(
                ResolvedTheme::resolve(ThemeOptions::default(), system).dark,
                system
            );
            assert!(
                !ResolvedTheme::resolve(
                    ThemeOptions {
                        mode: ThemeMode::Light,
                        ..ThemeOptions::default()
                    },
                    system
                )
                .dark
            );
            assert!(
                ResolvedTheme::resolve(
                    ThemeOptions {
                        mode: ThemeMode::Dark,
                        ..ThemeOptions::default()
                    },
                    system
                )
                .dark
            );
        }
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use gpui::{
        AppContext, Entity, IntoElement, Render, Styled, Subscription,
        TestAppContext, Window, div,
    };
    struct WindowTheme {
        theme: Entity<ThemeState>,
        _subscription: Subscription,
    }
    impl Render for WindowTheme {
        fn render(
            &mut self,
            _window: &mut Window,
            cx: &mut Context<'_, Self>,
        ) -> impl IntoElement {
            div()
                .size_full()
                .bg(self.theme.read(cx).resolved().canvas())
        }
    }
    #[gpui::test]
    fn changing_one_window_theme_does_not_recolor_another(
        cx: &mut TestAppContext,
    ) {
        let make = |dark: bool, cx: &mut Context<'_, WindowTheme>| {
            let theme =
                cx.new(|_| ThemeState::new(ThemeOptions::default(), dark));
            let subscription = cx.observe(&theme, |_, _, cx| cx.notify());
            WindowTheme {
                theme,
                _subscription: subscription,
            }
        };
        let (first, first_cx) = cx.add_window_view(|_, cx| make(false, cx));
        first_cx.update(|window, cx| window.draw(cx).clear(cx));
        let first_theme =
            first_cx.read_entity(&first, |view, _| view.theme.clone());
        let _ = first_cx;
        let (second, second_cx) = cx.add_window_view(|_, cx| make(true, cx));
        second_cx.update(|window, cx| window.draw(cx).clear(cx));
        second_cx.update(|_, cx| {
            first_theme.update(cx, |theme, cx| {
                theme.update(
                    ThemeOptions {
                        mode: ThemeMode::Dark,
                        accent: Accent::Orange,
                        ..ThemeOptions::default()
                    },
                    false,
                    cx,
                );
            });
            assert_eq!(
                first_theme.read(cx).resolved().options.accent,
                Accent::Orange
            );
        });
        second_cx.read_entity(&second, |view, cx| {
            let theme = view.theme.read(cx).resolved();
            assert!(theme.dark);
            assert_eq!(theme.options.accent, Accent::Default);
        });
    }
}
