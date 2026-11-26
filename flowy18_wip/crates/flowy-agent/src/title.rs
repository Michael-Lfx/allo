//! Session auto-title and single-line inline Markdown stripping helpers.

pub const TITLE_MAX_CHARS: usize = 24;

const TITLE_SYSTEM: &str = "Summarize the user's request into a crisp 6-12 character task title \
focusing on the concrete deliverable or goal. Reply with the title only. \
No quotes or trailing punctuation. At most 24 characters. Prefer the user's language.";

/// Strip common inline/block Markdown syntax so single-line previews and
/// fallback titles never leak raw `**`, `` ` ``, `#`, or `[label](url)` tokens.
#[must_use]
pub fn strip_markdown_inline(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (line_idx, raw_line) in raw.lines().enumerate() {
        if line_idx > 0 {
            out.push(' ');
        }
        let mut line = raw_line.trim();
        // Strip leading blockquote / heading / list markers.
        loop {
            if let Some(rest) = line.strip_prefix('>') {
                line = rest.trim_start();
            } else if line.starts_with('#') {
                let rest = line.trim_start_matches('#');
                if rest.starts_with(char::is_whitespace) || rest.is_empty() {
                    line = rest.trim_start();
                } else {
                    break;
                }
            } else if let Some(rest) = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .or_else(|| line.strip_prefix("+ "))
            {
                line = rest.trim_start();
            } else {
                break;
            }
        }
        strip_inline_line(line, &mut out);
    }
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !ch.is_control() {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    collapsed.trim().to_owned()
}

fn strip_inline_line(line: &str, out: &mut String) {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        // Inline image ![alt](url) or link [label](url) -> keep inner label.
        let is_image = chars[i] == '!' && i + 1 < len && chars[i + 1] == '[';
        if is_image || chars[i] == '[' {
            let start = if is_image { i + 2 } else { i + 1 };
            if let Some(close_bracket) =
                (start..len).find(|&idx| chars[idx] == ']')
                && close_bracket + 1 < len
                && chars[close_bracket + 1] == '('
                && let Some(close_paren) =
                    (close_bracket + 2..len).find(|&idx| chars[idx] == ')')
            {
                let label: String =
                    chars[start..close_bracket].iter().collect();
                strip_inline_line(&label, out);
                i = close_paren + 1;
                continue;
            }
        }
        // Doubled markers: **, __, ~~.
        if i + 1 < len
            && ((chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_')
                || (chars[i] == '~' && chars[i + 1] == '~'))
        {
            i += 2;
            continue;
        }
        // Inline code ticks.
        if chars[i] == '`' {
            i += 1;
            continue;
        }
        // Single-star emphasis *word* (preserve standalone arithmetic like `2 * 3`).
        if chars[i] == '*' {
            let prev_ws = i == 0 || chars[i - 1].is_whitespace();
            let next_ws = i + 1 >= len || chars[i + 1].is_whitespace();
            if !(prev_ws && next_ws) {
                i += 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
}

#[must_use]
pub fn fallback_title(input: &str, max_chars: usize) -> String {
    let line = input
        .lines()
        .map(strip_markdown_inline)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let cleaned = normalize_title_text(&line);
    let chars: Vec<char> = cleaned.chars().collect();
    if chars.len() <= max_chars {
        return cleaned;
    }
    if max_chars <= 1 {
        return chars.into_iter().take(max_chars).collect();
    }
    let limit = max_chars - 1;
    let min_break = (limit / 2).max(1);
    let cut = (min_break..=limit)
        .rev()
        .find(|&idx| is_title_boundary(chars[idx]))
        .unwrap_or(limit);
    let head: String = chars[..cut].iter().collect();
    let head = head
        .trim_end_matches(|c: char| c.is_whitespace() || is_title_boundary(c));
    if head.is_empty() {
        let hard: String = chars[..limit].iter().collect();
        format!("{}\u{2026}", hard.trim_end())
    } else {
        format!("{head}\u{2026}")
    }
}

const fn is_title_boundary(ch: char) -> bool {
    matches!(
        ch,
        ' ' | ','
            | ';'
            | ':'
            | '.'
            | '!'
            | '?'
            | '，'
            | '、'
            | '；'
            | '：'
            | '。'
            | '！'
            | '？'
    )
}

fn normalize_title_text(raw: &str) -> String {
    let mut collapsed = String::with_capacity(raw.len());
    let mut prev_space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !c.is_control() {
            collapsed.push(c);
            prev_space = false;
        }
    }
    collapsed
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim()
        .to_owned()
}

#[must_use]
pub fn clamp_title(raw: &str, max_chars: usize) -> String {
    let cleaned = normalize_title_text(&strip_markdown_inline(raw));
    cleaned
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Normalizes whitespace and clamps length for manually entered titles while
/// preserving literal punctuation/Markdown tokens (`__`, `#`, `-`, `[a](b)`).
#[must_use]
pub fn clamp_manual_title(raw: &str, max_chars: usize) -> String {
    let mut collapsed = String::with_capacity(raw.len());
    let mut prev_space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !c.is_control() {
            collapsed.push(c);
            prev_space = false;
        }
    }
    collapsed
        .trim()
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Scrubs credential-like substrings before in-memory search matching and
/// snippet extraction so plaintext secrets cannot be probed or exposed via
/// Quick Search.
#[must_use]
pub fn redact_search_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_was_auth_scheme = false;
    for part in raw.split_inclusive(char::is_whitespace) {
        let trimmed = part.trim_end_matches(char::is_whitespace);
        let ws = &part[trimmed.len()..];
        let scrubbed = if prev_was_auth_scheme {
            prev_was_auth_scheme = false;
            if trimmed.is_empty() {
                String::new()
            } else {
                "[REDACTED]".to_owned()
            }
        } else {
            let lower = trimmed.to_ascii_lowercase();
            if lower == "bearer" || lower == "basic" {
                prev_was_auth_scheme = true;
                trimmed.to_owned()
            } else {
                scrub_token(trimmed)
            }
        };
        out.push_str(&scrubbed);
        out.push_str(ws);
    }
    out
}

fn scrub_token(token: &str) -> String {
    // Strip URL userinfo (`https://user:pass@host` -> `https://[REDACTED]@host`).
    if let Some(scheme_end) = token.find("://") {
        let rest = &token[scheme_end + 3..];
        if let Some(at_pos) = rest.find('@')
            && rest[..at_pos].contains(':')
            && !rest[..at_pos].contains('/')
        {
            let mut out = String::with_capacity(token.len());
            out.push_str(&token[..scheme_end + 3]);
            out.push_str("[REDACTED]");
            out.push_str(&rest[at_pos..]);
            return out;
        }
    }
    // Redact `key=value` or `key:value` for secret-bearing keys.
    for sep in ['=', ':'] {
        if let Some(idx) = token.find(sep) {
            let key = token[..idx]
                .trim_matches(|c: char| {
                    !c.is_ascii_alphanumeric() && c != '_' && c != '-'
                })
                .to_ascii_lowercase();
            let val = token[idx + 1..].trim_matches(|c: char| {
                matches!(c, '"' | '\'' | ',' | ';' | ')' | ']' | '}')
            });
            if !val.is_empty() && is_secret_key(&key) {
                return format!("{}{}[REDACTED]", &token[..=idx], "");
            }
        }
    }
    // Redact standalone token prefixes / JWT shapes.
    let core = token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    });
    if is_secret_literal(core) {
        return "[REDACTED]".to_owned();
    }
    token.to_owned()
}

fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "api_key"
            | "apikey"
            | "api-key"
            | "token"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "auth_token"
            | "secret"
            | "client_secret"
            | "password"
            | "passwd"
            | "pwd"
            | "authorization"
            | "x-api-key"
            | "private_key"
    ) || key.ends_with("_api_key")
        || key.ends_with("_token")
        || key.ends_with("_secret")
        || key.ends_with("_password")
}

