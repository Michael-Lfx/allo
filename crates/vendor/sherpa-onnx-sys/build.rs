use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{collections::HashSet, ffi::OsString};

use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;

const RELEASE_BASE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
const SHERPA_ONNX_STATIC_LIBS: &[&str] = &[
    "sherpa-onnx-c-api",
    "sherpa-onnx-core",
    "kaldi-decoder-core",
    "sherpa-onnx-kaldifst-core",
    "sherpa-onnx-fstfar",
    "sherpa-onnx-fst",
    "kaldi-native-fbank-core",
    "kissfft-float",
    "piper_phonemize",
    "espeak-ng",
    "ucd",
    "onnxruntime",
    "ssentencepiece_core",
];

type DynError = Box<dyn Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkMode {
    Static,
    Shared,
}

fn main() {
    if let Err(err) = try_main() {
        panic!("{err}");
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SherpaPack {
    /// Static CPU prebuilt (default).
    Cpu,
}

fn resolve_sherpa_pack(_target_os: &str, _target_arch: &str) -> Result<SherpaPack, DynError> {
    let pack = env::var("SHERPA_ONNX_PACK").unwrap_or_else(|_| "cpu".to_string());
    let pack = pack.trim().to_ascii_lowercase();
    match pack.as_str() {
        "cpu" | "auto" => Ok(SherpaPack::Cpu),
        "cuda" | "gpu" | "directml" | "dml" | "macos" | "coreml" | "osx" => {
            Err(format!("SHERPA_ONNX_PACK={pack} is no longer supported; use cpu (default)").into())
        }
        other => Err(format!("unknown SHERPA_ONNX_PACK='{other}' (expected cpu)").into()),
    }
}

fn try_main() -> Result<(), DynError> {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_PACK");
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_ARCHIVE_DIR");
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_ARCHIVE_SHA256");
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_BASE_URL");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    if env::var_os("DOCS_RS").is_some() {
        // docs.rs sets DOCS_RS=1; skip downloading/linking native libraries
        // so that `cargo doc` can succeed without the real C artifacts.
        return Ok(());
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH")?;
    let _pack = resolve_sherpa_pack(&target_os, &target_arch)?;
    let link_mode = resolve_link_mode()?;
    let lib_dir = resolve_lib_dir(link_mode, &target_os, &target_arch)?;

    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    if link_mode == LinkMode::Shared && matches!(target_os.as_str(), "linux" | "macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
        emit_relative_rpath(&target_os);
        copy_unix_runtime_libs(&lib_dir, &target_os)?;
    }

    if link_mode == LinkMode::Shared && target_os == "windows" {
        copy_windows_runtime_dlls(&lib_dir)?;
    }

    match link_mode {
        LinkMode::Static => emit_static_link_directives(&target_os),
        LinkMode::Shared => emit_shared_link_directives(),
    }

    Ok(())
}

fn resolve_link_mode() -> Result<LinkMode, DynError> {
    let static_enabled = env::var_os("CARGO_FEATURE_STATIC").is_some();
    let shared_enabled = env::var_os("CARGO_FEATURE_SHARED").is_some();

    if static_enabled && shared_enabled {
        return Err("Features `static` and `shared` cannot be enabled at the same time".into());
    }

    if shared_enabled {
        Ok(LinkMode::Shared)
    } else {
        Ok(LinkMode::Static)
    }
}

fn resolve_lib_dir(
    link_mode: LinkMode,
    target_os: &str,
    target_arch: &str,
) -> Result<PathBuf, DynError> {
    if let Some(path) = env::var_os("SHERPA_ONNX_LIB_DIR") {
        let path = PathBuf::from(path);
        if !path.is_dir() {
            return Err(format!(
                "SHERPA_ONNX_LIB_DIR does not exist or is not a directory: {}",
                path.display()
            )
            .into());
        }
        return Ok(path);
    }

    download_prebuilt_libs(link_mode, target_os, target_arch)
}

struct DownloadCandidate {
    name: &'static str,
    url: String,
}

fn is_china_network_environment() -> bool {
    if let Ok(val) = env::var("FLOWY_MIRROR").or_else(|_| env::var("SHERPA_ONNX_MIRROR")) {
        let v = val.trim().to_lowercase();
        if v == "cn" || v == "china" || v == "domestic" {
            return true;
        }
        if v == "global" || v == "en" || v == "overseas" {
            return false;
        }
    }

    if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        if let Some(root_path) = Path::new(&manifest_dir).ancestors().nth(3) {
            let cargo_config = root_path.join(".cargo").join("config.toml");
            if let Ok(content) = fs::read_to_string(cargo_config) {
                if content.contains("rsproxy") {
                    return true;
                }
            }
        }
    }

    for var in ["LC_ALL", "LANG", "LC_MESSAGES", "USERLOCALE"] {
        if let Ok(val) = env::var(var) {
            let lower = val.to_lowercase();
            if lower.contains("zh_cn") || lower.contains("zh-cn") || lower.contains("chinese") {
                return true;
            }
        }
    }

    if let Ok(tz) = env::var("TZ") {
        let lower = tz.to_lowercase();
        if lower.contains("shanghai")
            || lower.contains("beijing")
            || lower.contains("prc")
            || lower.contains("cst-8")
        {
            return true;
        }
    }

    false
}

fn build_candidate_urls(version: &str, archive_name: &str) -> Vec<DownloadCandidate> {
    let official_url = format!("{RELEASE_BASE_URL}/v{version}/{archive_name}");
    let mut candidates = Vec::new();

    if let Ok(custom_base) = env::var("SHERPA_ONNX_BASE_URL") {
        let custom_base = custom_base.trim_end_matches('/');
        candidates.push(DownloadCandidate {
            name: "custom-base",
            url: format!("{custom_base}/v{version}/{archive_name}"),
        });
    }

    let is_cn = is_china_network_environment();

    let ghproxy_com = DownloadCandidate {
        name: "mirror.ghproxy.com",
        url: format!("https://mirror.ghproxy.com/{official_url}"),
    };
    let gitmirror = DownloadCandidate {
        name: "hub.gitmirror.com",
        url: format!("https://hub.gitmirror.com/{official_url}"),
    };
    let ghproxy_net = DownloadCandidate {
        name: "ghproxy.net",
        url: format!("https://ghproxy.net/{official_url}"),
    };
    let official = DownloadCandidate {
        name: "github-official",
        url: official_url.clone(),
    };

    if is_cn {
        println!("cargo:warning=[sherpa-onnx] Domestic network detected; prioritizing China mirrors");
        candidates.push(ghproxy_com);
        candidates.push(gitmirror);
        candidates.push(ghproxy_net);
        candidates.push(official);
    } else {
        candidates.push(official);
        candidates.push(ghproxy_com);
        candidates.push(gitmirror);
        candidates.push(ghproxy_net);
    }

    candidates
}

fn global_cache_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|h| h.join(".flowy").join("cache").join("sherpa-onnx-prebuilt"))
}

