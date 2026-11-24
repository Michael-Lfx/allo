//! Horizontal card row: shared paging geometry and the clipped track.
//!
//! The paging state lives with the caller so it survives re-renders and can be
//! reset when the row's contents change; this module owns the arithmetic so
//! every carousel in the app clamps and pages identically.

use gpui::{AnyElement, IntoElement, ParentElement, Pixels, Styled, div, px};

/// Paging state for a horizontal card row.
///
/// `offset` is 0 at the start and grows negative as the row scrolls right.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarouselState {
    offset: f32,
}

impl CarouselState {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn offset(&self) -> f32 {
        self.offset
    }

    /// Distance the row can travel: content width minus the visible width.
    pub fn max_offset(total_w: f32, visible_w: f32) -> f32 {
        (total_w - visible_w).max(0.)
    }

    pub fn can_prev(&self) -> bool {
        self.offset < -0.5
    }

    pub fn can_next(&self, total_w: f32, visible_w: f32) -> bool {
        self.offset > -Self::max_offset(total_w, visible_w) + 0.5
    }

    pub fn prev(&mut self, page: f32) {
        self.offset = (self.offset + page).min(0.);
    }

    pub fn next(&mut self, page: f32, total_w: f32, visible_w: f32) {
        let floor = -Self::max_offset(total_w, visible_w);
        self.offset = (self.offset - page).max(floor);
    }

    pub const fn reset(&mut self) {
        self.offset = 0.;
    }
}

/// Geometry of a row of fixed-width cards.
///
/// `page_width` and `needs_paging` take the row's *stage* width: the design
/// ceiling the row was laid out for, not a measured one. GPUI's
/// `viewport_size` keeps reporting the launch size after a resize, so callers
/// cannot pass a live width; a viewport narrower than the stage therefore
/// scrolls less far than it physically could.
#[derive(Clone, Copy, Debug)]
pub struct CarouselGeometry {
    pub item_w: f32,
    pub gap: f32,
    pub count: usize,
}

impl CarouselGeometry {
    pub fn total_width(&self) -> f32 {
        if self.count == 0 {
            0.
        } else {
            (self.count as f32).mul_add(self.item_w + self.gap, -self.gap)
        }
    }

    /// One page is the whole cards that fit, never less than a single card.
    pub fn page_width(&self, visible_w: f32) -> f32 {
        let per_page = ((visible_w + self.gap) / (self.item_w + self.gap))
            .floor()
            .max(1.);
        per_page.mul_add(self.item_w + self.gap, -self.gap).max(self.item_w)
    }

    pub fn needs_paging(&self, visible_w: f32) -> bool {
        self.total_width() > visible_w + 0.5
    }
}

/// The clipped track that shifts with `state`.
pub fn carousel_track(
    state: &CarouselState,
    gap: Pixels,
    items: Vec<AnyElement>,
) -> impl IntoElement {
    div().w_full().overflow_hidden().child(
        div()
            .flex()
            .gap(gap)
            .flex_none()
            .relative()
            .left(px(state.offset()))
            .children(items),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_width_adds_gaps_between_cards_only() {
        let g = CarouselGeometry {
            item_w: 220.,
            gap: 12.,
            count: 4,
        };
        assert_eq!(g.total_width(), 220. * 4. + 12. * 3.);
    }

    #[test]
    fn empty_row_has_no_width() {
        let g = CarouselGeometry {
            item_w: 220.,
            gap: 12.,
            count: 0,
        };
        assert_eq!(g.total_width(), 0.);
    }

    #[test]
    fn next_clamps_at_the_end() {
        let mut s = CarouselState::new();
        s.next(660., 916., 880.);
        assert_eq!(s.offset(), -36.);
        assert!(!s.can_next(916., 880.));
    }

    #[test]
    fn prev_never_passes_the_start() {
        let mut s = CarouselState::new();
        s.prev(660.);
        assert_eq!(s.offset(), 0.);
        assert!(!s.can_prev());
    }

    #[test]
    fn page_width_counts_whole_cards() {
        let g = CarouselGeometry {
            item_w: 220.,
            gap: 12.,
            count: 4,
        };
        assert_eq!(g.page_width(700.), 220. * 3. + 12. * 2.);
        assert_eq!(g.page_width(200.), 220.);
    }

    #[test]
    fn needs_paging_only_when_content_overflows() {
        let g = CarouselGeometry {
            item_w: 220.,
            gap: 12.,
            count: 4,
        };
        assert!(g.needs_paging(880.));
        assert!(!g.needs_paging(1000.));
    }
}
