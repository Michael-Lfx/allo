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
//! Scope guard: the engine lints WHAT happens in the film. Clip packing and
//! duration folding live in [`crate::planning`] and `pipelines/clip_beats.rs`.
//! Coverage repair inserts a locked `engine.action` onto the scene that owns
//! the beat when that beat is missing from the concatenated boards.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use crate::domain::ShotBriefDescription;

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

pub fn load_drama_engine(dir: &Path) -> Option<DramaEngine> {
    for candidate in [dir, dir.parent().unwrap_or(dir)] {
        let path = candidate.join("drama_engine.json");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(engine) = serde_json::from_str(&raw) {
                return Some(engine);
            }
        }
    }
    None
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

/// Where the engine contract is injected. Film = story/screenplay; Scene =
/// one storyboard, which must not restage other scenes' beats.
#[derive(Clone, Copy)]
enum DramaContractScope {
    Film,
    Scene,
}

/// Requirement block that carries the validated engine into every downstream
/// LLM call (story, scene scripts, storyboards). Rendered as prose, not JSON,
/// so it reads like the other `[SECTION]` directives in the requirement.
pub fn drama_engine_block(engine: &DramaEngine) -> String {
    drama_engine_block_for(engine, DramaContractScope::Film)
}

fn drama_engine_block_for(engine: &DramaEngine, scope: DramaContractScope) -> String {
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
    let rules = match scope {
        DramaContractScope::Film => {
            "         - 节拍表按序在成片某处兑现即可，尤其 turn 与 payoff 不得从全片消失。\n\
         - 每一场推动 Want / Obstacle / 地位弧线至少一项；同一拍不要在多场各演一遍。\n\
         - 情绪一律外化成可拍行为（手、眼、走位、道具），禁止只写抽象情绪词。\n\
         - 密度优先做进已有成片；时长不够时把后半场折进最后一场，不要砍掉反转与收束。跨 turn/payoff 必须拆成新的成片行。"
        }
        DramaContractScope::Scene => {
            "         - 本场分镜只拍摄 SCRIPT 里已经发生的事，禁止把其他场的 hook/turn/payoff 再写进本场。\n\
         - 本场仍须推动 Want / Obstacle / 地位弧线至少一项。\n\
         - 情绪一律外化成可拍行为（手、眼、走位、道具），禁止只写抽象情绪词。\n\
         - 本场若包含 turn/payoff，跨该拍拆成新的成片行；不要为整片节拍表另造镜头。"
        }
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
{rules}",
        protagonist = engine.protagonist.trim(),
        want = engine.want.trim(),
        obstacle = engine.obstacle.trim(),
        stakes = engine.stakes.trim(),
        status_start = engine.status_start.trim(),
        status_end = engine.status_end.trim(),
        reversal = engine.reversal.trim(),
    )
}

fn compose_requirement(base_requirement: &str, block: String) -> String {
    let base = base_requirement.trim();
    if base.is_empty() {
        block
    } else {
        format!("{base}\n\n{block}")
    }
}

/// Append the engine block to a base requirement (idempotent per plan run —
/// the base comes fresh from the session record every time).
pub fn with_drama_engine(base_requirement: &str, engine: &DramaEngine) -> String {
    compose_requirement(base_requirement, drama_engine_block(engine))
}

/// Scene storyboard contract: film this SCRIPT only, do not restage the film.
pub fn with_scene_drama_engine(base_requirement: &str, engine: &DramaEngine) -> String {
    compose_requirement(
        base_requirement,
        drama_engine_block_for(engine, DramaContractScope::Scene),
    )
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

fn has_screen_side(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("画面左")
        || text.contains("画面右")
        || text.contains("画面中")
        || text.contains("画面正中")
        || lower.contains("screen left")
        || lower.contains("screen right")
        || lower.contains("screen center")
}

fn has_facing(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("面向")
        || text.contains("背对")
        || lower.contains("facing ")
        || lower.contains("back to camera")
        || lower.contains("back to the camera")
}

/// Angle-bracket names in picture text. ASCII `<>` and fullwidth `〈〉` both
/// count. A bracket closed by 道具 is a prop, not a body.
pub(crate) fn bracket_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '<' => '>',
            '〈' => '〉',
            _ => {
                i += 1;
                continue;
            }
        };
        let start = i + 1;
        let Some(rel) = chars[start..].iter().position(|c| *c == close) else {
            break;
        };
        let name: String = chars[start..start + rel].iter().collect();
        let name = name.trim();
        let after = start + rel + 1;
        let tail: String = chars.get(after..).unwrap_or(&[]).iter().take(6).collect();
        if name.chars().count() >= 2
            && !name.chars().any(|c| matches!(c, '\n' | '<' | '>' | '〈' | '〉'))
            && !tail.contains("道具")
            && !out.iter().any(|n| n == name)
        {
            out.push(name.to_string());
        }
        i = after;
    }
    out
}

/// This name owns a screen side here: `<名> 在画面左/右/中`, or `画面左侧的<名>`.
/// A name that only appears as someone else's anchor (`高二林身后`) does not.
pub(crate) fn name_has_own_screen_side(text: &str, name: &str) -> bool {
    let name = name.trim();
    if name.chars().count() < 2 || !text.contains(name) {
        return false;
    }
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        let end = at + name.len();
        if places_name_forward(&text[end..]) || screen_side_names_person(&text[..at]) {
            return true;
        }
        from = end;
    }
    false
}

