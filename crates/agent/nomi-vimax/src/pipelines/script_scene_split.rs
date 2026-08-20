//! Split a finished screenplay into shootable units (episodes / scenes).
//!
//! First principles:
//! - Prefer structure already in the text (`第N集`, `N-M` headings) over LLM rewrite.
//! - Default scope is the full selected bible (全集); users narrow with requirement text.
//! - Non-「全部」scopes still get a per-run hard cap to avoid accidental huge FirstN.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Hard cap per plan/render run for non-「全部」selections.
pub const HARD_MAX_SCENES_PER_RUN: usize = 12;

static EPISODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^第\s*(\d+)\s*集\s*$").expect("episode regex"));
static SCENE_CODE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d+)\s*[-–—]\s*(\d+)\b").expect("scene-code regex")
});
static INT_EXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(INT|EXT|INT\.?/EXT\.?)\b").expect("int/ext regex")
});

/// One continuous shootable beat from the screenplay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenplayUnit {
    /// 1-based order in the full split (stable across selection).
    pub index: usize,
    pub episode: Option<u32>,
    /// `Some("1-3")` when the heading used a code like `1-3 夜 内 …`.
    pub scene_code: Option<String>,
    pub heading: String,
    /// Full text for this unit (includes heading line).
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenplaySplit {
    /// Character bios / synopsis before the first episode or scene heading.
    pub preamble: String,
    pub units: Vec<ScreenplayUnit>,
}

impl ScreenplaySplit {
    pub fn has_episodes(&self) -> bool {
        self.units.iter().any(|u| u.episode.is_some())
    }

    /// Join preamble + selected bodies for film-level cast extraction.
    pub fn extract_corpus(&self, selected: &[ScreenplayUnit]) -> String {
        let mut parts = Vec::new();
        let pre = self.preamble.trim();
        if !pre.is_empty() {
            parts.push(pre.to_string());
        }
        for u in selected {
            let body = u.body.trim();
            if !body.is_empty() {
                parts.push(body.to_string());
            }
        }
        parts.join("\n\n")
    }
}

