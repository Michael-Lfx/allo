//! Beats of a clip: merging adjacent same-camera shots, pricing the result, and
//! laying its beats out on the clip's timeline.
//!
//! Every splice between two generated clips is a chance to stutter: the model
//! re-establishes the pose, the soundtrack restarts, the motion resets. The
//! cheapest fix is to not splice at all. When two adjacent shots share a camera,
//! nothing about the framing changes across their boundary — so they can render
//! as **one** clip with two beats, as long as the combined length still fits the
//! selected model's window ([`ClipBounds`]).
//!
//! What is left after merging (a same-camera run too long for one clip, or a
//! genuine camera change) is handled by the seam machinery in
//! [`crate::media_local`]: head trim plus de-click fade for a continued take, a
//! full cut otherwise.
//!
//! The merged shape is decided at plan time and cached with the plan, so
//! switching to a video model with a different window keeps the old shape until
//! the plan is re-derived. Clip length is always re-clamped to the *current*
//! window at render time, so a stale shape can only pace beats tighter than
//! planned — it can never ask the model for a duration it rejects.
//!
//! Timing is the renderer's, not the planner's: clip length is allocated from
//! the scene budget long after a shot was described, so [`clip_timeline`] is the
//! only thing allowed to put seconds in front of the model, and
//! [`strip_authored_timecodes`] removes any a planner wrote anyway.

use std::sync::LazyLock;

use regex::Regex;

use crate::clip_bounds::ClipBounds;
use crate::domain::{ShotBeat, ShotDescription};

/// Merge maximal runs of adjacent same-camera shots into single multi-beat clips.
///
/// `shots` is consumed in timeline order (callers sort by `idx`). The merged clip
/// keeps the run's **first** `idx`, so shot directories and concat order stay
/// stable; the absorbed indices disappear from the plan.
///
/// A run is only merged while all of these hold:
/// - the shots share a `cam_idx` (same framing, nothing to cut to),
/// - none of them is already merged (merging is not idempotent), and
/// - the summed content need still fits [`ClipBounds::max_secs`].
pub(crate) fn merge_same_camera_shots(
    bounds: ClipBounds,
    shots: Vec<ShotDescription>,
) -> Vec<ShotDescription> {
    let mut out: Vec<ShotDescription> = Vec::with_capacity(shots.len());
    let mut run: Vec<ShotDescription> = Vec::new();
    let mut run_need = 0u32;

    for shot in shots {
        let need = beat_need_secs(bounds, &shot.variation_type, &shot);
        let joins = match run.last() {
            Some(prev) => {
                prev.cam_idx == shot.cam_idx
                    && !prev.is_merged()
                    && !shot.is_merged()
                    && run_need + need <= bounds.max_secs()
            }
            None => false,
        };
        if !joins {
            flush(&mut out, std::mem::take(&mut run));
            run_need = 0;
        }
        run_need += need;
        run.push(shot);
    }
    flush(&mut out, run);
    out
}

/// Content need for one clip, in seconds, clamped to the model window.
///
/// A merged clip pays for every beat it absorbed. Pricing it as a single beat
/// would squeeze the whole run into one beat's worth of seconds, and the model
/// would rush the lines or silently drop the later beats.
pub(crate) fn clip_need_secs(bounds: ClipBounds, shot: &ShotDescription) -> u32 {
    if !shot.is_merged() {
        return beat_need_secs(bounds, &shot.variation_type, shot);
    }
    let total: u32 = shot
        .beats
        .iter()
        .map(|beat| beat_need_secs(bounds, &shot.variation_type, beat))
        .sum();
    bounds.clamp_secs(total)
}

/// One beat placed on the clip's real timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimedBeat {
    pub(crate) start_secs: u32,
    pub(crate) end_secs: u32,
    pub(crate) text: String,
}

