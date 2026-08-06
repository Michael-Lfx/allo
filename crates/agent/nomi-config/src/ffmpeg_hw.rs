//! Hardware-accelerated ffmpeg capability probing + encode-plan selection.
//!
//! Every process probes once (cached by ffmpeg path + size + mtime + OS), then
//! picks the best H.264 encoder by platform priority, verified with a tiny real
//! encode probe (guards against encoders listed but broken at runtime), and
//! falls back to `libx264`. Also exposes the best decode hwaccel for frame
//! extraction. Selection can be forced with `NOMIFUN_FFMPEG_ENCODER`
//! (`nvenc|qsv|amf|videotoolbox|vaapi|x264|auto`, default `auto`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// H.264 encoders / hwaccels reported by `ffmpeg -encoders` / `ffmpeg -hwaccels`.
#[derive(Debug, Clone, Default)]
pub struct FfmpegHwCapabilities {
    /// `h264_*` encoders present (e.g. `h264_nvenc`, `h264_qsv`, `libx264`).
    pub h264_encoders: Vec<String>,
    /// Decode hwaccels present (e.g. `cuda`, `qsv`, `d3d11va`, `videotoolbox`).
    pub hwaccels: Vec<String>,
}

/// A concrete H.264 encode recipe for a final re-encode (concat / normalize).
#[derive(Debug, Clone)]
pub struct VideoEncodePlan {
    /// Codec name passed to `-c:v` (e.g. `h264_nvenc`).
    pub codec: &'static str,
    /// Encoder-specific args AFTER `-c:v <codec>` (includes `-pix_fmt` when the
    /// encoder takes regular frames).
    pub args: Vec<&'static str>,
    /// Input-side args placed BEFORE every `-i` (e.g. `-vaapi_device /dev/dri/…`
    /// for `h264_vaapi`). Empty for encoders that take regular frames.
    pub input_args: Vec<&'static str>,
    /// Non-empty only for encoders that need an hwupload stage appended to the
    /// caller's `-vf` / `-filter_complex` chain (currently `h264_vaapi`).
    pub hwupload_vf: Option<&'static str>,
    /// Short human label for logs, e.g. `nvenc p4/cq19`.
    pub description: &'static str,
    /// True when a hardware encoder was chosen (false = software `libx264`).
    pub uses_hw: bool,
}

impl VideoEncodePlan {
    /// Flattened `["-c:v", codec, ...args]` for direct `Command::args` use.
    pub fn encode_args(&self) -> Vec<&'static str> {
        let mut out = vec!["-c:v", self.codec];
        out.extend_from_slice(&self.args);
        out
    }
}

/// Per-encoder concurrency cap for parallel clip normalization. Consumer NVENC
/// GPUs allow only ~3-5 concurrent sessions, so keep headroom for the encode.
pub fn recommended_parallelism(plan: &VideoEncodePlan) -> usize {
    match plan.codec {
        "h264_qsv" | "h264_videotoolbox" => 3,
        _ => 2,
    }
}

/// Guaranteed-software plan for retrying a failed hardware encode: returns the
/// `libx264` recipe when `plan` uses hardware, otherwise the same plan.
pub fn software_fallback_plan(plan: &VideoEncodePlan) -> VideoEncodePlan {
    if plan.uses_hw {
        x264_plan()
    } else {
        plan.clone()
    }
}

