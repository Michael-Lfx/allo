//! Energy-based pseudo diarization (no ML). Suitable for unit tests and
//! graceful fallback when Sherpa models are unavailable.

use super::{Diarizer, SpeakerSpan};

/// Tuning for [`EnergyClusterDiarizer`].
#[derive(Debug, Clone)]
pub struct EnergyClusterDiarizerConfig {
    pub frame_ms: u32,
    pub energy_threshold: f32,
    pub min_speech_ms: u32,
    /// Silence longer than this starts a new speech island.
    pub silence_gap_ms: u32,
    /// Cap on distinct pseudo-speakers (1-D energy k-means).
    pub max_speakers: usize,
}

impl Default for EnergyClusterDiarizerConfig {
    fn default() -> Self {
        Self {
            frame_ms: 30,
            energy_threshold: 0.02,
            min_speech_ms: 200,
            silence_gap_ms: 400,
            max_speakers: 4,
        }
    }
}

/// Simple RMS-island diarizer that clusters islands by mean energy.
#[derive(Debug, Clone)]
pub struct EnergyClusterDiarizer {
    config: EnergyClusterDiarizerConfig,
}

impl Default for EnergyClusterDiarizer {
    fn default() -> Self {
        Self::new(EnergyClusterDiarizerConfig::default())
    }
}

impl EnergyClusterDiarizer {
    pub fn new(config: EnergyClusterDiarizerConfig) -> Self {
        Self { config }
    }
}

impl Diarizer for EnergyClusterDiarizer {
    fn diarize(&self, pcm: &[f32], sample_rate: u32) -> Vec<SpeakerSpan> {
        if pcm.is_empty() || sample_rate == 0 {
            return Vec::new();
        }

        let frame_len = ((sample_rate as u64 * self.config.frame_ms as u64) / 1000).max(1) as usize;
        let frame_ms = self.config.frame_ms as i64;
        let min_frames =
            (self.config.min_speech_ms / self.config.frame_ms.max(1)).max(1) as usize;
        let gap_frames =
            (self.config.silence_gap_ms / self.config.frame_ms.max(1)).max(1) as usize;

        let mut voiced: Vec<(usize, f32)> = Vec::new();
        let mut frame_idx = 0usize;
        while frame_idx * frame_len < pcm.len() {
            let start = frame_idx * frame_len;
            let end = (start + frame_len).min(pcm.len());
            let slice = &pcm[start..end];
            let rms = frame_rms(slice);
            if rms >= self.config.energy_threshold {
                voiced.push((frame_idx, rms));
            }
            frame_idx += 1;
        }

        if voiced.is_empty() {
            return Vec::new();
        }

        let islands = build_islands(&voiced, min_frames, gap_frames);
        if islands.is_empty() {
            return Vec::new();
        }

        let energies: Vec<f32> = islands.iter().map(|i| i.mean_energy).collect();
        let labels = cluster_1d(&energies, self.config.max_speakers.max(1));

        let mut spans: Vec<SpeakerSpan> = islands
            .into_iter()
            .zip(labels)
            .map(|(island, label)| SpeakerSpan {
                speaker_key: label.to_string(),
                start_ms: island.start_frame as i64 * frame_ms,
                end_ms: (island.end_frame as i64 + 1) * frame_ms,
            })
            .collect();

        merge_adjacent_same_speaker(&mut spans);
        spans
    }
}

#[derive(Debug)]
struct SpeechIsland {
    start_frame: usize,
    end_frame: usize,
    mean_energy: f32,
}

fn frame_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

fn build_islands(
    voiced: &[(usize, f32)],
    min_frames: usize,
    gap_frames: usize,
) -> Vec<SpeechIsland> {
    let mut islands = Vec::new();
    let mut start = voiced[0].0;
    let mut end = voiced[0].0;
    let mut energy_sum = voiced[0].1;
    let mut count = 1usize;

    for &(frame, rms) in &voiced[1..] {
        if frame <= end + gap_frames {
            end = frame;
            energy_sum += rms;
            count += 1;
        } else {
            if end >= start && (end - start + 1) >= min_frames {
                islands.push(SpeechIsland {
                    start_frame: start,
                    end_frame: end,
                    mean_energy: energy_sum / count as f32,
                });
            }
            start = frame;
            end = frame;
            energy_sum = rms;
            count = 1;
        }
    }

    if end >= start && (end - start + 1) >= min_frames {
        islands.push(SpeechIsland {
            start_frame: start,
            end_frame: end,
            mean_energy: energy_sum / count as f32,
        });
    }
    islands
}