/// Lay a clip's beats on its **actual** `duration_secs`, or return empty when the
/// clip plays a single beat.
///
/// Two kinds of clip need a timeline, and both must be measured against the
/// duration the render actually requests — never against seconds a planner
/// guessed before the duration existed:
/// - a clip that absorbed adjacent same-camera shots ([`merge_same_camera_shots`]);
/// - a clip whose `motion_desc` was written as timed segments (`0-4s: … 4-7s: …`)
///   by the storyboard LLM, which cannot know the final clip length.
///
/// Spans are proportional to each beat's weight, cover the clip exactly, and
/// never collapse to zero length — a beat with no time on screen is a beat the
/// model will skip.
pub(crate) fn clip_timeline(
    bounds: ClipBounds,
    shot: &ShotDescription,
    duration_secs: u32,
) -> Vec<TimedBeat> {
    let beats = weighted_beats(bounds, shot);
    if beats.len() < 2 || duration_secs < beats.len() as u32 {
        return Vec::new();
    }
    let weights: Vec<u32> = beats.iter().map(|(_, weight)| (*weight).max(1)).collect();
    let mut cursor = 0u32;
    apportion(&weights, duration_secs)
        .into_iter()
        .zip(beats)
        .map(|(len, (text, _))| {
            let beat = TimedBeat {
                start_secs: cursor,
                end_secs: cursor + len,
                text,
            };
            cursor += len;
            beat
        })
        .collect()
}

/// `(text, weight)` per beat, from whichever source this clip has.
fn weighted_beats(bounds: ClipBounds, shot: &ShotDescription) -> Vec<(String, u32)> {
    if shot.beats.len() >= 2 {
        return shot
            .beats
            .iter()
            .map(|beat| {
                (
                    strip_authored_timecodes(&beat.motion_desc),
                    beat_need_secs(bounds, &shot.variation_type, beat),
                )
            })
            .collect();
    }
    // A single shot whose motion was authored as timed segments: keep the
    // planner's relative pacing as the weight, drop its absolute seconds.
    authored_beats(&shot.motion_desc)
        .into_iter()
        .map(|beat| (beat.text, beat.secs))
        .collect()
}

/// Split `duration_secs` across `weights`, giving every entry at least 1s.
///
/// The seconds lost to integer division go to the entries with the largest
/// fractions (largest remainder), not to whoever happens to be last — otherwise
/// a silent beat can end up with more screen time than the spoken beat next to
/// it, which is the pacing bug this whole layout exists to prevent.
fn apportion(weights: &[u32], duration_secs: u32) -> Vec<u32> {
    let count = weights.len() as u32;
    debug_assert!(count >= 1 && duration_secs >= count);
    let total_weight: u32 = weights.iter().sum::<u32>().max(1);
    let spare = duration_secs - count;
    let mut lens: Vec<u32> = Vec::with_capacity(weights.len());
    let mut fractions: Vec<(u32, usize)> = Vec::with_capacity(weights.len());
    for (i, &weight) in weights.iter().enumerate() {
        let share = spare * weight;
        lens.push(1 + share / total_weight);
        fractions.push((share % total_weight, i));
    }
    // Biggest fraction first; ties go to the earlier beat.
    fractions.sort_by_key(|&(fraction, i)| (std::cmp::Reverse(fraction), i));
    let mut leftover = duration_secs - lens.iter().sum::<u32>();
    for &(_, i) in &fractions {
        if leftover == 0 {
            break;
        }
        lens[i] += 1;
        leftover -= 1;
    }
    lens
}

/// `0-4s:` / `[4-7s]` / `0～3秒：` — a timecode a planner wrote into prose.
static AUTHORED_TIMECODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[?\s*(\d{1,3})\s*[-–—~～]\s*(\d{1,3})\s*(?:s\b|sec\b|secs\b|seconds\b|秒)\s*\]?\s*[:：]?\s*")
        .expect("authored timecode regex")
});

struct AuthoredBeat {
    text: String,
    /// Length the planner asked for, kept only as a relative weight.
    secs: u32,
}

