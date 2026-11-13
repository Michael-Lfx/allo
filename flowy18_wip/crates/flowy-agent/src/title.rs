//! Session auto-title helpers.

pub const TITLE_MAX_CHARS: usize = 24;

const TITLE_SYSTEM: &str = "Summarize the user's request into a short task title. \
Reply with the title only. No quotes. At most 24 characters. Prefer the user's language.";

#[must_use]
pub fn fallback_title(input: &str, max_chars: usize) -> String {
    let line = input
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    clamp_title(line, max_chars)
}

#[must_use]
pub fn clamp_title(raw: &str, max_chars: usize) -> String {
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
    let trimmed = collapsed
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim();
    trimmed
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[must_use]
pub fn title_system_prompt() -> &'static str {
    TITLE_SYSTEM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_takes_first_line() {
        assert_eq!(fallback_title("  npm run build  \n more", 40), "npm run build");
    }

    #[test]
    fn clamp_strips_quotes() {
        assert_eq!(
            clamp_title("  \"Fix   the\nlogin bug\"  ", 40),
            "Fix the login bug"
        );
    }
}