/// What the user (or default policy) asked to film this run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScriptShootScope {
    /// All discovered units (still hard-capped).
    All,
    /// One episode by number (`第3集`).
    Episode { episode: u32 },
    /// First N units in document order.
    FirstN { n: usize },
    /// Inclusive 1-based indices into the flat unit list.
    Range { start: usize, end: usize },
    /// Specific beat inside an episode (`第3集第2场` → episode scene ordinal).
    EpisodeScene { episode: u32, scene_ordinal: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptSelection {
    pub scope: ScriptShootScope,
    /// True when scope came from requirement text rather than the default policy.
    pub from_user: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenesIndexEntry {
    pub index: usize,
    pub episode: Option<u32>,
    pub scene_code: Option<String>,
    pub heading: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenesIndex {
    pub selection: ScriptSelection,
    pub total_units: usize,
    pub selected_count: usize,
    pub scenes: Vec<ScenesIndexEntry>,
}

/// Split screenplay text into units. No markers → one unit (whole script).
pub fn split_screenplay(script: &str) -> ScreenplaySplit {
    let lines: Vec<&str> = script.lines().collect();
    if lines.is_empty() || script.trim().is_empty() {
        return ScreenplaySplit {
            preamble: String::new(),
            units: vec![ScreenplayUnit {
                index: 1,
                episode: None,
                scene_code: None,
                heading: "全片".into(),
                body: script.to_string(),
            }],
        };
    }

    let mut markers: Vec<(usize, Marker)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(cap) = EPISODE_RE.captures(t) {
            let ep: u32 = cap[1].parse().unwrap_or(0);
            markers.push((i, Marker::Episode(ep)));
            continue;
        }
        if let Some(cap) = SCENE_CODE_RE.captures(t) {
            let a = &cap[1];
            let b = &cap[2];
            markers.push((
                i,
                Marker::Scene {
                    episode_hint: a.parse().ok(),
                    code: format!("{a}-{b}"),
                    heading: t.to_string(),
                },
            ));
            continue;
        }
        if INT_EXT_RE.is_match(t) {
            markers.push((
                i,
                Marker::Scene {
                    episode_hint: None,
                    code: String::new(),
                    heading: t.to_string(),
                },
            ));
        }
    }

    if markers.is_empty() {
        return ScreenplaySplit {
            preamble: String::new(),
            units: vec![ScreenplayUnit {
                index: 1,
                episode: None,
                scene_code: None,
                heading: "全片".into(),
                body: script.to_string(),
            }],
        };
    }

    let first_marker_line = markers[0].0;
    let preamble = if first_marker_line > 0 {
        lines[..first_marker_line].join("\n").trim().to_string()
    } else {
        String::new()
    };

    let mut units: Vec<ScreenplayUnit> = Vec::new();
    let mut current_episode: Option<u32> = None;
    let mut unit_start: Option<usize> = None;
    let mut unit_heading = String::new();
    let mut unit_code: Option<String> = None;
    let mut unit_episode: Option<u32> = None;

    let flush = |units: &mut Vec<ScreenplayUnit>,
                 lines: &[&str],
                 start: usize,
                 end: usize,
                 episode: Option<u32>,
                 code: Option<String>,
                 heading: &str| {
        if start >= end || start >= lines.len() {
            return;
        }
        let end = end.min(lines.len());
        let body = lines[start..end].join("\n").trim().to_string();
        if body.is_empty() {
            return;
        }
        let index = units.len() + 1;
        units.push(ScreenplayUnit {
            index,
            episode,
            scene_code: code.filter(|c| !c.is_empty()),
            heading: if heading.is_empty() {
                format!("场{index}")
            } else {
                heading.to_string()
            },
            body,
        });
    };

    let has_scene_markers = markers
        .iter()
        .any(|(_, m)| matches!(m, Marker::Scene { .. }));

    if has_scene_markers {
        for (line_idx, marker) in &markers {
            match marker {
                Marker::Episode(ep) => {
                    if let Some(start) = unit_start {
                        flush(
                            &mut units,
                            &lines,
                            start,
                            *line_idx,
                            unit_episode,
                            unit_code.take(),
                            &unit_heading,
                        );
                        unit_start = None;
                    }
                    current_episode = Some(*ep);
                }
                Marker::Scene {
                    episode_hint,
                    code,
                    heading,
                } => {
                    if let Some(start) = unit_start {
                        flush(
                            &mut units,
                            &lines,
                            start,
                            *line_idx,
                            unit_episode,
                            unit_code.take(),
                            &unit_heading,
                        );
                    }
                    unit_start = Some(*line_idx);
                    unit_heading = heading.clone();
                    unit_code = if code.is_empty() {
                        None
                    } else {
                        Some(code.clone())
                    };
                    unit_episode = current_episode.or(*episode_hint);
                }
            }
        }
        if let Some(start) = unit_start {
            flush(
                &mut units,
                &lines,
                start,
                lines.len(),
                unit_episode,
                unit_code,
                &unit_heading,
            );
        }
    } else {
        // Episode headers only — one unit per episode block.
        let mut ep_markers: Vec<(usize, u32)> = markers
            .iter()
            .filter_map(|(i, m)| match m {
                Marker::Episode(ep) => Some((*i, *ep)),
                _ => None,
            })
            .collect();
        ep_markers.push((lines.len(), 0));
        for w in ep_markers.windows(2) {
            let (start_line, ep) = w[0];
            let end_line = w[1].0;
            let body = lines[start_line..end_line].join("\n").trim().to_string();
            if body.is_empty() {
                continue;
            }
            let index = units.len() + 1;
            units.push(ScreenplayUnit {
                index,
                episode: Some(ep),
                scene_code: None,
                heading: format!("第{ep}集"),
                body,
            });
        }
    }

    if units.is_empty() {
        return ScreenplaySplit {
            preamble,
            units: vec![ScreenplayUnit {
                index: 1,
                episode: None,
                scene_code: None,
                heading: "全片".into(),
                body: script.to_string(),
            }],
        };
    }

    ScreenplaySplit { preamble, units }
}

#[derive(Clone)]
enum Marker {
    Episode(u32),
    Scene {
        episode_hint: Option<u32>,
        code: String,
        heading: String,
    },
}

/// Parse shoot scope from free-text user requirement.
pub fn parse_script_selection(user_requirement: &str) -> Option<ScriptSelection> {
    let text = user_requirement.trim();
    if text.is_empty() {
        return None;
    }

    // 第3集第2场 / 第3集的第2场
    static EP_SCENE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"第\s*(\d+)\s*集\s*(?:的?\s*)?第\s*(\d+)\s*场").expect("ep-scene")
    });
    if let Some(c) = EP_SCENE.captures(text) {
        let episode = c[1].parse().ok()?;
        let scene_ordinal = c[2].parse().ok()?;
        return Some(ScriptSelection {
            scope: ScriptShootScope::EpisodeScene {
                episode,
                scene_ordinal,
            },
            from_user: true,
            note: format!("用户指定：第{episode}集第{scene_ordinal}场"),
        });
    }

    // 第N集 (but not when only discussing characters)
    static EP_ONLY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"第\s*(\d+)\s*集").expect("ep-only"));
    // Prefer explicit shoot verbs / short requirement.
    let wants_episode = text.contains("拍")
        || text.contains("拍摄")
        || text.contains("生成")
        || text.contains("成片")
        || text.contains("只")
        || text.len() < 80
        || EP_ONLY.find(text).is_some_and(|m| m.start() < 40);
    if wants_episode {
        if let Some(c) = EP_ONLY.captures(text) {
            // Avoid matching when followed by 第M场 (already handled).
            let episode = c[1].parse().ok()?;
            return Some(ScriptSelection {
                scope: ScriptShootScope::Episode { episode },
                from_user: true,
                note: format!("用户指定：第{episode}集"),
            });
        }
    }

    static FIRST_N: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:前|先拍|只拍)\s*(\d+)\s*(?:场|个场景|个场次|scenes?)")
            .expect("first-n")
    });
    if let Some(c) = FIRST_N.captures(text) {
        let n = c[1].parse::<usize>().ok()?.max(1);
        return Some(ScriptSelection {
            scope: ScriptShootScope::FirstN { n },
            from_user: true,
            note: format!("用户指定：前{n}场"),
        });
    }

    static RANGE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:第)?\s*(\d+)\s*[-~～到至]\s*(?:第)?\s*(\d+)\s*场").expect("range")
    });
    if let Some(c) = RANGE.captures(text) {
        let a = c[1].parse::<usize>().ok()?.max(1);
        let b = c[2].parse::<usize>().ok()?.max(1);
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        return Some(ScriptSelection {
            scope: ScriptShootScope::Range { start, end },
            from_user: true,
            note: format!("用户指定：第{start}–{end}场"),
        });
    }

    let lower = text.to_ascii_lowercase();
    if text.contains("全部")
        || text.contains("所有集")
        || text.contains("所有场")
        || text.contains("整部")
        || text.contains("完整剧本")
        || lower.contains("entire script")
        || lower.contains("all episodes")
        || lower.contains("all scenes")
    {
        return Some(ScriptSelection {
            scope: ScriptShootScope::All,
            from_user: true,
            note: "用户指定：尽量覆盖所选范围内全部场次（仍受单次硬上限约束）".into(),
        });
    }

    None
}