fn download_prebuilt_libs(
    link_mode: LinkMode,
    target_os: &str,
    target_arch: &str,
) -> Result<PathBuf, DynError> {
    let archive_name = archive_name(link_mode, target_os, target_arch)?;
    let archive_stem = archive_name.trim_end_matches(".tar.bz2");

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let cache_root = target_dir_from_out_dir(&out_dir)?.join("sherpa-onnx-prebuilt");
    let extracted_dir = cache_root.join(archive_stem);
    let lib_dir = extracted_dir.join("lib");
    let completion_marker = cache_root.join(format!("{archive_stem}.complete"));

    if completion_marker.is_file() && lib_dir.is_dir() {
        return Ok(lib_dir);
    }

    fs::create_dir_all(&cache_root)?;

    let archive_path = cache_root.join(&archive_name);
    let legacy_part = PathBuf::from(format!("{}.part", archive_path.display()));
    if legacy_part.is_file() {
        let _ = fs::remove_file(&legacy_part);
    }
    if !archive_path.is_file() {
        if let Some(local_archive_dir) = env::var_os("SHERPA_ONNX_ARCHIVE_DIR") {
            let local_archive_path = PathBuf::from(local_archive_dir).join(&archive_name);
            if !local_archive_path.is_file() {
                return Err(format!(
                    "SHERPA_ONNX_ARCHIVE_DIR does not contain expected archive: {}",
                    local_archive_path.display()
                )
                .into());
            }

            copy_file_atomically(&local_archive_path, &archive_path)?;
        } else if let Some(global_archive) = global_cache_dir()
            .map(|d| d.join(&archive_name))
            .filter(|p| p.is_file())
        {
            println!(
                "cargo:warning=[sherpa-onnx] Reusing prebuilt archive from global cache: {}",
                global_archive.display()
            );
            copy_file_atomically(&global_archive, &archive_path)?;
        } else {
            let version = env!("CARGO_PKG_VERSION");
            let candidates = build_candidate_urls(version, &archive_name);

            let client = ureq::builder()
                .try_proxy_from_env(true)
                .timeout_connect(std::time::Duration::from_secs(10))
                .timeout_read(std::time::Duration::from_secs(15))
                .build();

            let mut last_err = None;
            let mut downloaded = false;

            let total_candidates = candidates.len();
            for (idx, candidate) in candidates.iter().enumerate() {
                let is_last = idx + 1 == total_candidates;
                let speed_gate_enabled = !is_last;

                println!(
                    "cargo:warning=[sherpa-onnx] Trying download ({}/{}) from {}: {}",
                    idx + 1,
                    total_candidates,
                    candidate.name,
                    candidate.url
                );

                match client.get(&candidate.url).call() {
                    Ok(response) => {
                        let total_len = response
                            .header("Content-Length")
                            .and_then(|v| v.parse::<u64>().ok());
                        let mut reader = response.into_reader();
                        match write_reader_with_progress_and_speed_gate(
                            &mut reader,
                            &archive_path,
                            total_len,
                            candidate.name,
                            speed_gate_enabled,
                        ) {
                            Ok(()) => {
                                downloaded = true;
                                println!(
                                    "cargo:warning=[sherpa-onnx] Download completed successfully via {}",
                                    candidate.name
                                );
                                if let Some(global_dir) = global_cache_dir() {
                                    let _ = fs::create_dir_all(&global_dir);
                                    let _ = copy_file_atomically(
                                        &archive_path,
                                        &global_dir.join(&archive_name),
                                    );
                                }
                                break;
                            }
                            Err(e) => {
                                eprintln!(
                                    "Warning: failed writing stream from {}: {e}",
                                    candidate.url
                                );
                                println!(
                                    "cargo:warning=[sherpa-onnx] Mirror {} failed ({e}); falling back to next candidate",
                                    candidate.name
                                );
                                let _ = fs::remove_file(temp_path_for(&archive_path));
                                let _ = fs::remove_file(&archive_path);
                                last_err = Some(format!(
                                    "Failed to write stream from {}: {e}",
                                    candidate.url
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: failed to download from {}: {e}",
                            candidate.url
                        );
                        println!(
                            "cargo:warning=[sherpa-onnx] Connect to {} failed ({e}); trying next candidate",
                            candidate.name
                        );
                        last_err = Some(format!(
                            "Failed to download sherpa-onnx archive from {}: {e}",
                            candidate.url
                        ));
                    }
                }
            }

            if !downloaded {
                return Err(last_err
                    .unwrap_or_else(|| "No download URLs available for sherpa-onnx".to_string())
                    .into());
            }
        }
    }

    if let Some(expected) = env::var("SHERPA_ONNX_ARCHIVE_SHA256")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        if let Err(error) = verify_archive_sha256(&archive_path, &expected) {
            let _ = fs::remove_file(&archive_path);
            return Err(error);
        }
    }

    let _ = fs::remove_file(&completion_marker);
    if extracted_dir.exists() {
        fs::remove_dir_all(&extracted_dir)?;
    }

    let unpack_result: Result<(), DynError> = (|| {
        let tar_file = File::open(&archive_path)?;
        let decoder = BzDecoder::new(tar_file);
        let mut archive = Archive::new(decoder);
        archive.unpack(&cache_root)?;
        Ok(())
    })();
    if let Err(err) = unpack_result {
        let _ = fs::remove_file(&archive_path);
        let _ = fs::remove_dir_all(&extracted_dir);
        let _ = fs::remove_file(&completion_marker);
        return Err(format!(
            "Failed to unpack cached archive {}: {err}",
            archive_path.display()
        )
        .into());
    }

    if !lib_dir.is_dir() {
        return Err(format!(
            "Downloaded archive did not contain a lib directory: {}",
            lib_dir.display()
        )
        .into());
    }

    File::create(&completion_marker)?;
    eprintln!("Downloaded sherpa-onnx libs to {}", extracted_dir.display());

    Ok(lib_dir)
}

fn archive_name(
    link_mode: LinkMode,
    target_os: &str,
    target_arch: &str,
) -> Result<String, DynError> {
    let version = env!("CARGO_PKG_VERSION");
    let crt = if env::var("CARGO_CFG_TARGET_FEATURE")
        .map(|features| features.split(',').any(|f| f == "crt-static"))
        .unwrap_or(false)
    {
        "MT"
    } else {
        "MD"
    };

    let name = match (link_mode, target_os, target_arch) {
        (LinkMode::Static, "linux", "x86_64") => {
            format!("sherpa-onnx-v{version}-linux-x64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "linux", "aarch64") => {
            format!("sherpa-onnx-v{version}-linux-aarch64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "macos", "x86_64") => {
            format!("sherpa-onnx-v{version}-osx-x64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "macos", "aarch64") => {
            format!("sherpa-onnx-v{version}-osx-arm64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "windows", "x86_64") => {
            format!("sherpa-onnx-v{version}-win-x64-static-{crt}-Release-lib.tar.bz2")
        }
        (LinkMode::Static, "windows", "aarch64") => {
            format!("sherpa-onnx-v{version}-win-arm64-static-{crt}-Release-lib.tar.bz2")
        }
        (LinkMode::Static, "windows", "x86") => {
            format!("sherpa-onnx-v{version}-win-x86-static-{crt}-Release-lib.tar.bz2")
        }
        (LinkMode::Shared, "linux", "x86_64") => {
            format!("sherpa-onnx-v{version}-linux-x64-shared-lib.tar.bz2")
        }
        (LinkMode::Shared, "linux", "aarch64") => {
            format!("sherpa-onnx-v{version}-linux-aarch64-shared-cpu-lib.tar.bz2")
        }
        (LinkMode::Shared, "macos", "x86_64") => {
            format!("sherpa-onnx-v{version}-osx-x64-shared-lib.tar.bz2")
        }
        (LinkMode::Shared, "macos", "aarch64") => {
            format!("sherpa-onnx-v{version}-osx-arm64-shared-lib.tar.bz2")
        }
        (LinkMode::Shared, "windows", "x86_64") => {
            format!("sherpa-onnx-v{version}-win-x64-shared-{crt}-Release-lib.tar.bz2")
        }
        (LinkMode::Shared, "windows", "aarch64") => {
            format!("sherpa-onnx-v{version}-win-arm64-shared-{crt}-Release-lib.tar.bz2")
        }
        (LinkMode::Shared, "windows", "x86") => {
            format!("sherpa-onnx-v{version}-win-x86-shared-{crt}-Release-lib.tar.bz2")
        }
        _ => {
            return Err(format!(
                "Unsupported sherpa-onnx link={link_mode:?} os={target_os} arch={target_arch}"
            )
            .into());
        }
    };

    Ok(name)
}

fn emit_shared_link_directives() {
    println!("cargo:rustc-link-lib=dylib=sherpa-onnx-c-api");
    println!("cargo:rustc-link-lib=dylib=onnxruntime");
}

fn emit_static_link_directives(target_os: &str) {
    for lib in SHERPA_ONNX_STATIC_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }

    match target_os {
        "linux" => {
            println!("cargo:rustc-link-lib=dylib=stdc++");
            println!("cargo:rustc-link-lib=dylib=m");
            println!("cargo:rustc-link-lib=dylib=pthread");
            println!("cargo:rustc-link-lib=dylib=dl");
        }
        "macos" => {
            println!("cargo:rustc-link-lib=dylib=c++");
            println!("cargo:rustc-link-lib=framework=Foundation");
        }
        _ => {}
    }
}

fn target_dir_from_out_dir(out_dir: &Path) -> Result<PathBuf, DynError> {
    if let Ok(explicit_target_dir) = env::var("CARGO_TARGET_DIR") {
        return Ok(PathBuf::from(explicit_target_dir));
    }

    if let Some(target_dir) = out_dir
        .ancestors()
        .find(|path| {
            let name = path.file_name();
            name == Some(OsStr::new("target")) || name == Some(OsStr::new("build.noindex"))
        })
    {
        return Ok(target_dir.to_path_buf());
    }

    Ok(out_dir.to_path_buf())
}

fn emit_relative_rpath(target_os: &str) {
    match target_os {
        "linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        "macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}

fn profile_output_dirs() -> Result<[PathBuf; 2], DynError> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let profile = env::var("PROFILE")?;
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new(&profile)))
        .ok_or_else(|| {
            format!(
                "Could not locate Cargo profile directory from {}",
                out_dir.display()
            )
        })?
        .to_path_buf();

    Ok([profile_dir.clone(), profile_dir.join("examples")])
}

fn copy_unix_runtime_libs(lib_dir: &Path, target_os: &str) -> Result<(), DynError> {
    let runtime_libs: Vec<PathBuf> = fs::read_dir(lib_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .map(|name| match target_os {
                    "linux" => name.contains(".so"),
                    "macos" => name.ends_with(".dylib"),
                    _ => false,
                })
                .unwrap_or(false)
        })
        .collect();

    if runtime_libs.is_empty() {
        return Err(format!("No shared runtime libraries found in {}", lib_dir.display()).into());
    }

    let mut copy_plan = Vec::<(PathBuf, OsString)>::new();
    let mut planned_names = HashSet::<OsString>::new();

    for lib in runtime_libs {
        if !lib.exists() {
            continue;
        }

        let lib_name = lib
            .file_name()
            .ok_or_else(|| format!("Invalid runtime library path: {}", lib.display()))?
            .to_os_string();

        let source = fs::canonicalize(&lib).unwrap_or(lib.clone());
        if planned_names.insert(lib_name.clone()) {
            copy_plan.push((source.clone(), lib_name));
        }

        if let Some(source_name) = source.file_name() {
            let source_name = source_name.to_os_string();
            if planned_names.insert(source_name.clone()) {
                copy_plan.push((source.clone(), source_name));
            }
        }
    }

    if copy_plan.is_empty() {
        return Err(format!(
            "No usable shared runtime libraries found in {}",
            lib_dir.display()
        )
        .into());
    }

    for dest_dir in profile_output_dirs()? {
        fs::create_dir_all(&dest_dir)?;
        for (source, dest_name) in &copy_plan {
            let dest = dest_dir.join(dest_name);
            fs::copy(source, &dest)?;
        }
    }

    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut temp_name = path
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_else(|| OsString::from("tmp"));
    temp_name.push(format!(".{}.part", std::process::id()));
    path.with_file_name(temp_name)
}

fn verify_archive_sha256(path: &Path, expected: &str) -> Result<(), DynError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "Downloaded archive SHA256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn copy_file_atomically(src: &Path, dst: &Path) -> Result<(), DynError> {
    let temp_path = temp_path_for(dst);
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }
    fs::copy(src, &temp_path)?;
    fs::rename(&temp_path, dst)?;
    Ok(())
}

