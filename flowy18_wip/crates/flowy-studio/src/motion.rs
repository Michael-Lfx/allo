//! Motion tokens: the single source of durations and easing for the studio UI.
//!
//! Durations follow IBM Carbon's ladder; the curves are Carbon's standard,
//! entrance and exit cubic-beziers, sampled here so the code runs exactly the
//! curve the design document names instead of an approximate built-in.
//!
//! `FAST` and `SLOW` are declared before their call sites exist: chip selection
//! still switches instantly and the deliverables drawer is not built yet. They
//! stay so this file matches the design document one-to-one.
#![allow(dead_code)]

use std::time::Duration;

/// High-frequency state change, such as selecting a chip.
pub(crate) const FAST: Duration = Duration::from_millis(150);
/// Medium change, such as swapping the example cards.
pub(crate) const MODERATE: Duration = Duration::from_millis(240);
/// Large travel, such as opening or closing the deliverables drawer.
pub(crate) const SLOW: Duration = Duration::from_millis(320);

/// Entrance travel in pixels. Fading alone is invisible on a dark surface;
/// movement is what the eye actually catches.
pub(crate) const ENTER_OFFSET: f32 = 8.;

/// Carbon standard easing, `cubic-bezier(0.2, 0, 0.38, 0.9)`.
pub(crate) fn standard() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.2, 0.0, 0.38, 0.9)
}

/// Carbon entrance easing, `cubic-bezier(0, 0, 0.38, 0.9)`.
pub(crate) fn entrance() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.0, 0.0, 0.38, 0.9)
}

/// Carbon exit easing, `cubic-bezier(0.2, 0, 1, 0.9)`.
pub(crate) fn exit() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.2, 0.0, 1.0, 0.9)
}

/// Samples a CSS-style cubic-bezier curve: solves x for the requested `t`,
/// then evaluates y at that parameter.
pub(crate) fn cubic_bezier(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
) -> impl Fn(f32) -> f32 + 'static {
    move |t| {
        let t = t.clamp(0.0, 1.0);
        let mut low = 0.0;
        let mut high = 1.0;
        // 12 bisection steps resolve the parameter to about 1/4096.
        for _ in 0..12 {
            let mid = (low + high) * 0.5;
            if axis(mid, x1, x2) < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        axis((low + high) * 0.5, y1, y2)
    }
}

/// One axis of a cubic bezier whose endpoints are fixed at 0 and 1.
fn axis(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

/// Holds the curve at 0 for the first `share` of the run, then plays it. This
/// is how siblings stagger: GPUI's [`gpui::Animation`] has no delay field.
pub(crate) fn delayed(
    share: f32,
    ease: impl Fn(f32) -> f32 + 'static,
) -> impl Fn(f32) -> f32 + 'static {
    let share = share.clamp(0.0, 0.9);
    move |t| {
        if t <= share {
            0.0
        } else {
            ease((t - share) / (1.0 - share))
        }
    }
}

/// Delay share for the `index`-th sibling, expressed in milliseconds against
/// the run's total so callers do not hand-tune fractions.
pub(crate) fn stagger(index: u64, step_ms: u64, total: Duration) -> f32 {
    let delay = index as f32 * step_ms as f32;
    (delay / total.as_millis().max(1) as f32).min(0.75)
}

#[cfg(test)]
mod tests {
    use super::{FAST, MODERATE, SLOW, cubic_bezier, entrance, exit, standard};

    #[test]
    fn durations_follow_the_carbon_ladder() {
        assert_eq!(FAST.as_millis(), 150);
        assert_eq!(MODERATE.as_millis(), 240);
        assert_eq!(SLOW.as_millis(), 320);
        // Nothing routine may reach the 800ms ceiling.
        assert!(SLOW.as_millis() < 800);
    }

    fn assert_endpoints(curve: impl Fn(f32) -> f32) {
        assert!(curve(0.0).abs() < 0.01, "start must be 0");
        assert!((curve(1.0) - 1.0).abs() < 0.01, "end must be 1");
    }

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        // Each curve returns its own opaque type, so they are checked one by one.
        assert_endpoints(standard());
        assert_endpoints(entrance());
        assert_endpoints(exit());
    }

    #[test]
    fn curves_are_monotonic() {
        let curve = cubic_bezier(0.2, 0.0, 0.38, 0.9);
        let mut previous = curve(0.0);
        for step in 1..=20 {
            let value = curve(step as f32 / 20.0);
            assert!(
                value + 0.001 >= previous,
                "curve must not go backwards at {step}"
            );
            previous = value;
        }
    }
}