fn places_name_forward(after: &str) -> bool {
    let s = after.trim_start_matches(['>', '》', '〉', ' ', '\t', '\n', '　']);
    let s = s.trim_start();
    let s = s.strip_prefix("位于").unwrap_or(s).trim_start();
    let s = s.strip_prefix("在").unwrap_or(s).trim_start();
    let lower = s.to_ascii_lowercase();
    const EN: &[&str] = &[
        "on the screen left",
        "on the screen right",
        "on the screen center",
        "on screen left",
        "on screen right",
        "on screen center",
        "at the screen left",
        "at the screen right",
        "at the screen center",
        "at screen left",
        "at screen right",
        "at screen center",
        "screen left",
        "screen right",
        "screen center",
    ];
    if EN.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    if s.starts_with("画面中景") {
        return false;
    }
    s.starts_with("画面左")
        || s.starts_with("画面右")
        || s.starts_with("画面正中")
        || s.starts_with("画面中")
}

/// The clause immediately before the name is itself a screen side (`画面左侧的`).
/// `在画面右侧、高二林身后` does not place 高二林: the顿号 starts a new clause.
fn screen_side_names_person(before: &str) -> bool {
    let tail = before
        .rsplit(['，', ',', '。', '！', '？', '；', ';', '、', '\n', '!', '?'])
        .next()
        .unwrap_or(before);
    is_exact_screen_side(strip_side_connectors(tail))
}

fn strip_side_connectors(clause: &str) -> &str {
    let mut t = clause.trim();
    loop {
        let trimmed = t.trim_end_matches([' ', '\t', '<', '〈', '〉', '的', '是', '有', '　']);
        let next = ["坐着", "站着", "趴着", "跪着", "走着", "坐", "站"]
            .iter()
            .find_map(|suf| trimmed.strip_suffix(suf))
            .unwrap_or(trimmed)
            .trim_end();
        if next.len() == t.len() {
            return next;
        }
        t = next;
    }
}

fn is_exact_screen_side(phrase: &str) -> bool {
    let p = phrase.trim();
    let p = p.strip_prefix("位于").unwrap_or(p).trim_start();
    let p = p.strip_prefix("在").unwrap_or(p).trim_start();
    if matches!(
        p,
        "画面左侧"
            | "画面右侧"
            | "画面左边"
            | "画面右边"
            | "画面正中"
            | "画面中央"
            | "画面左"
            | "画面右"
            | "画面中"
    ) {
        return true;
    }
    let lower = p.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "screen left"
            | "screen right"
            | "screen center"
            | "the screen left"
            | "the screen right"
            | "the screen center"
    )
}

fn people_missing_own_side(visual: &str, audio: &str) -> Vec<String> {
    let mut names = bracket_names(visual);
    for name in crate::dialogue::spoken_speaker_names(audio) {
        if !names.iter().any(|n| n == &name) {
            names.push(name);
        }
    }
    names
        .into_iter()
        .filter(|name| !name_has_own_screen_side(visual, name))
        .collect()
}

/// Rows whose picture text never places anyone. The video model does not see
/// the previous clip, so a missing side or facing cannot be recovered later.
pub fn lint_storyboard_blocking(rows: &[ShotBriefDescription]) -> Vec<String> {
    let mut issues = Vec::new();
    for row in rows {
        let mut texts: Vec<&str> = vec![row.visual_desc.as_str()];
        if row.beats.len() >= 2 {
            texts.extend(row.beats.iter().map(|b| b.visual_desc.as_str()));
        }
        for text in texts {
            let text = text.trim();
            if text.chars().count() < 8 {
                continue;
            }
            let side = has_screen_side(text);
            let facing = has_facing(text);
            if !side || !facing {
                let mut missing = Vec::new();
                if !side {
                    missing.push("画面左/右/中");
                }
                if !facing {
                    missing.push("朝向");
                }
                issues.push(format!(
                    "shot {}: 站位不完整，缺{}。在同一行写清景别、每个人的画面左右、朝向、姿势、谁更靠近镜头、与固定陈设的距离，不要新增镜头: {}",
                    row.idx,
                    missing.join("和"),
                    clip_for_report(text)
                ));
                continue;
            }
            // The joined parent visual places a person who only stands in one
            // beat. Per-person checks run on each beat, and on a single-beat row.
            if row.beats.len() >= 2 && text == row.visual_desc.trim() {
                continue;
            }
            let audio = audio_for_blocking_text(row, text);
            for name in people_missing_own_side(text, audio) {
                issues.push(format!(
                    "shot {}: {name} 在这一拍出镜或开口，但没有自己的画面左/右/中。在同一拍给这个人单独写站位、朝向、姿势和与固定陈设的距离；一个身份只能有一个身体，没写名字的座位不能留给参考脸。不要新增镜头: {}",
                    row.idx,
                    clip_for_report(text)
                ));
            }
        }
    }
    issues.extend(lint_roster_hold(rows));
    issues.extend(lint_carried_prop_state(rows));
    issues
}