pub fn default_script_selection(split: &ScreenplaySplit) -> ScriptSelection {
    if split.units.len() <= 1 {
        return ScriptSelection {
            scope: ScriptShootScope::All,
            from_user: false,
            note: "单场剧本，整场拍摄".into(),
        };
    }
    let scope_hint = if split.has_episodes() {
        format!(
            "未指定范围：默认拍全集（共 {} 场 / 含多集；可在需求中写「拍第N集」「前N场」缩小范围）",
            split.units.len()
        )
    } else {
        format!(
            "未指定范围：默认拍全集（共 {} 场；可在需求中写「前N场」缩小范围）",
            split.units.len()
        )
    };
    ScriptSelection {
        scope: ScriptShootScope::All,
        from_user: false,
        note: scope_hint,
    }
}

pub fn resolve_script_selection(
    user_requirement: &str,
    split: &ScreenplaySplit,
) -> ScriptSelection {
    parse_script_selection(user_requirement).unwrap_or_else(|| default_script_selection(split))
}

/// Apply selection and hard-cap. Returns selected units in document order.
pub fn apply_script_selection(
    split: &ScreenplaySplit,
    selection: &ScriptSelection,
) -> Vec<ScreenplayUnit> {
    let mut picked: Vec<ScreenplayUnit> = match &selection.scope {
        ScriptShootScope::All => split.units.clone(),
        ScriptShootScope::Episode { episode } => split
            .units
            .iter()
            .filter(|u| u.episode == Some(*episode))
            .cloned()
            .collect(),
        ScriptShootScope::FirstN { n } => split.units.iter().take((*n).max(1)).cloned().collect(),
        ScriptShootScope::Range { start, end } => split
            .units
            .iter()
            .filter(|u| u.index >= *start && u.index <= *end)
            .cloned()
            .collect(),
        ScriptShootScope::EpisodeScene {
            episode,
            scene_ordinal,
        } => {
            let in_ep: Vec<_> = split
                .units
                .iter()
                .filter(|u| u.episode == Some(*episode))
                .cloned()
                .collect();
            let ord = (*scene_ordinal as usize).max(1);
            in_ep.into_iter().nth(ord - 1).into_iter().collect()
        }
    };

    if picked.is_empty() {
        // Fall back rather than planning nothing.
        picked = split.units.iter().take(1).cloned().collect();
    }

    // 「全部 / 默认全集」不截断；其它范围仍受单次硬上限保护。
    if !matches!(selection.scope, ScriptShootScope::All)
        && picked.len() > HARD_MAX_SCENES_PER_RUN
    {
        tracing::warn!(
            kept = HARD_MAX_SCENES_PER_RUN,
            dropped = picked.len() - HARD_MAX_SCENES_PER_RUN,
            from_user = selection.from_user,
            "truncated selected screenplay units to hard max per run"
        );
        picked.truncate(HARD_MAX_SCENES_PER_RUN);
    }
    picked
}