// Quality targets mirror the existing `libx264 -crf 18` look; hardware `cq` /
// `global_quality` are NOT 1:1 with CRF, so values are slightly relaxed and
// favour a marginally larger file over visible quality loss.
const X264_ARGS: &[&str] = &["-crf", "18", "-preset", "fast", "-pix_fmt", "yuv420p"];
const NVENC_ARGS: &[&str] = &[
    "-preset", "p4", "-rc", "vbr", "-cq", "19", "-b:v", "0", "-profile:v", "high",
    "-pix_fmt", "yuv420p",
];
const QSV_ARGS: &[&str] = &["-global_quality", "20", "-preset", "medium", "-pix_fmt", "yuv420p"];
const AMF_ARGS: &[&str] = &[
    "-quality", "quality", "-rc", "cqp", "-qp_i", "18", "-qp_p", "18", "-pix_fmt", "yuv420p",
];
const VT_ARGS: &[&str] = &["-q:v", "65", "-pix_fmt", "yuv420p", "-allow_sw", "1"];
/// Quality args only — `-vaapi_device` is an INPUT option and lives in
/// [`VideoEncodePlan::input_args`] (must precede every `-i`).
const VAAPI_ARGS: &[&str] = &["-qp", "20"];

/// First existing DRM render node for VAAPI (Linux).
fn vaapi_device_path() -> &'static str {
    if Path::new("/dev/dri/renderD128").exists() {
        "/dev/dri/renderD128"
    } else if Path::new("/dev/dri/card0").exists() {
        "/dev/dri/card0"
    } else {
        "/dev/dri/renderD128"
    }
}

fn x264_plan() -> VideoEncodePlan {
    VideoEncodePlan {
        codec: "libx264",
        args: X264_ARGS.to_vec(),
        input_args: Vec::new(),
        hwupload_vf: None,
        description: "libx264 crf18/fast",
        uses_hw: false,
    }
}

fn nvenc_plan() -> VideoEncodePlan {
    VideoEncodePlan {
        codec: "h264_nvenc",
        args: NVENC_ARGS.to_vec(),
        input_args: Vec::new(),
        hwupload_vf: None,
        description: "nvenc p4/cq19",
        uses_hw: true,
    }
}

fn qsv_plan() -> VideoEncodePlan {
    VideoEncodePlan {
        codec: "h264_qsv",
        args: QSV_ARGS.to_vec(),
        input_args: Vec::new(),
        hwupload_vf: None,
        description: "qsv gq20/medium",
        uses_hw: true,
    }
}

fn amf_plan() -> VideoEncodePlan {
    VideoEncodePlan {
        codec: "h264_amf",
        args: AMF_ARGS.to_vec(),
        input_args: Vec::new(),
        hwupload_vf: None,
        description: "amf cqp18",
        uses_hw: true,
    }
}

fn videotoolbox_plan() -> VideoEncodePlan {
    VideoEncodePlan {
        codec: "h264_videotoolbox",
        args: VT_ARGS.to_vec(),
        input_args: Vec::new(),
        hwupload_vf: None,
        description: "videotoolbox q65",
        uses_hw: true,
    }
}

fn vaapi_plan() -> VideoEncodePlan {
    VideoEncodePlan {
        codec: "h264_vaapi",
        args: VAAPI_ARGS.to_vec(),
        input_args: vec!["-vaapi_device", vaapi_device_path()],
        hwupload_vf: Some("format=nv12,hwupload"),
        description: "vaapi qp20",
        uses_hw: true,
    }
}

fn plan_for_codec(codec: &str) -> Option<VideoEncodePlan> {
    match codec {
        "h264_nvenc" => Some(nvenc_plan()),
        "h264_qsv" => Some(qsv_plan()),
        "h264_amf" => Some(amf_plan()),
        "h264_videotoolbox" => Some(videotoolbox_plan()),
        "h264_vaapi" => Some(vaapi_plan()),
        "libx264" => Some(x264_plan()),
        _ => None,
    }
}

/// Per-OS priority order for the hardware candidates (software fallback implicit).
fn candidate_order(os: &str) -> &'static [&'static str] {
    match os {
        "windows" => &["h264_nvenc", "h264_qsv", "h264_amf"],
        "macos" => &["h264_videotoolbox"],
        _ => &["h264_nvenc", "h264_vaapi", "h264_qsv"],
    }
}

