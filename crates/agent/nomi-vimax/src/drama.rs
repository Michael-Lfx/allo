//! Typed short-drama IR: a lintable "drama engine" (want / obstacle / stakes /
//! reversal + beat sheet) that planning validates BEFORE any image or video
//! model spends credits.
//!
//! Why this exists: thin drama ("戏太瘦") is not a prompt-wording problem — it
//! is prose with no rejectable structure. Prose cannot fail a check; a typed
//! engine can. The lints here are deterministic (no LLM judging LLM output),
//! so a failing plan is reproducible and the one repair round gets concrete,
//! actionable feedback instead of "make it better".
//!
//! Scope guard: everything in this module changes WHAT happens inside the
//! clips that planning already produces. It never adds storyboard rows, never
//! splits clips, and never touches clip packing — the "镜太多" fixes in
//! [`crate::planning`] and `pipelines/clip_beats.rs` stay authoritative.

use serde::{Deserialize, Serialize};

use crate::domain::{ShotBriefBeat, ShotBriefDescription};

/// Dramatic skeleton of the whole film, generated from the idea before the
/// story is written and persisted as `drama_engine.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DramaEngine {
    pub protagonist: String,
    /// Concrete desire driving the protagonist (visible goal, not a mood).
    pub want: String,
    /// The force actively blocking the want.
    pub obstacle: String,
    /// What is lost if the want fails — why the audience should care.
    pub stakes: String,
    /// Protagonist's power/relationship position when the film opens.
    pub status_start: String,
    /// Position when the film ends — MUST differ from `status_start`.
    pub status_end: String,
    /// The turn that flips expectation (the film's one real reversal).
    pub reversal: String,
    /// Recurring filmable motif (object / gesture / light cue).
    #[serde(default)]
    pub visual_motif: String,
    /// Play-order beat sheet; each beat is a dramatic promise the script and
    /// storyboard must keep.
    pub beats: Vec<DramaBeat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DramaBeat {
    pub role: BeatRole,
    /// ONE filmable visible behavior (bodies, props, space).
    pub action: String,
    /// What this beat changes in plot / relationship / status.
    pub advance: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeatRole {
    Hook,
    Incite,
    Escalate,
    Turn,
    Payoff,
    Button,
}

impl BeatRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::Incite => "incite",
            Self::Escalate => "escalate",
            Self::Turn => "turn",
            Self::Payoff => "payoff",
            Self::Button => "button",
        }
    }
}

pub const MIN_ENGINE_BEATS: usize = 4;
pub const MAX_ENGINE_BEATS: usize = 8;

/// Abstract emotion words that name a feeling instead of showing behavior.
/// A text is only flagged when it contains one of these AND no physical
/// anchor from [`PHYSICAL_ANCHORS`] — "他攥紧拳头，强忍愤怒" passes, "他很愤怒" fails.
const ABSTRACT_EMOTIONS: &[&str] = &[
    "伤心", "难过", "悲伤", "悲痛", "痛苦", "心痛", "心碎", "开心", "高兴", "快乐", "喜悦",
    "愤怒", "生气", "恼怒", "紧张", "焦虑", "不安", "害怕", "恐惧", "恐慌", "心动", "感动",
    "绝望", "震惊", "惊讶", "尴尬", "委屈", "失望", "愧疚", "内疚", "嫉妒", "羡慕",
    "sad", "angry", "furious", "nervous", "anxious", "afraid", "scared", "terrified",
    "heartbroken", "ashamed", "jealous", "desperate",
];

/// Visible-body / concrete-action tokens that "cash" an emotion on camera.
const PHYSICAL_ANCHORS: &[&str] = &[
    "手", "拳", "指", "掌", "眼", "目光", "泪", "眉", "嘴", "唇", "牙", "咬", "肩", "背",
    "膝", "腿", "脚", "脸", "颊", "额", "喉", "颤", "抖", "攥", "抿", "瞪", "盯", "眨",
    "皱", "僵", "缩", "退", "转身", "起身", "俯身", "冲", "摔", "砸", "推", "抓", "拍",
    "捏", "踩", "跪", "撞", "甩", "拽", "哽咽", "深吸", "呼吸", "停顿",
    "clench", "tremble", "slam", "grab", "fist", "eyes", "tear", "jaw", "shoulder",
    "freeze", "step", "turn", "breath", "stare", "bite", "grip", "flinch",
];