/// Beats a planner wrote as timed segments inside one motion line.
///
/// Empty unless at least two segments are present: one lone timecode carries no
/// pacing information, only a wrong number.
fn authored_beats(motion: &str) -> Vec<AuthoredBeat> {
    let marks: Vec<(usize, usize, u32)> = AUTHORED_TIMECODE
        .captures_iter(motion)
        .map(|caps| {
            let whole = caps.get(0).expect("group 0 always matches");
            let secs = |i: usize| caps[i].parse::<u32>().unwrap_or(0);
            (
                whole.start(),
                whole.end(),
                secs(2).saturating_sub(secs(1)).max(1),
            )
        })
        .collect();
    if marks.len() < 2 {
        return Vec::new();
    }
    // Prose before the first timecode applies to the whole clip; it reads as the
    // opening of the first beat.
    let lead = motion[..marks[0].0].trim();
    let mut out: Vec<AuthoredBeat> = Vec::with_capacity(marks.len());
    for (i, &(_, body_start, secs)) in marks.iter().enumerate() {
        let body_end = marks.get(i + 1).map_or(motion.len(), |next| next.0);
        let body = motion[body_start..body_end]
            .trim()
            .trim_end_matches([';', '；', ',', '，']);
        let text = match i {
            0 if !lead.is_empty() => format!("{lead} {body}"),
            _ => body.to_string(),
        };
        out.push(AuthoredBeat {
            text: text.trim().to_string(),
            secs,
        });
    }
    out.retain(|beat| !beat.text.is_empty());
    match out.len() {
        0 | 1 => Vec::new(),
        _ => out,
    }
}

/// Drop planner-authored timecodes from a motion line, keeping the prose.
///
/// The renderer decides how long a clip is, so any absolute seconds surviving
/// into the prompt would contradict the duration actually requested.
pub(crate) fn strip_authored_timecodes(motion: &str) -> String {
    if !AUTHORED_TIMECODE.is_match(motion) {
        return motion.trim().to_string();
    }
    AUTHORED_TIMECODE
        .replace_all(motion, "")
        .trim()
        .trim_start_matches([';', '；', ',', '，'])
        .trim()
        .to_string()
}

/// Anything that carries one beat's worth of motion and audio.
trait Beat {
    fn motion(&self) -> &str;
    fn audio(&self) -> Option<&str>;
}

impl Beat for ShotDescription {
    fn motion(&self) -> &str {
        &self.motion_desc
    }
    fn audio(&self) -> Option<&str> {
        self.audio_desc.as_deref()
    }
}

impl Beat for ShotBeat {
    fn motion(&self) -> &str {
        &self.motion_desc
    }
    fn audio(&self) -> Option<&str> {
        self.audio_desc.as_deref()
    }
}

fn beat_need_secs(bounds: ClipBounds, variation_type: &str, beat: &impl Beat) -> u32 {
    crate::planning::estimate_shot_need_secs(bounds, beat.audio(), beat.motion(), variation_type)
}

/// Collapse a finished run into `out`. A run of one is passed through untouched
/// so unmerged clips keep their exact planner artifact.
fn flush(out: &mut Vec<ShotDescription>, mut run: Vec<ShotDescription>) {
    match run.len() {
        0 => {}
        1 => out.push(run.pop().expect("len checked")),
        _ => out.push(collapse(run)),
    }
}

fn collapse(run: Vec<ShotDescription>) -> ShotDescription {
    let absorbed: Vec<i32> = run.iter().skip(1).map(|s| s.idx).collect();
    let beats: Vec<ShotBeat> = run
        .iter()
        .map(|s| ShotBeat {
            motion_desc: s.motion_desc.clone(),
            audio_desc: s.audio_desc.clone(),
        })
        .collect();
    let variation_type = run
        .iter()
        .max_by_key(|s| variation_rank(&s.variation_type))
        .map(|s| s.variation_type.clone())
        .unwrap_or_default();
    let last = run.last().expect("len >= 2");
    let is_last = last.is_last;
    let lf_desc = last.lf_desc.clone();
    let lf_vis_char_idxs = last.lf_vis_char_idxs.clone();
    let motion_desc = join_distinct(run.iter().map(|s| s.motion_desc.as_str()), ", then ");
    let audio_desc = {
        let joined = join_distinct(run.iter().filter_map(|s| s.audio_desc.as_deref()), " ");
        (!joined.is_empty()).then_some(joined)
    };

    let mut head = run.into_iter().next().expect("len >= 2");
    tracing::info!(
        clip = head.idx,
        camera = head.cam_idx,
        absorbed = ?absorbed,
        beats = beats.len(),
        "merged adjacent same-camera shots into one clip (one splice fewer)"
    );
    head.is_last = is_last;
    head.variation_type = variation_type;
    head.lf_desc = lf_desc;
    head.lf_vis_char_idxs = lf_vis_char_idxs;
    head.motion_desc = motion_desc;
    head.audio_desc = audio_desc;
    head.beats = beats;
    head
}

