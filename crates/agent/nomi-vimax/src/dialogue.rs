//! Spoken-line vs SFX/UI quotes, plus Seedance-safe dialogue cleanup.
//!
//! Storyboard `audio_desc` reuses 「」 for foley (`键盘「咔哒」`) and UI copy
//! (`屏幕提示音「发布成功」`). Seedance `{…}` captions must only contain words
//! a character actually says. Off-screen / phone VO still counts as speech.

const QUOTE_PAIRS: &[(char, char)] = &[('「', '」'), ('“', '”'), ('"', '"'), ('{', '}')];

#[derive(Debug, Clone)]
struct QuoteSpan {
    open_byte: usize,
    payload_start: usize,
    payload_end: usize,
    end_byte: usize,
    open: char,
    payload: String,
}

fn next_quote(s: &str, from: usize) -> Option<QuoteSpan> {
    let rest = s.get(from..)?;
    let mut best: Option<(usize, char, char)> = None;
    for &(open, close) in QUOTE_PAIRS {
        if let Some(rel) = rest.find(open) {
            let at = from + rel;
            if best.is_none_or(|(b, _, _)| at < b) {
                best = Some((at, open, close));
            }
        }
    }
    let (open_byte, open, close) = best?;
    let after = open_byte + open.len_utf8();
    let Some(rel) = s.get(after..).and_then(|tail| tail.find(close)) else {
        return next_quote(s, after);
    };
    let payload_end = after + rel;
    Some(QuoteSpan {
        open_byte,
        payload_start: after,
        payload_end,
        end_byte: payload_end + close.len_utf8(),
        open,
        payload: s[after..payload_end].to_string(),
    })
}

fn for_each_quote(s: &str, mut visit: impl FnMut(QuoteSpan)) {
    let mut i = 0;
    while let Some(span) = next_quote(s, i) {
        i = span.end_byte;
        visit(span);
    }
}

fn is_pause_dash(ch: char) -> bool {
    matches!(ch, '—' | '–' | '―' | '─')
}

fn replace_pause_dashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_pause_dash(chars[i]) {
            while i < chars.len() && is_pause_dash(chars[i]) {
                i += 1;
            }
            out.push_str("......");
            continue;
        }
        if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] == '-' {
                i += 1;
            }
            out.push_str("......");
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Strip nested book-quotes / brackets that Seedance reads as caption glyphs,
/// and turn em-dash pauses into ellipses.
pub(crate) fn sanitize_spoken_payload(raw: &str) -> String {
    let s = replace_pause_dashes(raw);
    const STRIP: &[char] = &[
        '『', '』', '「', '」', '“', '”', '"', '【', '】', '〔', '〕', '〖', '〗', '{', '}',
    ];
    s.chars().filter(|c| !STRIP.contains(c)).collect::<String>()
}

/// Rewrite spoken quote payloads in-place; SFX/UI quotes stay untouched.
pub(crate) fn rewrite_spoken_payloads(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0;
    for_each_quote(s, |span| {
        if !quote_is_spoken(s, &span) {
            return;
        }
        out.push_str(&s[cursor..span.payload_start]);
        out.push_str(&sanitize_spoken_payload(&span.payload));
        cursor = span.payload_end;
    });
    out.push_str(&s[cursor..]);
    out
}

/// Byte offset of the first spoken quote opener, if any.
pub(crate) fn find_spoken_quote_byte(s: &str) -> Option<usize> {
    let mut found = None;
    for_each_quote(s, |span| {
        if found.is_none() && quote_is_spoken(s, &span) {
            found = Some(span.open_byte);
        }
    });
    found
}

/// Exclusive byte end of the last spoken quote (after its closer).
pub(crate) fn last_spoken_quote_end(s: &str) -> Option<usize> {
    let mut found = None;
    for_each_quote(s, |span| {
        if quote_is_spoken(s, &span) {
            found = Some(span.end_byte);
        }
    });
    found
}

/// Sanitized payloads of spoken quotes, in order.
pub(crate) fn spoken_payloads(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for_each_quote(s, |span| {
        if quote_is_spoken(s, &span) {
            let payload = sanitize_spoken_payload(&span.payload);
            if !payload.trim().is_empty() {
                out.push(payload);
            }
        }
    });
    out
}

