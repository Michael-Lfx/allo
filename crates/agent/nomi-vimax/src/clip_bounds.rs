//! Per-clip duration window of the selected video model.
//!
//! A "clip" is one generated shot. Every model accepts its own `[min, max]`
//! seconds window (Seedance 2.0 and MiniMax-H3 both top out at 15s today; a
//! future model may accept tens of seconds). Planning must size shots inside
//! that window instead of hardcoding one vendor's numbers, so the window travels
//! with the session as a value.
//!
//! Lookup by model id lives in [`crate::video_quality`]; this module only owns
//! the type and its invariants.

/// Duration window one video model accepts for a single clip, plus the beat
/// length short drama aims for inside that window.
///
/// Invariants (enforced by [`ClipBounds::new`]):
/// `1 <= min_secs <= preferred_min_secs <= typical_beat_secs <= preferred_max_secs
/// <= max_secs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipBounds {
    /// Shortest clip the model accepts (and the shortest we bill).
    min_secs: u32,
    /// Longest clip the model accepts.
    max_secs: u32,
    /// Lower end of the short-drama beat length.
    preferred_min_secs: u32,
    /// Upper end of the short-drama beat length; above this a clip needs a
    /// reason (long spoken line, continuous camera move).
    preferred_max_secs: u32,
    /// Beat length planning assumes when it has to guess before seeing content
    /// (shot-count hints). Sits in the lower part of the beat window.
    typical_beat_secs: u32,
}

/// `u32::clamp` is not `const`; this keeps [`ClipBounds::new`] usable in consts.
const fn clamp_u32(v: u32, lo: u32, hi: u32) -> u32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

impl ClipBounds {
    /// A glance, a line, or one action — the beat length short drama lands on
    /// regardless of how high the model's ceiling is. 5–15s is what every
    /// mainstream video model covers today, so planning is free to use the whole
    /// range; a higher model ceiling only buys headroom beyond it (very long
    /// speech, a merged same-camera run).
    const BEAT_MIN_SECS: u32 = 5;
    const BEAT_MAX_SECS: u32 = 15;
    /// Everyday short-drama clip: rich enough for 2–3 related beats, short
    /// enough that spoken lines still finish inside a 15s model ceiling.
    const DRAMA_BEAT_SECS: u32 = 12;
    /// Pack related beats toward this length when speech and motion actually
    /// need it. Not a pad — empty holds stay forbidden.
    const DRAMA_PACK_SECS: u32 = 14;

    /// Window used when the model id matches no known family. 5–12s is accepted
    /// by every currently integrated model, so an unknown model cannot be
    /// rejected for asking too long a clip.
    pub const DEFAULT: Self = Self::new(5, 12);

    /// Build a window, repairing an inverted or zero range instead of panicking:
    /// these numbers come from model-id heuristics, not from user input.
    pub const fn new(min_secs: u32, max_secs: u32) -> Self {
        let min_secs = if min_secs == 0 { 1 } else { min_secs };
        let max_secs = if max_secs < min_secs { min_secs } else { max_secs };
        let preferred_min_secs = clamp_u32(Self::BEAT_MIN_SECS, min_secs, max_secs);
        let preferred_max_secs = clamp_u32(Self::BEAT_MAX_SECS, min_secs, max_secs);
        Self {
            min_secs,
            max_secs,
            preferred_min_secs,
            preferred_max_secs,
            // Guess made before content exists: pack toward a 12s drama beat
            // so leftover seconds become longer clips, not extra splices.
            typical_beat_secs: clamp_u32(
                Self::DRAMA_BEAT_SECS,
                preferred_min_secs,
                preferred_max_secs,
            ),
        }
    }

    pub const fn min_secs(&self) -> u32 {
        self.min_secs
    }

    pub const fn max_secs(&self) -> u32 {
        self.max_secs
    }

    pub const fn preferred_min_secs(&self) -> u32 {
        self.preferred_min_secs
    }