fn write_reader_with_progress_and_speed_gate(
    reader: &mut dyn io::Read,
    dst: &Path,
    total_len: Option<u64>,
    source_name: &str,
    speed_gate_enabled: bool,
) -> Result<(), DynError> {
    let temp_path = temp_path_for(dst);
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    let result = (|| -> Result<(), DynError> {
        let mut file = File::create(&temp_path)?;
        let mut buffer = [0u8; 64 * 1024];
        let mut downloaded: u64 = 0;
        let start_time = Instant::now();
        let mut last_progress_time = Instant::now();
        let mut last_pct = 0;

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])?;
            downloaded += bytes_read as u64;

            let elapsed = start_time.elapsed().as_secs_f64();

            // Speed gate: after 5 seconds, if downloading a large archive (>5MB) and
            // average transfer rate is below 300 KB/s, abort early to switch to a faster candidate.
            if speed_gate_enabled && elapsed >= 5.0 && elapsed <= 20.0 {
                let is_large_file = total_len.map(|t| t > 5 * 1024 * 1024).unwrap_or(true);
                if is_large_file {
                    let speed_kb_s = (downloaded as f64 / 1024.0) / elapsed;
                    if speed_kb_s < 300.0 {
                        return Err(format!(
                            "Speed gate triggered for {source_name}: rate is only {:.1} KB/s (< 300 KB/s threshold)",
                            speed_kb_s
                        )
                        .into());
                    }
                }
            }

            // Periodic progress reporting via cargo:warning every 20% or every 5 seconds
            let now = Instant::now();
            let should_report = if let Some(total) = total_len {
                if total > 0 {
                    let pct = ((downloaded as f64 / total as f64) * 100.0) as u32;
                    if pct >= last_pct + 20 || pct == 100 || now.duration_since(last_progress_time).as_secs() >= 5 {
                        last_pct = pct;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else if now.duration_since(last_progress_time).as_secs() >= 5 {
                true
            } else {
                false
            };

            if should_report {
                last_progress_time = now;
                let downloaded_mb = downloaded as f64 / (1024.0 * 1024.0);
                if let Some(total) = total_len {
                    let total_mb = total as f64 / (1024.0 * 1024.0);
                    let pct = ((downloaded as f64 / total as f64) * 100.0) as u32;
                    println!(
                        "cargo:warning=[sherpa-onnx] Downloading prebuilts: {pct}% ({downloaded_mb:.1}/{total_mb:.1} MB) via {source_name}"
                    );
                } else {
                    println!(
                        "cargo:warning=[sherpa-onnx] Downloading prebuilts: {downloaded_mb:.1} MB via {source_name}"
                    );
                }
            }
        }

        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    fs::rename(&temp_path, dst)?;
    Ok(())
}

fn copy_windows_runtime_dlls(lib_dir: &Path) -> Result<(), DynError> {
    let dlls: Vec<PathBuf> = fs::read_dir(lib_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension() == Some(OsStr::new("dll")))
        .collect();

    if dlls.is_empty() {
        println!(
            "cargo:warning=No runtime DLLs found in {}",
            lib_dir.display()
        );
        return Ok(());
    }

    let [profile_dir, examples_dir] = profile_output_dirs()?;
    for dest_dir in [profile_dir.clone(), examples_dir] {
        fs::create_dir_all(&dest_dir)?;
        for dll in &dlls {
            let dest = dest_dir.join(
                dll.file_name()
                    .ok_or_else(|| format!("Invalid DLL path: {}", dll.display()))?,
            );
            fs::copy(dll, &dest)?;
        }
    }

    println!(
        "cargo:warning=Copied Windows runtime DLLs to {} and {}/examples",
        profile_dir.display(),
        profile_dir.display()
    );

    Ok(())
}
