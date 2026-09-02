//! Beats of a clip: packing adjacent **narrative** events into one generation,
//! pricing the result, and laying its beats out on the clip's timeline.
//!
//! A storyboard row is a story unit, not a tripod position. Reverse angles,
//! inserts, and push-ins that still belong to the same beat live *inside* one
//! file as CUT TO — the model window ([`ClipBounds`]) is the only hard split
//! (speech that would 吞字, or a run that no longer fits). Camera changes never
//! start a new row by themselves.
//!
//! Packing belongs to **planning**, not render: the storyboard the user sees
//! must be the same list the renderer submits. [`pack_scene_briefs`] collapses
//! leftover micro-shots before decompose; [`pack_scene_clips`] is the same
//! rule on already-decomposed shots (resume / stale plans). The LLM draft
//! never becomes `storyboard.json` — only the packed list is published.
//! After align, [`densify_aligned_indices`] renumbers surviving clips `0..n`
//! so shot directories and the filmstrip do not keep absorbed holes.
//!
//! What is left after packing (a run too long for one clip) is handled by the
//! seam machinery in [`crate::media_local`]: head trim plus de-click fade for a
//! continued take, a match-cut otherwise.
//!
//! The packed shape is decided at plan time and cached with the plan, so
//! switching to a video model with a different window keeps the old shape until
//! the plan is re-derived. Clip length is always re-clamped to the *current*
//! window at render time, so a stale shape can only pace beats tighter than
//! planned — it can never ask the model for a duration it rejects.
//!
//! Timing is the renderer's, not the planner's: clip length is allocated from
//! the scene budget long after a shot was described, so [`clip_timeline`] is the
//! only thing allowed to put seconds in front of the model, and
//! [`strip_authored_timecodes`] removes any a planner wrote anyway.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::clip_bounds::ClipBounds;
use crate::domain::{ShotBeat, ShotBriefBeat, ShotBriefDescription, ShotDescription};

/// Pack maximal runs of adjacent **storyboard rows** into single clips.
///
/// Same join rule as [`pack_scene_clips`], applied before decompose so the
/// storyboard the user sees is already the render list: one row, one video.
/// Packed rows are reindexed `0..n` — absorbed micro-shots must not keep a
/// card in 「故事分镜」.
pub(crate) fn pack_scene_briefs(
    bounds: ClipBounds,
    briefs: Vec<ShotBriefDescription>,
) -> Vec<ShotBriefDescription> {
    let mut out: Vec<ShotBriefDescription> = Vec::with_capacity(briefs.len());
    let mut run: Vec<ShotBriefDescription> = Vec::new();
    let mut run_need = 0u32;

    for brief in briefs {
        let need = brief_need_secs(bounds, &brief);
        let joins = run.last().is_some_and(|prev| {
            can_pack_briefs(prev, &brief, run_need, need, bounds)
        });
        if !joins {
            flush_briefs(&mut out, std::mem::take(&mut run));
            run_need = 0;
        }
        run_need += need;
        run.push(brief);
    }
    flush_briefs(&mut out, run);
    reindex_briefs(&mut out);
    out
}

/// Rebuild the storyboard so it has exactly one row per renderable clip.
///
/// Keeps each clip's `idx` (shot directories / videos stay put). Absorbed
/// micro-shots disappear from the board. Used when a plan was packed after
/// decompose — resume must not keep empty storyboard cards.
///
/// Never invents a row for a clip idx that was not on the board. Inventing is
/// what turned a leftover `shot_descriptions.json` entry into a phantom last
/// shot in 「故事分镜」 when video started.
pub(crate) fn sync_storyboard_to_clips(
    briefs: &[ShotBriefDescription],
    clips: &[ShotDescription],
) -> Vec<ShotBriefDescription> {
    if clips.is_empty() {
        return briefs.to_vec();
    }
    let by_idx: HashMap<i32, &ShotBriefDescription> =
        briefs.iter().map(|b| (b.idx, b)).collect();
    let last = clips.len().saturating_sub(1);
    clips
        .iter()
        .enumerate()
        .filter_map(|(i, clip)| {
            let row = by_idx.get(&clip.idx).cloned().cloned()?;
            Some(apply_clip_to_brief(row, clip, i == last))
        })
        .collect()
}

