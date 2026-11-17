//! Local Lucide icons used by the GPUI renderer.
//!
//! The SVG sources are vendored under `assets/icons` so icon rendering stays
//! deterministic and does not depend on a network request at runtime.

use gpui::{Styled, Svg, px, svg};

fn icon(data: &'static [u8]) -> Svg {
    svg().data(data).size(px(16.)).text_color(crate::ink())
}

macro_rules! lucide_icons {
    ($($name:ident => $file:literal),+ $(,)?) => {
        $(
            pub fn $name() -> Svg {
                icon(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/icons/",
                    $file
                )))
            }
        )+
    };
}

lucide_icons! {
    arrow_left => "arrow-left.svg",
    arrow_right => "arrow-right.svg",
    arrow_up_right => "arrow-up-right.svg",
    bell => "bell.svg",
    book => "book.svg",
    check => "check.svg",
    chevron_down => "chevron-down.svg",
    chevron_left => "chevron-left.svg",
    chevron_right => "chevron-right.svg",
    chevron_up => "chevron-up.svg",
    chevrons_up_down => "chevrons-up-down.svg",
    circle_alert => "circle-exclamation-point.svg",
    circle_help => "circle-question-mark.svg",
    clock => "clock.svg",
    ellipsis => "ellipsis.svg",
    eye => "eye.svg",
    eye_off => "eye-off.svg",
    file_plus => "file-plus.svg",
    folder => "folder.svg",
    folder_plus => "folder-plus.svg",
    hash => "hash.svg",
    house => "house.svg",
    infinity => "infinity.svg",
    info => "info.svg",
    layout_grid => "layout-grid.svg",
    library => "library.svg",
    list_check => "list-check.svg",
    loader_circle => "loader-circle.svg",
    log_out => "log-out.svg",
    mail => "mail.svg",
    maximize => "maximize.svg",
    monitor => "monitor.svg",
    moon => "moon.svg",
    play => "play.svg",
    plus => "plus.svg",
    puzzle => "puzzle.svg",
    refresh_cw => "refresh-cw.svg",
    search => "search.svg",
    send => "send.svg",
    sliders_horizontal => "sliders-horizontal.svg",
    sparkles => "sparkles.svg",
    square => "square.svg",
    square_pen => "square-pen.svg",
    star => "star.svg",
    sun => "sun.svg",
    user => "user.svg",
    volume_2 => "volume-2.svg",
    wand_sparkles => "wand-sparkles.svg",
    workflow => "workflow.svg",
    x => "x.svg",
}