/// True when `text` carries a real spoken line (not SFX/UI quotes, not camera verbs).
pub(crate) fn text_looks_like_dialogue(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if find_spoken_quote_byte(t).is_some() {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    lower.contains("says")
        || lower.contains("said")
        || lower.contains("dialogue")
        || lower.contains("speech")
        || lower.contains("whisper")
        || lower.contains("shouts")
        || t.contains("台词")
        || t.contains("说道")
        || t.contains("喊道")
        || t.contains("怒吼")
        || t.contains("说话")
        || t.contains("轻声")
}

/// Split unmarked `音效… 角色：「台词」 …foley` into spoken vs leftover SFX.
pub(crate) fn split_spoken_and_sfx(raw: &str) -> (String, String) {
    let t = raw.trim();
    if t.is_empty() {
        return (String::new(), String::new());
    }
    let mut spoken_spans: Vec<(usize, usize)> = Vec::new();
    for_each_quote(t, |span| {
        if quote_is_spoken(t, &span) {
            let start = speaker_line_start(t, span.open_byte);
            spoken_spans.push((start, span.end_byte));
        }
    });
    if spoken_spans.is_empty() {
        if text_looks_like_dialogue(t) {
            return (t.to_string(), String::new());
        }
        return (String::new(), t.to_string());
    }
    let mut line_parts = Vec::new();
    let mut sfx_parts = Vec::new();
    let mut cursor = 0usize;
    for (start, end) in spoken_spans {
        let start = start.max(cursor);
        if start > cursor {
            let prefix = t[cursor..start]
                .trim()
                .trim_end_matches(['；', ';', '，', ',', '。', '.'])
                .trim();
            if !prefix.is_empty() {
                sfx_parts.push(prefix.to_string());
            }
        }
        let piece = t[start..end].trim();
        if !piece.is_empty() {
            line_parts.push(piece.to_string());
        }
        cursor = end;
    }
    let tail = t[cursor..]
        .trim()
        .trim_start_matches(['；', ';', '，', ',', '。', '.'])
        .trim();
    if !tail.is_empty() {
        sfx_parts.push(tail.to_string());
    }
    (line_parts.join(" "), sfx_parts.join(" "))
}

pub(crate) fn speaker_name_before_quote(blob: &str, quote_byte: usize) -> Option<String> {
    let (start, end) = speaker_name_span(blob, quote_byte)?;
    let name = blob.get(start..end)?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Inclusive start of `角色：` / `Alice:` immediately before a quote.
pub(crate) fn speaker_line_start(s: &str, quote_byte: usize) -> usize {
    speaker_name_span(s, quote_byte)
        .map(|(start, _)| start)
        .unwrap_or(quote_byte)
}

fn speaker_name_span(s: &str, quote_byte: usize) -> Option<(usize, usize)> {
    let prefix = s.get(..quote_byte)?;
    let chars: Vec<(usize, char)> = prefix.char_indices().collect();
    let mut i = chars.len();
    while i > 0 && chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    if i == 0 {
        return None;
    }
    let last = chars[i - 1].1;
    let tagged = if matches!(last, ':' | '：') {
        i -= 1;
        true
    } else if is_say_verb_char(last) {
        i -= 1;
        if last == '道' && i > 0 && chars[i - 1].1 == '说' {
            i -= 1;
        }
        true
    } else {
        false
    };
    if !tagged {
        return None;
    }
    i = trim_trailing_parentheticals(&chars, i);
    while i > 0 && chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    let name_end = i;
    let mut start = i;
    let mut taken = 0u32;
    while start > 0 && is_speaker_name_char(chars[start - 1].1) && taken < 16 {
        start -= 1;
        taken += 1;
    }
    if taken == 0 {
        return None;
    }
    let start_byte = chars[start].0;
    let (last_byte, last_ch) = chars[name_end - 1];
    Some((start_byte, last_byte + last_ch.len_utf8()))
}

fn trim_trailing_parentheticals(chars: &[(usize, char)], mut end: usize) -> usize {
    loop {
        while end > 0 && chars[end - 1].1.is_whitespace() {
            end -= 1;
        }
        if end == 0 {
            return 0;
        }
        let close = chars[end - 1].1;
        let open = match close {
            ')' => '(',
            '）' => '（',
            _ => return end,
        };
        let mut depth = 0i32;
        let mut j = end;
        let mut found = false;
        while j > 0 {
            j -= 1;
            if chars[j].1 == close {
                depth += 1;
            } else if chars[j].1 == open {
                depth -= 1;
                if depth == 0 {
                    end = j;
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return end;
        }
    }
}

fn is_say_verb_char(ch: char) -> bool {
    matches!(ch, '说' | '道' | '喊' | '叫' | '吼')
}

fn is_speaker_name_char(ch: char) -> bool {
    matches!(ch, '<' | '>' | '《' | '》' | '·' | '-' | '_')
        || ch.is_ascii_alphanumeric()
        || crate::planning::is_cjk_speech_char(ch)
}

fn quote_is_spoken(s: &str, span: &QuoteSpan) -> bool {
    let payload = span.payload.trim();
    if payload.is_empty() {
        return false;
    }
    if follows_sfx_suffix(s, span.end_byte) {
        return false;
    }
    if payload_looks_like_onomatopoeia(payload) {
        return false;
    }
    let speaker = speaker_name_before_quote(s, span.open_byte);
    if speaker.as_deref().is_some_and(is_announcer_speaker) {
        return false;
    }
    if speaker.is_some() {
        return true;
    }
    if span.open == '{' {
        return true;
    }
    if prefix_looks_like_read_aloud(s, span.open_byte) {
        return true;
    }
    if prefix_looks_like_sfx_or_ui(s, span.open_byte) {
        return false;
    }
    false
}

fn follows_sfx_suffix(s: &str, after: usize) -> bool {
    let rest = s.get(after..).unwrap_or("").trim_start();
    rest.starts_with("一声")
        || rest.starts_with("两声")
        || rest.starts_with("闷响")
        || rest.starts_with("脆响")
        || rest.starts_with('声')
        || rest.starts_with('响')
}

fn last_chars(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        s.to_string()
    } else {
        chars[chars.len() - n..].iter().collect()
    }
}

fn prefix_looks_like_sfx_or_ui(s: &str, open_byte: usize) -> bool {
    let tail = last_chars(s.get(..open_byte).unwrap_or(""), 18);
    const NEEDLES: &[&str] = &[
        "提示音",
        "音效",
        "SFX",
        "sfx",
        "键盘",
        "屏幕弹出",
        "系统提示",
        "写着",
        "显示",
        "弹出",
        "标题",
        "屏幕上",
        "黑板上",
        "手机扣",
    ];
    NEEDLES.iter().any(|n| tail.contains(n))
}

fn prefix_looks_like_read_aloud(s: &str, open_byte: usize) -> bool {
    let tail = last_chars(s.get(..open_byte).unwrap_or(""), 8);
    ["念出", "读出", "喊道", "说道", "开口"].iter().any(|n| tail.contains(n))
}

fn is_announcer_speaker(name: &str) -> bool {
    let n = name
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    n.eq_ignore_ascii_case("sfx")
        || n.eq_ignore_ascii_case("ui")
        || n == "系统"
        || n == "音效"
        || n == "弹幕"
        || n == "弹幕提示音"
        || n == "旁白提示"
        || n.contains("提示音")
}

fn payload_looks_like_onomatopoeia(payload: &str) -> bool {
    let compact: String = payload
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '！' | '!' | '。' | '.' | '～' | '~'))
        .collect();
    if compact.is_empty() || compact.chars().count() > 8 {
        return false;
    }
    const ATOMS: &[&str] = &[
        "咔哒", "咔嗒", "喀哒", "砰", "啪", "咚", "叮", "嗡", "沙沙", "哗啦", "笃", "笃笃",
        "咯噔", "叮咚", "滴", "滴滴", "呼呼", "嗖", "轰", "轰隆", "刺啦", "咯吱", "哐", "铛",
        "噗",
    ];
    ATOMS.iter().any(|atom| compact == *atom || compact == format!("{atom}{atom}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOT4: &str =
        "BGM:同前,弦乐渐强至收束。键盘「咔哒」一声脆响,屏幕提示音「面试课降价公告已发布」;随后是<粉总>轻轻呼出一口气的声音。";
    const SHOT10: &str =
        "手机扣在桌面的「砰」一声闷响,办公室空调低鸣。BGM:同前,低沉的弦乐重新铺底,鼓点缓慢而沉重,延续到场景结束。";
    const SHOT3: &str = "BGM:同前,弦乐稍显压抑。四海老板(听筒,冷笑):「你举报我挖你老师,行,我认。200万,一年,我出的。你出得起吗？」;粉总(沉默两秒后,一字一顿):「……我出不起。」";
    const YUAN: &str = "袁老师:「感谢『追梦人超哥』的火箭！超哥你是来听课的还是来拱火的？」";
    const DASH: &str = "袁老师:「这位同学,你要搞清楚,我们今天是来分析文风的,不是来——」";
    const BAN: &str = "系统提示音:「该直播间涉嫌违规,已被封禁。」";

    #[test]
    fn sfx_quotes_are_not_dialogue() {
        assert!(!text_looks_like_dialogue(SHOT4));
        assert!(!text_looks_like_dialogue(SHOT10));
        assert!(spoken_payloads(SHOT4).is_empty());
        assert!(spoken_payloads(SHOT10).is_empty());
        assert_eq!(split_spoken_and_sfx(SHOT4).0, "");
        assert!(split_spoken_and_sfx(SHOT4).1.contains("咔哒"));
        assert_eq!(split_spoken_and_sfx(SHOT10).0, "");
    }

    #[test]
    fn phone_vo_with_parenthetical_is_spoken() {
        assert!(text_looks_like_dialogue(SHOT3));
        assert_eq!(
            speaker_name_before_quote(SHOT3, find_spoken_quote_byte(SHOT3).unwrap()).as_deref(),
            Some("四海老板")
        );
        let payloads = spoken_payloads(SHOT3);
        assert!(payloads.iter().any(|p| p.contains("你出得起吗")));
        assert!(payloads.iter().any(|p| p.contains("我出不起")));
        let (line, sfx) = split_spoken_and_sfx(SHOT3);
        assert!(line.contains("四海老板"));
        assert!(line.contains("粉总"));
        assert!(sfx.contains("BGM") || sfx.contains("弦乐"));
    }

    #[test]
    fn sanitizes_book_quotes_and_em_dashes() {
        assert_eq!(
            spoken_payloads(YUAN).join(""),
            "感谢追梦人超哥的火箭！超哥你是来听课的还是来拱火的？"
        );
        assert_eq!(
            spoken_payloads(DASH).join(""),
            "这位同学,你要搞清楚,我们今天是来分析文风的,不是来......"
        );
        let rewritten = rewrite_spoken_payloads(YUAN);
        assert!(!rewritten.contains('『') && !rewritten.contains('』'));
        assert!(rewritten.contains("追梦人超哥"));
    }

    #[test]
    fn announcer_and_typed_caption() {
        assert!(!text_looks_like_dialogue(BAN));
        assert!(spoken_payloads(BAN).is_empty());
        assert_eq!(spoken_payloads("{快跑}").join(""), "快跑");
        assert!(text_looks_like_dialogue("{快跑}"));
        assert_eq!(
            spoken_payloads("环境底噪。李薇：「今晚别等我」").join(""),
            "今晚别等我"
        );
    }

    #[test]
    fn mixed_speech_and_keyboard_sfx() {
        let raw = "粉总:「同行者,边赚钱,边哭穷,哄着小孩喊恩师。」(轻声念出);随后是键盘「咔哒」一声脆响,屏幕提示音「发布成功」。";
        let (line, sfx) = split_spoken_and_sfx(raw);
        assert!(line.contains("同行者"));
        assert!(!line.contains("咔哒"));
        assert!(sfx.contains("咔哒") || sfx.contains("发布成功"));
        assert_eq!(spoken_payloads(raw).len(), 1);
    }
}
