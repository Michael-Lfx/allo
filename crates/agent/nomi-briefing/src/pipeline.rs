use crate::cards::default_card_for_visual;
use crate::error::{BriefingError, BriefingResult};
use crate::ir::{
    Beat, BeatScript, Claim, Dossier, ResearchPlan, VisualKind, spoken_numerals,
};
use crate::lint::{beat_lint, card_lint, merge_reports, motion_check};
use crate::progress::{RunSnapshot, RunStatus};
use crate::research::{
    dossier_from_urls, draft_plan, merge_citations, require_plan_confirmed, require_source_coverage,
    script_ready_for_tts, unique_domains, SourceRetriever, MIN_INDEPENDENT_DOMAINS,
};
use crate::session::SessionIndex;
use crate::stills::{persist_atmosphere_stills, StillSynth};
use crate::voice::{
    align_chunks, align_from_asr, align_from_durations, apply_timing_to_beats, chunk_beats,
    load_asr_words, persist_tts_chunks, AsrWord, TtsChunk, VoiceSynth,
};
use crate::compose::{compose_working_dir_with_progress, write_beats_file};

pub struct PipelineOutcome {
    pub status: RunStatus,
    pub error_code: Option<String>,
    pub beat_count: i64,
    pub citation_count: i64,
}

pub fn run_pipeline(
    index: &SessionIndex,
    id: &str,
    confirm_plan: bool,
    voice: Option<&dyn VoiceSynth>,
    stills: Option<&dyn StillSynth>,
    retriever: Option<&dyn SourceRetriever>,
) -> BriefingResult<PipelineOutcome> {
    let mut record = index.get(id)?;
    let now = chrono::Local::now().to_rfc3339();

    let mut plan = index.load_plan(id).unwrap_or_else(|_| {
        draft_plan(&record.intent, record.time_window_hours, record.research_depth)
    });
    if confirm_plan {
        plan.confirmed = true;
        index.save_plan(id, &plan)?;
        record.plan_confirmed = true;
    }
    require_plan_confirmed(&plan)?;

    let mut snapshot = RunSnapshot {
        status: RunStatus::Researching,
        ..RunSnapshot::default()
    };
    snapshot.emit("research", "retrieve and rank sources");
    persist(index, &mut record, &snapshot)?;

    let mut dossier = dossier_from_urls(&record.source_urls, &now);
    if unique_domains(&dossier.sources).len() < MIN_INDEPENDENT_DOMAINS {
        if let Some(retriever) = retriever {
            snapshot.emit("research", "search independent sources from intent");
            persist(index, &mut record, &snapshot)?;
            match retriever.retrieve(
                &record.intent,
                record.time_window_hours,
                record.research_depth,
            ) {
                Ok(found) => {
                    dossier = merge_citations(dossier, found);
                    record.source_urls = dossier.sources.iter().map(|row| row.url.clone()).collect();
                }
                Err(err) => {
                    let reason = format!("web research failed: {err}");
                    snapshot.status = RunStatus::Hold;
                    snapshot.emit("hold", &reason);
                    record.stage = "hold".into();
                    record.summary = reason.clone();
                    persist(index, &mut record, &snapshot)?;
                    return Ok(PipelineOutcome {
                        status: RunStatus::Hold,
                        error_code: Some("no_sources".into()),
                        beat_count: 0,
                        citation_count: dossier.sources.len() as i64,
                    });
                }
            }
        }
    }
    index.save_dossier(id, &dossier)?;
    write_sources_md(index, id, &dossier)?;

    if let Err(BriefingError::Hold(reason)) = require_source_coverage(&dossier) {
        let reason = if retriever.is_some() {
            "web research did not find two independent sources; paste URLs or refine the intent"
                .into()
        } else {
            reason
        };
        snapshot.status = RunStatus::Hold;
        snapshot.emit("hold", &reason);
        record.stage = "hold".into();
        record.summary = reason.clone();
        persist(index, &mut record, &snapshot)?;
        return Ok(PipelineOutcome {
            status: RunStatus::Hold,
            error_code: Some("no_sources".into()),
            beat_count: 0,
            citation_count: dossier.sources.len() as i64,
        });
    }

    snapshot.status = RunStatus::Scripting;
    snapshot.emit("script", "build cited beat script");
    persist(index, &mut record, &snapshot)?;

    let mut script = build_script(&record.intent, record.format_secs, &plan, &dossier);
    index.save_script(id, &script)?;
    script_ready_for_tts(&script)?;

    snapshot.status = RunStatus::Aligning;
    snapshot.emit("align", "synthesize speech and align word timestamps");
    persist(index, &mut record, &snapshot)?;

    let working = index.working_dir(id)?;
    let chunks = chunk_beats(&script.beats);
    let asr_words = load_asr_words(&working.join("asr.json"));
    let timing = if let Some(voice) = voice {
        snapshot.emit("tts", "synthesize narration chunks");
        persist(index, &mut record, &snapshot)?;
        let durations = persist_tts_chunks(&working, &chunks, voice, record.tts_choice().as_ref())?;
        if let Some(words) = asr_words.as_deref() {
            align_from_asr_or_durations(&chunks, words, &durations)
        } else {
            align_from_durations(&chunks, &durations)
        }
    } else {
        align_chunks(&chunks, asr_words.as_deref())
    };
    apply_timing_to_beats(&mut script.beats, &timing);
    index.save_script(id, &script)?;
    write_beats_file(&working, &script, &timing)?;

    if let (Some(choice), Some(stills)) = (record.image_choice(), stills) {
        snapshot.emit("stills", "generate atmosphere plates for opening cards");
        persist(index, &mut record, &snapshot)?;
        let _ = persist_atmosphere_stills(&working, &script.beats, stills, &choice);
    }

    let qa = merge_reports(&[
        beat_lint(&script.beats, &timing),
        card_lint(&script.beats),
        motion_check(&script.beats),
    ]);
    index.write_json(id, "qa.json", &qa)?;

    snapshot.status = RunStatus::Composing;
    snapshot.emit("compose", "compose original news cards");
    persist(index, &mut record, &snapshot)?;

    let composed = compose_working_dir_with_progress(&working, &script, |message, meta| {
        snapshot.emit_meta("compose", message, meta);
        let _ = persist(index, &mut record, &snapshot);
    })?;
    if let Some(video) = composed.video_path.clone() {
        record.final_video = Some(video.clone());
        snapshot.final_video = Some(video);
    }

    snapshot.status = RunStatus::Succeeded;
    snapshot.emit("export", "briefing ready");
    record.stage = "ready".into();
    record.summary = "ready".into();
    persist(index, &mut record, &snapshot)?;

    Ok(PipelineOutcome {
        status: RunStatus::Succeeded,
        error_code: None,
        beat_count: script.beats.len() as i64,
        citation_count: dossier.sources.len() as i64,
    })
}

