//! Motion tokens, Linger-inspired curves, arithmetic FLIP helpers, and
//! pre-measured height utilities shared across `flowy-app` and `flowy-studio`.
//!
//! Durations follow IBM Carbon's ladder for routine transitions plus tuned
//! windows for sidebar FLIP gliding, smooth height collapse, and conditional
//! breathing liveness.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::time::Duration;

use gpui::{App, Element, Entity};

use crate::{MotionPolicy, PrepaintBounds};

/// High-frequency state change, such as selecting a chip.
pub const FAST: Duration = Duration::from_millis(150);
/// Medium change, such as swapping the example cards.
pub const MODERATE: Duration = Duration::from_millis(240);
/// Large travel, such as opening or closing the deliverables drawer.
pub const SLOW: Duration = Duration::from_millis(320);
/// Sidebar row FLIP glide duration when items pin or reorder.
pub const FLIP: Duration = Duration::from_millis(280);
/// Pre-measured collapsible body expansion/collapse duration.
pub const AUTO_HEIGHT: Duration = Duration::from_millis(220);
/// Subtle breathing liveness cycle when hovered or actively running.
pub const PULSE: Duration = Duration::from_millis(2400);
/// Rolling pill counter arrival duration.
pub const COUNTER_ROLL: Duration = Duration::from_millis(180);

/// Entrance travel in pixels. Fading alone is invisible on a dark surface;
/// movement is what the eye actually catches.
pub const ENTER_OFFSET: f32 = 8.;

/// Scales positional travel for accessibility: returns `0.0` under reduced
/// motion so elements keep opacity feedback without spatial displacement.
#[must_use]
pub const fn travel_scale(policy: MotionPolicy) -> f32 {
    match policy {
        MotionPolicy::Reduced => 0.0,
        MotionPolicy::Full => 1.0,
    }
}

/// Carbon standard easing, `cubic-bezier(0.2, 0, 0.38, 0.9)`.
pub fn standard() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.2, 0.0, 0.38, 0.9)
}

/// Carbon entrance easing, `cubic-bezier(0, 0, 0.38, 0.9)`.
pub fn entrance() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.0, 0.0, 0.38, 0.9)
}

/// Carbon exit easing, `cubic-bezier(0.2, 0, 1, 0.9)`.
pub fn exit() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.2, 0.0, 1.0, 0.9)
}

/// Quintic ease-out (`1 - (1 - t)^5`): crisp launch with a feather-soft settle,
/// used for smooth collapsible height and drawer entrances.
pub fn out_quint() -> impl Fn(f32) -> f32 + 'static {
    move |t| {
        let u = 1.0 - t.clamp(0.0, 1.0);
        let u2 = u * u;
        (u2 * u2).mul_add(-u, 1.0)
    }
}

/// Linger-style sidebar FLIP glide curve, `cubic-bezier(0.22, 1.0, 0.36, 1.0)`.
pub fn flip_glide() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.22, 1.0, 0.36, 1.0)
}

/// Subtle damped arrival curve for pills and status counters,
/// `cubic-bezier(0.18, 0.89, 0.32, 1.08)`.
pub fn damped_pop() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.18, 0.89, 0.32, 1.08)
}

/// Smooth breathing wave `0 -> 1 -> 0` for conditional working/hover liveness.
pub fn pulsing_sine() -> impl Fn(f32) -> f32 + 'static {
    move |t| {
        let t = t.clamp(0.0, 1.0);
        0.5f32.mul_add(-(t * TAU).cos(), 0.5)
    }
}

/// Samples a CSS-style cubic-bezier curve: solves x for the requested `t`,
/// then evaluates y at that parameter.
pub fn cubic_bezier(
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
            let mid = f32::midpoint(low, high);
            if axis(mid, x1, x2) < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        axis(f32::midpoint(low, high), y1, y2)
    }
}

/// One axis of a cubic bezier whose endpoints are fixed at 0 and 1.
fn axis(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    (t * t).mul_add(t, (3.0 * u * t * t).mul_add(p2, 3.0 * u * u * t * p1))
}

/// Holds the curve at 0 for the first `share` of the run, then plays it. This
/// is how siblings stagger: GPUI's [`gpui::Animation`] has no delay field.
pub fn delayed(
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
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn stagger(index: u64, step_ms: u64, total: Duration) -> f32 {
    let delay = index as f32 * step_ms as f32;
    (delay / total.as_millis().max(1) as f32).min(0.75)
}

/// Pure arithmetic FLIP delta calculator for uniform-pitch lists.
///
/// When an item moves from index `old_idx` to `new_idx`, it starts offset by
/// `(old_idx - new_idx) * row_pitch` pixels and glides back to `0`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn compute_flip_deltas(
    prev_order: &[String],
    next_order: &[String],
    row_pitch: f32,
) -> HashMap<String, f32> {
    if prev_order.is_empty() || row_pitch <= 0.0 {
        return HashMap::new();
    }
    let prev_index: HashMap<&str, usize> = prev_order
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect();
    let mut deltas = HashMap::new();
    for (new_idx, id) in next_order.iter().enumerate() {
        if let Some(&old_idx) = prev_index.get(id.as_str())
            && old_idx != new_idx
        {
            let delta = (old_idx as f32 - new_idx as f32) * row_pitch;
            deltas.insert(id.clone(), delta);
        }
    }
    deltas
}