fn apply_clip_to_brief(
    mut row: ShotBriefDescription,
    clip: &ShotDescription,
    is_last: bool,
) -> ShotBriefDescription {
    row.idx = clip.idx;
    row.is_last = is_last;
    row.cam_idx = clip.cam_idx;
    if clip.is_merged() {
        row.beats = clip
            .beats
            .iter()
            .map(|beat| ShotBriefBeat {
                visual_desc: beat.motion_desc.clone(),
                audio_desc: beat.audio_desc.clone(),
                cam_idx: beat.cam_idx.unwrap_or(clip.cam_idx),
            })
            .collect();
        row.visual_desc = join_visual_with_cuts(
            row.beats
                .iter()
                .map(|beat| (beat.cam_idx, beat.visual_desc.as_str())),
        );
        row.audio_desc = clip.audio_desc.clone();
    }
    row
}

/// Drop stale extra clips that were never on the user-facing storyboard.
///
/// A leftover `shot_descriptions.json` (pre-pack cache, absorbed dirs) can have
/// more rows than `storyboard.json`. Syncing the board *up* to that list is what
/// makes 「故事分镜」 gain a phantom shot when video starts. Clips are the render
/// list only after they are a subset of, or a packed collapse of, the board.
///
/// Length is not enough: equal-length lists with a stray last `idx` must still
/// drop that clip, otherwise resume invents a new last card.
pub(crate) fn drop_stale_extra_clips(
    briefs: &[ShotBriefDescription],
    clips: Vec<ShotDescription>,
) -> Vec<ShotDescription> {
    if briefs.is_empty() {
        return clips;
    }
    let keep: HashSet<i32> = briefs.iter().map(|b| b.idx).collect();
    let mut filtered: Vec<ShotDescription> = clips
        .iter()
        .filter(|c| keep.contains(&c.idx))
        .cloned()
        .collect();
    if filtered.is_empty() {
        let mut truncated = clips;
        truncated.sort_by_key(|c| c.idx);
        truncated.truncate(briefs.len());
        return truncated;
    }
    let mut seen = HashSet::new();
    filtered.retain(|c| seen.insert(c.idx));
    if filtered.len() > briefs.len() {
        filtered.sort_by_key(|c| c.idx);
        filtered.truncate(briefs.len());
    }
    filtered
}

/// Make the storyboard and the render list the same set of idxs.
///
/// The board never grows. Blank last rows (skipped by the studio parser) are
/// stripped so they cannot become visible once a clip fills `visual_desc`.
/// Packed plans (fewer clips than rows) still collapse the board downward.
pub(crate) fn align_storyboard_and_clips(
    briefs: Vec<ShotBriefDescription>,
    clips: Vec<ShotDescription>,
) -> (Vec<ShotBriefDescription>, Vec<ShotDescription>) {
    let briefs = visible_briefs(briefs);
    if clips.is_empty() {
        return (briefs, clips);
    }
    let clips = drop_stale_extra_clips(&briefs, clips);
    let briefs = if !clips.is_empty() && clips.len() < briefs.len() {
        sync_storyboard_to_clips(&briefs, &clips)
    } else {
        overlay_clips_onto_briefs(&briefs, &clips)
    };
    let keep: HashSet<i32> = briefs.iter().map(|b| b.idx).collect();
    let clips = clips
        .into_iter()
        .filter(|c| keep.contains(&c.idx))
        .collect();
    (briefs, clips)
}

fn visible_briefs(briefs: Vec<ShotBriefDescription>) -> Vec<ShotBriefDescription> {
    let mut seen = HashSet::new();
    briefs
        .into_iter()
        .filter_map(|mut brief| {
            if brief.visual_desc.trim().is_empty() && brief.is_merged() {
                brief.visual_desc = join_visual_with_cuts(
                    brief
                        .beats
                        .iter()
                        .map(|beat| (beat.cam_idx, beat.visual_desc.as_str())),
                );
            }
            if brief.visual_desc.trim().is_empty() {
                return None;
            }
            seen.insert(brief.idx).then_some(brief)
        })
        .collect()
}

fn overlay_clips_onto_briefs(
    briefs: &[ShotBriefDescription],
    clips: &[ShotDescription],
) -> Vec<ShotBriefDescription> {
    let by_idx: HashMap<i32, &ShotDescription> = clips.iter().map(|c| (c.idx, c)).collect();
    let last = briefs.len().saturating_sub(1);
    briefs
        .iter()
        .enumerate()
        .map(|(i, brief)| match by_idx.get(&brief.idx) {
            Some(clip) => apply_clip_to_brief(brief.clone(), clip, i == last),
            None => {
                let mut row = brief.clone();
                row.is_last = i == last;
                row
            }
        })
        .collect()
}

