//! Map diarization spans (+ optional voiceprints) onto meeting segment speaker fields.

use std::collections::HashMap;

use uuid::Uuid;

use super::{SpeakerSpan, dominant_speaker_key};
use crate::session::types::MeetingSegmentSnapshot;
use crate::voiceprint::{VoiceprintEncoder, VoiceprintEntry, VoiceprintGallery, slice_pcm_ms};

/// Stable speaker identity for one diarization `speaker_key` within a session.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerIdentity {
    pub speaker_id: String,
    pub display_name: String,
    pub voiceprint_id: Option<String>,
}

/// Assigns UUIDv7 `speaker_id` and display names (`S1`/`S2`/`Me`/…) to segments.
///
/// I2: channel is ignored for identity; only diarization + voiceprint matter.
#[derive(Debug, Default)]
pub struct SpeakerAssigner {
    by_key: HashMap<String, SpeakerIdentity>,
    next_s_index: usize,
}

impl SpeakerAssigner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn identities(&self) -> &HashMap<String, SpeakerIdentity> {
        &self.by_key
    }

    /// Ensure every key in `speaker_keys` has an identity. Voiceprint hits win
    /// (display_name + voiceprint_id); others get `S{n}` in discovery order.
    pub fn bind_keys(
        &mut self,
        speaker_keys: impl IntoIterator<Item = impl AsRef<str>>,
        voiceprint_hits: &HashMap<String, VoiceprintEntry>,
    ) {
        for key in speaker_keys {
            let key = key.as_ref();
            if self.by_key.contains_key(key) {
                continue;
            }
            if let Some(hit) = voiceprint_hits.get(key) {
                self.by_key.insert(
                    key.to_string(),
                    SpeakerIdentity {
                        speaker_id: Uuid::now_v7().to_string(),
                        display_name: hit.display_name.clone(),
                        voiceprint_id: Some(hit.voiceprint_id.clone()),
                    },
                );
            } else {
                self.next_s_index += 1;
                let display_name = format!("S{}", self.next_s_index);
                self.by_key.insert(
                    key.to_string(),
                    SpeakerIdentity {
                        speaker_id: Uuid::now_v7().to_string(),
                        display_name,
                        voiceprint_id: None,
                    },
                );
            }
        }
    }

    /// Extract per-key embeddings from PCM, match against `gallery`, then bind.
    pub fn resolve_from_audio<E: VoiceprintEncoder + ?Sized>(
        &mut self,
        spans: &[SpeakerSpan],
        pcm: &[f32],
        sample_rate: u32,
        encoder: &E,
        gallery: &VoiceprintGallery,
        threshold: f32,
    ) -> Result<(), String> {
        let mut hits: HashMap<String, VoiceprintEntry> = HashMap::new();
        let keys = unique_keys(spans);

        for key in &keys {
            let Some(embedding) = embed_key(spans, key, pcm, sample_rate, encoder)? else {
                continue;
            };
            if let Some(m) = gallery.best_match(&embedding, threshold) {
                hits.insert(key.clone(), m.entry.clone());
            }
        }

        self.bind_keys(keys, &hits);
        Ok(())
    }

    /// Bind keys from spans without voiceprint matching (all become S1/S2…).
    pub fn resolve_spans_only(&mut self, spans: &[SpeakerSpan]) {
        let empty = HashMap::new();
        self.bind_keys(unique_keys(spans), &empty);
    }

    pub fn identity_for_key(&self, speaker_key: &str) -> Option<&SpeakerIdentity> {
        self.by_key.get(speaker_key)
    }

    /// Fill `speaker_id` / `speaker_label` from overlapping diarization spans.
    /// Leaves `channel` unchanged.
    pub fn assign_segment(
        &self,
        mut segment: MeetingSegmentSnapshot,
        spans: &[SpeakerSpan],
    ) -> MeetingSegmentSnapshot {
        if let Some(key) = dominant_speaker_key(spans, segment.start_ms, segment.end_ms) {
            if let Some(id) = self.by_key.get(key) {
                segment.speaker_id = Some(id.speaker_id.clone());
                segment.speaker_label = id.display_name.clone();
            }
        }
        segment
    }

    pub fn assign_segments(
        &self,
        segments: impl IntoIterator<Item = MeetingSegmentSnapshot>,
        spans: &[SpeakerSpan],
    ) -> Vec<MeetingSegmentSnapshot> {
        segments
            .into_iter()
            .map(|s| self.assign_segment(s, spans))
            .collect()
    }
}