/// Padded-hold / wandering actions the storyboard prompt already forbids;
/// the lint makes the ban enforceable instead of advisory.
const FILLER_ACTIONS: &[&str] = &[
    "环顾四周",
    "缓缓环视",
    "陷入沉思",
    "望向远方",
    "看向远方",
    "looks around",
    "gazes into the distance",
];

/// Emotion words present without any physical anchor in the same text.
pub fn abstract_emotion_hits(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    if PHYSICAL_ANCHORS.iter().any(|a| lower.contains(a)) {
        return Vec::new();
    }
    ABSTRACT_EMOTIONS
        .iter()
        .copied()
        .filter(|w| lower.contains(w))
        .collect()
}

fn filler_hits(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    FILLER_ACTIONS
        .iter()
        .copied()
        .filter(|w| lower.contains(w))
        .collect()
}

/// Structural + show-don't-tell lint. Empty result means the engine is dense
/// enough to build a film on; each message is concrete repair feedback.
pub fn lint_drama_engine(engine: &DramaEngine) -> Vec<String> {
    let mut issues = Vec::new();
    for (field, value) in [
        ("protagonist", &engine.protagonist),
        ("want", &engine.want),
        ("obstacle", &engine.obstacle),
        ("stakes", &engine.stakes),
        ("status_start", &engine.status_start),
        ("status_end", &engine.status_end),
        ("reversal", &engine.reversal),
    ] {
        if value.trim().is_empty() {
            issues.push(format!("{field} 为空——没有它就没有戏剧发动机"));
        }
    }
    if !engine.status_start.trim().is_empty()
        && engine.status_start.trim() == engine.status_end.trim()
    {
        issues.push(
            "status_start 与 status_end 完全相同——全片主角地位/处境零变化，等于没有故事".into(),
        );
    }
    let n = engine.beats.len();
    if !(MIN_ENGINE_BEATS..=MAX_ENGINE_BEATS).contains(&n) {
        issues.push(format!(
            "节拍数 {n} 超出 {MIN_ENGINE_BEATS}–{MAX_ENGINE_BEATS} 范围"
        ));
    }
    if let Some(first) = engine.beats.first() {
        if first.role != BeatRole::Hook {
            issues.push("第一拍必须是 hook（开场即钩子），不能先铺垫".into());
        }
    }
    if !engine.beats.iter().any(|b| b.role == BeatRole::Turn) {
        issues.push("缺少 turn 节拍——没有反转的短剧就是流水账".into());
    }
    if !engine.beats.iter().any(|b| b.role == BeatRole::Payoff) {
        issues.push("缺少 payoff 节拍——钩子必须在画面上兑现".into());
    }
    for (i, beat) in engine.beats.iter().enumerate() {
        let role = beat.role.as_str();
        if beat.action.trim().is_empty() {
            issues.push(format!("beat {i} ({role}): action 为空"));
            continue;
        }
        let hits = abstract_emotion_hits(&beat.action);
        if !hits.is_empty() {
            issues.push(format!(
                "beat {i} ({role}): action 只有抽象情绪词「{}」——改写成可拍的身体行为（手/眼/走位/道具）",
                hits.join("、")
            ));
        }
    }
    issues
}

