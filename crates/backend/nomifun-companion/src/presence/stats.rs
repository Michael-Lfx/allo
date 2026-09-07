//! Needs decay and CalMode-style mood derivation.

use nomifun_api_types::{CompanionMood, PresenceStats};

/// Energy / affection drop and boredom rise per real hour at rate 1.0.
const ENERGY_PER_HOUR: f64 = 8.0;
const AFFECTION_PER_HOUR: f64 = 3.0;
const BOREDOM_PER_HOUR: f64 = 10.0;

pub fn tick_stats(
    stats: PresenceStats,
    dt_secs: f64,
    quiet: bool,
    now_ms: i64,
    last_interact_ms: Option<i64>,
) -> PresenceStats {
    let hours = (dt_secs.max(0.0)) / 3600.0;
    if hours == 0.0 {
        return stats.clamp();
    }
    let idle_boost = match last_interact_ms {
        Some(ts) if now_ms.saturating_sub(ts) > 30 * 60 * 1000 => 1.35,
        None => 1.2,
        Some(_) => 1.0,
    };
    let rate = if quiet { 0.45 } else { 1.0 } * idle_boost;
    PresenceStats {
        energy: stats.energy - ENERGY_PER_HOUR * hours * rate,
        affection: stats.affection - AFFECTION_PER_HOUR * hours * rate,
        boredom: stats.boredom + BOREDOM_PER_HOUR * hours * rate,
    }
    .clamp()
}

/// Stats → mood. Recent interaction can lift content into excited/happy.
pub fn derive_mood(
    stats: &PresenceStats,
    last_interact_ms: Option<i64>,
    now_ms: i64,
) -> CompanionMood {
    let recent = last_interact_ms
        .map(|ts| now_ms.saturating_sub(ts) < 2 * 60 * 1000)
        .unwrap_or(false);
    if stats.energy < 22.0 {
        return CompanionMood::Sleepy;
    }
    if stats.affection < 28.0 && stats.boredom > 55.0 {
        return CompanionMood::Worried;
    }
    if stats.affection > 78.0 && stats.boredom < 38.0 && stats.energy > 42.0 {
        if recent {
            return CompanionMood::Excited;
        }
        return CompanionMood::Happy;
    }
    if stats.affection > 68.0 && stats.energy > 50.0 && stats.boredom < 45.0 {
        return CompanionMood::Happy;
    }
    CompanionMood::Content
}

/// Representative stats that [`derive_mood`] maps back to `mood`. Used when a
/// companion has a stored mood word but no presence record yet (boot seed).
pub fn stats_matching_mood(mood: CompanionMood) -> PresenceStats {
    match mood {
        CompanionMood::Sleepy => PresenceStats {
            energy: 16.0,
            affection: 50.0,
            boredom: 30.0,
        },
        CompanionMood::Worried => PresenceStats {
            energy: 50.0,
            affection: 20.0,
            boredom: 70.0,
        },
        CompanionMood::Happy => PresenceStats {
            energy: 72.0,
            affection: 76.0,
            boredom: 20.0,
        },
        CompanionMood::Excited => PresenceStats {
            energy: 85.0,
            affection: 86.0,
            boredom: 12.0,
        },
        CompanionMood::Content => PresenceStats::default(),
    }
}

/// Learner diary mood is a hint: nudge stats, then derive_mood owns the word.
pub fn nudge_from_learn_mood(stats: PresenceStats, mood_word: &str) -> PresenceStats {
    let mood = CompanionMood::parse_loose(mood_word);
    let nudged = match mood {
        CompanionMood::Happy => PresenceStats {
            affection: stats.affection + 8.0,
            boredom: stats.boredom - 8.0,
            energy: stats.energy + 2.0,
        },
        CompanionMood::Excited => PresenceStats {
            affection: stats.affection + 10.0,
            boredom: stats.boredom - 12.0,
            energy: stats.energy - 2.0,
        },
        CompanionMood::Sleepy => PresenceStats {
            energy: stats.energy - 10.0,
            boredom: stats.boredom + 4.0,
            affection: stats.affection,
        },
        CompanionMood::Worried => PresenceStats {
            affection: stats.affection - 6.0,
            boredom: stats.boredom + 6.0,
            energy: stats.energy - 3.0,
        },
        CompanionMood::Content => PresenceStats {
            boredom: stats.boredom - 3.0,
            ..stats
        },
    };
    nudged.clamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_idle_lowers_energy_and_raises_boredom() {
        let next = tick_stats(PresenceStats::default(), 3600.0, false, 3_600_000, None);
        assert!(next.energy < PresenceStats::default().energy);
        assert!(next.boredom > PresenceStats::default().boredom);
    }

    #[test]
    fn quiet_hours_decay_slower() {
        let loud = tick_stats(PresenceStats::default(), 3600.0, false, 1, Some(1));
        let quiet = tick_stats(PresenceStats::default(), 3600.0, true, 1, Some(1));
        assert!(loud.energy < quiet.energy);
    }

    #[test]
    fn low_energy_is_sleepy() {
        let stats = PresenceStats {
            energy: 10.0,
            affection: 80.0,
            boredom: 10.0,
        };
        assert_eq!(derive_mood(&stats, None, 0), CompanionMood::Sleepy);
    }

    #[test]
    fn learn_happy_nudges_affection() {
        let before = PresenceStats::default();
        let after = nudge_from_learn_mood(before, "proud");
        assert!(after.affection > before.affection);
    }

    #[test]
    fn stats_matching_mood_round_trips() {
        for mood in [
            CompanionMood::Happy,
            CompanionMood::Content,
            CompanionMood::Sleepy,
            CompanionMood::Worried,
        ] {
            let stats = stats_matching_mood(mood);
            assert_eq!(derive_mood(&stats, None, 0), mood);
        }
        let stats = stats_matching_mood(CompanionMood::Excited);
        assert_eq!(derive_mood(&stats, Some(1), 1), CompanionMood::Excited);
    }
}