/// "同一机位、左右不变" after the people changed tells the model to keep a
/// blocking that is no longer true.
fn lint_roster_hold(rows: &[ShotBriefDescription]) -> Vec<String> {
    let mut issues = Vec::new();
    for pair in rows.windows(2) {
        let prev = bracket_names(&pair[0].visual_desc);
        let next = bracket_names(&pair[1].visual_desc);
        if prev.is_empty() || next.is_empty() || same_names(&prev, &next) {
            continue;
        }
        let text = pair[1].visual_desc.as_str();
        if text.contains("同一机位") || text.contains("左右不变") {
            issues.push(format!(
                "shot {}: 入画的人已经变了，不能再写同一机位或左右不变。按这一行的人重写画面左右、朝向和姿势: {}",
                pair[1].idx,
                clip_for_report(text)
            ));
        }
    }
    issues
}

fn same_names(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|name| b.iter().any(|other| other == name))
}

const CARRIED_PROP_STATES: &[&str] = &[
    "头顶", "后脑", "头上", "太阳穴", "手里", "手中", "口袋",
];

/// A prop that leaves on a body has to be in the next row's opening, unless
/// that row says it was put down or the person left.
fn lint_carried_prop_state(rows: &[ShotBriefDescription]) -> Vec<String> {
    let mut issues = Vec::new();
    for pair in rows.windows(2) {
        if crate::domain::location_changed(&pair[0].location_id, &pair[1].location_id) {
            continue;
        }
        let next = pair[1].visual_desc.as_str();
        if next.trim().is_empty() {
            continue;
        }
        for (prop, state) in carried_prop_states(&pair[0].visual_desc) {
            if next.contains(&state) || prop_was_released(next, &prop) {
                continue;
            }
            issues.push(format!(
                "shot {}: 上一行结束时{prop}在{state}，这一行开场要写上这个状态，或写明它放下了、扔掉了、人离开了: {}",
                pair[1].idx,
                clip_for_report(next)
            ));
        }
    }
    issues
}

fn carried_prop_states(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for state in CARRIED_PROP_STATES {
        let mut from = 0;
        while let Some(rel) = text[from..].find(state) {
            let at = from + rel;
            if let Some(prop) = prop_noun_near(text, at, state.len()) {
                if !out.iter().any(|(have, s)| have == &prop && s == *state) {
                    out.push((prop, (*state).to_string()));
                }
            }
            from = at + state.len();
        }
    }
    out
}

fn prop_noun_near(text: &str, state_at: usize, state_len: usize) -> Option<String> {
    let after = &text[state_at + state_len..];
    if let Some(rest) = after.strip_prefix('的') {
        if let Some(noun) = leading_han_noun(rest) {
            return Some(noun);
        }
    }
    trailing_han_noun(&text[..state_at])
}

fn trailing_han_noun(before: &str) -> Option<String> {
    const SKIP: &[char] = &['在', '到', '了', '着', '的', '把', '将', '被', '给', '向', '往', '上'];
    let chars: Vec<char> = before.chars().collect();
    let mut end = chars.len();
    while end > 0 && !is_han(chars[end - 1]) {
        end -= 1;
    }
    while end > 0 && SKIP.contains(&chars[end - 1]) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_han(chars[start - 1]) && end - start < 4 {
        start -= 1;
    }
    clean_prop_noun(&chars[start..end].iter().collect::<String>())
}

fn leading_han_noun(after: &str) -> Option<String> {
    let mut noun = String::new();
    for ch in after.chars() {
        if is_han(ch) && noun.chars().count() < 4 {
            noun.push(ch);
        } else {
            break;
        }
    }
    clean_prop_noun(&noun)
}

fn clean_prop_noun(noun: &str) -> Option<String> {
    const SKIP: &[&str] = &[
        "他", "她", "它", "我", "你", "自己", "一个", "这个", "那个", "手里", "手中", "头顶",
    ];
    let noun = noun.trim();
    if noun.chars().count() < 2 || SKIP.iter().any(|s| *s == noun) {
        return None;
    }
    Some(noun.to_string())
}

fn is_han(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn prop_was_released(text: &str, _prop: &str) -> bool {
    const RELEASE: &[&str] = &["放下", "扔掉", "丢掉", "收起", "离开", "离场", "不见", "拿开"];
    RELEASE.iter().any(|word| text.contains(word))
}

/// Beat text is checked against that beat's lines. A single-beat row uses the
/// row audio. The joined parent visual is not a beat, so it does not borrow audio.
fn audio_for_blocking_text<'a>(row: &'a ShotBriefDescription, text: &str) -> &'a str {
    if row.beats.len() >= 2 {
        return row
            .beats
            .iter()
            .find(|beat| beat.visual_desc.as_str() == text)
            .and_then(|beat| beat.audio_desc.as_deref())
            .unwrap_or("");
    }
    if text == row.visual_desc.as_str() {
        row.audio_desc.as_deref().unwrap_or("")
    } else {
        ""
    }
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

/// Distinctive tokens from turn / payoff / reversal used to split packing
/// so a reversal does not get absorbed into the previous clip.
pub fn pack_split_needles(engine: &DramaEngine) -> Vec<String> {
    let mut parts = vec![engine.reversal.as_str()];
    for beat in &engine.beats {
        if matches!(beat.role, BeatRole::Turn | BeatRole::Payoff) {
            parts.push(beat.action.as_str());
        }
    }
    distinctive_needles(&parts.join("\n"))
}