/// Stateful FLIP tracker suitable for `window.use_keyed_state(...)` in GPUI.
#[derive(Clone, Debug, Default)]
pub struct FlipState {
    last_order: Vec<String>,
    deltas: HashMap<String, f32>,
    epoch: u64,
}

impl FlipState {
    /// Updates the tracked order. When any surviving item shifts index, a new
    /// animation epoch starts with each moved item's vertical starting offset.
    pub fn reconcile(&mut self, next_order: &[String], row_pitch: f32) {
        if self.last_order == next_order {
            return;
        }
        let deltas =
            compute_flip_deltas(&self.last_order, next_order, row_pitch);
        self.last_order.clear();
        self.last_order.extend(next_order.iter().cloned());
        if deltas.is_empty() {
            self.deltas.clear();
        } else {
            self.epoch = self.epoch.wrapping_add(1);
            self.deltas = deltas;
        }
    }

    /// Returns `(epoch, starting_offset_px)` if `id` moved in the latest reorder.
    #[must_use]
    pub fn offset_for(&self, id: &str) -> Option<(u64, f32)> {
        self.deltas
            .get(id)
            .copied()
            .map(|delta| (self.epoch, delta))
    }
}

/// Records an element's natural laid-out height into `measured` after prepaint
/// without triggering reactive redraw loops.
pub fn measure_height<E: Element>(
    element: E,
    measured: Entity<f32>,
) -> PrepaintBounds<E> {
    let weak = measured.downgrade();
    PrepaintBounds::new(element, move |bounds, cx: &mut App| {
        let height = f32::from(bounds.size.height);
        if height <= 0.5 {
            return;
        }
        let _ = weak.update(cx, |stored, _| {
            if (*stored - height).abs() > 0.5 {
                *stored = height;
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_the_carbon_ladder() {
        assert_eq!(FAST.as_millis(), 150);
        assert_eq!(MODERATE.as_millis(), 240);
        assert_eq!(SLOW.as_millis(), 320);
        assert!(SLOW.as_millis() < 800);
        assert!(FLIP.as_millis() < 400);
        assert!(AUTO_HEIGHT.as_millis() < 400);
    }

    fn assert_endpoints(curve: impl Fn(f32) -> f32) {
        assert!(curve(0.0).abs() < 0.01, "start must be 0");
        assert!((curve(1.0) - 1.0).abs() < 0.01, "end must be 1");
    }

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        assert_endpoints(standard());
        assert_endpoints(entrance());
        assert_endpoints(exit());
        assert_endpoints(out_quint());
        assert_endpoints(flip_glide());
        assert_endpoints(damped_pop());
    }

    fn assert_monotonic(curve: impl Fn(f32) -> f32) {
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

    #[test]
    fn curves_are_monotonic() {
        assert_monotonic(cubic_bezier(0.2, 0.0, 0.38, 0.9));
        assert_monotonic(out_quint());
        assert_monotonic(flip_glide());
    }

    #[test]
    fn pulsing_sine_peaks_midway_and_returns_to_zero() {
        let wave = pulsing_sine();
        assert!(wave(0.0).abs() < 0.001);
        assert!((wave(0.5) - 1.0).abs() < 0.001);
        assert!(wave(1.0).abs() < 0.001);
    }

    #[test]
    fn travel_scale_clamps_under_reduced_motion() {
        assert!(
            (travel_scale(MotionPolicy::Reduced) - 0.0).abs() < f32::EPSILON
        );
        assert!((travel_scale(MotionPolicy::Full) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn flip_deltas_capture_reordered_rows_and_ignore_unchanged() {
        let prev = vec!["a".into(), "b".into(), "c".into()];
        let next = vec!["c".into(), "a".into(), "b".into()];
        let deltas = compute_flip_deltas(&prev, &next, 36.0);
        assert!((deltas["c"] - 72.0).abs() < 0.01);
        assert!((deltas["a"] + 36.0).abs() < 0.01);
        assert!((deltas["b"] + 36.0).abs() < 0.01);

        let mut state = FlipState::default();
        state.reconcile(&prev, 36.0);
        assert_eq!(state.offset_for("a"), None);
        state.reconcile(&next, 36.0);
        let (epoch, delta) = state.offset_for("c").expect("c moved");
        assert_eq!(epoch, 1);
        assert!((delta - 72.0).abs() < 0.01);
        // Reconciling the same order again does not bump epoch.
        state.reconcile(&next, 36.0);
        assert_eq!(state.offset_for("c").map(|(e, _)| e), Some(1));
    }
}