pub(crate) fn storyboard_differs(
    left: &[ShotBriefDescription],
    right: &[ShotBriefDescription],
) -> bool {
    if left.len() != right.len() {
        return true;
    }
    left.iter().zip(right).any(|(a, b)| {
        a.idx != b.idx
            || a.cam_idx != b.cam_idx
            || a.is_last != b.is_last
            || a.beats.len() != b.beats.len()
            || a.visual_desc != b.visual_desc
    })
}

/// Copy packed beats from the storyboard row onto the decomposed clip so the
/// renderer (and a later packer) sees the same multi-shot the board advertised.
pub(crate) fn stamp_beats_from_brief(desc: &mut ShotDescription, brief: &ShotBriefDescription) {
    if !brief.is_merged() || desc.is_merged() {
        return;
    }
    desc.beats = brief
        .beats
        .iter()
        .map(|beat| ShotBeat {
            motion_desc: beat.visual_desc.clone(),
            audio_desc: beat.audio_desc.clone(),
            cam_idx: Some(beat.cam_idx),
        })
        .collect();
}

/// Pack maximal runs of adjacent shots into single multi-beat clips.
///
/// `shots` is consumed in timeline order (callers sort by `idx`). The packed clip
/// keeps the run's **first** `idx`, so shot directories and concat order stay
/// stable; the absorbed indices disappear from the plan.
///
/// A run continues while none of the shots is already packed and the summed
/// content need still fits [`ClipBounds::max_secs`]. Camera is coverage inside
/// the file (CUT TO), never a reason to start a new clip.
pub(crate) fn pack_scene_clips(
    bounds: ClipBounds,
    shots: Vec<ShotDescription>,
) -> Vec<ShotDescription> {
    let mut out: Vec<ShotDescription> = Vec::with_capacity(shots.len());
    let mut run: Vec<ShotDescription> = Vec::new();
    let mut run_need = 0u32;

    for shot in shots {
        let need = beat_need_secs(bounds, &shot.variation_type, &shot);
        let joins = run.last().is_some_and(|prev| {
            can_pack_onto(prev, &shot, run_need, need, bounds)
        });
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

fn can_pack_onto(
    prev: &ShotDescription,
    shot: &ShotDescription,
    run_need: u32,
    need: u32,
    bounds: ClipBounds,
) -> bool {
    if prev.is_merged() || shot.is_merged() {
        return false;
    }
    if swapped_screen_blocking(&prev.visual_desc, &shot.visual_desc)
        || swapped_screen_blocking(&prev.lf_desc, &shot.ff_desc)
    {
        return coverage_fits(
            bounds,
            run_need,
            need,
            prev.audio_desc.as_deref(),
            shot.audio_desc.as_deref(),
        );
    }
    // Narrative-adjacent and still inside one generation. Angle changes are
    // CUT TO inside the file, not a new splice.
    run_need + need <= bounds.max_secs()
}

fn can_pack_briefs(
    prev: &ShotBriefDescription,
    brief: &ShotBriefDescription,
    run_need: u32,
    need: u32,
    bounds: ClipBounds,
) -> bool {
    if prev.is_merged() || brief.is_merged() {
        return false;
    }
    if swapped_screen_blocking(&prev.visual_desc, &brief.visual_desc) {
        return coverage_fits(
            bounds,
            run_need,
            need,
            prev.audio_desc.as_deref(),
            brief.audio_desc.as_deref(),
        );
    }
    run_need + need <= bounds.max_secs()
}

/// Reverse / over-shoulder of the same beat occupies the *same* story seconds.
fn coverage_fits(
    bounds: ClipBounds,
    run_need: u32,
    need: u32,
    prev_audio: Option<&str>,
    next_audio: Option<&str>,
) -> bool {
    if run_need.max(need) > bounds.max_secs() {
        return false;
    }
    let speech = crate::planning::estimate_speech_secs(prev_audio.unwrap_or(""))
        .saturating_add(crate::planning::estimate_speech_secs(next_audio.unwrap_or("")));
    speech <= bounds.max_secs()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScreenSide {
    Left,
    Right,
}

/// True when adjacent rows flip who stands screen-left vs screen-right.
///
/// That is almost always a reverse-angle of the same blocking, which must live
/// inside one generated file (CUT TO) — splicing two files reads as a teleport.
pub(crate) fn swapped_screen_blocking(a: &str, b: &str) -> bool {
    let Some((a_left, a_right)) = primary_screen_pair(a) else {
        return explicit_reverse_cut(b);
    };
    let Some((b_left, b_right)) = primary_screen_pair(b) else {
        return explicit_reverse_cut(b);
    };
    names_overlap(&a_left, &b_right) && names_overlap(&a_right, &b_left)
}

fn explicit_reverse_cut(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("反打")
        || t.contains("过肩")
        || t.contains("反打镜头")
        || t.contains("reverse angle")
        || t.contains("over-the-shoulder")
        || t.contains("over the shoulder")
}

fn names_overlap(a: &str, b: &str) -> bool {
    let a = normalize_subject(a);
    let b = normalize_subject(b);
    if a.len() < 2 || b.len() < 2 {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

fn normalize_subject(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '<' && *c != '>')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn primary_screen_pair(text: &str) -> Option<(String, String)> {
    let mut left: Option<String> = None;
    let mut right: Option<String> = None;
    for (name, side) in screen_side_assignments(text) {
        match side {
            ScreenSide::Left if left.is_none() => left = Some(name),
            ScreenSide::Right if right.is_none() => right = Some(name),
            _ => {}
        }
        if left.is_some() && right.is_some() {
            break;
        }
    }
    Some((left?, right?))
}

fn screen_side_assignments(text: &str) -> Vec<(String, ScreenSide)> {
    static AFTER_SUBJECT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"([^，。；;,\n]{2,24}?)(?:在画面|位于画面|位于|在)(左|右)(?:侧|边)",
        )
        .expect("screen-side after-subject regex")
    });
    static BEFORE_SUBJECT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(左|右)(?:侧|边)(?:的)?([^，。；;,\n]{2,16}?)(?:在|位于|蹲|站|坐)")
            .expect("screen-side before-subject regex")
    });
    let mut out = Vec::new();
    let mut seen: HashMap<String, ScreenSide> = HashMap::new();
    for cap in AFTER_SUBJECT.captures_iter(text) {
        let name = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        let side = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        push_assignment(&mut out, &mut seen, name, side);
    }
    for cap in BEFORE_SUBJECT.captures_iter(text) {
        let side = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let name = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        push_assignment(&mut out, &mut seen, name, side);
    }
    out
}

