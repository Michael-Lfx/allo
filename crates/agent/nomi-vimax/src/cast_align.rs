//! Bind storyboard names back onto the cast list.
//!
//! The character extractor's `identifier_in_scene` is the only person namespace
//! that has a portrait. A storyboard that invents 林悦 / 阿哲 for 主角 / 同事
//! leaves those plates unused. This pass rewrites the invented names using
//! evidence that is already in the script: the same actions and lines.

use std::collections::{HashMap, HashSet};

use crate::domain::{CharacterInScene, ShotBriefDescription, ShotDescription};

const FUNCTION: &str = "的了是在和与或及也都很就不而又把被从对到让给跟向着过吗呢吧啊呀哦嗯我你他她它";

/// Rewrite invented bracket names and speaker prefixes onto the cast.
pub fn align_briefs(
    briefs: &mut [ShotBriefDescription],
    characters: &[CharacterInScene],
    script: &str,
) {
    let corpus = briefs
        .iter()
        .map(brief_blob)
        .collect::<Vec<_>>()
        .join("\n");
    let map = name_map(&corpus, characters, script);
    if map.is_empty() {
        return;
    }
    tracing::info!(?map, "rewrote storyboard names onto the cast list");
    for brief in briefs {
        rewrite(&mut brief.visual_desc, &map, characters);
        if let Some(audio) = brief.audio_desc.as_mut() {
            rewrite(audio, &map, characters);
        }
        for beat in &mut brief.beats {
            rewrite(&mut beat.visual_desc, &map, characters);
            if let Some(audio) = beat.audio_desc.as_mut() {
                rewrite(audio, &map, characters);
            }
        }
    }
}

/// `corpus` is the whole board when one shot is rendered on its own. Evidence
/// for who 林悦 is may sit in an earlier row.
pub fn align_shot(
    shot: &mut ShotDescription,
    characters: &[CharacterInScene],
    script: &str,
    corpus: &str,
) {
    let blob = format!("{corpus}\n{}", shot_blob(shot));
    let map = name_map(&blob, characters, script);
    if map.is_empty() {
        return;
    }
    tracing::info!(shot = shot.idx, ?map, "rewrote shot names onto the cast list");
    rewrite(&mut shot.visual_desc, &map, characters);
    rewrite(&mut shot.motion_desc, &map, characters);
    rewrite(&mut shot.ff_desc, &map, characters);
    rewrite(&mut shot.lf_desc, &map, characters);
    if let Some(audio) = shot.audio_desc.as_mut() {
        rewrite(audio, &map, characters);
    }
    for beat in &mut shot.beats {
        rewrite(&mut beat.motion_desc, &map, characters);
        if let Some(audio) = beat.audio_desc.as_mut() {
            rewrite(audio, &map, characters);
        }
    }
}

fn brief_blob(brief: &ShotBriefDescription) -> String {
    let mut blob = brief.visual_desc.clone();
    if let Some(audio) = &brief.audio_desc {
        blob.push('\n');
        blob.push_str(audio);
    }
    for beat in &brief.beats {
        blob.push('\n');
        blob.push_str(&beat.visual_desc);
        if let Some(audio) = &beat.audio_desc {
            blob.push('\n');
            blob.push_str(audio);
        }
    }
    blob
}

fn shot_blob(shot: &ShotDescription) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        shot.visual_desc,
        shot.motion_desc,
        shot.ff_desc,
        shot.lf_desc,
        shot.audio_desc.as_deref().unwrap_or("")
    )
}

fn rewrite(text: &mut String, map: &[(String, String)], characters: &[CharacterInScene]) {
    let mut next = text.clone();
    let mut pairs = map.to_vec();
    pairs.sort_by_key(|(from, _)| std::cmp::Reverse(from.chars().count()));
    for (from, to) in &pairs {
        next = replace_name(&next, from, to);
    }
    next = fix_pronouns(&next, characters);
    *text = next;
}