/// Requirement block that carries the validated engine into every downstream
/// LLM call (story, scene scripts, storyboards). Rendered as prose, not JSON,
/// so it reads like the other `[SECTION]` directives in the requirement.
pub fn drama_engine_block(engine: &DramaEngine) -> String {
    let mut beats = String::new();
    for (i, beat) in engine.beats.iter().enumerate() {
        beats.push_str(&format!(
            "           {}. [{}] {} ⇒ {}\n",
            i + 1,
            beat.role.as_str(),
            beat.action.trim(),
            beat.advance.trim()
        ));
    }
    let motif = engine.visual_motif.trim();
    let motif_line = if motif.is_empty() {
        String::new()
    } else {
        format!("         - 视觉母题（须跨场景复现）: {motif}\n")
    };
    format!(
        "[DRAMA_ENGINE — MUST FOLLOW]\n\
         - 主角: {protagonist}\n\
         - 想要 (Want): {want}\n\
         - 阻力 (Obstacle): {obstacle}\n\
         - 赌注 (Stakes): {stakes}\n\
         - 地位/处境弧线: {status_start} → {status_end}\n\
         - 反转 (Reversal): {reversal}\n\
{motif_line}\
         - 节拍表（按序执行，每一拍都是必须兑现的戏剧承诺）:\n\
{beats}\
         - 每一场必须推动 Want / Obstacle / 地位弧线至少一项；turn 与 payoff 节拍不得删除、稀释或合并掉。\n\
         - 情绪一律外化成可拍行为（手、眼、走位、道具），禁止只写抽象情绪词。\n\
         - 这是密度要求，不是镜头数要求：把戏做进已有的镜头里，不要为此增加镜头或切镜。",
        protagonist = engine.protagonist.trim(),
        want = engine.want.trim(),
        obstacle = engine.obstacle.trim(),
        stakes = engine.stakes.trim(),
        status_start = engine.status_start.trim(),
        status_end = engine.status_end.trim(),
        reversal = engine.reversal.trim(),
    )
}

/// Append the engine block to a base requirement (idempotent per plan run —
/// the base comes fresh from the session record every time).
pub fn with_drama_engine(base_requirement: &str, engine: &DramaEngine) -> String {
    let base = base_requirement.trim();
    let block = drama_engine_block(engine);
    if base.is_empty() {
        block
    } else {
        format!("{base}\n\n{block}")
    }
}

/// Show-don't-tell lint for one scene's screenplay text.
///
/// Only ACTION lines are scanned: dialogue may legitimately speak feelings
/// ("我很难过" is a line, not a stage direction), and parentheticals are
/// performance cues by convention, so both are excluded before matching.
pub fn lint_scene_action_lines(scene: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for (line_no, raw) in scene.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || is_dialogue_or_heading(line) {
            continue;
        }
        let action = strip_parentheticals(line);
        let hits = abstract_emotion_hits(&action);
        if !hits.is_empty() {
            issues.push(format!(
                "第{}行动作描写只有抽象情绪词「{}」——改写成可见的身体行为/表情/走位: {}",
                line_no + 1,
                hits.join("、"),
                clip_for_report(line)
            ));
        }
    }
    issues
}

/// Lint every scene; message prefix carries the scene index for repair feedback.
pub fn lint_scenes(scenes: &[String]) -> Vec<String> {
    let mut issues = Vec::new();
    for (i, scene) in scenes.iter().enumerate() {
        for issue in lint_scene_action_lines(scene) {
            issues.push(format!("场景{}: {issue}", i + 1));
        }
        for issue in lint_scene_spoken_plot(scene) {
            issues.push(format!("场景{}: {issue}", i + 1));
        }
    }
    issues
}

/// Short-drama viewers hear the plot; a mute pantomime scene is a defect.
///
/// A scene passes when it has at least one named-speaker line whose quoted
/// payload is long enough to carry information (not a grunt or a sound effect).
pub fn lint_scene_spoken_plot(scene: &str) -> Vec<String> {
    if scene_has_audible_plot(scene) {
        return Vec::new();
    }
    vec!["缺少能听懂剧情的角色对白——至少加一句带说话人姓名和「」的对白，把本场的欲望/阻碍/新信息说出来，不要只写动作默片".into()]
}

fn scene_has_audible_plot(scene: &str) -> bool {
    scene.lines().any(is_plot_dialogue_line)
}

fn is_plot_dialogue_line(line: &str) -> bool {
    is_character_dialogue_line(line) && spoken_payload_weight(&dialogue_quoted_payload(line)) >= 3
}

