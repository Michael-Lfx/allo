use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::cards::CARD_CATALOG;
use crate::error::BriefingResult;
use crate::ir::BeatScript;
use crate::lint::{card_lint, merge_reports, motion_check};
use crate::session::{BEATS_FILENAME, TIMING_FILENAME};
use crate::voice::TimingFile;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComposeResult {
    pub video_path: Option<String>,
    pub mode: String,
    pub qa: crate::lint::LintReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComposePlan {
    cards: Vec<String>,
    beats: usize,
}

pub fn write_beats_file(working_dir: &Path, script: &BeatScript, timing: &TimingFile) -> BriefingResult<()> {
    let beats_payload = serde_json::json!({
        "version": 1,
        "beats": script.beats,
        "timing": timing,
        "cards": CARD_CATALOG,
    });
    std::fs::write(
        working_dir.join(BEATS_FILENAME),
        serde_json::to_vec_pretty(&beats_payload)?,
    )?;
    std::fs::write(
        working_dir.join(TIMING_FILENAME),
        serde_json::to_vec_pretty(timing)?,
    )?;
    Ok(())
}

pub fn compose_working_dir(working_dir: &Path, script: &BeatScript) -> BriefingResult<ComposeResult> {
    let card_report = card_lint(&script.beats);
    let motion_report = motion_check(&script.beats);
    let qa = merge_reports(&[card_report, motion_report]);
    if !qa.ok {
        return Ok(ComposeResult {
            video_path: None,
            mode: "lint_failed".into(),
            qa,
        });
    }

    let plan = ComposePlan {
        cards: script.beats.iter().map(|b| b.card.clone()).collect(),
        beats: script.beats.len(),
    };
    std::fs::write(
        working_dir.join("compose-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;

    if let Some(video) = spawn_compositor(working_dir) {
        return Ok(ComposeResult {
            video_path: Some(video),
            mode: "compositor".into(),
            qa,
        });
    }
    if let Some(video) = ffmpeg_stills(working_dir, script) {
        return Ok(ComposeResult {
            video_path: Some(video),
            mode: "ffmpeg_stills".into(),
            qa,
        });
    }
    Ok(ComposeResult {
        video_path: None,
        mode: "stills_audio".into(),
        qa,
    })
}

fn spawn_compositor(working_dir: &Path) -> Option<String> {
    let cli = locate_compositor()?;
    let status = silent_command("node")
        .arg(&cli)
        .arg("--input")
        .arg(working_dir)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let mp4 = working_dir.join("briefing.mp4");
    mp4.exists().then(|| mp4.to_string_lossy().into_owned())
}

fn silent_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn locate_compositor() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("NOMIFUN_BRIEFING_COMPOSITOR") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..6 {
        let candidate = dir.join("packaging/briefing-compositor/cli.mjs");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn ffmpeg_stills(working_dir: &Path, script: &BeatScript) -> Option<String> {
    let video_only = working_dir.join("clips").join("ffmpeg-stills.mp4");
    let _ = std::fs::create_dir_all(working_dir.join("clips"));
    let duration = stills_duration_secs(working_dir, script);
    let color = format!("color=c=0x101418:s=1920x1080:d={duration:.3}:r=30");
    let status = silent_command("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &color,
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            video_only.to_str()?,
        ])
        .status()
        .ok()?;
    if !status.success() || !video_only.is_file() {
        return None;
    }
    let out = working_dir.join("briefing.mp4");
    if let Some(narration) = find_narration(working_dir) {
        let mux = silent_command("ffmpeg")
            .args(["-y", "-i"])
            .arg(&video_only)
            .arg("-i")
            .arg(&narration)
            .args([
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-shortest",
                "-movflags",
                "+faststart",
            ])
            .arg(&out)
            .status()
            .ok()?;
        if mux.success() && out.is_file() {
            return Some(out.to_string_lossy().into_owned());
        }
    }
    if std::fs::copy(&video_only, &out).is_ok() && out.is_file() {
        Some(out.to_string_lossy().into_owned())
    } else {
        None
    }
}

fn stills_duration_secs(working_dir: &Path, script: &BeatScript) -> f64 {
    let fallback = f64::from(script.format_secs.max(4));
    let Ok(raw) = std::fs::read_to_string(working_dir.join(TIMING_FILENAME)) else {
        return fallback;
    };
    let Ok(timing) = serde_json::from_str::<TimingFile>(&raw) else {
        return fallback;
    };
    timing
        .chunks
        .iter()
        .map(|chunk| chunk.end_secs)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|end| end.max(4.0))
        .unwrap_or(fallback)
}

fn find_narration(working_dir: &Path) -> Option<PathBuf> {
    for name in ["narration.wav", "narration.mp3", "audio.wav", "audio.mp3", "full.wav"] {
        let path = working_dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn compositor_missing_is_ok(result: &ComposeResult) -> bool {
    result.mode != "lint_failed"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Beat, BeatScript};

    #[test]
    fn lint_blocks_unknown_card_before_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let script = BeatScript {
            format_secs: 90,
            beats: vec![Beat {
                id: "b1".into(),
                spoken_text: "今日".into(),
                on_screen: String::new(),
                visual: crate::ir::VisualKind::UserAsset,
                card: "not_a_card".into(),
                claims: vec![],
                citations: vec![],
                anchors: vec![],
            }],
            unknowns: vec![],
        };
        let result = compose_working_dir(dir.path(), &script).unwrap();
        assert_eq!(result.mode, "lint_failed");
    }

    #[test]
    fn sidecar_catalog_matches_engine_cards() {
        let catalog_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../packaging/briefing-compositor/catalog.json");
        let raw = std::fs::read_to_string(&catalog_path).unwrap();
        let catalog: Vec<String> = serde_json::from_str(&raw).unwrap();
        let expected: Vec<String> = CARD_CATALOG.iter().map(|id| (*id).to_string()).collect();
        assert_eq!(catalog, expected);
        let cli = catalog_path
            .parent()
            .unwrap()
            .join("cli.mjs")
            .canonicalize()
            .unwrap();
        let source = std::fs::read_to_string(cli).unwrap();
        assert!(!source.contains("video-talkcraft"));
        assert!(!source.contains("@remotion"));
    }
}