fn align_from_asr_or_durations(
    chunks: &[TtsChunk],
    words: &[AsrWord],
    durations: &[f64],
) -> crate::voice::TimingFile {
    align_from_asr(chunks, words).unwrap_or_else(|| align_from_durations(chunks, durations))
}

fn persist(
    index: &SessionIndex,
    record: &mut crate::session::SessionRecord,
    snapshot: &RunSnapshot,
) -> BriefingResult<()> {
    record.status = snapshot.status;
    record.stage = snapshot.stage.clone();
    record.updated_at = chrono::Local::now().to_rfc3339();
    record.final_video = snapshot.final_video.clone();
    index.upsert(record.clone())?;
    index.save_run_status(&record.id, snapshot)?;
    Ok(())
}

fn write_sources_md(index: &SessionIndex, id: &str, dossier: &Dossier) -> BriefingResult<()> {
    let mut md = String::from("# Sources\n\n");
    for source in &dossier.sources {
        md.push_str(&format!("- [{}]({})\n", source.domain, source.url));
    }
    let path = index.working_dir(id)?.join(crate::session::SOURCES_FILENAME);
    std::fs::write(path, md)?;
    Ok(())
}

fn build_script(intent: &str, format_secs: u32, plan: &ResearchPlan, dossier: &Dossier) -> BeatScript {
    let spoken = spoken_numerals(&format!(
        "今天关注：{}。以下事实均来自已核验来源，未核实的内容不会播出。",
        intent.trim()
    ));
    let citations = dossier.sources.clone();
    let urls: Vec<String> = citations.iter().map(|c| c.url.clone()).collect();
    let visual = VisualKind::EvidenceScreenshot;
    let beats = vec![
        Beat {
            id: "open".into(),
            spoken_text: spoken.clone(),
            on_screen: intent.trim().to_string(),
            visual,
            card: "title_desk".into(),
            claims: vec![Claim {
                text: intent.trim().to_string(),
                citation_urls: urls.clone(),
            }],
            citations: citations.clone(),
            anchors: vec![],
        },
        Beat {
            id: "evidence".into(),
            spoken_text: spoken_numerals("证据来自至少两个独立域名，冲突会并列保留。"),
            on_screen: plan.questions.first().cloned().unwrap_or_default(),
            visual: VisualKind::EvidenceScreenshot,
            card: default_card_for_visual(VisualKind::EvidenceScreenshot.as_str()).to_string(),
            claims: vec![Claim {
                text: "独立源交叉核验".into(),
                citation_urls: urls,
            }],
            citations,
            anchors: vec![],
        },
    ];
    BeatScript {
        format_secs: if format_secs == 0 { 90 } else { format_secs },
        beats,
        unknowns: dossier.unknowns.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ResearchDepth;
    use crate::progress::RunStatus;
    use crate::session::SessionRecord;

    #[test]
    fn hold_without_sources() {
        let dir = tempfile::tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let session = index
            .create(SessionRecord {
                title: "早报".into(),
                intent: "今日科技".into(),
                created_at: now.clone(),
                updated_at: now,
                ..SessionRecord::default()
            })
            .unwrap();
        let outcome = run_pipeline(&index, &session.id, true, None, None, None).unwrap();
        assert_eq!(outcome.status, RunStatus::Hold);
        assert_eq!(outcome.error_code.as_deref(), Some("no_sources"));
    }

    struct FakeSearch;

    impl SourceRetriever for FakeSearch {
        fn retrieve(
            &self,
            _intent: &str,
            _time_window_hours: u32,
            _depth: ResearchDepth,
        ) -> Result<Vec<crate::ir::Citation>, String> {
            Ok(vec![
                crate::ir::Citation {
                    url: "https://news.example/a".into(),
                    domain: "news.example".into(),
                    excerpt: "独立来源甲".into(),
                    retrieved_at: "now".into(),
                },
                crate::ir::Citation {
                    url: "https://reuters.test/b".into(),
                    domain: "reuters.test".into(),
                    excerpt: "独立来源乙".into(),
                    retrieved_at: "now".into(),
                },
            ])
        }
    }

    #[test]
    fn retriever_covers_missing_urls() {
        let dir = tempfile::tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let session = index
            .create(SessionRecord {
                title: "早报".into(),
                intent: "今日科技".into(),
                created_at: now.clone(),
                updated_at: now,
                ..SessionRecord::default()
            })
            .unwrap();
        let outcome = run_pipeline(&index, &session.id, true, None, None, Some(&FakeSearch)).unwrap();
        assert_eq!(outcome.status, RunStatus::Succeeded);
        let stored = index.get(&session.id).unwrap();
        assert!(stored.source_urls.len() >= 2);
    }

    #[test]
    fn two_urls_produce_cited_script() {
        let dir = tempfile::tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let session = index
            .create(SessionRecord {
                title: "早报".into(),
                intent: "芯片出口管制".into(),
                format_secs: 90,
                research_depth: ResearchDepth::Deep,
                time_window_hours: 48,
                source_urls: vec![
                    "https://news.example/a".into(),
                    "https://reuters.test/b".into(),
                ],
                created_at: now.clone(),
                updated_at: now,
                ..SessionRecord::default()
            })
            .unwrap();
        let outcome = run_pipeline(&index, &session.id, true, None, None, None).unwrap();
        assert_eq!(outcome.status, RunStatus::Succeeded);
        let script = index.load_script(&session.id).unwrap();
        assert!(script.beats.iter().all(|b| !b.citations.is_empty()));
        assert!(script.beats.iter().all(|b| !b.anchors.is_empty()));
        assert!(index.working_dir(&session.id).unwrap().join("timing.json").is_file());
        assert!(index.working_dir(&session.id).unwrap().join("beats.json").is_file());
    }

    struct SilenceVoice;

    impl VoiceSynth for SilenceVoice {
        fn synthesize(
            &self,
            _text: &str,
            _choice: Option<&crate::voice::TtsChoice>,
        ) -> Result<crate::voice::SynthesizedClip, String> {
            Ok(crate::voice::SynthesizedClip {
                bytes: crate::voice::silence_wav_for_tests(0.4),
                mime: "audio/wav".into(),
            })
        }
    }

    #[test]
    fn voiced_pipeline_writes_narration() {
        let dir = tempfile::tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let session = index
            .create(SessionRecord {
                title: "早报".into(),
                intent: "芯片出口管制".into(),
                format_secs: 90,
                research_depth: ResearchDepth::Deep,
                time_window_hours: 48,
                source_urls: vec![
                    "https://news.example/a".into(),
                    "https://reuters.test/b".into(),
                ],
                created_at: now.clone(),
                updated_at: now,
                ..SessionRecord::default()
            })
            .unwrap();
        let outcome = run_pipeline(&index, &session.id, true, Some(&SilenceVoice), None, None).unwrap();
        assert_eq!(outcome.status, RunStatus::Succeeded);
        let working = index.working_dir(&session.id).unwrap();
        assert!(
            working.join("narration.wav").is_file() || working.join("narration.mp3").is_file(),
            "TTS must land a narration file for the compositor mux"
        );
    }
}