fn unique_keys(spans: &[SpeakerSpan]) -> Vec<String> {
    let mut seen = HashMap::new();
    let mut keys = Vec::new();
    for span in spans {
        if seen.insert(span.speaker_key.clone(), ()).is_none() {
            keys.push(span.speaker_key.clone());
        }
    }
    keys
}

fn embed_key<E: VoiceprintEncoder + ?Sized>(
    spans: &[SpeakerSpan],
    key: &str,
    pcm: &[f32],
    sample_rate: u32,
    encoder: &E,
) -> Result<Option<Vec<f32>>, String> {
    let mut chunks: Vec<f32> = Vec::new();
    for span in spans.iter().filter(|s| s.speaker_key == key) {
        chunks.extend(slice_pcm_ms(pcm, sample_rate, span.start_ms, span.end_ms));
    }
    if chunks.is_empty() {
        return Ok(None);
    }
    Ok(Some(encoder.encode(&chunks, sample_rate)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voiceprint::{FakeVoiceprintEncoder, VoiceprintEntry, VoiceprintGallery};

    fn seg(start: i64, end: i64) -> MeetingSegmentSnapshot {
        MeetingSegmentSnapshot {
            segment_id: "seg".into(),
            session_id: "sess".into(),
            channel: None,
            speaker_id: None,
            speaker_label: String::new(),
            text: "hi".into(),
            is_partial: false,
            is_manual_edit: false,
            start_ms: start,
            end_ms: end,
        }
    }

    #[test]
    fn assigns_s_labels_without_voiceprint() {
        let spans = vec![
            SpeakerSpan {
                speaker_key: "0".into(),
                start_ms: 0,
                end_ms: 1000,
            },
            SpeakerSpan {
                speaker_key: "1".into(),
                start_ms: 1000,
                end_ms: 2000,
            },
        ];
        let mut assigner = SpeakerAssigner::new();
        assigner.resolve_spans_only(&spans);
        let out = assigner.assign_segments(vec![seg(100, 400), seg(1200, 1500)], &spans);
        assert_eq!(out[0].speaker_label, "S1");
        assert_eq!(out[1].speaker_label, "S2");
        assert!(out[0].speaker_id.is_some());
        assert_ne!(out[0].speaker_id, out[1].speaker_id);
    }

    #[test]
    fn voiceprint_hit_uses_display_name() {
        let spans = vec![SpeakerSpan {
            speaker_key: "0".into(),
            start_ms: 0,
            end_ms: 500,
        }];
        let entry = VoiceprintEntry {
            voiceprint_id: "vp1".into(),
            display_name: "Me".into(),
            embedding: vec![1.0, 0.0, 0.0, 0.0],
        };
        let mut hits = HashMap::new();
        hits.insert("0".into(), entry);
        let mut assigner = SpeakerAssigner::new();
        assigner.bind_keys(["0"], &hits);
        let out = assigner.assign_segment(seg(0, 400), &spans);
        assert_eq!(out.speaker_label, "Me");
        assert_eq!(
            assigner.identity_for_key("0").unwrap().voiceprint_id.as_deref(),
            Some("vp1")
        );
    }

    #[test]
    fn resolve_from_audio_matches_gallery() {
        let encoder = FakeVoiceprintEncoder::new(8);
        let pcm: Vec<f32> = (0..8000).map(|i| ((i as f32) * 0.01).sin() * 0.4).collect();
        let emb = encoder.encode(&pcm, 16_000).unwrap();
        let gallery = VoiceprintGallery::from_entries(vec![VoiceprintEntry {
            voiceprint_id: "vp-me".into(),
            display_name: "Me".into(),
            embedding: emb,
        }]);
        let spans = vec![SpeakerSpan {
            speaker_key: "a".into(),
            start_ms: 0,
            end_ms: 500,
        }];
        let mut assigner = SpeakerAssigner::new();
        assigner
            .resolve_from_audio(&spans, &pcm, 16_000, &encoder, &gallery, 0.5)
            .unwrap();
        let out = assigner.assign_segment(seg(0, 400), &spans);
        assert_eq!(out.speaker_label, "Me");
    }
}
