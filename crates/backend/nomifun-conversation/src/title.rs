//! Conversation title helpers shared by the service and any callers that need
//! to derive or clean a title without going through the LLM path.

pub const TITLE_MAX_CHARS: usize = 24;

/// Clamp a title to at most `max_len` characters (Unicode scalar values).
pub fn clamp_title(raw: &str, max_len: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_len).collect::<String>()
}

/// Build a simple fallback title from the first user message.
pub fn fallback_title_from_first_message(first_user_msg: &str, max_len: usize) -> String {
    let line = first_user_msg
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    clamp_title(line, max_len)
}

/// Strip common LLM output artifacts from a raw title string.
pub fn clean_generated_title(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let candidate = match lines.len() {
        0 => raw.trim(),
        1 => lines[0],
        _ => lines
            .iter()
            .copied()
            .filter(|l| l.chars().count() <= 40)
            .min_by_key(|l| l.chars().count())
            .unwrap_or_else(|| lines.last().copied().unwrap_or(lines[0])),
    };
    let mut collapsed = String::new();
    let mut prev_space = false;
    for c in candidate.chars() {
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
    let trimmed = collapsed
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '「' || c == '」');
    let stripped = trimmed
        .strip_prefix("Title:")
        .or_else(|| trimmed.strip_prefix("title:"))
        .or_else(|| trimmed.strip_prefix("标题:"))
        .or_else(|| trimmed.strip_prefix("标题："))
        .unwrap_or(trimmed)
        .trim();
    clamp_title(stripped, TITLE_MAX_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_title_short() {
        assert_eq!(clamp_title("hello", 80), "hello");
    }

    #[test]
    fn test_clamp_title_cjk_by_chars() {
        let s = "部署生产环境的脚本任务说明";
        let result = clamp_title(&s, 6);
        assert_eq!(result.chars().count(), 6);
    }

    #[test]
    fn test_fallback_title() {
        let t = fallback_title_from_first_message("Hello, how are you?", 80);
        assert_eq!(t, "Hello, how are you?");
    }

    #[test]
    fn test_clean_generated_title_multiline() {
        assert_eq!(clean_generated_title("thinking…\n修复登录"), "修复登录");
    }
}
