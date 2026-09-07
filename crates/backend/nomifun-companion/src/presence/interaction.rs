//! ABC interaction FSM. Stats deltas live here so the table stays in one place.

use nomifun_api_types::{
    clip_for_phase, HitIntent, InteractionPhase, PresencePointerEvent, PresenceStats,
};

use super::PresenceRecord;

const STALE_INTERACTION_MS: i64 = 8_000;
const C_END_HOLD_MS: i64 = 450;

pub fn apply_pointer(
    mut record: PresenceRecord,
    intent: HitIntent,
    event: PresencePointerEvent,
    now_ms: i64,
) -> PresenceRecord {
    if let Some(current) = record.interaction_intent
        && current != intent
        && !matches!(record.interaction, InteractionPhase::Idle)
    {
        record = apply_pointer(record, current, PresencePointerEvent::Up, now_ms);
    }
    match (record.interaction, event) {
        (InteractionPhase::Idle, PresencePointerEvent::Down)
        | (InteractionPhase::CEnd, PresencePointerEvent::Down) => {
            record.stats = apply_intent_delta(record.stats, intent);
            record.interaction = InteractionPhase::AStart;
            record.interaction_intent = Some(intent);
            record.last_interact_ms = Some(now_ms);
            record.active_clip = clip_for_phase(intent, InteractionPhase::AStart);
        }
        (InteractionPhase::AStart, PresencePointerEvent::Hold)
        | (InteractionPhase::BLoop, PresencePointerEvent::Hold) => {
            record.interaction = InteractionPhase::BLoop;
            record.interaction_intent = Some(intent);
            record.last_interact_ms = Some(now_ms);
            record.active_clip = clip_for_phase(intent, InteractionPhase::BLoop);
        }
        (InteractionPhase::AStart, PresencePointerEvent::Up)
        | (InteractionPhase::BLoop, PresencePointerEvent::Up) => {
            record.interaction = InteractionPhase::CEnd;
            record.interaction_intent = Some(intent);
            record.last_interact_ms = Some(now_ms);
            record.active_clip = clip_for_phase(intent, InteractionPhase::CEnd);
        }
        (InteractionPhase::CEnd, PresencePointerEvent::Hold) => {}
        (InteractionPhase::Idle, PresencePointerEvent::Hold | PresencePointerEvent::Up) => {}
        (InteractionPhase::CEnd, PresencePointerEvent::Up) => {}
        (InteractionPhase::AStart | InteractionPhase::BLoop, PresencePointerEvent::Down) => {
            record.last_interact_ms = Some(now_ms);
        }
    }
    record
}

pub fn expire_stale_interaction(mut record: PresenceRecord, now_ms: i64) -> PresenceRecord {
    let Some(last) = record.last_interact_ms else {
        return record;
    };
    let age = now_ms.saturating_sub(last);
    match record.interaction {
        InteractionPhase::AStart | InteractionPhase::BLoop if age >= STALE_INTERACTION_MS => {
            if let Some(intent) = record.interaction_intent {
                record = apply_pointer(record, intent, PresencePointerEvent::Up, now_ms);
            }
        }
        InteractionPhase::CEnd if age >= C_END_HOLD_MS => {
            record.interaction = InteractionPhase::Idle;
            record.interaction_intent = None;
            record.active_clip = Some("idle".into());
        }
        InteractionPhase::Idle
        | InteractionPhase::AStart
        | InteractionPhase::BLoop
        | InteractionPhase::CEnd => {}
    }
    record
}

fn apply_intent_delta(stats: PresenceStats, intent: HitIntent) -> PresenceStats {
    match intent {
        HitIntent::Head => PresenceStats {
            affection: stats.affection + 4.0,
            boredom: stats.boredom - 6.0,
            energy: stats.energy - 0.5,
        },
        HitIntent::Body => PresenceStats {
            affection: stats.affection + 2.0,
            boredom: stats.boredom - 3.0,
            energy: stats.energy - 0.3,
        },
        HitIntent::Raise => PresenceStats {
            affection: stats.affection + 1.5,
            boredom: stats.boredom - 4.0,
            energy: stats.energy - 1.0,
        },
    }
    .clamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abc_sequence_head() {
        let mut record = PresenceRecord::default();
        record = apply_pointer(record, HitIntent::Head, PresencePointerEvent::Down, 10);
        assert_eq!(record.interaction, InteractionPhase::AStart);
        record = apply_pointer(record, HitIntent::Head, PresencePointerEvent::Hold, 200);
        assert_eq!(record.interaction, InteractionPhase::BLoop);
        record = apply_pointer(record, HitIntent::Head, PresencePointerEvent::Up, 400);
        assert_eq!(record.interaction, InteractionPhase::CEnd);
        record = expire_stale_interaction(record, 400 + C_END_HOLD_MS);
        assert_eq!(record.interaction, InteractionPhase::Idle);
    }

    #[test]
    fn head_touch_raises_affection() {
        let before = PresenceStats::default().affection;
        let after = apply_pointer(
            PresenceRecord::default(),
            HitIntent::Head,
            PresencePointerEvent::Down,
            1,
        );
        assert!(after.stats.affection > before);
    }
}