fn is_character_dialogue_line(line: &str) -> bool {
    let line = line.trim().trim_start_matches('△').trim();
    if line.is_empty() || is_scene_heading_line(line) {
        return false;
    }
    let Some(pos) = line.find(['：', ':']) else {
        return false;
    };
    let speaker = line[..pos].trim();
    let len = speaker.chars().count();
    if !(1..=12).contains(&len) || speaker.chars().any(char::is_whitespace) {
        return false;
    }
    !dialogue_quoted_payload(line).trim().is_empty()
}

fn is_scene_heading_line(line: &str) -> bool {
    let upper = line.to_uppercase();
    line.starts_with("场景")
        || line.starts_with("内景")
        || line.starts_with("外景")
        || upper.starts_with("INT.")
        || upper.starts_with("EXT.")
        || upper.starts_with("SCENE")
}

fn dialogue_quoted_payload(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut chunks = String::new();
    while i < chars.len() {
        let close = match chars[i] {
            '「' => Some('」'),
            '“' => Some('”'),
            '"' => Some('"'),
            _ => None,
        };
        if let Some(close) = close {
            i += 1;
            while i < chars.len() && chars[i] != close {
                chunks.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    chunks
}

fn spoken_payload_weight(text: &str) -> u32 {
    let mut cjk = 0u32;
    let mut words = 0u32;
    let mut in_word = false;
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            cjk += 1;
            in_word = false;
        } else if ch.is_ascii_alphabetic() {
            if !in_word {
                words += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    cjk + words
}

/// Performance lint on a designed storyboard.
///
/// Flags rows whose visual content names emotions instead of behavior, and
/// rows containing forbidden filler holds. Repair rewrites text INSIDE the
/// flagged rows only — callers must keep the row count unchanged so this can
/// never regrow the shot count.
pub fn lint_storyboard_performance(rows: &[ShotBriefDescription]) -> Vec<String> {
    let mut issues = Vec::new();
    for row in rows {
        let mut texts: Vec<&str> = vec![&row.visual_desc];
        texts.extend(row.beats.iter().map(|b| b.visual_desc.as_str()));
        for text in texts {
            let hits = abstract_emotion_hits(text);
            if !hits.is_empty() {
                issues.push(format!(
                    "shot {}: 抽象情绪词「{}」没有对应的身体表演——在同一行内改写成可拍的表演拍（表情/手部/走位/道具），不要新增镜头: {}",
                    row.idx,
                    hits.join("、"),
                    clip_for_report(text)
                ));
            }
            let fillers = filler_hits(text);
            if !fillers.is_empty() {
                issues.push(format!(
                    "shot {}: 空持镜动作「{}」被禁止——换成推进剧情的具体行为，不要新增镜头: {}",
                    row.idx,
                    fillers.join("、"),
                    clip_for_report(text)
                ));
            }
        }
    }
    issues
}

/// Structure guard for the storyboard performance repair round: the repaired
/// board may only rewrite TEXT inside flagged rows. Row count, row identity
/// (`idx` / `cam_idx` / `is_last`) and the beat skeleton (count + per-beat
/// `cam_idx`) must be byte-identical — beats with a different `cam_idx` become
/// native in-clip cuts downstream, so letting a repair grow or re-camera them
/// would quietly reintroduce "镜太多".
pub fn storyboard_structure_matches(
    original: &[ShotBriefDescription],
    repaired: &[ShotBriefDescription],
) -> bool {
    original.len() == repaired.len()
        && original.iter().zip(repaired).all(|(a, b)| {
            a.idx == b.idx
                && a.cam_idx == b.cam_idx
                && a.is_last == b.is_last
                && a.beats.len() == b.beats.len()
                && a.beats
                    .iter()
                    .zip(&b.beats)
                    .all(|(ba, bb)| ba.cam_idx == bb.cam_idx)
        })
}

/// Performance beats that live *inside* an already-packed clip.
///
/// 2–3 same-camera actions so `clip_timeline` can compile a Seedance beat
/// script at render time. Never more than 3: stuffing a clip is how "镜太多"
/// sneaks back in as in-file CUT TO. Camera must stay the row's `cam_idx`.
pub const MIN_IN_CLIP_BEATS: usize = 2;
pub const MAX_IN_CLIP_BEATS: usize = 3;

pub fn needs_in_clip_beats(row: &ShotBriefDescription) -> bool {
    row.beats.len() < MIN_IN_CLIP_BEATS
}

/// Merge LLM-proposed same-camera performance beats onto rows that still
/// have fewer than [`MIN_IN_CLIP_BEATS`]. Packed / already-dense rows are
/// left untouched. Returns `None` when the proposal changes row identity
/// or count — callers must keep the original board.
pub fn apply_in_clip_performance_beats(
    original: &[ShotBriefDescription],
    proposed: &[ShotBriefDescription],
) -> Option<Vec<ShotBriefDescription>> {
    if original.len() != proposed.len() {
        return None;
    }
    let mut out = original.to_vec();
    for (dst, src) in out.iter_mut().zip(proposed) {
        if dst.idx != src.idx || dst.cam_idx != src.cam_idx || dst.is_last != src.is_last {
            return None;
        }
        if !needs_in_clip_beats(dst) {
            continue;
        }
        if !in_clip_beats_acceptable(dst.cam_idx, &src.beats) {
            continue;
        }
        dst.beats = src.beats.clone();
    }
    Some(out)
}

fn in_clip_beats_acceptable(row_cam: i32, beats: &[ShotBriefBeat]) -> bool {
    (MIN_IN_CLIP_BEATS..=MAX_IN_CLIP_BEATS).contains(&beats.len())
        && beats.iter().all(|beat| {
            beat.cam_idx == row_cam && !beat.visual_desc.trim().is_empty()
        })
}

fn is_dialogue_or_heading(line: &str) -> bool {
    if line.contains('「') || line.contains('“') || line.contains('"') {
        return true;
    }
    // Scene headings / sluglines.
    if is_scene_heading_line(line) {
        return true;
    }
    // Speaker-prefixed dialogue: 李薇：…… / Alice: …
    if let Some(pos) = line.find(['：', ':']) {
        let speaker = &line[..pos];
        let len = speaker.chars().count();
        if (1..=12).contains(&len) && !speaker.chars().any(char::is_whitespace) {
            return true;
        }
    }
    false
}

fn strip_parentheticals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth = 0usize;
    for ch in line.chars() {
        match ch {
            '（' | '(' => depth += 1,
            '）' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

fn clip_for_report(text: &str) -> String {
    const MAX: usize = 40;
    let mut s: String = text.chars().take(MAX).collect();
    if text.chars().count() > MAX {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dense_engine() -> DramaEngine {
        DramaEngine {
            protagonist: "林晚".into(),
            want: "拿回被姐姐顶替的大学录取通知书".into(),
            obstacle: "母亲当众撕掉通知书，逼她认命".into(),
            stakes: "认命就一辈子留在县城纺织厂".into(),
            status_start: "家里最不被看见的小女儿".into(),
            status_end: "拿着复印件独自登上北上的火车".into(),
            reversal: "帮她保管录取副本的竟是一直沉默的父亲".into(),
            visual_motif: "折成纸船的通知书复印件".into(),
            beats: vec![
                DramaBeat {
                    role: BeatRole::Hook,
                    action: "林晚从垃圾桶里抢出被撕成两半的通知书，攥进袖口".into(),
                    advance: "亮出欲望与家庭对立".into(),
                },
                DramaBeat {
                    role: BeatRole::Escalate,
                    action: "姐姐把行李箱摔在林晚脚边，抢走她的身份证".into(),
                    advance: "阻力升级为实际控制".into(),
                },
                DramaBeat {
                    role: BeatRole::Turn,
                    action: "父亲默默推来一辆自行车，车筐里是完整的录取副本".into(),
                    advance: "同盟反转，力量易位".into(),
                },
                DramaBeat {
                    role: BeatRole::Payoff,
                    action: "林晚在站台展开折成纸船的复印件，检票进站".into(),
                    advance: "欲望兑现，地位改变".into(),
                },
            ],
        }
    }

    #[test]
    fn dense_engine_passes_lint() {
        assert!(lint_drama_engine(&dense_engine()).is_empty());
    }

    #[test]
    fn thin_engine_is_rejected() {
        // The "戏太瘦" fixture: no obstacle, no status change, mood-word beats.
        let engine = DramaEngine {
            protagonist: "小美".into(),
            want: "过上幸福生活".into(),
            obstacle: "".into(),
            stakes: "".into(),
            status_start: "普通女孩".into(),
            status_end: "普通女孩".into(),
            reversal: "".into(),
            visual_motif: String::new(),
            beats: vec![
                DramaBeat {
                    role: BeatRole::Escalate,
                    action: "小美很伤心".into(),
                    advance: "情绪变化".into(),
                },
                DramaBeat {
                    role: BeatRole::Escalate,
                    action: "小美很开心".into(),
                    advance: "情绪变化".into(),
                },
            ],
        };
        let issues = lint_drama_engine(&engine);
        assert!(issues.iter().any(|i| i.contains("obstacle")));
        assert!(issues.iter().any(|i| i.contains("status_start 与 status_end")));
        assert!(issues.iter().any(|i| i.contains("节拍数")));
        assert!(issues.iter().any(|i| i.contains("hook")));
        assert!(issues.iter().any(|i| i.contains("turn")));
        assert!(issues.iter().any(|i| i.contains("payoff")));
        assert!(issues.iter().any(|i| i.contains("抽象情绪词")));
    }

    #[test]
    fn emotion_with_physical_anchor_passes() {
        assert!(abstract_emotion_hits("她攥紧拳头，指节发白，强忍愤怒").is_empty());
        assert_eq!(abstract_emotion_hits("她非常愤怒"), vec!["愤怒"]);
    }

    #[test]
    fn mute_scene_fails_spoken_plot_lint() {
        let mute = "场景一：雨巷\n△ 两人沉默对视，雨落在石板上。";
        let issues = lint_scene_spoken_plot(mute);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("对白"));

        let spoken = "场景一：雨巷\n林晚：「今晚别等我。」\n△ 她把伞递过去。";
        assert!(lint_scene_spoken_plot(spoken).is_empty());
    }

    fn scene_lint_skips_dialogue_and_parentheticals() {
        let scene = "场景一：县城车站\n\
                     林晚：「我真的很难过。」\n\
                     △ 林晚（伤心地）把票根塞进口袋，转身走向检票口。\n\
                     △ 姐姐很愤怒。";
        let issues = lint_scene_action_lines(scene);
        // Spoken feeling + parenthetical cue are fine; bare mood action line is not.
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("愤怒"));
    }

    #[test]
    fn storyboard_lint_flags_mood_words_and_filler() {
        let rows = vec![
            ShotBriefDescription {
                idx: 0,
                is_last: false,
                cam_idx: 0,
                visual_desc: "<林晚>攥着撕碎的通知书冲进雨里，纸片粘在掌心".into(),
                audio_desc: Some("雨声".into()),
                beats: vec![],
            },
            ShotBriefDescription {
                idx: 1,
                is_last: true,
                cam_idx: 1,
                visual_desc: "<姐姐>站在门口，非常愤怒，然后环顾四周".into(),
                audio_desc: Some("门响".into()),
                beats: vec![],
            },
        ];
        let issues = lint_storyboard_performance(&rows);
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().all(|i| i.contains("shot 1")));
        assert!(issues.iter().any(|i| i.contains("愤怒")));
        assert!(issues.iter().any(|i| i.contains("环顾四周")));
    }

    #[test]
    fn structure_guard_rejects_grown_or_recut_boards() {
        use crate::domain::ShotBriefBeat;
        let row = |idx: i32, beats: Vec<i32>| ShotBriefDescription {
            idx,
            is_last: false,
            cam_idx: idx,
            visual_desc: "v".into(),
            audio_desc: None,
            beats: beats
                .into_iter()
                .map(|cam_idx| ShotBriefBeat {
                    visual_desc: "b".into(),
                    audio_desc: None,
                    cam_idx,
                })
                .collect(),
        };
        let original = vec![row(0, vec![0, 1]), row(1, vec![])];
        // Text-only rewrite passes.
        let mut rewritten = original.clone();
        rewritten[0].visual_desc = "她攥紧拳头".into();
        rewritten[0].beats[1].visual_desc = "他后退半步".into();
        assert!(storyboard_structure_matches(&original, &rewritten));
        // Extra row = new shot → rejected.
        let grown = vec![row(0, vec![0, 1]), row(1, vec![]), row(2, vec![])];
        assert!(!storyboard_structure_matches(&original, &grown));
        // Extra beat = potential in-clip cut → rejected.
        let more_beats = vec![row(0, vec![0, 1, 2]), row(1, vec![])];
        assert!(!storyboard_structure_matches(&original, &more_beats));
        // Re-camera'd beat → rejected.
        let recut = vec![row(0, vec![0, 2]), row(1, vec![])];
        assert!(!storyboard_structure_matches(&original, &recut));
    }

    #[test]
    fn in_clip_beats_fill_thin_rows_without_recutting() {
        use crate::domain::ShotBriefBeat;
        let thin = ShotBriefDescription {
            idx: 0,
            is_last: true,
            cam_idx: 3,
            visual_desc: "<林晚>攥着碎纸".into(),
            audio_desc: None,
            beats: vec![],
        };
        let packed = ShotBriefDescription {
            idx: 1,
            is_last: false,
            cam_idx: 0,
            visual_desc: "packed".into(),
            audio_desc: None,
            beats: vec![
                ShotBriefBeat {
                    visual_desc: "a".into(),
                    audio_desc: None,
                    cam_idx: 0,
                },
                ShotBriefBeat {
                    visual_desc: "b".into(),
                    audio_desc: None,
                    cam_idx: 1,
                },
            ],
        };
        let original = vec![thin.clone(), packed.clone()];
        let mut proposed = original.clone();
        proposed[0].beats = vec![
            ShotBriefBeat {
                visual_desc: "攥紧碎纸".into(),
                audio_desc: None,
                cam_idx: 3,
            },
            ShotBriefBeat {
                visual_desc: "塞进袖口".into(),
                audio_desc: None,
                cam_idx: 3,
            },
        ];
        let merged = apply_in_clip_performance_beats(&original, &proposed).expect("merge");
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].beats.len(), 2);
        assert!(merged[0].beats.iter().all(|b| b.cam_idx == 3));
        // Packed reverse-angle beats stay exactly as packing left them.
        assert_eq!(merged[1].beats.len(), 2);
        assert_eq!(merged[1].beats[1].cam_idx, 1);

        // Extra row → reject the whole proposal.
        proposed.push(thin);
        assert!(apply_in_clip_performance_beats(&original, &proposed).is_none());

        // Recut beats on a thin row are skipped, not applied.
        let mut recut = original.clone();
        recut[0].beats = vec![
            ShotBriefBeat {
                visual_desc: "a".into(),
                audio_desc: None,
                cam_idx: 3,
            },
            ShotBriefBeat {
                visual_desc: "b".into(),
                audio_desc: None,
                cam_idx: 9,
            },
        ];
        let kept = apply_in_clip_performance_beats(&original, &recut).expect("skip recut");
        assert!(kept[0].beats.is_empty());
    }

    #[test]
    fn engine_block_carries_beats_and_scope_guard() {
        let block = drama_engine_block(&dense_engine());
        assert!(block.starts_with("[DRAMA_ENGINE"));
        assert!(block.contains("[hook]"));
        assert!(block.contains("[payoff]"));
        assert!(block.contains("纸船"));
        assert!(block.contains("不要为此增加镜头"));
        let composed = with_drama_engine("多用日常场景", &dense_engine());
        assert!(composed.starts_with("多用日常场景"));
        assert!(composed.contains("[DRAMA_ENGINE"));
    }
}