pub fn build_scenes_index(
    split: &ScreenplaySplit,
    selection: &ScriptSelection,
    selected: &[ScreenplayUnit],
) -> ScenesIndex {
    let selected_idxs: std::collections::HashSet<usize> =
        selected.iter().map(|u| u.index).collect();
    let scenes = split
        .units
        .iter()
        .map(|u| ScenesIndexEntry {
            index: u.index,
            episode: u.episode,
            scene_code: u.scene_code.clone(),
            heading: u.heading.clone(),
            selected: selected_idxs.contains(&u.index),
        })
        .collect();
    ScenesIndex {
        selection: selection.clone(),
        total_units: split.units.len(),
        selected_count: selected.len(),
        scenes,
    }
}

/// Bodies written to `script.json` (idea2video-compatible).
pub fn selected_script_bodies(selected: &[ScreenplayUnit]) -> Vec<String> {
    selected.iter().map(|u| u.body.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"人物小传
姓名：花卷

第1集

1-1 日 外 街
花卷走路。

1-2 夜 内 店
花卷吃面。

第2集

2-1 日 内 店
客人进门。
"#;

    #[test]
    fn splits_episodes_and_scene_codes() {
        let split = split_screenplay(SAMPLE);
        assert!(split.preamble.contains("花卷"));
        assert_eq!(split.units.len(), 3);
        assert_eq!(split.units[0].episode, Some(1));
        assert_eq!(split.units[0].scene_code.as_deref(), Some("1-1"));
        assert_eq!(split.units[1].scene_code.as_deref(), Some("1-2"));
        assert_eq!(split.units[2].episode, Some(2));
        assert!(split.units[0].body.contains("花卷走路"));
    }

    #[test]
    fn unmarked_script_is_single_unit() {
        let split = split_screenplay("只有一段对白。\n甲：你好。");
        assert_eq!(split.units.len(), 1);
        assert!(split.units[0].body.contains("你好"));
    }

    #[test]
    fn default_picks_all_units() {
        let split = split_screenplay(SAMPLE);
        let sel = default_script_selection(&split);
        assert_eq!(sel.scope, ScriptShootScope::All);
        let picked = apply_script_selection(&split, &sel);
        assert_eq!(picked.len(), split.units.len());
    }

    #[test]
    fn parses_episode_and_first_n() {
        let s = parse_script_selection("请拍第2集，电影感").unwrap();
        assert_eq!(s.scope, ScriptShootScope::Episode { episode: 2 });
        let s = parse_script_selection("前5场即可").unwrap();
        assert_eq!(s.scope, ScriptShootScope::FirstN { n: 5 });
        let s = parse_script_selection("拍第1集第2场").unwrap();
        assert_eq!(
            s.scope,
            ScriptShootScope::EpisodeScene {
                episode: 1,
                scene_ordinal: 2
            }
        );
    }

    #[test]
    fn apply_episode_scene_ordinal() {
        let split = split_screenplay(SAMPLE);
        let sel = ScriptSelection {
            scope: ScriptShootScope::EpisodeScene {
                episode: 1,
                scene_ordinal: 2,
            },
            from_user: true,
            note: String::new(),
        };
        let picked = apply_script_selection(&split, &sel);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].scene_code.as_deref(), Some("1-2"));
    }

    #[test]
    fn hard_caps_non_all_only() {
        let mut units = Vec::new();
        for i in 1..=30 {
            units.push(ScreenplayUnit {
                index: i,
                episode: Some(1),
                scene_code: Some(format!("1-{i}")),
                heading: format!("1-{i}"),
                body: format!("body {i}"),
            });
        }
        let split = ScreenplaySplit {
            preamble: String::new(),
            units,
        };
        let all = ScriptSelection {
            scope: ScriptShootScope::All,
            from_user: false,
            note: String::new(),
        };
        assert_eq!(apply_script_selection(&split, &all).len(), 30);

        let first = ScriptSelection {
            scope: ScriptShootScope::FirstN { n: 30 },
            from_user: true,
            note: String::new(),
        };
        assert_eq!(
            apply_script_selection(&split, &first).len(),
            HARD_MAX_SCENES_PER_RUN
        );
    }
}