fn forced_codec(forced: &str) -> Option<&'static str> {
    match forced {
        "nvenc" => Some("h264_nvenc"),
        "qsv" => Some("h264_qsv"),
        "amf" => Some("h264_amf"),
        "videotoolbox" => Some("h264_videotoolbox"),
        "vaapi" => Some("h264_vaapi"),
        "x264" | "libx264" | "sw" => Some("libx264"),
        _ => None,
    }
}

/// Pick the best encode plan from probed capabilities.
///
/// `probe_ok` is the micro-probe gate (injectable for tests): an encoder listed
/// in `-encoders` must still survive a real 1-frame encode before being used.
fn select_plan_from(
    os: &str,
    caps: &FfmpegHwCapabilities,
    forced: Option<&str>,
    probe_ok: &dyn Fn(&str) -> bool,
) -> VideoEncodePlan {
    // Explicit force: honour it when available and working; otherwise fall through
    // to the normal priority loop (the forced codec is skipped there because its
    // probe result is already known to be false).
    if let Some(f) = forced.and_then(forced_codec) {
        if f == "libx264" {
            tracing::info!(encoder = "libx264", "ffmpeg encoder forced to software");
            return x264_plan();
        }
        if caps.h264_encoders.iter().any(|e| e == f) && probe_ok(f) {
            if let Some(plan) = plan_for_codec(f) {
                tracing::info!(encoder = f, "ffmpeg encoder forced by NOMIFUN_FFMPEG_ENCODER");
                return plan;
            }
        }
        tracing::warn!(
            forced = f,
            "forced ffmpeg encoder unavailable or failed probe; trying next candidate"
        );
    }

    for codec in candidate_order(os) {
        // The forced codec already failed its probe above — skip re-testing it.
        if forced.and_then(forced_codec) == Some(codec) {
            continue;
        }
        if !caps.h264_encoders.iter().any(|e| e == codec) {
            continue;
        }
        if !probe_ok(codec) {
            tracing::warn!(encoder = codec, "ffmpeg encoder probe failed; trying next candidate");
            continue;
        }
        if let Some(plan) = plan_for_codec(codec) {
            tracing::info!(
                encoder = codec,
                os,
                "selected ffmpeg hardware encoder"
            );
            return plan;
        }
    }
    tracing::info!(os, "no usable ffmpeg hardware encoder; using libx264");
    x264_plan()
}

/// Best decode hwaccel for frame extraction, per platform + probed capabilities.
pub fn decide_decode_hwaccel(caps: &FfmpegHwCapabilities) -> Option<&'static str> {
    let has = |name: &str| caps.hwaccels.iter().any(|h| h == name);
    match std::env::consts::OS {
        "windows" => ["cuda", "d3d11va", "qsv"].into_iter().find(|h| has(h)),
        "macos" => has("videotoolbox").then_some("videotoolbox"),
        _ => ["cuda", "vaapi", "qsv"].into_iter().find(|h| has(h)),
    }
}

