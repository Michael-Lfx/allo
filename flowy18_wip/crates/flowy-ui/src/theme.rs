use crate::{Accent, Density};
use gpui::{Pixels, Rgba, px, rgb};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub const DOCK_WIDTH: f32 = 248.0;

static DARK: AtomicBool = AtomicBool::new(false);
static ACCENT: AtomicU8 = AtomicU8::new(0);
static DENSITY: AtomicU8 = AtomicU8::new(0);

pub fn install(dark: bool, accent: Accent, density: Density) {
    DARK.store(dark, Ordering::Relaxed);
    ACCENT.store(accent as u8, Ordering::Relaxed);
    DENSITY.store(density as u8, Ordering::Relaxed);
}

pub fn canvas() -> Rgba {
    if dark() {
        rgb(0x001c_1c1a)
    } else {
        rgb(0x00f4_f3ef)
    }
}

pub fn dock() -> Rgba {
    if dark() {
        rgb(0x0024_2422)
    } else {
        rgb(0x00f7_f6f3)
    }
}

pub fn card() -> Rgba {
    if dark() {
        rgb(0x002c_2c2a)
    } else {
        rgb(0x00ff_ffff)
    }
}

pub fn ink() -> Rgba {
    if dark() {
        rgb(0x00f4_f3ef)
    } else {
        rgb(0x001c_1c1c)
    }
}

pub fn muted() -> Rgba {
    if dark() {
        rgb(0x00a8_a49c)
    } else {
        rgb(0x008a_877f)
    }
}

pub fn line() -> Rgba {
    if dark() {
        rgb(0x003e_3e3a)
    } else {
        rgb(0x00e6_e2da)
    }
}

pub fn soft() -> Rgba {
    if dark() {
        rgb(0x0034_3432)
    } else {
        rgb(0x00ef_eee9)
    }
}

pub fn danger() -> Rgba {
    if dark() {
        rgb(0x00e0_7070)
    } else {
        rgb(0x009f_2d2d)
    }
}

pub fn on_ink() -> Rgba {
    if dark() {
        rgb(0x001c_1c1a)
    } else {
        rgb(0x00ff_ffff)
    }
}

pub fn accent() -> Rgba {
    match current_accent() {
        Accent::Default => rgb(0x002f_6bff),
        other => swatch(other),
    }
}

pub fn fill() -> Rgba {
    let accent = current_accent();
    if accent == Accent::Default {
        ink()
    } else {
        swatch(accent)
    }
}

pub fn on_fill() -> Rgba {
    if current_accent() == Accent::Default {
        on_ink()
    } else {
        rgb(0x00ff_ffff)
    }
}

pub fn swatch(accent: Accent) -> Rgba {
    match accent {
        Accent::Default => muted(),
        Accent::Blue => rgb(0x002f_6bff),
        Accent::Green => rgb(0x001f_8a4c),
        Accent::Orange => rgb(0x00d4_6b08),
        Accent::Red => rgb(0x00c4_3737),
        Accent::Purple => rgb(0x006b_46c1),
    }
}

pub fn space(base: f32) -> Pixels {
    let scale = match current_density() {
        Density::Compact => 1.0,
        Density::Regular => 1.25,
        Density::Comfortable => 1.5,
    };
    px(base * scale)
}

fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

fn current_accent() -> Accent {
    match ACCENT.load(Ordering::Relaxed) {
        1 => Accent::Blue,
        2 => Accent::Green,
        3 => Accent::Orange,
        4 => Accent::Red,
        5 => Accent::Purple,
        _ => Accent::Default,
    }
}

fn current_density() -> Density {
    match DENSITY.load(Ordering::Relaxed) {
        1 => Density::Regular,
        2 => Density::Comfortable,
        _ => Density::Compact,
    }
}