pub fn lint_storyboard_coverage(engine: &DramaEngine, rows: &[ShotBriefDescription]) -> Vec<String> {
    uncovered_critical_beats(engine, rows)
        .into_iter()
        .map(|beat| {
            format!(
                "{} 节拍未出现在任何成片行——必须有对应镜头: {}",
                beat.role.as_str(),
                clip_for_report(&beat.action)
            )
        })
        .collect()
}

pub fn uncovered_critical_beats<'a>(
    engine: &'a DramaEngine,
    rows: &[ShotBriefDescription],
) -> Vec<&'a DramaBeat> {
    let blob = board_blob(rows);
    engine
        .beats
        .iter()
        .filter(|beat| matches!(beat.role, BeatRole::Turn | BeatRole::Payoff | BeatRole::Hook))
        .filter(|beat| !beat_covered_in(&blob, beat))
        .collect()
}

/// Insert missing turn/payoff/hook onto this board. Returns whether it grew.
/// Caller packs afterwards.
pub fn ensure_storyboard_coverage(
    engine: &DramaEngine,
    rows: &mut Vec<ShotBriefDescription>,
) -> bool {
    let mut boards = vec![std::mem::take(rows)];
    let dirty = place_missing_film_beats(engine, &[""], &mut boards);
    *rows = boards.pop().unwrap_or_default();
    !dirty.is_empty()
}

/// Quoted lines the source SCRIPT already locked. Missing them is 少戏, not a
/// cue to invent extra rows — the storyboard artist must rewrite the board.
pub fn lint_script_promise_coverage(script: &str, rows: &[ShotBriefDescription]) -> Vec<String> {
    let blob = board_blob(rows);
    let mut missing: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for quote in extract_quoted_promises(script) {
        if !seen.insert(quote.clone()) {
            continue;
        }
        if quote_covered_in(&blob, &quote) {
            continue;
        }
        missing.push(quote);
    }
    if missing.len() > MAX_PROMISE_LINT {
        missing = missing.split_off(missing.len() - MAX_PROMISE_LINT);
    }
    missing
        .into_iter()
        .map(|quote| format!("剧本原句未出现在任何分镜：{quote}"))
        .collect()
}

/// Dialogue-heavy scripts can lock many lines; lint the uncovered tail so the
/// repair round focuses on the closing payoff, not every paraphrased aside.
const MAX_PROMISE_LINT: usize = 4;