fn replace_name(text: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let before_ok = rest[..at]
            .chars()
            .last()
            .map(|c| !is_name_char(c))
            .unwrap_or(true);
        let after = &rest[at + from.len()..];
        let after_ok = after
            .chars()
            .next()
            .map(|c| !is_name_char(c))
            .unwrap_or(true);
        if before_ok && after_ok {
            out.push_str(&rest[..at]);
            out.push_str(to);
            rest = after;
        } else {
            let end = at + from.len();
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphabetic() || is_cjk(c)
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn name_map(corpus: &str, characters: &[CharacterInScene], script: &str) -> Vec<(String, String)> {
    let cast: Vec<&CharacterInScene> = characters
        .iter()
        .filter(|ch| ch.identifier_in_scene.trim().chars().count() >= 2)
        .collect();
    if cast.is_empty() || script.trim().is_empty() || corpus.trim().is_empty() {
        return Vec::new();
    }
    let evidence = cast_evidence(script, &cast);
    let mut invented = invented_names(corpus, &cast);
    invented.sort();
    invented.dedup();
    let mut choices = Vec::new();
    for name in invented {
        let grams = name_grams(corpus, &name, &cast);
        let mut scored: Vec<(i32, String)> = cast
            .iter()
            .map(|ch| {
                let id = ch.identifier_in_scene.trim().to_string();
                let score = evidence
                    .get(&id)
                    .map(|set| {
                        grams
                            .iter()
                            .filter(|gram| set.contains(*gram))
                            .map(|gram| gram_weight(gram))
                            .sum()
                    })
                    .unwrap_or(0);
                (score, id)
            })
            .filter(|(score, _)| *score > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        if scored.first().is_some_and(|(score, _)| *score >= 2) {
            choices.push((name, scored));
        }
    }
    choices.sort_by(|a, b| {
        let margin = |scored: &[(i32, String)]| {
            let best = scored.first().map(|row| row.0).unwrap_or(0);
            let second = scored.get(1).map(|row| row.0).unwrap_or(0);
            best - second
        };
        margin(&b.1)
            .cmp(&margin(&a.1))
            .then(b.1[0].0.cmp(&a.1[0].0))
    });
    let mut used = HashSet::new();
    let mut map = Vec::new();
    for (invented, scored) in choices {
        let available: Vec<&(i32, String)> = scored
            .iter()
            .filter(|(_, cast_name)| !used.contains(cast_name))
            .collect();
        let Some((score, cast_name)) = available.first() else {
            continue;
        };
        let next = available.get(1).map(|row| row.0).unwrap_or(0);
        if *score >= 2 && *score >= next + 2 {
            used.insert(cast_name.clone());
            map.push((invented, cast_name.clone()));
        }
    }
    map
}

fn invented_names(corpus: &str, cast: &[&CharacterInScene]) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |raw: &str| {
        let name = raw.trim();
        if name.chars().count() < 2 || name.chars().count() > 12 {
            return;
        }
        if !name.chars().all(|c| is_name_char(c) || c == '·' || c == ' ') {
            return;
        }
        if is_known_cast_name(name, cast) || names.iter().any(|have| have == name) {
            return;
        }
        names.push(name.to_string());
    };
    for name in bracket_names(corpus) {
        push(&name);
    }
    for (name, _) in attributed_quotes(corpus) {
        push(&name);
    }
    names
}

fn is_known_cast_name(name: &str, cast: &[&CharacterInScene]) -> bool {
    let owned: Vec<CharacterInScene> = cast.iter().map(|ch| (*ch).clone()).collect();
    owned.iter().any(|ch| {
        ch.identifier_in_scene.trim() == name
            || crate::domain::character_mentioned_in(&format!("<{name}>"), ch, &owned)
    })
}

fn bracket_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '<' | '〈' => Some(if chars[i] == '<' { '>' } else { '〉' }),
            _ => None,
        };
        let Some(close) = close else {
            i += 1;
            continue;
        };
        let start = i + 1;
        let Some(rel) = chars[start..].iter().position(|c| *c == close) else {
            break;
        };
        let name: String = chars[start..start + rel].iter().collect();
        if !name.is_empty() {
            out.push(name);
        }
        i = start + rel + 1;
    }
    out
}

fn attributed_quotes(text: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '「' => Some('」'),
            '“' => Some('”'),
            '"' => Some('"'),
            _ => None,
        };
        let Some(close) = close else {
            i += 1;
            continue;
        };
        let start = i + 1;
        let Some(rel) = chars[start..].iter().position(|c| *c == close) else {
            break;
        };
        let quote: String = chars[start..start + rel].iter().collect();
        if let Some(name) = speaker_before(&chars, i) {
            if !quote.trim().is_empty() {
                out.push((name, quote));
            }
        }
        i = start + rel + 1;
    }
    out
}

fn speaker_before(chars: &[char], quote_at: usize) -> Option<String> {
    let mut j = quote_at;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    if j == 0 || (chars[j - 1] != ':' && chars[j - 1] != '：') {
        return None;
    }
    j -= 1;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    if j > 0 && (chars[j - 1] == ')' || chars[j - 1] == '）') {
        let open = if chars[j - 1] == ')' { '(' } else { '（' };
        j -= 1;
        while j > 0 && chars[j - 1] != open {
            j -= 1;
        }
        if j == 0 {
            return None;
        }
        j -= 1;
        while j > 0 && chars[j - 1].is_whitespace() {
            j -= 1;
        }
    }
    let mut start = j;
    while start > 0 && is_name_char(chars[start - 1]) {
        start -= 1;
    }
    let name: String = chars[start..j].iter().collect();
    (name.chars().count() >= 2).then_some(name)
}

