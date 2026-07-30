//! MoA privacy filter: redact secrets + PII from advisory text surfaces.
//!
//! Advisor (reference) outputs can echo PII from the conversation — emails,
//! phone numbers, credentials pasted by the user — into surfaces the user may
//! not expect: the labelled reference blocks rendered in the UI and (in
//! `full` mode) the guidance block injected into the aggregator prompt.
//! Secret/credential shapes (API-key prefixes, bearer tokens, private keys)
//! are handled by the repo's central redactor, [`nomi_redact::redact_secrets`]
//! — this module never re-implements those. The two patterns below cover the
//! PII classes the central redactor deliberately leaves alone (emails and
//! formatted phone numbers).
//!
//! Pattern safety: advisory text is frequently code-review-shaped — line
//! numbers, timestamps, git SHAs, IDs, IP addresses. A bare 10-digit match
//! would mangle all of those, so the phone pattern requires clearly delimited
//! formatting: a parenthesized area code and/or explicit `-`/`.` separators
//! between groups ((555) 123-4567, 555-123-4567, 555.123.4567,
//! +1 555-123-4567). Undelimited digit runs (5551234567), dates (2026-07-12),
//! times (12:34:56), hex IDs, versions and dotted quads never match.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use super::MoaAdvice;

/// Privacy filter level, parsed from `moa.privacy_filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyLevel {
    /// No redaction anywhere (the default).
    Off,
    /// Redact only advisory text shown to the user (reference events).
    Display,
    /// Additionally redact advice injected into the aggregator prompt.
    Full,
}

impl PrivacyLevel {
    /// Parse the config string. Empty/`off` and any unknown value map to
    /// [`PrivacyLevel::Off`] — a config typo must never break the loop.
    pub fn from_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "display" => Self::Display,
            "full" => Self::Full,
            _ => Self::Off,
        }
    }
}

static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[\w.+-]+@[\w-]+\.[\w.-]+\b").expect("valid email regex"));

/// Formatted North-American phone numbers. The Rust `regex` crate has no
/// lookaround, so the hermes-style `(?<![\w.+-]) … (?![\w-])` guards are
/// simulated with consuming boundary groups; [`redact_advisor_text`] loops
/// the replacement so a boundary consumed by one match can still delimit
/// the next.
static PHONE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        (?P<pre>^|[^\w.+-])                # no leading word char / dot / + / -
        (?:\+?1[\ .-])?                    # optional NA country code
        (?:\(\d{3}\)[\ .-]?|\d{3}[.-])     # delimited area code: (555) or 555- / 555.
        \d{3}[.-]\d{4}                     # exchange-subscriber with explicit separator
        (?P<post>[^\w-]|$)                 # no trailing word char / hyphen
        ",
    )
    .expect("valid phone regex")
});

/// Redact secrets + PII from one advisor text surface.
///
/// Central secret shapes first, then the MoA-specific email/formatted-phone
/// patterns. Always returns an owned string; callers gate on
/// [`PrivacyLevel`] before paying for it.
pub fn redact_advisor_text(text: &str) -> String {
    let text = nomi_redact::redact_secrets(text);
    let mut out = EMAIL_RE.replace_all(&text, "<email>").into_owned();
    // The consuming boundary groups make adjacent matches non-overlapping;
    // loop until stable. Each pass removes digits, so this terminates.
    loop {
        let next = PHONE_RE
            .replace_all(&out, "${pre}<phone>${post}")
            .into_owned();
        if next == out {
            break;
        }
        out = next;
    }
    out
}

/// Redaction for the user-facing display path (`emit_moa_reference`):
/// both `display` and `full` redact what the user sees.
pub fn redact_for_display(text: &str, level: PrivacyLevel) -> Cow<'_, str> {
    match level {
        PrivacyLevel::Off => Cow::Borrowed(text),
        PrivacyLevel::Display | PrivacyLevel::Full => Cow::Owned(redact_advisor_text(text)),
    }
}