fn extract_quoted_promises(script: &str) -> Vec<String> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let closer = match chars[i] {
            '「' => Some('」'),
            '“' => Some('”'),
            '"' => Some('"'),
            _ => None,
        };
        if let Some(end_ch) = closer {
            let open_at = i;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != end_ch {
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let quote: String = chars[start..i].iter().collect();
            let quote = quote.trim();
            if is_locked_script_line(quote) && quote_is_spoken_dialogue(&chars, open_at) {
                out.push(quote.to_string());
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

/// Only named-speaker / speech-verb lines are promises. Scare-quoted terms in
/// prose (`在「千年的时间尺度」上`) and on-screen cards are not spoken plot.
fn quote_is_spoken_dialogue(chars: &[char], open_at: usize) -> bool {
    let mut j = open_at;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    if j == 0 {
        return false;
    }
    let prefix: String = chars[..j].iter().collect();
    let line_start = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_prefix = prefix[line_start..].trim_end();
    if citation_hedge(line_prefix) {
        return false;
    }
    if let Some(speaker) = speaker_before_colon(line_prefix) {
        return !is_on_screen_caption_speaker(&speaker);
    }
    SPEECH_VERBS.iter().any(|verb| line_prefix.ends_with(verb))
}

fn speaker_before_colon(line_prefix: &str) -> Option<String> {
    let trimmed = line_prefix.trim_end();
    if !trimmed.ends_with('：') && !trimmed.ends_with(':') {
        return None;
    }
    let stem = trimmed
        .strip_suffix('：')
        .or_else(|| trimmed.strip_suffix(':'))
        .unwrap_or(trimmed);
    let speaker = stem
        .rsplit(|ch: char| ch.is_whitespace() || matches!(ch, '，' | ',' | '。' | ';' | '；'))
        .next()
        .unwrap_or("")
        .trim();
    let len = speaker.chars().count();
    if (1..=12).contains(&len) {
        Some(speaker.to_string())
    } else {
        None
    }
}

fn is_on_screen_caption_speaker(speaker: &str) -> bool {
    const CAPTIONS: &[&str] = &[
        "字幕", "标题", "黑屏", "卡片", "花字", "片名", "片头", "片尾", "角标", "提词",
        "打出", "一张", "一幕", "闪回",
    ];
    CAPTIONS
        .iter()
        .any(|cap| speaker == *cap || speaker.ends_with(cap))
}

fn citation_hedge(line_prefix: &str) -> bool {
    const HEDGES: &[&str] = &[
        "所谓的",
        "正所谓",
        "俗话说",
        "所谓",
        "称为",
        "名为",
        "叫作",
        "叫做",
        "这种",
        "那种",
        "传说",
        "据说",
        "听说",
        "如同",
        "像是",
    ];
    let t = line_prefix.trim_end_matches(['：', ':', ' ']);
    HEDGES.iter().any(|h| t.ends_with(h))
}

const SPEECH_VERBS: &[&str] = &[
    "怒怼", "回怼", "宣读", "说道", "问道", "喊道", "叫道", "念道", "骂道", "吼道", "答道",
    "回道", "嘲道", "笑道", "哭道", "唱道", "低语", "开口", "说", "问", "喊", "叫", "念", "骂",
    "吼", "唱", "怼",
];

/// Last-resort: keep every still-missing spoken line on the closing row so a
/// paraphrase cannot abort planning. Prefer LLM repair; this only runs after it.
pub fn ensure_script_promise_lines(script: &str, rows: &mut Vec<ShotBriefDescription>) {
    if rows.is_empty() {
        return;
    }
    let blob = board_blob(rows);
    let mut extra = String::new();
    let mut seen = HashSet::new();
    for quote in extract_quoted_promises(script) {
        if !seen.insert(quote.clone()) || quote_covered_in(&blob, &quote) {
            continue;
        }
        extra.push_str("「");
        extra.push_str(&quote);
        extra.push('」');
    }
    if extra.is_empty() {
        return;
    }
    let last = rows.last_mut().expect("len checked");
    match &mut last.audio_desc {
        Some(audio) if !audio.contains(extra.trim()) => {
            if !audio.is_empty() && !audio.ends_with(char::is_whitespace) {
                audio.push(' ');
            }
            audio.push_str(&extra);
        }
        Some(_) => {}
        None => last.audio_desc = Some(extra),
    }
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn is_locked_script_line(quote: &str) -> bool {
    spoken_payload_weight(quote) >= 4 && !looks_like_title_card(quote)
}

/// Episode titles / on-screen cards like 「正宫出征」 are quoted in scripts
/// but are not spoken punchlines the storyboard must transcribe.
fn looks_like_title_card(quote: &str) -> bool {
    let cjk = quote.chars().filter(|c| is_cjk(*c)).count();
    if !(4..=6).contains(&cjk) {
        return false;
    }
    if quote.chars().any(|c| {
        matches!(
            c,
            '，' | '。' | '！' | '？' | '…' | ',' | '!' | '?' | ';' | '；'
        )
    }) {
        return false;
    }
    const SPEECH: &[char] = &['吧', '么', '吗', '呢', '嘛', '呀', '啊', '哦', '哼', '我', '你', '您'];
    const IMPERATIVE: &[char] = &['把', '别', '请', '让', '给'];
    if quote.chars().any(|c| SPEECH.contains(&c) || IMPERATIVE.contains(&c)) {
        return false;
    }
    true
}

fn quote_match_payload(quote: &str) -> &str {
    strip_leading_article_label(quote.trim())
}

/// "第 1079 条：夫妻感情…" → the spoken payload after the clause number.
fn strip_leading_article_label(s: &str) -> &str {
    let rest = s.trim();
    let Some(after_di) = rest.strip_prefix('第') else {
        return rest;
    };
    let Some(tiao_at) = after_di.find('条') else {
        return rest;
    };
    let between = &after_di[..tiao_at];
    if between.is_empty()
        || !between
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_whitespace())
    {
        return rest;
    }
    after_di[tiao_at + '条'.len_utf8()..]
        .trim_start()
        .trim_start_matches(['：', ':', ' '])
}

fn quote_covered_in(blob: &str, quote: &str) -> bool {
    if blob.contains(quote) {
        return true;
    }
    let payload = quote_match_payload(quote);
    if !payload.is_empty() && payload != quote && blob.contains(payload) {
        return true;
    }
    let cjk_src = if payload.is_empty() { quote } else { payload };
    let cjk: String = cjk_src.chars().filter(|c| is_cjk(*c)).take(4).collect();
    if cjk.chars().count() >= 4 && blob.contains(&cjk) {
        return true;
    }
    let needles = distinctive_needles(cjk_src);
    if needles.is_empty() {
        return false;
    }
    let hits = needles.iter().filter(|n| blob.contains(n.as_str())).count();
    if needles.len() <= 1 {
        hits == 1
    } else {
        hits >= 2
    }
}

/// Scene whose script already contains this beat, if any.
pub fn owner_scene_for_beat<S: AsRef<str>>(beat: &DramaBeat, scene_scripts: &[S]) -> Option<usize> {
    scene_scripts
        .iter()
        .position(|script| beat_covered_in(script.as_ref(), beat))
}

fn default_owner_scene(beat: &DramaBeat, scene_count: usize) -> usize {
    if scene_count == 0 {
        return 0;
    }
    match beat.role {
        BeatRole::Hook => 0,
        BeatRole::Payoff | BeatRole::Button => scene_count - 1,
        _ => (scene_count / 2).min(scene_count - 1),
    }
}

/// Insert missing film-level beats onto the owning scene boards.
///
/// Returns dirty scene indexes. Does nothing when the concatenated boards
/// already cover the beat — this is an existence check, not a later dedup.
pub fn place_missing_film_beats<S: AsRef<str>>(
    engine: &DramaEngine,
    scene_scripts: &[S],
    boards: &mut [Vec<ShotBriefDescription>],
) -> Vec<usize> {
    let n = boards.len();
    if n == 0 {
        return Vec::new();
    }
    let concat: Vec<ShotBriefDescription> = boards.iter().flatten().cloned().collect();
    let missing = uncovered_critical_beats(engine, &concat);
    if missing.is_empty() {
        return Vec::new();
    }
    let mut dirty = BTreeSet::new();
    for beat in missing {
        let i = owner_scene_for_beat(beat, scene_scripts).unwrap_or_else(|| default_owner_scene(beat, n));
        let i = i.min(n - 1);
        let at_front = beat.role == BeatRole::Hook;
        insert_coverage_beat(&mut boards[i], beat, at_front);
        dirty.insert(i);
    }
    dirty.into_iter().collect()
}

fn insert_coverage_beat(rows: &mut Vec<ShotBriefDescription>, beat: &DramaBeat, at_front: bool) {
    insert_coverage_row(rows, beat.action.clone(), None, at_front);
}

fn insert_coverage_row(
    rows: &mut Vec<ShotBriefDescription>,
    visual_desc: String,
    audio_desc: Option<String>,
    at_front: bool,
) {
    let cam_idx = if at_front {
        rows.first().map(|r| r.cam_idx).unwrap_or(0)
    } else {
        rows.last().map(|r| r.exit_cam_idx()).unwrap_or(0)
    };
    let location_id = if at_front {
        rows.first()
    } else {
        rows.last()
    }
    .map(|r| r.location_id.clone())
    .unwrap_or_default();
    let row = ShotBriefDescription {
        idx: 0,
        is_last: false,
        cam_idx,
        visual_desc,
        audio_desc,
        location_id,
        beats: Vec::new(),
    };
    if at_front {
        rows.insert(0, row);
    } else {
        rows.push(row);
    }
    let last = rows.len().saturating_sub(1);
    for (i, row) in rows.iter_mut().enumerate() {
        row.idx = i as i32;
        row.is_last = i == last;
    }
}

fn board_blob(rows: &[ShotBriefDescription]) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(&row.visual_desc);
        out.push('\n');
        if let Some(audio) = &row.audio_desc {
            out.push_str(audio);
            out.push('\n');
        }
        for beat in &row.beats {
            out.push_str(&beat.visual_desc);
            out.push('\n');
            if let Some(audio) = &beat.audio_desc {
                out.push_str(audio);
                out.push('\n');
            }
        }
    }
    out
}

fn beat_covered_in(blob: &str, beat: &DramaBeat) -> bool {
    let action = beat.action.trim();
    if !action.is_empty() && blob.contains(action) {
        return true;
    }
    let needles = distinctive_needles(action);
    if needles.is_empty() {
        return false;
    }
    let hits = needles.iter().filter(|n| blob.contains(n.as_str())).count();
    if needles.len() <= 1 {
        hits == 1
    } else {
        hits >= 2
    }
}

fn distinctive_needles(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            if current.chars().any(|c| c.is_ascii_alphabetic()) {
                flush_latin_word(&mut current, &mut out);
            }
            current.push(ch);
            continue;
        }
        flush_cjk_run(&mut current, &mut out);
        if ch.is_ascii_alphabetic() {
            current.push(ch);
        } else {
            flush_latin_word(&mut current, &mut out);
        }
    }
    flush_cjk_run(&mut current, &mut out);
    flush_latin_word(&mut current, &mut out);
    out.sort();
    out.dedup();
    out.truncate(8);
    out
}

