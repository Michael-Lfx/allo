/// Convert HTML to markdown via `htmd`, falling back to `<title>` + stripped
/// body text when conversion fails. Returns `(title, markdown)`.
pub fn html_to_markdown(html: &str) -> (Option<String>, String) {
    let title = extract_title(html);
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "head", "iframe", "noscript"])
        .build();
    let markdown = match converter.convert(html) {
        Ok(md) if !md.trim().is_empty() => md,
        _ => title
            .as_ref()
            .map(|t| format!("# {t}\n\n{}", strip_tags(html)))
            .unwrap_or_else(|| strip_tags(html)),
    };
    (title, markdown)
}

/// First `<title>…</title>` content, whitespace-collapsed.
fn extract_title(html: &str) -> Option<String> {
    // ASCII-only lowercasing keeps byte offsets aligned with `html` (full
    // `to_lowercase` can change byte lengths, e.g. 'İ' → "i̇").
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let open_end = lower[open..].find('>').map(|i| open + i + 1)?;
    let close = lower[open_end..].find("</title").map(|i| open_end + i)?;
    let title = html.get(open_end..close)?;
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then_some(title)
}

/// Crude tag stripper used only as a conversion fallback: drops `<…>` spans
/// and collapses blank-line runs.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let mut lines: Vec<&str> = Vec::new();
    let mut last_blank = true;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !last_blank {
                lines.push("");
            }
            last_blank = true;
        } else {
            lines.push(trimmed);
            last_blank = false;
        }
    }
    lines.join("\n").trim().to_owned()
}

/// Truncate `s` to at most `max_chars` Unicode scalar values.
pub fn truncate_chars(s: &str, max_chars: usize) -> (String, bool) {
    if s.chars().count() <= max_chars {
        return (s.to_owned(), false);
    }
    let truncated: String = s.chars().take(max_chars).collect();
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_markdown_keeps_title() {
        let (title, md) = html_to_markdown(
            "<html><head><title>Hi</title></head><body><p>Hello</p></body></html>",
        );
        assert_eq!(title.as_deref(), Some("Hi"));
        assert!(md.to_lowercase().contains("hello"));
    }
}