/// Decode-side args for a hwaccel (output-side `hwdownload` handled by callers).
pub fn hwaccel_decode_args(accel: &str) -> Option<Vec<&'static str>> {
    match accel {
        "cuda" => Some(vec![
            "-hwaccel", "cuda", "-hwaccel_output_format", "cuda",
        ]),
        "d3d11va" => Some(vec![
            "-hwaccel", "d3d11va", "-hwaccel_output_format", "d3d11",
        ]),
        "qsv" => Some(vec![
            "-hwaccel", "qsv", "-hwaccel_output_format", "qsv",
        ]),
        "videotoolbox" => Some(vec!["-hwaccel", "videotoolbox"]),
        "vaapi" => Some(vec![
            "-hwaccel", "vaapi", "-hwaccel_output_format", "vaapi",
        ]),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Probing
// ---------------------------------------------------------------------------

/// Cache key: the exact ffmpeg binary (path + size + mtime) and OS. Re-probing
/// only happens when the binary changes or the process restarts.
#[derive(Debug, Clone, PartialEq)]
struct ProbeKey {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    os: &'static str,
}

#[derive(Debug, Clone)]
struct CachedProbe {
    key: ProbeKey,
    caps: FfmpegHwCapabilities,
    plan: VideoEncodePlan,
}

fn probe_cache() -> &'static Mutex<Option<CachedProbe>> {
    static CACHE: OnceLock<Mutex<Option<CachedProbe>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Combined probe result: raw capabilities + the selected encode plan.
#[derive(Debug, Clone)]
pub struct FfmpegHwProbe {
    pub caps: FfmpegHwCapabilities,
    pub encode_plan: VideoEncodePlan,
}

/// Probe ffmpeg once per process (cached), returning capabilities + the best
/// encode plan. Never panics; every failure degrades to `libx264`.
pub async fn probe_ffmpeg_hw(ffmpeg: &Path) -> FfmpegHwProbe {
    let key = build_probe_key(ffmpeg);
    {
        let guard = probe_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.as_ref()
            && cached.key == key
        {
            return FfmpegHwProbe {
                caps: cached.caps.clone(),
                encode_plan: cached.plan.clone(),
            };
        }
    }

    let caps = match run_probe(ffmpeg).await {
        Some(caps) => caps,
        None => FfmpegHwCapabilities::default(),
    };
    let forced = std::env::var("NOMIFUN_FFMPEG_ENCODER")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    // Micro-probe each *available* candidate in priority order (libx264 is
    // software and needs no probe). Stop at the first passing encoder — the
    // pure `select_plan_from` below only consults these precomputed results.
    let mut probe_results: std::collections::HashMap<&'static str, bool> =
        std::collections::HashMap::new();
    for codec in candidate_order(std::env::consts::OS) {
        if !caps.h264_encoders.iter().any(|e| e == codec) {
            continue;
        }
        let plan = plan_for_codec(codec).expect("plan exists");
        let ok = micro_probe_encoder(ffmpeg, &plan).await;
        probe_results.insert(codec, ok);
        if ok {
            break;
        }
    }
    // A forced encoder may sit outside the OS priority list — probe it explicitly.
    if let Some(f) = forced.as_deref().and_then(forced_codec)
        && f != "libx264"
        && !probe_results.contains_key(f)
    {
        let plan = plan_for_codec(f).expect("plan exists");
        let ok = micro_probe_encoder(ffmpeg, &plan).await;
        probe_results.insert(f, ok);
    }
    let probe_ok = |codec: &str| probe_results.get(codec).copied().unwrap_or(false);
    let plan = select_plan_from(std::env::consts::OS, &caps, forced.as_deref(), &probe_ok);
    let decode_accel = decide_decode_hwaccel(&caps);
    tracing::info!(
        encoder = plan.codec,
        encode_desc = plan.description,
        decode_accel = decode_accel.unwrap_or("none"),
        ffmpeg = %ffmpeg.display(),
        "ffmpeg hardware plan (probed once per process)"
    );

    let cached = CachedProbe { key, caps: caps.clone(), plan: plan.clone() };
    if let Ok(mut guard) = probe_cache().lock() {
        *guard = Some(cached);
    }
    FfmpegHwProbe { caps, encode_plan: plan }
}

/// Shortcut for callers that only need the encode recipe.
pub async fn select_video_encode_plan(ffmpeg: &Path) -> VideoEncodePlan {
    probe_ffmpeg_hw(ffmpeg).await.encode_plan
}

/// Shortcut for frame-extraction callers that only need the decode hwaccel.
pub async fn select_decode_hwaccel(ffmpeg: &Path) -> Option<&'static str> {
    let probe = probe_ffmpeg_hw(ffmpeg).await;
    decide_decode_hwaccel(&probe.caps)
}

fn build_probe_key(ffmpeg: &Path) -> ProbeKey {
    let meta = std::fs::metadata(ffmpeg).ok();
    ProbeKey {
        path: ffmpeg.to_path_buf(),
        size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        modified: meta.and_then(|m| m.modified().ok()),
        os: std::env::consts::OS,
    }
}

