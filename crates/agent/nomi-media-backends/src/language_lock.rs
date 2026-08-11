//! Language detection + output locks for chat backends.

/// Dominant natural language for planning narrative outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLanguage {
    Chinese,
    English,
    Unspecified,
}

fn is_cjk_speech_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

/// Detect output language from user creative text (idea / script / novel / requirement).
pub fn detect_output_language(samples: &[&str]) -> OutputLanguage {
    let mut cjk = 0u32;
    let mut latin = 0u32;
    for sample in samples {
        for ch in sample.chars() {
            if is_cjk_speech_char(ch) {
                cjk += 1;
            } else if ch.is_ascii_alphabetic() {
                latin += 1;
            }
        }
    }
    if cjk == 0 && latin == 0 {
        return OutputLanguage::Unspecified;
    }
    if cjk >= 6 || (cjk > 0 && cjk.saturating_mul(2) >= latin) {
        OutputLanguage::Chinese
    } else {
        OutputLanguage::English
    }
}

/// Hard language lock for planning LLM system/user prompts.
pub fn language_lock_clause(lang: OutputLanguage) -> String {
    match lang {
        OutputLanguage::Chinese => "\
[OUTPUT_LANGUAGE — MUST FOLLOW]
The user's creative input is predominantly Chinese (中文).
Write ALL natural-language narrative content in 简体中文: story, script, scene headings, action lines, dialogue, character names when originally Chinese, visual_desc, audio_desc, motion_desc, environment/prop descriptions, and any other prose fields.
JSON keys, enum tokens (large|medium|small), and cam_idx/idx numbers stay as the schema requires (English keys OK).
Do NOT default to English prose just because this instruction is written in English."
            .into(),
        OutputLanguage::English => "\
[OUTPUT_LANGUAGE — MUST FOLLOW]
The user's creative input is predominantly English.
Write ALL natural-language narrative content in English: story, script, dialogue, visual_desc, audio_desc, and other prose fields.
JSON keys and enum tokens stay as the schema requires."
            .into(),
        OutputLanguage::Unspecified => "\
[OUTPUT_LANGUAGE — MUST FOLLOW]
Match the language of the user's creative input for ALL natural-language narrative fields (story, script, dialogue, descriptions).
JSON keys and enum tokens stay as the schema requires. Do not translate the user's language into another language."
            .into(),
    }
}

/// Detect language from samples and return the lock block.
pub fn language_lock_for_sources(samples: &[&str]) -> String {
    language_lock_clause(detect_output_language(samples))
}

/// Detect language from a single planning user message (may include XML tags).
pub fn language_lock_for_text(text: &str) -> String {
    language_lock_for_sources(&[text])
}

/// Prepend a language lock derived from `sources` onto a requirement string.
pub fn with_language_lock(base: &str, sources: &[&str]) -> String {
    let lock = language_lock_for_sources(sources);
    let base = base.trim();
    if base.is_empty() {
        lock
    } else if base.contains("[OUTPUT_LANGUAGE") {
        base.to_string()
    } else {
        format!("{lock}\n\n{base}")
    }
}