/// Redaction for the aggregator-prompt path (`format_guidance` input):
/// only `full` redacts here — `display` keeps the model's view intact.
pub fn redact_for_guidance(advices: &[MoaAdvice], level: PrivacyLevel) -> Cow<'_, [MoaAdvice]> {
    match level {
        PrivacyLevel::Full => Cow::Owned(
            advices
                .iter()
                .map(|advice| MoaAdvice {
                    label: advice.label.clone(),
                    text: redact_advisor_text(&advice.text),
                })
                .collect(),
        ),
        PrivacyLevel::Off | PrivacyLevel::Display => Cow::Borrowed(advices),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_level_parses_known_values_and_falls_back_to_off() {
        assert_eq!(PrivacyLevel::from_str(""), PrivacyLevel::Off);
        assert_eq!(PrivacyLevel::from_str("off"), PrivacyLevel::Off);
        assert_eq!(PrivacyLevel::from_str("display"), PrivacyLevel::Display);
        assert_eq!(PrivacyLevel::from_str("full"), PrivacyLevel::Full);
        assert_eq!(PrivacyLevel::from_str(" Display "), PrivacyLevel::Display);
        assert_eq!(PrivacyLevel::from_str("FULL"), PrivacyLevel::Full);
        assert_eq!(PrivacyLevel::from_str("everything"), PrivacyLevel::Off);
    }

    #[test]
    fn emails_are_redacted() {
        assert_eq!(
            redact_advisor_text("contact alice.smith+dev@example.co.uk today"),
            "contact <email> today"
        );
    }

    #[test]
    fn formatted_phone_numbers_are_redacted() {
        for (input, expected) in [
            ("call (555) 123-4567 now", "call <phone> now"),
            ("call 555-123-4567 now", "call <phone> now"),
            ("call 555.123.4567 now", "call <phone> now"),
            ("call +1 555-123-4567 now", "call <phone> now"),
            ("555-123-4567", "<phone>"),
            ("a: 555-123-4567, b: 555-123-9999.", "a: <phone>, b: <phone>."),
        ] {
            assert_eq!(redact_advisor_text(input), expected, "{input}");
        }
    }

    #[test]
    fn secrets_are_redacted_via_central_redactor() {
        let out = redact_advisor_text("key is sk-ABCDEFGHIJ0123456789xyz here");
        assert!(!out.contains("sk-ABCDEFGHIJ0123456789xyz"), "{out}");
        assert!(out.contains("[REDACTED_SECRET]"), "{out}");
    }

    #[test]
    fn benign_numeric_text_is_untouched() {
        for text in [
            "released on 2026-07-12 at 12:34:56",
            "version 1.2.3 fixes bug #59959",
            "server at 192.168.100.1234 is fake but must survive",
            "undelimited 5551234567 run",
            "sha 4567-123-4567b stays",
        ] {
            assert_eq!(redact_advisor_text(text), text, "{text}");
        }
    }

    #[test]
    fn display_helper_redacts_for_display_and_full_only() {
        let text = "mail bob@example.com";
        assert_eq!(redact_for_display(text, PrivacyLevel::Off), text);
        assert_eq!(
            redact_for_display(text, PrivacyLevel::Display),
            "mail <email>"
        );
        assert_eq!(redact_for_display(text, PrivacyLevel::Full), "mail <email>");
    }

    #[test]
    fn guidance_helper_redacts_only_at_full_level() {
        let advices = vec![MoaAdvice {
            label: "p/m".into(),
            text: "reach bob@example.com or (555) 123-4567".into(),
        }];
        // Off/Display: the aggregator sees the raw advice.
        for level in [PrivacyLevel::Off, PrivacyLevel::Display] {
            let out = redact_for_guidance(&advices, level);
            assert_eq!(out.as_ref(), advices.as_slice());
        }
        // Full: injected advice is redacted too.
        let out = redact_for_guidance(&advices, PrivacyLevel::Full);
        assert_eq!(out[0].text, "reach <email> or <phone>");
        assert_eq!(out[0].label, "p/m");
    }
}