fn push_assignment(
    out: &mut Vec<(String, ScreenSide)>,
    seen: &mut HashMap<String, ScreenSide>,
    name: &str,
    side_token: &str,
) {
    let name = name
        .trim_start_matches(['，', ',', '、', '的', '：', ':'])
        .trim();
    if name.chars().count() < 2 || name.contains("面朝") || name.contains("转向") {
        return;
    }
    let side = match side_token {
        "左" => ScreenSide::Left,
        "右" => ScreenSide::Right,
        _ => return,
    };
    let key = normalize_subject(name);
    if key.is_empty() || seen.contains_key(&key) {
        return;
    }
    seen.insert(key, side);
    out.push((name.to_string(), side));
}

fn brief_need_secs(bounds: ClipBounds, brief: &ShotBriefDescription) -> u32 {
    if brief.is_merged() {
        let total: u32 = brief
            .beats
            .iter()
            .map(|beat| {
                crate::planning::estimate_shot_need_secs(
                    bounds,
                    beat.audio_desc.as_deref(),
                    &beat.visual_desc,
                    "small",
                )
            })
            .sum();
        return bounds.clamp_secs(total);
    }
    crate::planning::estimate_shot_need_secs(
        bounds,
        brief.audio_desc.as_deref(),
        &brief.visual_desc,
        "small",
    )
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
    pub(crate) cam_idx: Option<i32>,
}

/// Lay a clip's beats on its **actual** `duration_secs`, or return empty when the
/// clip plays a single beat.
///
/// Two kinds of clip need a timeline, and both must be measured against the
/// duration the render actually requests — never against seconds a planner
/// guessed before the duration existed:
/// - a clip that absorbed adjacent shots ([`pack_scene_clips`]);
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
    let weights: Vec<u32> = beats.iter().map(|(_, weight, _)| (*weight).max(1)).collect();
    let mut cursor = 0u32;
    apportion(&weights, duration_secs)
        .into_iter()
        .zip(beats)
        .map(|(len, (text, _, cam_idx))| {
            let beat = TimedBeat {
                start_secs: cursor,
                end_secs: cursor + len,
                text,
                cam_idx,
            };
            cursor += len;
            beat
        })
        .collect()
}