/// Lightweight 1-D k-means on energies; returns labels in `0..k`.
fn cluster_1d(values: &[f32], max_k: usize) -> Vec<usize> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 || max_k <= 1 {
        return vec![0; n];
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let distinct = count_distinct_eps(&sorted, 1e-6);
    let k = max_k.min(distinct).min(n).max(1);

    if k == 1 {
        return vec![0; n];
    }

    // Initialize centroids at equal quantiles.
    let mut centroids: Vec<f32> = (0..k)
        .map(|i| {
            let idx = ((i as f32 + 0.5) / k as f32 * (n as f32 - 1.0)).round() as usize;
            sorted[idx.min(n - 1)]
        })
        .collect();

    let mut labels = vec![0usize; n];
    for _ in 0..16 {
        for (i, &v) in values.iter().enumerate() {
            let mut best = 0usize;
            let mut best_dist = f32::MAX;
            for (c, &centroid) in centroids.iter().enumerate() {
                let d = (v - centroid).abs();
                if d < best_dist {
                    best_dist = d;
                    best = c;
                }
            }
            labels[i] = best;
        }

        let mut sums = vec![0.0f32; k];
        let mut counts = vec![0usize; k];
        for (i, &label) in labels.iter().enumerate() {
            sums[label] += values[i];
            counts[label] += 1;
        }
        let mut moved = false;
        for c in 0..k {
            if counts[c] > 0 {
                let next = sums[c] / counts[c] as f32;
                if (next - centroids[c]).abs() > 1e-6 {
                    moved = true;
                }
                centroids[c] = next;
            }
        }
        if !moved {
            break;
        }
    }

    // Relabel so lowest centroid → 0, next → 1, …
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&a, &b| {
        centroids[a]
            .partial_cmp(&centroids[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut remap = vec![0usize; k];
    for (new_id, &old_id) in order.iter().enumerate() {
        remap[old_id] = new_id;
    }
    for label in &mut labels {
        *label = remap[*label];
    }
    labels
}

fn count_distinct_eps(sorted: &[f32], eps: f32) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let mut n = 1usize;
    let mut last = sorted[0];
    for &v in &sorted[1..] {
        if (v - last).abs() > eps {
            n += 1;
            last = v;
        }
    }
    n
}

fn merge_adjacent_same_speaker(spans: &mut Vec<SpeakerSpan>) {
    if spans.len() < 2 {
        return;
    }
    let mut out = Vec::with_capacity(spans.len());
    let mut cur = spans[0].clone();
    for next in spans.iter().skip(1) {
        if next.speaker_key == cur.speaker_key && next.start_ms <= cur.end_ms + 50 {
            cur.end_ms = cur.end_ms.max(next.end_ms);
        } else {
            out.push(cur);
            cur = next.clone();
        }
    }
    out.push(cur);
    *spans = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq_hz: f32, amp: f32, sample_rate: u32, duration_ms: u32) -> Vec<f32> {
        let n = (sample_rate as u64 * duration_ms as u64 / 1000) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * freq_hz * t).sin() * amp
            })
            .collect()
    }

    fn silence(sample_rate: u32, duration_ms: u32) -> Vec<f32> {
        vec![0.0; (sample_rate as u64 * duration_ms as u64 / 1000) as usize]
    }

    #[test]
    fn silence_yields_no_spans() {
        let d = EnergyClusterDiarizer::default();
        let pcm = silence(16_000, 1000);
        assert!(d.diarize(&pcm, 16_000).is_empty());
    }

    #[test]
    fn two_energy_levels_produce_two_speakers() {
        let cfg = EnergyClusterDiarizerConfig {
            frame_ms: 20,
            energy_threshold: 0.01,
            min_speech_ms: 100,
            silence_gap_ms: 200,
            max_speakers: 4,
        };
        let d = EnergyClusterDiarizer::new(cfg);
        let mut pcm = tone(440.0, 0.8, 16_000, 500);
        pcm.extend(silence(16_000, 500));
        pcm.extend(tone(440.0, 0.05, 16_000, 500));

        let spans = d.diarize(&pcm, 16_000);
        assert!(spans.len() >= 2, "expected >=2 spans, got {spans:?}");
        let keys: std::collections::HashSet<_> =
            spans.iter().map(|s| s.speaker_key.as_str()).collect();
        assert!(keys.len() >= 2, "expected >=2 speaker keys, got {spans:?}");
    }

    #[test]
    fn single_burst_is_one_speaker() {
        let d = EnergyClusterDiarizer::default();
        let pcm = tone(440.0, 0.5, 16_000, 800);
        let spans = d.diarize(&pcm, 16_000);
        assert!(!spans.is_empty());
        assert!(spans.iter().all(|s| s.speaker_key == spans[0].speaker_key));
    }
}