async fn run_probe(ffmpeg: &Path) -> Option<FfmpegHwCapabilities> {
    let (encoders, hwaccels) = tokio::join!(
        probe_encoders(ffmpeg),
        probe_hwaccels(ffmpeg),
    );
    let caps = FfmpegHwCapabilities {
        h264_encoders: encoders,
        hwaccels: hwaccels.unwrap_or_default(),
    };
    if caps.h264_encoders.is_empty() {
        // ffmpeg itself is broken — treat as capability-less (callers already
        // handle missing ffmpeg upstream; degrade to software args here).
        tracing::warn!(ffmpeg = %ffmpeg.display(), "ffmpeg -encoders probe produced no h264 encoders");
    }
    Some(caps)
}

/// `ffmpeg -hide_banner -encoders` — return the `h264_*` encoder names.
async fn probe_encoders(ffmpeg: &Path) -> Vec<String> {
    let Some(output) = run_captured(ffmpeg, &["-hide_banner", "-encoders"], 8).await else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in stdout.lines() {
        // Encoder lines start with ` V` (video). Match `h264_*` codec names.
        let Some(tok) = line.split_whitespace().find(|t| t.starts_with("h264_")) else {
            continue;
        };
        let name = tok.trim_end_matches(',').to_string();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    // libx264 is almost always present; treat a parse miss as absent and rely
    // on the software fallback args anyway.
    out
}

/// `ffmpeg -hide_banner -hwaccels` — one hwaccel name per line.
async fn probe_hwaccels(ffmpeg: &Path) -> Option<Vec<String>> {
    let output = run_captured(ffmpeg, &["-hide_banner", "-hwaccels"], 8).await?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let out: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    Some(out)
}

/// Encode one 64x64 testsrc frame with the candidate encoder. A listed-but-broken
/// encoder (driver init failure, missing device, limited session) fails here.
async fn micro_probe_encoder(ffmpeg: &Path, plan: &VideoEncodePlan) -> bool {
    let dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("nomi_ffmpeg_hw_probe_{}_{}.mp4", std::process::id(), nanos);
    let out = dir.join(name);
    let out_s = out.to_string_lossy().into_owned();

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
    ];
    // Input-side options (e.g. `-vaapi_device`) must precede the input.
    args.extend(plan.input_args.iter().map(|s| (*s).to_string()));
    args.extend([
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "testsrc=size=64x64:rate=1:duration=1".into(),
        "-an".into(),
    ]);
    if let Some(vf) = plan.hwupload_vf {
        args.push("-vf".into());
        args.push(vf.into());
    }
    args.extend(plan.encode_args().iter().map(|s| (*s).to_string()));
    args.push("-y".into());
    args.push(out_s.clone());

    let ok = match run_captured(ffmpeg, &args, 15).await {
        Some(output) => output.status.success(),
        None => false,
    };
    let _ = std::fs::remove_file(&out);
    ok
}

async fn run_captured<A: AsRef<std::ffi::OsStr> + Send + Sync>(
    ffmpeg: &Path,
    args: &[A],
    timeout_secs: u64,
) -> Option<std::process::Output> {
    let mut cmd = hw_command(ffmpeg);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
        Ok(Ok(output)) => Some(output),
        Ok(Err(e)) => {
            tracing::debug!(error = %e, ffmpeg = %ffmpeg.display(), "ffmpeg probe spawn failed");
            None
        }
        Err(_) => {
            tracing::warn!(ffmpeg = %ffmpeg.display(), "ffmpeg probe timed out");
            None
        }
    }
}

/// Spawn ffmpeg without flashing a console on Windows GUI hosts.
fn hw_command(bin: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(bin);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

// ---------------------------------------------------------------------------
// Stream-signature checks for stream-copy concat
// ---------------------------------------------------------------------------

/// Compact stream fingerprint used to decide whether files can be joined by
/// stream copy (uniform codec + parameters).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StreamSignature {
    pub video_codec: String,
    pub width: u32,
    pub height: u32,
    pub time_base: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
}