/// `(text, weight, cam)` per beat, from whichever source this clip has.
fn weighted_beats(bounds: ClipBounds, shot: &ShotDescription) -> Vec<(String, u32, Option<i32>)> {
    if shot.beats.len() >= 2 {
        return shot
            .beats
            .iter()
            .map(|beat| {
                (
                    strip_authored_timecodes(&beat.motion_desc),
                    beat_need_secs(bounds, &shot.variation_type, beat),
                    beat.cam_idx.or(Some(shot.cam_idx)),
                )
            })
            .collect();
    }
    // A single shot whose motion was authored as timed segments: keep the
    // planner's relative pacing as the weight, drop its absolute seconds.
    authored_beats(&shot.motion_desc)
        .into_iter()
        .map(|beat| (beat.text, beat.secs, Some(shot.cam_idx)))
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
    let same_camera = run.windows(2).all(|pair| pair[0].cam_idx == pair[1].cam_idx);
    let beats: Vec<ShotBeat> = run
        .iter()
        .map(|s| ShotBeat {
            motion_desc: s.motion_desc.clone(),
            audio_desc: s.audio_desc.clone(),
            cam_idx: Some(s.cam_idx),
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
    let exit_camera = last.cam_idx;
    let motion_desc = join_distinct(run.iter().map(|s| s.motion_desc.as_str()), ", then ");
    let visual_desc =
        join_visual_with_cuts(run.iter().map(|s| (s.cam_idx, s.visual_desc.as_str())));
    let audio_desc = {
        let joined = join_distinct(run.iter().filter_map(|s| s.audio_desc.as_deref()), " ");
        (!joined.is_empty()).then_some(joined)
    };

    let mut head = run.into_iter().next().expect("len >= 2");
    if same_camera {
        tracing::info!(
            clip = head.idx,
            camera = head.cam_idx,
            absorbed = ?absorbed,
            beats = beats.len(),
            "merged adjacent same-camera shots into one clip (one splice fewer)"
        );
    } else {
        tracing::info!(
            clip = head.idx,
            entry_camera = head.cam_idx,
            exit_camera,
            absorbed = ?absorbed,
            beats = beats.len(),
            "packed adjacent shots into one native multi-shot clip (camera cuts inside the generation)"
        );
    }
    head.is_last = is_last;
    head.variation_type = variation_type;
    head.lf_desc = lf_desc;
    head.lf_vis_char_idxs = lf_vis_char_idxs;
    head.visual_desc = visual_desc;
    head.motion_desc = motion_desc;
    head.audio_desc = audio_desc;
    head.beats = beats;
    head
}

fn flush_briefs(out: &mut Vec<ShotBriefDescription>, mut run: Vec<ShotBriefDescription>) {
    match run.len() {
        0 => {}
        1 => out.push(run.pop().expect("len checked")),
        _ => out.push(collapse_briefs(run)),
    }
}

fn collapse_briefs(run: Vec<ShotBriefDescription>) -> ShotBriefDescription {
    let beats: Vec<ShotBriefBeat> = run
        .iter()
        .map(|brief| ShotBriefBeat {
            visual_desc: brief.visual_desc.clone(),
            audio_desc: brief.audio_desc.clone(),
            cam_idx: brief.cam_idx,
        })
        .collect();
    let visual_desc =
        join_visual_with_cuts(beats.iter().map(|b| (b.cam_idx, b.visual_desc.as_str())));
    let audio_desc = {
        let joined = join_distinct(run.iter().filter_map(|s| s.audio_desc.as_deref()), " ");
        (!joined.is_empty()).then_some(joined)
    };
    let is_last = run.last().expect("len >= 2").is_last;
    let mut head = run.into_iter().next().expect("len >= 2");
    tracing::info!(
        clip = head.idx,
        entry_camera = head.cam_idx,
        beats = beats.len(),
        "packed storyboard rows into one renderable clip (one row = one video)"
    );
    head.is_last = is_last;
    head.visual_desc = visual_desc;
    head.audio_desc = audio_desc;
    head.beats = beats;
    head
}

fn reindex_briefs(briefs: &mut [ShotBriefDescription]) {
    let last = briefs.len().saturating_sub(1);
    for (i, brief) in briefs.iter_mut().enumerate() {
        brief.idx = i as i32;
        brief.is_last = i == last;
    }
}

/// True when clip idxs are already the dense render list `0..n`.
pub(crate) fn clip_indices_are_dense(clips: &[ShotDescription]) -> bool {
    clips
        .iter()
        .enumerate()
        .all(|(i, clip)| clip.idx == i as i32)
}

/// Renumber an aligned board+clip pair to `0..n`.
///
/// [`pack_scene_clips`] keeps each run's first idx so concat can find the
/// original shot dir. After absorb, that leaves holes (`0, 3, 5`). The
/// filmstrip and `shots/` tree then look like skipped middles. Mapping
/// returned here is `old_idx → new_idx` for directory relocate.
pub(crate) fn densify_aligned_indices(
    briefs: &mut Vec<ShotBriefDescription>,
    clips: &mut Vec<ShotDescription>,
) -> HashMap<i32, i32> {
    clips.sort_by_key(|clip| clip.idx);
    let mut map = HashMap::new();
    let last_clip = clips.len().saturating_sub(1);
    for (i, clip) in clips.iter_mut().enumerate() {
        let new_idx = i as i32;
        map.insert(clip.idx, new_idx);
        clip.idx = new_idx;
        clip.is_last = i == last_clip;
    }
    briefs.sort_by_key(|brief| brief.idx);
    let last_brief = briefs.len().saturating_sub(1);
    for (i, brief) in briefs.iter_mut().enumerate() {
        brief.idx = map.get(&brief.idx).copied().unwrap_or(i as i32);
        brief.is_last = i == last_brief;
    }
    map
}

/// Join beat visuals, inserting an explicit cut when the camera changes so the
/// storyboard caption and the decompose LLM both see the in-file transition.
fn join_visual_with_cuts<'a>(parts: impl IntoIterator<Item = (i32, &'a str)>) -> String {
    let mut out = String::new();
    let mut prev_cam: Option<i32> = None;
    for (cam, text) in parts {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            if prev_cam.is_some_and(|prev| prev != cam) {
                out.push_str("；然后切到新机位：");
            } else {
                out.push_str("；然后");
            }
        }
        out.push_str(text);
        prev_cam = Some(cam);
    }
    out
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

    fn brief(idx: i32, cam_idx: i32, visual: &str, audio: Option<&str>) -> ShotBriefDescription {
        ShotBriefDescription {
            idx,
            is_last: false,
            cam_idx,
            visual_desc: visual.into(),
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
        let merged = pack_scene_clips(SEEDANCE, shots);

        // 5+5+5s fits the 15s ceiling, so the reverse packs into the same
        // generation instead of becoming a splice.
        assert_eq!(merged.len(), 1);
        let clip = &merged[0];
        assert_eq!(clip.idx, 0);
        assert!(clip.is_merged());
        assert!(clip.has_camera_cuts());
        assert_eq!(clip.exit_cam_idx(), 1);
        assert_eq!(
            clip.motion_desc,
            "she turns, then she steps closer, then reverse angle"
        );
        assert_eq!(
            clip.audio_desc.as_deref(),
            Some("door creaks footsteps room tone")
        );
        assert_eq!(clip.ff_desc, "ff0");
        assert_eq!(clip.lf_desc, "lf2");
        assert_eq!(clip.lf_vis_char_idxs, vec![2]);
    }

    #[test]
    fn a_reverse_angle_that_fits_is_packed_as_native_multishot() {
        let shots = vec![shot(0, 0, "she speaks", None), shot(1, 1, "he answers", None)];
        let merged = pack_scene_clips(SEEDANCE, shots);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].is_merged());
        assert!(merged[0].has_camera_cuts());
        assert_eq!(merged[0].cam_idx, 0);
        assert_eq!(merged[0].exit_cam_idx(), 1);
        assert_eq!(merged[0].beats[0].cam_idx, Some(0));
        assert_eq!(merged[0].beats[1].cam_idx, Some(1));
    }

    #[test]
    fn a_camera_change_that_would_overflow_stays_split() {
        // 17 CJK → ~8s speech floor each; 16s > 15s ceiling.
        let line: String = "中".chars().cycle().take(17).collect();
        let shots = vec![
            shot(0, 0, "she speaks", Some(&line)),
            shot(1, 1, "he answers", Some(&line)),
        ];
        let merged = pack_scene_clips(SEEDANCE, shots);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().all(|s| !s.is_merged()));
    }

    /// Four silent beats need 20s; the window is 15s, so the fourth is a new
    /// clip. Different cameras do not themselves cause the split.
    #[test]
    fn a_run_that_overflows_the_window_starts_a_new_clip() {
        let shots: Vec<ShotDescription> = (0..4)
            .map(|i| shot(i, i, "hold", None))
            .collect();
        let merged = pack_scene_clips(SEEDANCE, shots);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].beats.len(), 3);
        assert!(merged[0].has_camera_cuts());
        assert_eq!(merged[1].idx, 3);
        assert!(!merged[1].is_merged());
    }

    #[test]
    fn a_run_stops_at_the_model_ceiling() {
        // Silent shots need the 5s model minimum each, so 15s fits exactly three.
        let shots: Vec<ShotDescription> =
            (0..5).map(|i| shot(i, 0, "hold", None)).collect();
        let merged = pack_scene_clips(SEEDANCE, shots);

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
        let merged = pack_scene_clips(narrow, shots);
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
        let once = pack_scene_clips(SEEDANCE, shots);
        let twice = pack_scene_clips(SEEDANCE, once.clone());
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
        let merged = pack_scene_clips(SEEDANCE, shots);
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
        let merged = pack_scene_clips(SEEDANCE, shots);
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
        let merged = pack_scene_clips(SEEDANCE, shots);
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
        let mut merged = pack_scene_clips(SEEDANCE, shots);
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
        let merged = pack_scene_clips(SEEDANCE, shots);
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
        let merged = pack_scene_clips(SEEDANCE, vec![a, b]);
        assert_eq!(merged.len(), 1);

        let timeline = clip_timeline(SEEDANCE, &merged[0], 12);
        assert_eq!(timeline.len(), 2);
        assert!(!timeline[0].text.contains("0-4s"), "{timeline:?}");
        assert!(!timeline[0].text.contains("4-7s"), "{timeline:?}");
        assert_eq!(timeline.last().expect("two beats").end_secs, 12);
    }

    #[test]
    fn pack_scene_briefs_collapses_micro_shots_and_reindexes() {
        let briefs = vec![
            brief(0, 0, "她转身开门", None),
            brief(1, 0, "她看见他", None),
            brief(2, 1, "他抬头", None),
            brief(3, 2, "窗外雨", None),
        ];
        let packed = pack_scene_briefs(SEEDANCE, briefs);
        // 5+5+5+5s: duration window fits three; the fourth is a new story clip,
        // not because its camera changed.
        assert_eq!(packed.len(), 2);
        assert_eq!(packed[0].idx, 0);
        assert_eq!(packed[1].idx, 1);
        assert!(packed[0].is_merged());
        assert_eq!(packed[0].beats.len(), 3);
        assert!(packed[0].visual_desc.contains("切到新机位"), "{}", packed[0].visual_desc);
        assert!(packed[1].is_last);
        assert!(!packed[0].is_last);
        assert_eq!(packed[0].exit_cam_idx(), 1);
        assert_eq!(packed[1].exit_cam_idx(), 2);
    }

    #[test]
    fn pack_scene_briefs_does_not_swallow_overflow_speech() {
        let line: String = "中".chars().cycle().take(17).collect();
        let briefs = vec![
            brief(0, 0, "她说话", Some(&line)),
            brief(1, 1, "他回答", Some(&line)),
        ];
        let packed = pack_scene_briefs(SEEDANCE, briefs);
        assert_eq!(packed.len(), 2);
        assert!(packed.iter().all(|b| !b.is_merged()));
        assert_eq!(packed[0].idx, 0);
        assert_eq!(packed[1].idx, 1);
    }

    #[test]
    fn pack_scene_briefs_is_idempotent() {
        let briefs = vec![
            brief(0, 0, "她转身", None),
            brief(1, 1, "他回答", None),
        ];
        let once = pack_scene_briefs(SEEDANCE, briefs);
        let twice = pack_scene_briefs(SEEDANCE, once.clone());
        assert_eq!(once.len(), twice.len());
        assert_eq!(once[0].beats.len(), twice[0].beats.len());
        assert_eq!(once[0].idx, twice[0].idx);
    }

    #[test]
    fn sync_storyboard_drops_absorbed_rows() {
        let briefs = vec![
            brief(0, 0, "她转身", None),
            brief(1, 0, "她走近", None),
            brief(2, 1, "他抬头", None),
        ];
        let clips = pack_scene_clips(
            SEEDANCE,
            vec![
                shot(0, 0, "她转身", None),
                shot(1, 0, "她走近", None),
                shot(2, 1, "他抬头", None),
            ],
        );
        assert_eq!(clips.len(), 1);
        let synced = sync_storyboard_to_clips(&briefs, &clips);
        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].idx, 0);
        assert!(synced[0].is_merged());
        assert_eq!(synced[0].beats.len(), 3);
        assert!(synced[0].is_last);
    }

    #[test]
    fn swapped_left_right_is_a_reverse_of_the_same_blocking() {
        let end = "近景,帅气男大学生在画面左侧一脚踩地刹车停住,身体转向右侧面向女生;女生在画面右侧蹲在散落的书本旁,面朝左下方伸手捡书";
        let next = "中近景,帅气男大学生位于画面右侧蹲着,面朝左下方,双手合拢几本书;女生位于画面左侧也蹲着,面朝右下方,正把一本笔记本抱回胸前";
        assert!(swapped_screen_blocking(end, next));
        assert!(!swapped_screen_blocking(end, end));
    }

    #[test]
    fn pack_scene_briefs_collapses_screen_direction_reverse() {
        let briefs = vec![
            brief(
                0,
                0,
                "近景,帅气男大学生在画面左侧刹车停住;女生在画面右侧蹲着捡书",
                Some("刹车声"),
            ),
            brief(
                1,
                1,
                "中近景,帅气男大学生位于画面右侧蹲着捡书;女生位于画面左侧抱起笔记本",
                Some("书本落地"),
            ),
        ];
        let packed = pack_scene_briefs(SEEDANCE, briefs);
        assert_eq!(packed.len(), 1, "{packed:?}");
        assert!(packed[0].is_merged());
        assert_eq!(packed[0].beats.len(), 2);
    }

    #[test]
    fn drop_stale_extra_clips_does_not_grow_the_storyboard() {
        let briefs = vec![brief(0, 0, "开门", None), brief(1, 0, "看见他", None)];
        let clips = vec![
            shot(0, 0, "开门", None),
            shot(1, 0, "看见他", None),
            shot(2, 1, "幽灵镜头", None),
        ];
        let clipped = drop_stale_extra_clips(&briefs, clips);
        assert_eq!(clipped.len(), 2);
        assert_eq!(clipped[1].idx, 1);
        let synced = sync_storyboard_to_clips(&briefs, &clipped);
        assert_eq!(synced.len(), 2);
    }

    #[test]
    fn align_drops_blank_last_brief_instead_of_revealing_it() {
        let mut blank = brief(3, 0, "", None);
        blank.visual_desc.clear();
        let briefs = vec![
            brief(0, 0, "开门", None),
            brief(1, 0, "看见他", None),
            blank,
        ];
        let clips = vec![
            shot(0, 0, "开门", None),
            shot(1, 0, "看见他", None),
            shot(3, 1, "幽灵镜头", None),
        ];
        let (board, clips) = align_storyboard_and_clips(briefs, clips);
        assert_eq!(board.len(), 2, "{board:?}");
        assert_eq!(clips.len(), 2);
        assert!(board.iter().all(|b| b.idx < 3));
    }

    #[test]
    fn align_does_not_invent_a_row_for_unknown_clip_idx() {
        let briefs = vec![brief(0, 0, "开门", None), brief(1, 0, "看见他", None)];
        let clips = vec![
            shot(0, 0, "开门", None),
            shot(1, 0, "看见他", None),
            shot(2, 1, "幽灵镜头", None),
        ];
        let (board, clips) = align_storyboard_and_clips(briefs, clips);
        assert_eq!(board.len(), 2);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1].idx, 1);
    }

    #[test]
    fn drop_stale_equal_length_stray_idx_is_still_dropped() {
        let briefs = vec![
            brief(0, 0, "开门", None),
            brief(1, 0, "看见他", None),
            brief(2, 0, "转身", None),
        ];
        let clips = vec![
            shot(0, 0, "开门", None),
            shot(1, 0, "看见他", None),
            shot(3, 1, "幽灵镜头", None),
        ];
        let clipped = drop_stale_extra_clips(&briefs, clips);
        assert_eq!(clipped.len(), 2);
        assert!(clipped.iter().all(|c| c.idx != 3));
        let synced = sync_storyboard_to_clips(&briefs, &clipped);
        assert_eq!(synced.len(), 2);
        assert!(synced.iter().all(|b| b.idx != 3));
    }

    #[test]
    fn densify_aligned_indices_closes_pack_holes() {
        let mut briefs = vec![brief(0, 0, "开门", None), brief(3, 1, "反打", None)];
        briefs[1].is_last = true;
        let mut clips = vec![shot(0, 0, "开门", None), shot(3, 1, "反打", None)];
        clips[1].is_last = true;
        let map = densify_aligned_indices(&mut briefs, &mut clips);
        assert_eq!(clips.iter().map(|c| c.idx).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(briefs.iter().map(|b| b.idx).collect::<Vec<_>>(), vec![0, 1]);
        assert!(clips[1].is_last);
        assert!(briefs[1].is_last);
        assert_eq!(map.get(&3), Some(&1));
        assert!(clip_indices_are_dense(&clips));
    }
}