fn is_secret_literal(core: &str) -> bool {
    let lower = core.to_ascii_lowercase();
    (lower.starts_with("sk-") && core.len() >= 10)
        || (lower.starts_with("ghp_") && core.len() >= 12)
        || (lower.starts_with("gho_") && core.len() >= 12)
        || (lower.starts_with("glpat-") && core.len() >= 12)
        || (core.starts_with("AIza") && core.len() >= 16)
        || (core.starts_with("AKIA") && core.len() >= 16)
        || (core.starts_with("eyJ")
            && core.len() >= 24
            && core.matches('.').count() >= 2)
}

#[must_use]
pub const fn title_system_prompt() -> &'static str {
    TITLE_SYSTEM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_takes_first_line() {
        assert_eq!(
            fallback_title("  npm run build  \n more", 40),
            "npm run build"
        );
    }

    #[test]
    fn clamp_strips_quotes() {
        assert_eq!(
            clamp_title("  \"Fix   the\nlogin bug\"  ", 40),
            "Fix the login bug"
        );
    }

    #[test]
    fn strip_markdown_cleans_bold_headings_code_and_links() {
        assert_eq!(
            strip_markdown_inline("北京现在**只有 1.2 ℃、体感..."),
            "北京现在只有 1.2 ℃、体感..."
        );
        assert_eq!(
            strip_markdown_inline(
                "## 查看 [Q3 汇报](https://example.com) 的 `deck.pptx`"
            ),
            "查看 Q3 汇报 的 deck.pptx"
        );
    }

    #[test]
    fn fallback_truncates_gracefully_at_clause_boundary() {
        let title = fallback_title(
            "为当前项目做一个个人作品集网站，包含项目卡片、案例详情和联系方式",
            TITLE_MAX_CHARS,
        );
        assert!(title.chars().count() <= TITLE_MAX_CHARS);
        assert!(title.ends_with('\u{2026}'));
        assert!(!title.contains('案'));
        assert_eq!(title, "为当前项目做一个个人作品集网站，包含项目卡片…");
    }
}