fn ffprobe_bin(ffmpeg: &Path) -> PathBuf {
    let name = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    ffmpeg
        .parent()
        .map(|d| d.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Probe one file's video + audio stream signature via ffprobe.
pub async fn probe_stream_signature(ffmpeg: &Path, path: &Path) -> Option<StreamSignature> {
    let ffprobe = ffprobe_bin(ffmpeg);
    let mut args: Vec<std::ffi::OsString> = vec![
        "-v".into(),
        "error".into(),
        "-show_entries".into(),
        "stream=codec_type,codec_name,width,height,time_base,sample_rate,channels".into(),
        "-of".into(),
        "json".into(),
    ];
    args.push(path.as_os_str().to_os_string());
    let output = run_captured_os(&ffprobe, &args, 10).await?;
    if !output.status.success() {
        return None;
    }
    parse_stream_signature(&String::from_utf8_lossy(&output.stdout))
}

/// True when every file shares an identical stream signature — a precondition
/// for a safe stream-copy concat. Any missing/unreadable file fails the check.
pub async fn streams_uniform(ffmpeg: &Path, paths: &[PathBuf]) -> bool {
    if paths.len() < 2 {
        return true;
    }
    let mut set = tokio::task::JoinSet::new();
    for p in paths {
        let ffmpeg = ffmpeg.to_path_buf();
        let p = p.clone();
        set.spawn(async move { probe_stream_signature(&ffmpeg, &p).await });
    }
    let mut first: Option<StreamSignature> = None;
    let mut n = 0usize;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Some(sig)) => {
                n += 1;
                match &first {
                    None => first = Some(sig),
                    Some(f) if *f == sig => {}
                    _ => return false,
                }
            }
            _ => return false,
        }
    }
    n == paths.len()
}

async fn run_captured_os(
    bin: &Path,
    args: &[std::ffi::OsString],
    timeout_secs: u64,
) -> Option<std::process::Output> {
    let mut cmd = hw_command(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
        Ok(Ok(output)) => Some(output),
        Ok(Err(e)) => {
            tracing::debug!(error = %e, bin = %bin.display(), "probe spawn failed");
            None
        }
        Err(_) => {
            tracing::warn!(bin = %bin.display(), "probe timed out");
            None
        }
    }
}