/// A merged clip shows every beat's change, so it inherits the busiest
/// variation of the run. Unranked planner words keep rank 0 and lose to any
/// explicit `small`/`medium`/`large`.
fn variation_rank(variation_type: &str) -> u8 {
    match variation_type.trim().to_ascii_lowercase().as_str() {
        "large" => 3,
        "medium" => 2,
        "small" => 1,
        _ => 0,
    }
}

/// Join non-empty parts, dropping a part identical to the one before it —
/// adjacent shots repeat the same ambience/BGM line, and repeating it in the
/// prompt reads as "play it twice".
fn join_distinct<'a>(parts: impl Iterator<Item = &'a str>, sep: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() || out.last() == Some(&part) {
            continue;
        }
        out.push(part);
    }
    out.join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Window of the models integrated today (Seedance 2.0, MiniMax-H3 ⊂ 4–15s).
    const SEEDANCE: ClipBounds = ClipBounds::new(5, 15);

    fn shot(idx: i32, cam_idx: i32, motion: &str, audio: Option<&str>) -> ShotDescription {
        ShotDescription {
            idx,
            is_last: false,
            cam_idx,
            visual_desc: format!("framing {cam_idx}"),
            variation_type: "small".into(),
            variation_reason: format!("reason {idx}"),
            ff_desc: format!("ff{idx}"),
            ff_vis_char_idxs: vec![idx],
            lf_desc: format!("lf{idx}"),
            lf_vis_char_idxs: vec![idx],
            motion_desc: motion.into(),
            audio_desc: audio.map(str::to_string),
            beats: Vec::new(),
        }
    }

    #[test]
    fn adjacent_same_camera_shots_become_one_clip() {
        let shots = vec![
            shot(0, 0, "she turns", Some("door creaks")),
            shot(1, 0, "she steps closer", Some("footsteps")),
            shot(2, 1, "reverse angle", Some("room tone")),
        ];
        let merged = merge_same_camera_shots(SEEDANCE, shots);

        assert_eq!(merged.len(), 2);
        let clip = &merged[0];
        // Head idx wins so shot dirs and concat order stay put.
        assert_eq!(clip.idx, 0);
        assert!(clip.is_merged());
        assert_eq!(clip.motion_desc, "she turns, then she steps closer");
        assert_eq!(clip.audio_desc.as_deref(), Some("door creaks footsteps"));
        // ff from the run's head, lf from its tail.
        assert_eq!(clip.ff_desc, "ff0");
        assert_eq!(clip.lf_desc, "lf1");
        assert_eq!(clip.lf_vis_char_idxs, vec![1]);
        // A camera change is still a separate clip.
        assert_eq!(merged[1].idx, 2);
        assert!(!merged[1].is_merged());
    }

    #[test]
    fn a_camera_change_is_never_merged() {
        let shots = vec![shot(0, 0, "a", None), shot(1, 1, "b", None)];
        let merged = merge_same_camera_shots(SEEDANCE, shots);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().all(|s| !s.is_merged()));
    }

    #[test]
    fn a_run_stops_at_the_model_ceiling() {
        // Silent shots need the 5s model minimum each, so 15s fits exactly three.
        let shots: Vec<ShotDescription> =
            (0..5).map(|i| shot(i, 0, "hold", None)).collect();
        let merged = merge_same_camera_shots(SEEDANCE, shots);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].beats.len(), 3);
        assert_eq!(merged[1].beats.len(), 2);
        for clip in &merged {
            assert!(clip_need_secs(SEEDANCE, clip) <= SEEDANCE.max_secs());
        }
    }

    #[test]
    fn a_narrow_model_window_merges_nothing() {
        // min == max: one beat already fills the clip, so every shot stands alone.
        let narrow = ClipBounds::new(5, 5);
        let shots: Vec<ShotDescription> =
            (0..3).map(|i| shot(i, 0, "hold", None)).collect();
        let merged = merge_same_camera_shots(narrow, shots);
        assert_eq!(merged.len(), 3);
        assert!(merged.iter().all(|s| !s.is_merged()));
    }

    #[test]
    fn an_already_merged_clip_is_left_alone() {
        // Re-running the merge (stale artifact resumed from disk) must not
        // absorb a merged clip again and replay its beats.
        let shots = vec![
            shot(0, 0, "she turns", None),
            shot(1, 0, "she steps closer", None),
        ];
        let once = merge_same_camera_shots(SEEDANCE, shots);
        let twice = merge_same_camera_shots(SEEDANCE, once.clone());
        assert_eq!(twice.len(), once.len());
        assert_eq!(twice[0].beats.len(), once[0].beats.len());
        assert_eq!(twice[0].motion_desc, once[0].motion_desc);
    }

    #[test]
    fn the_last_shot_flag_and_busiest_variation_survive_the_merge() {
        let mut shots = vec![
            shot(0, 0, "a", None),
            shot(1, 0, "b", None),
        ];
        shots[1].is_last = true;
        shots[1].variation_type = "large".into();
        let merged = merge_same_camera_shots(SEEDANCE, shots);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].is_last);
        assert_eq!(merged[0].variation_type, "large");
    }

    #[test]
    fn a_repeated_ambience_line_is_not_played_twice() {
        let bgm = "(soft continuous underscore)";
        let shots = vec![
            shot(0, 0, "a", Some(bgm)),
            shot(1, 0, "b", Some(bgm)),
        ];
        let merged = merge_same_camera_shots(SEEDANCE, shots);
        assert_eq!(merged[0].audio_desc.as_deref(), Some(bgm));
    }

    #[test]
    fn a_merged_clip_is_priced_for_every_beat() {
        let line: String = "中".chars().cycle().take(12).collect();
        let shots = vec![
            shot(0, 0, "she speaks", Some(&line)),
            shot(1, 0, "he answers", Some(&line)),
        ];
        let one_beat = clip_need_secs(SEEDANCE, &shots[0]);
        let merged = merge_same_camera_shots(SEEDANCE, shots);
        assert!(clip_need_secs(SEEDANCE, &merged[0]) > one_beat);
    }

    /// A spoken beat (17 CJK ⇒ ~8s) next to a silent one (model min 5s): 13s of
    /// content, still inside the 15s ceiling, so they merge.
    fn spoken_then_silent() -> ShotDescription {
        let line: String = "中".chars().cycle().take(17).collect();
        let shots = vec![
            shot(0, 0, "she speaks", Some(&line)),
            shot(1, 0, "he nods", None),
        ];
        let mut merged = merge_same_camera_shots(SEEDANCE, shots);
        assert_eq!(merged.len(), 1, "the run must merge for this fixture to mean anything");
        merged.remove(0)
    }

    fn lens(timeline: &[TimedBeat]) -> Vec<u32> {
        timeline
            .iter()
            .map(|beat| beat.end_secs - beat.start_secs)
            .collect()
    }

    #[test]
    fn the_timeline_covers_the_clip_and_gives_every_beat_time() {
        let clip = spoken_then_silent();
        let timeline = clip_timeline(SEEDANCE, &clip, 15);

        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].start_secs, 0);
        assert_eq!(timeline.last().expect("two beats").end_secs, 15);
        for beat in &timeline {
            assert!(
                beat.end_secs > beat.start_secs,
                "beat {beat:?} has no time on screen"
            );
        }
        for pair in timeline.windows(2) {
            assert_eq!(
                pair[0].end_secs, pair[1].start_secs,
                "the timeline must be contiguous"
            );
        }
        // The spoken beat gets the room its line needs.
        assert!(
            lens(&timeline)[0] > lens(&timeline)[1],
            "spoken beat must not be rushed for a silent one: {timeline:?}"
        );
    }

    /// Integer division must not hand its leftover seconds to whichever beat is
    /// last; they belong to the beat with the biggest unmet fraction.
    #[test]
    fn rounding_leftovers_go_to_the_beat_that_needs_them() {
        let clip = spoken_then_silent();

        for duration in 2..=SEEDANCE.max_secs() {
            let timeline = clip_timeline(SEEDANCE, &clip, duration);
            assert_eq!(
                timeline.last().expect("two beats").end_secs,
                duration,
                "the timeline must cover the whole {duration}s clip"
            );
            let lens = lens(&timeline);
            assert!(
                lens[0] >= lens[1],
                "{duration}s clip starved the spoken beat: {timeline:?}"
            );
        }
    }

    #[test]
    fn a_single_beat_clip_has_no_beat_layout() {
        let single = shot(0, 0, "hold", None);
        assert!(clip_timeline(SEEDANCE, &single, 8).is_empty());
    }

    #[test]
    fn the_timeline_bails_out_when_the_clip_is_shorter_than_its_beat_count() {
        let shots: Vec<ShotDescription> = (0..3).map(|i| shot(i, 0, "hold", None)).collect();
        let merged = merge_same_camera_shots(SEEDANCE, shots);
        assert_eq!(merged[0].beats.len(), 3);
        assert!(clip_timeline(SEEDANCE, &merged[0], 2).is_empty());
    }

    /// Regression: the storyboard LLM writes `0-4s / 4-7s` before the clip length
    /// is allocated, so its seconds (7s here) contradicted the rendered clip (5s).
    /// The renderer owns time — the authored pacing survives only as a ratio.
    #[test]
    fn authored_timecodes_are_relaid_on_the_real_duration() {
        let mut s = shot(0, 0, "", None);
        s.motion_desc = "0-4s:男生骑车从左向右横贯画面;4-7s:他继续向右骑行,落叶被风吹卷而起。".into();

        let timeline = clip_timeline(SEEDANCE, &s, 5);
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].start_secs, 0);
        assert_eq!(timeline.last().expect("two beats").end_secs, 5);
        // 4:3 authored ratio over a 5s clip → 3s then 2s.
        assert_eq!(lens(&timeline), vec![3, 2]);
        // No authored second survives into the beat text.
        for beat in &timeline {
            assert!(!beat.text.contains('s'), "{beat:?}");
            assert!(!beat.text.contains("4-7"), "{beat:?}");
        }
        assert!(timeline[0].text.starts_with("男生骑车"), "{timeline:?}");
        assert!(timeline[1].text.starts_with("他继续"), "{timeline:?}");
    }

    /// Prose before the first timecode belongs to the first beat, not to nobody.
    #[test]
    fn a_lead_in_before_the_first_timecode_opens_the_first_beat() {
        let mut s = shot(0, 0, "", None);
        s.motion_desc = "固定机位。[0-3s] 她抬头; [3-8s] 她起身离开".into();

        let timeline = clip_timeline(SEEDANCE, &s, 11);
        assert_eq!(timeline.len(), 2);
        assert!(timeline[0].text.starts_with("固定机位。"), "{timeline:?}");
        assert!(timeline[0].text.contains("她抬头"), "{timeline:?}");
        assert_eq!(lens(&timeline), vec![4, 7], "3:5 over 11s");
    }

    /// One timecode is not a pacing plan, just a wrong number: strip it and keep
    /// the clip on its plain motion line.
    #[test]
    fn a_lone_timecode_is_stripped_without_building_a_timeline() {
        let mut s = shot(0, 0, "", None);
        s.motion_desc = "0-8s: 她慢慢转身".into();

        assert!(clip_timeline(SEEDANCE, &s, 8).is_empty());
        assert_eq!(strip_authored_timecodes(&s.motion_desc), "她慢慢转身");
    }

    #[test]
    fn motion_without_timecodes_is_left_exactly_as_written() {
        let motion = "摄影机固定,她转身走向窗边";
        assert_eq!(strip_authored_timecodes(motion), motion);
        assert_eq!(strip_authored_timecodes("  spaced  "), "spaced");
    }

    /// A merged clip whose absorbed beats carry authored timecodes must not leak
    /// them: the merged clip's own windows are the only truth.
    #[test]
    fn a_merged_clip_drops_the_timecodes_of_the_beats_it_absorbed() {
        let mut a = shot(0, 0, "", None);
        a.motion_desc = "0-4s:她转身;4-7s:她走近".into();
        let b = shot(1, 0, "他抬头", None);
        let merged = merge_same_camera_shots(SEEDANCE, vec![a, b]);
        assert_eq!(merged.len(), 1);

        let timeline = clip_timeline(SEEDANCE, &merged[0], 12);
        assert_eq!(timeline.len(), 2);
        assert!(!timeline[0].text.contains("0-4s"), "{timeline:?}");
        assert!(!timeline[0].text.contains("4-7s"), "{timeline:?}");
        assert_eq!(timeline.last().expect("two beats").end_secs, 12);
    }
}
