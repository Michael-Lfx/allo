use std::collections::BTreeSet;

use crate::error::{BriefingError, BriefingResult};
use crate::ir::{Beat, BeatScript, Citation, Dossier, ResearchPlan, domain_of};

pub const MIN_INDEPENDENT_DOMAINS: usize = 2;
pub const FORMAT_SECS_MIN: u32 = 30;
pub const FORMAT_SECS_MAX: u32 = 300;
pub const FORMAT_SECS_DEFAULT: u32 = 90;

/// Host web search used when the user did not paste enough independent URLs.
pub trait SourceRetriever: Send + Sync {
    fn retrieve(
        &self,
        intent: &str,
        time_window_hours: u32,
        depth: crate::ir::ResearchDepth,
    ) -> Result<Vec<Citation>, String>;
}

pub fn clamp_format_secs(raw: u32) -> u32 {
    if raw == 0 {
        FORMAT_SECS_DEFAULT
    } else {
        raw.clamp(FORMAT_SECS_MIN, FORMAT_SECS_MAX)
    }
}

pub fn merge_citations(mut dossier: Dossier, extra: Vec<Citation>) -> Dossier {
    let mut seen = unique_domains(&dossier.sources);
    for citation in extra {
        let domain = if citation.domain.trim().is_empty() {
            match domain_of(&citation.url) {
                Some(domain) => domain,
                None => continue,
            }
        } else {
            citation.domain.trim().to_ascii_lowercase()
        };
        if !seen.insert(domain.clone()) {
            continue;
        }
        dossier.sources.push(Citation {
            domain,
            ..citation
        });
    }
    dossier
}

pub fn unique_domains(sources: &[Citation]) -> BTreeSet<String> {
    sources
        .iter()
        .filter_map(|c| {
            if c.domain.trim().is_empty() {
                domain_of(&c.url)
            } else {
                Some(c.domain.trim().to_ascii_lowercase())
            }
        })
        .collect()
}

pub fn dossier_from_urls(urls: &[String], retrieved_at: &str) -> Dossier {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    for url in urls {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(domain) = domain_of(trimmed) else {
            continue;
        };
        if !seen.insert(domain.clone()) {
            continue;
        }
        sources.push(Citation {
            url: trimmed.to_string(),
            domain,
            excerpt: String::new(),
            retrieved_at: retrieved_at.to_string(),
        });
    }
    Dossier {
        sources,
        conflicts: Vec::new(),
        unknowns: Vec::new(),
    }
}

pub fn require_plan_confirmed(plan: &ResearchPlan) -> BriefingResult<()> {
    if plan.confirmed {
        Ok(())
    } else {
        Err(BriefingError::InvalidParams(
            "research plan must be confirmed before retrieve".into(),
        ))
    }
}

pub fn require_source_coverage(dossier: &Dossier) -> BriefingResult<()> {
    let domains = unique_domains(&dossier.sources);
    if domains.len() < MIN_INDEPENDENT_DOMAINS {
        return Err(BriefingError::Hold(
            "need at least two independent source domains; refusing to invent today's news".into(),
        ));
    }
    Ok(())
}

pub fn beat_ready_for_tts(beat: &Beat) -> bool {
    !beat.spoken_text.trim().is_empty() && !beat.citations.is_empty()
}

pub fn script_ready_for_tts(script: &BeatScript) -> BriefingResult<()> {
    if script.beats.is_empty() {
        return Err(BriefingError::InvalidParams("beat script is empty".into()));
    }
    for beat in &script.beats {
        if !beat_ready_for_tts(beat) {
            return Err(BriefingError::InvalidParams(format!(
                "beat {} has no citation; cannot enter TTS",
                beat.id
            )));
        }
    }
    Ok(())
}

pub fn draft_plan(intent: &str, time_window_hours: u32, depth: crate::ir::ResearchDepth) -> ResearchPlan {
    let intent = intent.trim().to_string();
    ResearchPlan {
        questions: vec![
            format!("核心事实：{intent}"),
            format!("独立来源如何交叉核验：{intent}"),
        ],
        intent,
        time_window_hours,
        depth,
        confirmed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Claim, VisualKind};

    #[test]
    fn hold_without_two_domains() {
        let dossier = dossier_from_urls(&["https://example.com/a".into()], "now");
        assert!(require_source_coverage(&dossier).is_err());
    }

    #[test]
    fn two_domains_pass() {
        let dossier = dossier_from_urls(
            &[
                "https://news.example/a".into(),
                "https://reuters.test/b".into(),
            ],
            "now",
        );
        assert!(require_source_coverage(&dossier).is_ok());
    }

    #[test]
    fn clamp_format_secs_accepts_flexible_lengths() {
        assert_eq!(clamp_format_secs(0), 90);
        assert_eq!(clamp_format_secs(12), 30);
        assert_eq!(clamp_format_secs(75), 75);
        assert_eq!(clamp_format_secs(900), 300);
    }

    #[test]
    fn merge_citations_dedupes_domains() {
        let base = dossier_from_urls(&["https://news.example/a".into()], "now");
        let merged = merge_citations(
            base,
            vec![
                Citation {
                    url: "https://news.example/b".into(),
                    domain: "news.example".into(),
                    excerpt: String::new(),
                    retrieved_at: "now".into(),
                },
                Citation {
                    url: "https://reuters.test/b".into(),
                    domain: "reuters.test".into(),
                    excerpt: "wire".into(),
                    retrieved_at: "now".into(),
                },
            ],
        );
        assert_eq!(unique_domains(&merged.sources).len(), 2);
    }

    #[test]
    fn uncited_beat_cannot_tts() {
        let script = BeatScript {
            format_secs: 90,
            beats: vec![Beat {
                id: "b1".into(),
                spoken_text: "今日要闻".into(),
                on_screen: String::new(),
                visual: VisualKind::EvidenceScreenshot,
                card: "title_desk".into(),
                claims: vec![Claim {
                    text: "断言".into(),
                    citation_urls: vec![],
                }],
                citations: vec![],
                anchors: vec![],
            }],
            unknowns: vec![],
        };
        assert!(script_ready_for_tts(&script).is_err());
    }
}