fn cast_evidence(script: &str, cast: &[&CharacterInScene]) -> HashMap<String, HashSet<String>> {
    let names: Vec<&str> = cast
        .iter()
        .map(|ch| ch.identifier_in_scene.trim())
        .collect();
    let mut owned: HashMap<String, HashSet<String>> = HashMap::new();
    for name in &names {
        let others: Vec<&str> = names.iter().copied().filter(|n| *n != *name).collect();
        let mut grams = HashSet::new();
        for window in script_windows(script, name, &others) {
            grams.extend(content_grams(&window));
        }
        owned.insert((*name).to_string(), grams);
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for grams in owned.values() {
        for gram in grams {
            *counts.entry(gram.clone()).or_default() += 1;
        }
    }
    for grams in owned.values_mut() {
        grams.retain(|gram| counts.get(gram).copied().unwrap_or(0) == 1);
    }
    owned
}

fn name_grams(corpus: &str, name: &str, cast: &[&CharacterInScene]) -> HashSet<String> {
    let others = other_name_stops(corpus, name, cast);
    let other_refs: Vec<&str> = others.iter().map(String::as_str).collect();
    let mut grams = HashSet::new();
    for window in script_windows(corpus, name, &other_refs) {
        grams.extend(content_grams(&window));
    }
    for (speaker, quote) in attributed_quotes(corpus) {
        if speaker == name {
            grams.extend(content_grams(&quote));
        }
    }
    grams
}

fn other_name_stops(corpus: &str, name: &str, cast: &[&CharacterInScene]) -> Vec<String> {
    let mut stops: Vec<String> = cast
        .iter()
        .map(|ch| ch.identifier_in_scene.trim().to_string())
        .filter(|id| id != name)
        .collect();
    for other in bracket_names(corpus) {
        if other != name && !stops.iter().any(|s| s == &other) {
            stops.push(other);
        }
    }
    for (other, _) in attributed_quotes(corpus) {
        if other != name && !stops.iter().any(|s| s == &other) {
            stops.push(other);
        }
    }
    stops
}

fn script_windows(text: &str, name: &str, others: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        let start = at + name.len();
        let tail = &text[start..];
        let mut end = tail.len();
        // Same line only. The next 口播/画面 is a different sentence and would
        // glue one person's line onto whoever was named last.
        if let Some(nl) = tail.find('\n') {
            end = end.min(nl);
        }
        for other in others {
            if other.is_empty() {
                continue;
            }
            if let Some(pos) = tail.find(other) {
                end = end.min(pos);
            }
        }
        let mut chars = 0;
        let mut cap = 0;
        for (i, ch) in tail[..end].char_indices() {
            if chars >= 80 {
                cap = i;
                break;
            }
            chars += 1;
            cap = i + ch.len_utf8();
        }
        if cap > 0 {
            out.push(tail[..cap].to_string());
        }
        from = start.max(from + name.len());
        if from >= text.len() {
            break;
        }
    }
    out
}

fn content_grams(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for token in split_tokens(text) {
        let chars: Vec<char> = token.chars().collect();
        if chars.iter().any(|c| is_cjk(*c)) {
            if (2..=6).contains(&chars.len()) && gram_ok(&token) {
                set.insert(token.clone());
            }
            if chars.len() >= 2 {
                for n in 2..=4.min(chars.len()) {
                    for i in 0..=chars.len() - n {
                        let gram: String = chars[i..i + n].iter().collect();
                        if gram_ok(&gram) {
                            set.insert(gram);
                        }
                    }
                }
            }
        } else if chars.len() >= 4 {
            set.insert(token.to_ascii_lowercase());
        }
    }
    set
}

fn gram_ok(gram: &str) -> bool {
    let chars: Vec<char> = gram.chars().collect();
    if chars.len() < 2 {
        return false;
    }
    if chars.iter().all(|c| c.is_ascii()) {
        return chars.len() >= 4;
    }
    if chars.len() == 2 && chars.iter().any(|c| FUNCTION.contains(*c)) {
        return false;
    }
    !chars.iter().all(|c| c.is_ascii_digit())
}

fn gram_weight(gram: &str) -> i32 {
    if gram.chars().all(|c| c.is_ascii()) || gram.chars().count() >= 3 {
        2
    } else {
        1
    }
}