fn flush_cjk_run(run: &mut String, out: &mut Vec<String>) {
    if run.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        let chars: Vec<char> = run.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            let gram: String = chars[i..i + 2].iter().collect();
            if !CJK_STOP.contains(&gram.as_str()) {
                out.push(gram);
            }
            i += 2;
        }
        if chars.len() >= 3 {
            let long: String = chars.iter().take(4).collect();
            if !out.iter().any(|s| s == &long) {
                out.push(long);
            }
        }
    }
    run.clear();
}

fn flush_latin_word(run: &mut String, out: &mut Vec<String>) {
    let w = run.trim().to_ascii_lowercase();
    run.clear();
    if w.len() >= 4 && !LATIN_STOP.contains(&w.as_str()) {
        out.push(w);
    }
}

const CJK_STOP: &[&str] = &["一个", "这个", "那个", "自己", "他们", "她们", "然后", "因为"];
const LATIN_STOP: &[&str] = &["that", "this", "with", "from", "into", "then", "have"];

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

    #[test]
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
                location_id: String::new(),
                beats: vec![],
            },
            ShotBriefDescription {
                idx: 1,
                is_last: true,
                cam_idx: 1,
                visual_desc: "<姐姐>站在门口，非常愤怒，然后环顾四周".into(),
                audio_desc: Some("门响".into()),
                location_id: String::new(),
                beats: vec![],
            },
        ];
        let issues = lint_storyboard_performance(&rows);
        assert_eq!(issues.len(), 2);
        let blocking = lint_storyboard_blocking(&rows);
        assert_eq!(blocking.len(), 2);
        assert!(blocking.iter().all(|i| i.contains("站位不完整")));
        let placed = ShotBriefDescription {
            idx: 0,
            is_last: true,
            cam_idx: 0,
            visual_desc: "中景平视固定。<林晚>在画面左侧、铁门内侧一步，面向右，站着，比门口更靠近镜头。她把票根塞进口袋。"
                .into(),
            audio_desc: None,
            location_id: String::new(),
            beats: vec![],
        };
        assert!(lint_storyboard_blocking(&[placed]).is_empty());
        let speaker_unplaced = ShotBriefDescription {
            idx: 4,
            is_last: true,
            cam_idx: 0,
            visual_desc: "中景平视固定。<封彪>在画面右侧、主位椅子上，面向左，坐着。<韩跃平>在画面右侧、封彪身后，面向左，站着。旁边坐着一个同伴。".into(),
            audio_desc: Some("封彪：「有他吗？」大海：「有。」".into()),
            location_id: String::new(),
            beats: vec![],
        };
        let missing = lint_storyboard_blocking(&[speaker_unplaced]);
        assert!(
            missing.iter().any(|i| i.contains("shot 4: 大海 ")),
            "{missing:?}"
        );
        assert!(
            !missing
                .iter()
                .any(|i| i.contains("shot 4: 封彪 ") || i.contains("shot 4: 韩跃平 ")),
            "{missing:?}"
        );
        assert!(name_has_own_screen_side(
            "<高二林> 在画面中央、桌面上，面向下，趴着。<刘华强> 在画面右侧、高二林身后，面向左，俯身。",
            "高二林"
        ));
        assert!(name_has_own_screen_side(
            "<高二林> 在画面中央、桌面上，面向下，趴着。<刘华强> 在画面右侧、高二林身后，面向左，俯身。",
            "刘华强"
        ));
        assert!(name_has_own_screen_side(
            "〈封彪〉在画面左侧、门内一步，面向右，站着。",
            "封彪"
        ));
        assert!(bracket_names("〈封彪〉在画面左侧。<韩跃平>在画面右侧。").contains(&"封彪".to_string()));
        assert!(!name_has_own_screen_side(
            "<刘华强> 在画面右侧、高二林身后，面向左，俯身。转头问大海。",
            "高二林"
        ));
        assert!(!name_has_own_screen_side("转头问大海，旁边坐着一个同伴。", "大海"));
        assert!(name_has_own_screen_side("画面左侧的<大海>面向右，坐着。", "大海"));
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
            location_id: String::new(),
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
    fn coverage_lint_requires_turn_and_payoff_on_the_board() {
        let engine = dense_engine();
        let hook_only = vec![ShotBriefDescription {
            idx: 0,
            is_last: true,
            cam_idx: 0,
            visual_desc: "林晚从垃圾桶里抢出被撕成两半的通知书，攥进袖口".into(),
            audio_desc: None,
            location_id: String::new(),
            beats: vec![],
        }];
        let issues = lint_storyboard_coverage(&engine, &hook_only);
        assert!(issues.iter().any(|i| i.contains("turn")));
        assert!(issues.iter().any(|i| i.contains("payoff")));
        let mut rows = hook_only;
        assert!(ensure_storyboard_coverage(&engine, &mut rows));
        assert!(lint_storyboard_coverage(&engine, &rows).is_empty());
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn script_promise_coverage_flags_missing_punchline() {
        let script = "司机把手机支在仪表台上，对着镜头怒怼：「吃俩桃你不噎得慌么」";
        let rows = vec![brief("男主把桃扔进副驾车窗，女生接住桃笑着说谢谢")];
        let issues = lint_script_promise_coverage(script, &rows);
        assert!(issues.iter().any(|i| i.contains("吃俩桃你不噎得慌么")));
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn script_promise_coverage_passes_when_punchline_already_on_board() {
        let script = "司机怒怼：「吃俩桃你不噎得慌么」";
        let rows = vec![brief("司机对着镜头说吃俩桃你不噎得慌么")];
        assert!(lint_script_promise_coverage(script, &rows).is_empty());
    }

    #[test]
    fn script_promise_ignores_short_title_cards() {
        let script = "黑屏打出：「正宫出征」\n下一张：「出轨证据」\n再下一张：「创业祭坛」\n女主拎包出门。";
        let rows = vec![brief("女主拎着包推开家门走入夜色")];
        assert!(lint_script_promise_coverage(script, &rows).is_empty());
    }

    #[test]
    fn script_promise_matches_article_punchline_without_clause_number() {
        let script = "法官宣读：「第 1079 条：夫妻感情确已破裂，调解无效，应准予离婚。P.S. 但如果能一起赚钱，建议再忍忍。」";
        let rows = vec![brief(
            "法官举起判决书宣读夫妻感情确已破裂，调解无效应准予离婚",
        )];
        assert!(lint_script_promise_coverage(script, &rows).is_empty());
    }

    #[test]
    fn script_promise_flags_article_when_punchline_absent() {
        let script = "法官宣读：「第 1079 条：夫妻感情确已破裂，调解无效，应准予离婚。P.S. 但如果能一起赚钱，建议再忍忍。」";
        let rows = vec![brief("法官敲槌，全场起立")];
        let issues = lint_script_promise_coverage(script, &rows);
        assert!(
            issues.iter().any(|i| i.contains("夫妻感情确已破裂")),
            "{issues:?}"
        );
    }

    #[test]
    fn script_promise_counts_beat_audio_desc() {
        let script = "司机怒怼：「吃俩桃你不噎得慌么」";
        let mut row = brief("司机把手机支在仪表台上");
        row.beats = vec![crate::domain::ShotBriefBeat {
            visual_desc: "特写嘴型".into(),
            audio_desc: Some("司机：「吃俩桃你不噎得慌么」".into()),
            cam_idx: 0,
        }];
        assert!(lint_script_promise_coverage(script, &[row]).is_empty());
    }

    #[test]
    fn script_promise_ignores_prose_term_quotes() {
        let script = "完整剧本成片模式。星舰在「千年的时间尺度」上只是一瞬，镜头切到舱内。\n字幕：「千年的时间尺度」\n所谓「时间尺度」不过是旁白里的修辞。";
        let rows = vec![brief("近景，星舰掠过星云，舱内指示灯闪烁")];
        assert!(
            lint_script_promise_coverage(script, &rows).is_empty(),
            "scare-quoted terms and caption cards must not abort storyboard coverage"
        );
    }

    #[test]
    fn script_promise_still_locks_named_speaker_and_speech_verb_lines() {
        let script = "李薇：「把监控调出来」\n司机把手机支在仪表台上，对着镜头怒怼：「吃俩桃你不噎得慌么」";
        let rows = vec![brief("李薇伸手按向键盘")];
        let issues = lint_script_promise_coverage(script, &rows);
        assert!(
            issues.iter().any(|i| i.contains("把监控调出来")),
            "{issues:?}"
        );
        assert!(
            issues.iter().any(|i| i.contains("吃俩桃你不噎得慌么")),
            "{issues:?}"
        );
    }

    #[test]
    fn script_promise_stitch_keeps_missing_spoken_line() {
        let script = "李薇：「把监控调出来」";
        let mut rows = vec![brief("李薇伸手按向键盘")];
        ensure_script_promise_lines(script, &mut rows);
        assert!(lint_script_promise_coverage(script, &rows).is_empty());
        assert!(
            rows[0]
                .audio_desc
                .as_deref()
                .unwrap_or("")
                .contains("把监控调出来")
        );
    }

    fn brief(visual: &str) -> ShotBriefDescription {
        ShotBriefDescription {
            idx: 0,
            is_last: true,
            cam_idx: 0,
            visual_desc: visual.into(),
            audio_desc: None,
            location_id: String::new(),
            beats: vec![],
        }
    }

    #[test]
    fn film_beats_are_not_cloned_onto_every_scene() {
        let engine = dense_engine();
        let hook = engine.beats[0].action.clone();
        let turn = engine.beats.iter().find(|b| b.role == BeatRole::Turn).unwrap().action.clone();
        let payoff = engine.beats.iter().find(|b| b.role == BeatRole::Payoff).unwrap().action.clone();
        let mut boards = vec![
            vec![brief(&hook), brief(&turn), brief(&payoff)],
            vec![brief("姐姐把行李箱摔在林晚脚边，抢走她的身份证")],
        ];
        let dirty = place_missing_film_beats(
            &engine,
            &[
                format!("{hook}\n{turn}\n{payoff}"),
                "姐姐把行李箱摔在林晚脚边，抢走她的身份证".into(),
            ],
            &mut boards,
        );
        assert!(dirty.is_empty());
        assert_eq!(
            boards[1]
                .iter()
                .filter(|r| r.visual_desc == hook)
                .count(),
            0
        );
    }

    #[test]
    fn missing_hook_inserts_into_the_scene_that_owns_the_script() {
        let engine = dense_engine();
        let hook = engine.beats[0].action.clone();
        let turn = engine.beats[2].action.clone();
        let payoff = engine.beats[3].action.clone();
        let mut boards = vec![
            vec![brief("厨房里水烧开了")],
            vec![brief(&turn), brief(&payoff)],
        ];
        let scene1 = format!("{turn}\n{payoff}");
        let dirty = place_missing_film_beats(
            &engine,
            &[hook.as_str(), scene1.as_str()],
            &mut boards,
        );
        assert_eq!(dirty, vec![0]);
        assert_eq!(boards[0][0].visual_desc, hook);
        assert_ne!(boards[1][0].visual_desc, hook);
    }

    #[test]
    fn engine_block_carries_beats_and_scope_guard() {
        let block = drama_engine_block(&dense_engine());
        assert!(block.starts_with("[DRAMA_ENGINE"));
        assert!(block.contains("[hook]"));
        assert!(block.contains("[payoff]"));
        assert!(block.contains("纸船"));
        assert!(block.contains("折进最后一场"));
        assert!(block.contains("成片某处兑现"));
        assert!(!block.contains("不要为此增加镜头"));
        assert!(!block.contains("禁止把其他场"));
        let scene = with_scene_drama_engine("多用日常场景", &dense_engine());
        assert!(scene.contains("禁止把其他场"));
        assert!(!scene.contains("成片某处兑现"));
        let composed = with_drama_engine("多用日常场景", &dense_engine());
        assert!(composed.starts_with("多用日常场景"));
        assert!(composed.contains("[DRAMA_ENGINE"));
    }
}