fn parse_stream_signature(json: &str) -> Option<StreamSignature> {
    #[derive(serde::Deserialize)]
    struct StreamEntry {
        #[serde(rename = "codec_type")]
        codec_type: String,
        #[serde(rename = "codec_name", default)]
        codec_name: String,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(rename = "time_base", default)]
        time_base: String,
        #[serde(rename = "sample_rate", default)]
        sample_rate: Option<String>,
        #[serde(default)]
        channels: Option<u32>,
    }
    #[derive(serde::Deserialize)]
    struct ProbeOut {
        streams: Vec<StreamEntry>,
    }
    let parsed: ProbeOut = serde_json::from_str(json).ok()?;
    let video = parsed.streams.iter().find(|s| s.codec_type == "video")?;
    let audio = parsed.streams.iter().find(|s| s.codec_type == "audio");
    Some(StreamSignature {
        video_codec: video.codec_name.clone(),
        width: video.width?,
        height: video.height?,
        time_base: video.time_base.clone(),
        sample_rate: audio
            .and_then(|a| a.sample_rate.as_deref())
            .and_then(|s| s.parse().ok()),
        channels: audio.and_then(|a| a.channels),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(encoders: &[&str], hwaccels: &[&str]) -> FfmpegHwCapabilities {
        FfmpegHwCapabilities {
            h264_encoders: encoders.iter().map(|s| s.to_string()).collect(),
            hwaccels: hwaccels.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn ok_probe(_codec: &str) -> bool {
        true
    }

    #[test]
    fn intel_nvidia_prefers_nvenc_over_qsv() {
        // Intel CPU + NVIDIA GPU: both encoders present and working → NVENC wins.
        let c = caps(&["libx264", "h264_qsv", "h264_nvenc"], &["cuda", "qsv"]);
        let plan = select_plan_from("windows", &c, None, &ok_probe);
        assert_eq!(plan.codec, "h264_nvenc");
        assert!(plan.uses_hw);
    }

    #[test]
    fn nvenc_broken_falls_back_to_qsv() {
        // NVENC listed but the micro-probe fails (driver issue) → QSV next.
        let c = caps(&["libx264", "h264_qsv", "h264_nvenc"], &["qsv"]);
        let probe = |codec: &str| codec != "h264_nvenc";
        let plan = select_plan_from("windows", &c, None, &probe);
        assert_eq!(plan.codec, "h264_qsv");
    }

    #[test]
    fn intel_only_uses_qsv() {
        let c = caps(&["libx264", "h264_qsv"], &["qsv"]);
        let plan = select_plan_from("windows", &c, None, &ok_probe);
        assert_eq!(plan.codec, "h264_qsv");
    }

    #[test]
    fn amd_only_uses_amf_on_windows() {
        let c = caps(&["libx264", "h264_amf"], &["d3d11va"]);
        let plan = select_plan_from("windows", &c, None, &ok_probe);
        assert_eq!(plan.codec, "h264_amf");
    }

    #[test]
    fn no_hw_encoder_falls_back_to_libx264() {
        let c = caps(&["libx264"], &[]);
        let plan = select_plan_from("windows", &c, None, &ok_probe);
        assert_eq!(plan.codec, "libx264");
        assert!(!plan.uses_hw);
    }

    #[test]
    fn forced_x264_wins_even_with_nvenc() {
        let c = caps(&["libx264", "h264_nvenc"], &["cuda"]);
        let plan = select_plan_from("windows", &c, Some("x264"), &ok_probe);
        assert_eq!(plan.codec, "libx264");
    }

    #[test]
    fn forced_nvenc_unavailable_falls_through_to_qsv() {
        // Forcing nvenc on a machine without it must try the next candidate
        // (qsv) instead of jumping straight to libx264.
        let c = caps(&["libx264", "h264_qsv"], &["qsv"]);
        let plan = select_plan_from("windows", &c, Some("nvenc"), &ok_probe);
        assert_eq!(plan.codec, "h264_qsv");
    }

    #[test]
    fn forced_nvenc_broken_falls_through_to_qsv() {
        // Forced nvenc is listed but its micro-probe fails — next candidate wins.
        let c = caps(&["libx264", "h264_qsv", "h264_nvenc"], &["qsv"]);
        let probe = |codec: &str| codec != "h264_nvenc";
        let plan = select_plan_from("windows", &c, Some("nvenc"), &probe);
        assert_eq!(plan.codec, "h264_qsv");
    }

    #[test]
    fn forced_nvenc_alone_unavailable_ends_in_libx264() {
        // No other candidate exists — the loop bottoms out at libx264.
        let c = caps(&["libx264"], &[]);
        let plan = select_plan_from("windows", &c, Some("nvenc"), &ok_probe);
        assert_eq!(plan.codec, "libx264");
    }

    #[test]
    fn software_fallback_plan_downgrades_hw_to_x264() {
        let hw = nvenc_plan();
        let fb = software_fallback_plan(&hw);
        assert_eq!(fb.codec, "libx264");
        assert!(!fb.uses_hw);
        let sw = x264_plan();
        let fb2 = software_fallback_plan(&sw);
        assert_eq!(fb2.codec, "libx264");
    }

    #[test]
    fn parses_stream_signature_json() {
        let json = r#"{"streams":[
            {"codec_type":"video","codec_name":"h264","width":1280,"height":720,"time_base":"1/24"},
            {"codec_type":"audio","codec_name":"aac","sample_rate":"44100","channels":2}
        ]}"#;
        let sig = parse_stream_signature(json).unwrap();
        assert_eq!(sig.video_codec, "h264");
        assert_eq!(sig.width, 1280);
        assert_eq!(sig.height, 720);
        assert_eq!(sig.time_base, "1/24");
        assert_eq!(sig.sample_rate, Some(44100));
        assert_eq!(sig.channels, Some(2));
        // Audio-less file still yields a video signature.
        let video_only = r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":640,"height":360,"time_base":"1/30"}]}"#;
        let sig2 = parse_stream_signature(video_only).unwrap();
        assert_eq!(sig2.sample_rate, None);
        assert_eq!(sig2.channels, None);
    }

    #[test]
    fn vaapi_plan_puts_device_in_input_args() {
        let p = vaapi_plan();
        assert!(p.input_args.iter().any(|a| *a == "-vaapi_device"));
        assert!(p.hwupload_vf.is_some());
        let sw = software_fallback_plan(&p);
        assert!(sw.input_args.is_empty(), "software fallback must drop vaapi device args");
        assert_eq!(sw.codec, "libx264");
    }

    #[test]
    fn parallelism_caps_hw_encoders() {
        assert_eq!(recommended_parallelism(&nvenc_plan()), 2);
        assert_eq!(recommended_parallelism(&qsv_plan()), 3);
        assert_eq!(recommended_parallelism(&x264_plan()), 2);
    }

    #[test]
    fn macos_prefers_videotoolbox() {
        let c = caps(&["libx264", "h264_videotoolbox"], &["videotoolbox"]);
        let plan = select_plan_from("macos", &c, None, &ok_probe);
        assert_eq!(plan.codec, "h264_videotoolbox");
    }

    #[test]
    fn linux_with_nvenc_beats_vaapi() {
        let c = caps(&["libx264", "h264_vaapi", "h264_nvenc"], &["cuda", "vaapi"]);
        let plan = select_plan_from("linux", &c, None, &ok_probe);
        assert_eq!(plan.codec, "h264_nvenc");
    }

    #[test]
    fn decode_hwaccel_prefers_cuda_then_d3d11va() {
        assert_eq!(
            decide_decode_hwaccel(&caps(&[], &["d3d11va"])),
            Some("d3d11va")
        );
        assert_eq!(
            decide_decode_hwaccel(&caps(&[], &["cuda", "d3d11va"])),
            Some("cuda")
        );
        assert_eq!(decide_decode_hwaccel(&caps(&[], &["qsv"])), Some("qsv"));
        assert_eq!(decide_decode_hwaccel(&caps(&[], &[])), None);
    }

    #[test]
    fn encode_args_shape() {
        let plan = nvenc_plan();
        let args = plan.encode_args();
        assert_eq!(args[0], "-c:v");
        assert_eq!(args[1], "h264_nvenc");
        assert!(args.iter().any(|a| *a == "-cq"));
    }

    /// Real end-to-end probe against the machine's ffmpeg (PATH or managed bin).
    /// Slow and machine-dependent — run explicitly with `-- --ignored`.
    #[tokio::test]
    #[ignore = "requires ffmpeg on PATH or in the managed bin dir"]
    async fn real_probe_selects_some_plan() {
        let Some(path) = crate::dep_check::resolve_ffmpeg_executable() else {
            eprintln!("ffmpeg not found; skipping real probe");
            return;
        };
        let probe = probe_ffmpeg_hw(&path).await;
        assert!(!probe.encode_plan.codec.is_empty());
        eprintln!(
            "ffmpeg={} encoder={} ({}) decode_accel={:?} hw_encoders={:?}",
            path.display(),
            probe.encode_plan.codec,
            probe.encode_plan.description,
            decide_decode_hwaccel(&probe.caps),
            probe.caps.h264_encoders,
        );
    }
}