    pub const fn preferred_max_secs(&self) -> u32 {
        self.preferred_max_secs
    }

    /// Beat length to assume when planning has no content to price yet.
    pub const fn typical_beat_secs(&self) -> u32 {
        self.typical_beat_secs
    }

    /// Length a packed multi-beat clip should aim for when speech still fits.
    pub const fn pack_target_secs(&self) -> u32 {
        clamp_u32(
            Self::DRAMA_PACK_SECS,
            self.typical_beat_secs,
            self.preferred_max_secs,
        )
    }

    /// A single glance / sit / stand — stays short so packing does not inflate
    /// one-event clips toward the drama beat.
    pub const fn glance_secs(&self) -> u32 {
        clamp_u32(
            self.preferred_min_secs.saturating_add(2),
            self.min_secs,
            self.typical_beat_secs,
        )
    }

    /// Clamp a requested clip length into the model's accepted window.
    pub const fn clamp_secs(&self, secs: u32) -> u32 {
        clamp_u32(secs, self.min_secs, self.max_secs)
    }

    /// Add `extra` seconds without leaving the window.
    pub const fn saturating_add_within(&self, secs: u32, extra: u32) -> u32 {
        self.clamp_secs(secs.saturating_add(extra))
    }
}

impl Default for ClipBounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5–15s is what every mainstream model covers, so a model that accepts it
    /// gets the whole range to plan in.
    #[test]
    fn derives_beat_window_inside_the_model_window() {
        let wide = ClipBounds::new(5, 15);
        assert_eq!((wide.min_secs(), wide.max_secs()), (5, 15));
        assert_eq!(
            (wide.preferred_min_secs(), wide.preferred_max_secs()),
            (5, 15)
        );
        // Guesses pack toward a 12s drama beat, not the low end of the window.
        assert_eq!(wide.typical_beat_secs(), 12);
        assert_eq!(wide.pack_target_secs(), 14);
        assert_eq!(wide.glance_secs(), 7);

        // A higher ceiling is headroom, not the everyday beat length.
        let long_model = ClipBounds::new(5, 60);
        assert_eq!(
            (
                long_model.preferred_min_secs(),
                long_model.preferred_max_secs()
            ),
            (5, 15)
        );
        assert_eq!(long_model.typical_beat_secs(), 12);
        assert_eq!(long_model.pack_target_secs(), 14);
    }

    #[test]
    fn narrow_window_pulls_the_beat_window_inward() {
        let narrow = ClipBounds::new(4, 8);
        assert_eq!(
            (narrow.preferred_min_secs(), narrow.preferred_max_secs()),
            (5, 8)
        );
        assert_eq!(narrow.typical_beat_secs(), 8);
        assert_eq!(narrow.pack_target_secs(), 8);
        assert_eq!(narrow.glance_secs(), 7);

        let long_only = ClipBounds::new(20, 30);
        assert_eq!(
            (
                long_only.preferred_min_secs(),
                long_only.preferred_max_secs()
            ),
            (20, 20)
        );
        assert_eq!(long_only.typical_beat_secs(), 20);
    }

    #[test]
    fn repairs_degenerate_ranges() {
        let inverted = ClipBounds::new(12, 5);
        assert_eq!((inverted.min_secs(), inverted.max_secs()), (12, 12));

        let zero_min = ClipBounds::new(0, 6);
        assert_eq!(zero_min.min_secs(), 1);
    }

    #[test]
    fn clamps_into_the_window() {
        let b = ClipBounds::new(5, 15);
        assert_eq!(b.clamp_secs(1), 5);
        assert_eq!(b.clamp_secs(9), 9);
        assert_eq!(b.clamp_secs(99), 15);
        assert_eq!(b.saturating_add_within(14, 4), 15);
        assert_eq!(b.saturating_add_within(u32::MAX, 4), 15);
    }
}
