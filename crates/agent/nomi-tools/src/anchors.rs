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

fn is_anchor_hash_token(hash: &str) -> bool {
    (2..=8).contains(&hash.len()) && hash.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Parse `42:ab` (tolerates trailing `→…`).
///
/// Also accepts a unique 4-char hash with no line number (`hsz8`). Models often
/// copy only the hash from `line:hash→`. `line == 0` means "search the whole file".
pub fn parse_anchor(raw: &str) -> Option<ParsedAnchor> {
    let cleaned = raw
        .split(ANCHOR_SEPARATOR)
        .next()
        .unwrap_or(raw)
        .trim();
    if let Some((line_s, hash)) = cleaned.split_once(':') {
        let line: usize = line_s.parse().ok()?;
        if line < 1 {
            return None;
        }
        let hash = hash.trim();
        if !is_anchor_hash_token(hash) {
            return None;
        }
        return Some(ParsedAnchor {
            line,
            hash: hash.to_ascii_lowercase(),
        });
    }
    // Hash-only: require the rendered width so short tokens like "42" do not parse as anchors.
    if cleaned.len() == ANCHOR_HASH_WIDTH && is_anchor_hash_token(cleaned) {
        return Some(ParsedAnchor {
            line: 0,
            hash: cleaned.to_ascii_lowercase(),
        });
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorValidation {
    Ok { line: usize },
    Shifted { line: usize },
    Stale,
}

fn validate_unique_hash(lines: &[&str], hash: &str) -> AnchorValidation {
    let mut hit: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if anchor_line_hash(line) != hash {
            continue;
        }
        if hit.is_some() {
            return AnchorValidation::Stale;
        }
        hit = Some(i);
    }
    match hit {
        Some(i) => AnchorValidation::Ok { line: i + 1 },
        None => AnchorValidation::Stale,
    }
}

/// Validate anchor against full file lines (0-based slice, 1-based line numbers).
pub fn validate_anchor(
    lines: &[&str],
    anchor: &ParsedAnchor,
    radius: usize,
) -> AnchorValidation {
    if anchor.line == 0 {
        return validate_unique_hash(lines, &anchor.hash);
    }
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

fn hash_prefix_before_separator(prefix: &str) -> bool {
    is_anchor_hash_token(prefix.trim())
}

/// True when a line looks like a Read/Grep `line:hash→content` (or `hash→content`) prefix.
pub fn line_has_copied_read_prefix(line: &str) -> bool {
    let Some((prefix, _)) = line.split_once(ANCHOR_SEPARATOR) else {
        return false;
    };
    parse_anchor(prefix).is_some() || hash_prefix_before_separator(prefix)
}

fn strip_copied_read_prefix_line(line: &str) -> &str {
    let Some((prefix, rest)) = line.split_once(ANCHOR_SEPARATOR) else {
        return line;
    };
    if parse_anchor(prefix).is_some() || hash_prefix_before_separator(prefix) {
        rest
    } else {
        line
    }
}

/// Strip Read-output `line:hash→` / `hash→` prefixes the model pasted into Edit/Write payloads.
///
/// Only rewrites when a majority of non-empty lines carry a prefix, so real source that happens
/// to contain `→` is left alone.
pub fn strip_copied_read_prefixes(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let nonempty = lines.iter().filter(|line| !line.trim().is_empty()).count();
    let prefixed = lines
        .iter()
        .filter(|line| !line.trim().is_empty() && line_has_copied_read_prefix(line))
        .count();
    if nonempty == 0 || prefixed * 2 < nonempty {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(strip_copied_read_prefix_line(line));
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
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
    fn hash_only_unique_resolves_to_line() {
        let lines = ["alpha", "beta", "gamma"];
        let hash = anchor_line_hash("beta");
        let a = parse_anchor(&hash).unwrap();
        assert_eq!(a.line, 0);
        assert_eq!(
            validate_anchor_default(&lines, &a),
            AnchorValidation::Ok { line: 2 }
        );
    }

    #[test]
    fn hash_only_duplicate_is_stale() {
        let lines = ["alpha", "beta", "alpha"];
        let hash = anchor_line_hash("alpha");
        let a = parse_anchor(&hash).unwrap();
        assert_eq!(
            validate_anchor_default(&lines, &a),
            AnchorValidation::Stale
        );
    }

    #[test]
    fn hash_only_rejects_non_width_tokens() {
        assert!(parse_anchor("42").is_none());
        assert!(parse_anchor("abc").is_none());
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

    #[test]
    fn strips_majority_read_prefixes_including_hash_only() {
        let arrow = ANCHOR_SEPARATOR;
        let pasted = format!("1:h7x2{arrow}fn main() {{\n  m68q{arrow}    let x = 1;\n2:ab3c{arrow}}}");
        let stripped = strip_copied_read_prefixes(&pasted);
        assert_eq!(stripped, "fn main() {\n    let x = 1;\n}");
    }

    #[test]
    fn leaves_source_with_sparse_arrows_alone() {
        let text = "a → b\nplain line\nanother";
        assert_eq!(strip_copied_read_prefixes(text), text);
    }
}