fn split_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cjk_run: Option<bool> = None;
    for ch in text.chars() {
        let cjk = is_cjk(ch);
        let latin = ch.is_ascii_alphanumeric();
        if cjk || latin {
            if cjk_run != Some(cjk) {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cjk_run = Some(cjk);
            }
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cjk_run = None;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn fix_pronouns(text: &str, characters: &[CharacterInScene]) -> String {
    let sexes: Vec<Option<bool>> = characters.iter().map(cast_is_female).collect();
    let known: Vec<bool> = sexes.into_iter().flatten().collect();
    if known.is_empty() || !known.iter().all(|female| *female == known[0]) {
        return text.to_string();
    }
    if known[0] {
        swap_pronoun_outside_quotes(text, '他', '她')
    } else {
        swap_pronoun_outside_quotes(text, '她', '他')
    }
}

fn cast_is_female(ch: &CharacterInScene) -> Option<bool> {
    let features = ch.static_features.as_str();
    let female = features.contains("女性")
        || features.contains("女人")
        || features.contains("女孩")
        || features.contains("少女")
        || features.to_ascii_lowercase().contains("female");
    let male = features.contains("男性")
        || features.contains("男人")
        || features.contains("男孩")
        || features.to_ascii_lowercase().contains("male");
    if female && !male {
        Some(true)
    } else if male && !female {
        Some(false)
    } else {
        None
    }
}

fn swap_pronoun_outside_quotes(text: &str, from: char, to: char) -> String {
    let mut out = String::new();
    let mut end: Option<char> = None;
    for ch in text.chars() {
        match (end, ch) {
            (None, '「') => {
                end = Some('」');
                out.push(ch);
            }
            (None, '“') => {
                end = Some('”');
                out.push(ch);
            }
            (None, '"') => {
                end = Some('"');
                out.push(ch);
            }
            (Some(stop), c) if c == stop => {
                end = None;
                out.push(c);
            }
            (None, c) if c == from => out.push(to),
            (_, c) => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(idx: i32, name: &str, features: &str, visible: bool) -> CharacterInScene {
        CharacterInScene {
            idx,
            identifier_in_scene: name.into(),
            is_visible: visible,
            static_features: features.into(),
            dynamic_features: None,
            voice_profile: None,
        }
    }

    fn brief(visual: &str, audio: &str) -> ShotBriefDescription {
        ShotBriefDescription {
            idx: 0,
            is_last: false,
            cam_idx: 0,
            visual_desc: visual.into(),
            audio_desc: Some(audio.into()),
            location_id: String::new(),
            beats: Vec::new(),
        }
    }

    #[test]
    fn invented_storyboard_names_follow_the_script_cast() {
        let cast = vec![
            person(0, "主角", "成年男性,约28岁", true),
            person(1, "老板", "成年男性,约45岁,声音低沉", false),
            person(2, "同事", "成年男性,约30岁", true),
        ];
        let script = "\
图1为应用参考截图
> **画面**：主角背着包、关电脑，一脸下班的狂喜，手刚碰到门把手。
> **口播/字幕**：「周五，6点29分，我已经摸到了门把手——」
> **画面**：主角瘫在工位上，屏幕上是无从下笔的空白PPT。镜头扫过旁边同事：气定神闲，甚至打开了蜘蛛纸牌。
> **口播**：「我对着空白页发了半小时呆，我同事呢？他在玩纸牌！还说明天要给他女儿做个小游戏玩？？」
> **画面**：同事慢悠悠打开Agent应用。此处插入产品录屏：输入一句帮我做一份汇报PPT，商务风。主角瞳孔地震。
> **口播**：「他说，他就打了一句话。一句话——PPT自己长出来了，图表、排版全齐。」
> **画面**：主角抢过电脑亲自试。
";
        let mut rows = vec![
            brief(
                "<林悦>在画面左侧，手拧门把手，她肩膀一僵。<阿哲>在画面右侧，坐着玩蜘蛛纸牌。",
                "林悦：「周五,6点29分,我已经摸到了门把手……」 老板(语音外放,画外音,不入画)：「明早汇报的PPT今晚出。」 林悦：「我对着空白页发了半小时呆,我同事呢？他在玩纸牌！」",
            ),
            brief(
                "<阿哲>在画面右侧，手机屏幕上的Agent输入框，输入一句话“帮我做一份Q3运营汇报PPT,商务风”。<林悦>瞳孔放大，把手机转向她。",
                "阿哲：「我就打了一句话。」 林悦：「一句话……PPT自己长出来了,图表、排版全齐？」",
            ),
        ];
        align_briefs(&mut rows, &cast, script);
        let joined = rows
            .iter()
            .map(|row| format!("{}\n{}", row.visual_desc, row.audio_desc.as_deref().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!joined.contains("林悦"), "{joined}");
        assert!(!joined.contains("阿哲"), "{joined}");
        assert!(joined.contains("<主角>"), "{joined}");
        assert!(joined.contains("<同事>"), "{joined}");
        assert!(joined.contains("老板"), "{joined}");
        assert!(joined.contains("主角：「周五"), "{joined}");
        assert!(joined.contains("同事：「我就打了一句话。」"), "{joined}");
        assert!(!rows[0].visual_desc.contains('她'), "{}", rows[0].visual_desc);
        assert!(rows[0].visual_desc.contains('他'), "{}", rows[0].visual_desc);
    }
}
