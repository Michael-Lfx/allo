//! Line-anchor engine for Read/Grep/Edit (ported from Open Vetta coding-agent).
//!
//! Anchor format: `<line>:<hhhh>` — `line` is 1-based hint, `hhhh` is a whitespace-
//! normalized FNV-1a hash (4 base36 chars). Models must copy anchors from tool
//! output; fabricating hashes is rejected as stale. This forces read-before-edit.

/// Search radius when the line number drifted (±N lines).
pub const ANCHOR_SEARCH_RADIUS: usize = 20;

/// Separator between anchor and line content in Read output: `42:ab→content`.
pub const ANCHOR_SEPARATOR: char = '\u{2192}'; // →

/// Base36 width of the hash suffix.
pub const ANCHOR_HASH_WIDTH: usize = 4;

/// Whitespace-stripped FNV-1a 32-bit → base36, last [`ANCHOR_HASH_WIDTH`] chars.
pub fn anchor_line_hash(line: &str) -> String {
    let normalized: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    let mut hash: u32 = 0x811c_9dc5;
    for b in normalized.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut s = String::new();
    let mut n = hash;
    if n == 0 {
        s.push('0');
    } else {
        while n > 0 {
            let d = (n % 36) as u8;
            let ch = if d < 10 {
                (b'0' + d) as char
            } else {
                (b'a' + (d - 10)) as char
            };
            s.insert(0, ch);
            n /= 36;
        }
    }
    while s.len() < ANCHOR_HASH_WIDTH {
        s.insert(0, '0');
    }
    if s.len() > ANCHOR_HASH_WIDTH {
        s = s[s.len() - ANCHOR_HASH_WIDTH..].to_string();
    }
    s
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAnchor {
    pub line: usize,
    pub hash: String,
}

/// Parse `42:ab` (tolerates trailing `→…`).
pub fn parse_anchor(raw: &str) -> Option<ParsedAnchor> {
    let cleaned = raw
        .split(ANCHOR_SEPARATOR)
        .next()
        .unwrap_or(raw)
        .trim();
    let (line_s, hash) = cleaned.split_once(':')?;
    let line: usize = line_s.parse().ok()?;
    if line < 1 {
        return None;
    }
    let hash = hash.trim();
    if hash.len() < 2 || hash.len() > 8 || !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ParsedAnchor {
        line,
        hash: hash.to_ascii_lowercase(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorValidation {
    Ok { line: usize },
    Shifted { line: usize },
    Stale,
}

/// Validate anchor against full file lines (0-based slice, 1-based line numbers).
pub fn validate_anchor(
    lines: &[&str],
    anchor: &ParsedAnchor,
    radius: usize,
) -> AnchorValidation {
    let idx = anchor.line as isize - 1;
    if idx >= 0 {
        let i = idx as usize;
        if i < lines.len() && anchor_line_hash(lines[i]) == anchor.hash {
            return AnchorValidation::Ok { line: anchor.line };
        }
    }
    let mut first_hit: Option<usize> = None;
    for distance in 1..=radius as isize {
        for candidate in [idx - distance, idx + distance] {
            if candidate < 0 {
                continue;
            }
            let c = candidate as usize;
            if c < lines.len() && anchor_line_hash(lines[c]) == anchor.hash {
                if first_hit.is_some() {
                    return AnchorValidation::Stale;
                }
                first_hit = Some(c);
            }
        }
    }
    match first_hit {
        None => AnchorValidation::Stale,
        Some(i) => AnchorValidation::Shifted { line: i + 1 },
    }
}

pub fn validate_anchor_default(lines: &[&str], anchor: &ParsedAnchor) -> AnchorValidation {
    validate_anchor(lines, anchor, ANCHOR_SEARCH_RADIUS)
}

pub fn render_anchored_lines(lines: &[&str], start_line: usize) -> Vec<String> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                "{}:{}{}{}",
                start_line + i,
                anchor_line_hash(line),
                ANCHOR_SEPARATOR,
                line
            )
        })
        .collect()
}

pub fn render_anchor_region(lines: &[&str], center_line: usize, context: usize) -> String {
    if lines.is_empty() {
        return "(file is empty)".to_string();
    }
    let start = center_line.saturating_sub(context).max(1);
    let end = (center_line + context).min(lines.len());
    if end < start {
        return "(file is empty)".to_string();
    }
    render_anchored_lines(&lines[start - 1..end], start).join("\n")
}

/// If the inclusive end line is a bare structural closer, `new_text` must keep it
/// (unless `new_text` is empty = delete). Prevents silent brace/JSX damage.
pub fn structural_closer_ok(original_end_line: &str, new_text: &str) -> bool {
    if new_text.is_empty() {
        return true;
    }
    let trimmed = original_end_line.trim();
    let bare = matches!(
        trimmed,
        "}" | "};" | ")" | ");" | "]" | "];" | "}," | ")," | "]," | "/>"
    );
    if !bare {
        return true;
    }
    let core = trimmed.trim_matches(|c| c == ',' || c == ';');
    new_text.contains(core)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_stable_for_whitespace() {
        assert_eq!(anchor_line_hash("  foo  "), anchor_line_hash("foo"));
        assert_ne!(anchor_line_hash("foo"), anchor_line_hash("bar"));
    }

    #[test]
    fn parse_and_validate_ok() {
        let lines = ["alpha", "beta", "gamma"];
        let hash = anchor_line_hash("beta");
        let a = parse_anchor(&format!("2:{hash}")).unwrap();
        assert_eq!(
            validate_anchor_default(&lines, &a),
            AnchorValidation::Ok { line: 2 }
        );
    }

    #[test]
    fn shifted_when_line_moves() {
        let lines = ["alpha", "beta", "gamma"];
        let hash = anchor_line_hash("beta");
        let a = ParsedAnchor {
            line: 1,
            hash,
        };
        assert_eq!(
            validate_anchor_default(&lines, &a),
            AnchorValidation::Shifted { line: 2 }
        );
    }

    #[test]
    fn render_includes_separator() {
        let out = render_anchored_lines(&["hi"], 1);
        assert!(out[0].contains(ANCHOR_SEPARATOR));
        assert!(out[0].starts_with("1:"));
    }
}
