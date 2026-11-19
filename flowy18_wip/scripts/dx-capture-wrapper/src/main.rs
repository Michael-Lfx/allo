//! Outer `RUSTC_WRAPPER` for `dx serve` on Windows.
//!
//! Cargo puts long rustc invocations in a `@cargo-argfile`, with `--crate-name`
//! inside that file. dioxus-cli 0.7.10 only scans direct argv, so the tip binary
//! never gets a `{crate}.bin.json` and fat linking fails with "Missing rustc capture".
//! This wrapper expands those argfiles to write the capture dx expects and forwards
//! the original command. It keeps the library search paths in a persistent response
//! file so dx can replay the capture within the Windows command-line limit.

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{exit, Command};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("dx-capture-wrapper: missing rustc command");
        exit(1);
    }

    if let Ok(args_dir) = env::var("DX_RUSTC") {
        if args.iter().any(|arg| arg.starts_with('@')) {
            if let Some(rustc_args) = expand_rustc_args(&args) {
                if let Err(err) =
                    write_capture(Path::new(&args_dir), &rustc_args)
                {
                    eprintln!("dx-capture-wrapper: {err}");
                }
            }
        }
    }

    // Drop a leading sccache. Cargo's forced RUSTC_WRAPPER puts it in front of
    // rustc, and the rustup proxy then serves a cached object with no hotpatch.
    let forward = skip_sccache(&args);
    let mut cmd = Command::new(&forward[0]);
    cmd.args(&forward[1..]);
    cmd.env_remove("RUSTC_WRAPPER");
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    match cmd.status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("dx-capture-wrapper: failed to spawn {}: {err}", args[0]);
            exit(1);
        }
    }
}

fn expand_rustc_args(args: &[String]) -> Option<Vec<String>> {
    let rustc_idx = args.iter().position(|arg| is_rustc(arg))?;
    let mut expanded = Vec::with_capacity(args.len().saturating_sub(rustc_idx));
    expanded.push(args[rustc_idx].clone());
    for arg in &args[rustc_idx + 1..] {
        if let Some(path) = arg.strip_prefix('@') {
            expanded.extend(read_argfile(path)?);
        } else {
            expanded.push(arg.clone());
        }
    }
    Some(expanded)
}

fn skip_sccache(args: &[String]) -> &[String] {
    let mut start = 0;
    while start < args.len() && is_sccache(&args[start]) {
        start += 1;
    }
    if start >= args.len() {
        args
    } else {
        &args[start..]
    }
}

fn is_sccache(arg: &str) -> bool {
    Path::new(arg)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name == "sccache" || name == "sccache.exe"
        })
}

fn is_rustc(arg: &str) -> bool {
    Path::new(arg)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("rustc")
                || name.eq_ignore_ascii_case("rustc.exe")
        })
}

fn read_argfile(path: &str) -> Option<Vec<String>> {
    let bytes = fs::read(path).ok()?;
    let text = decode_argfile(&bytes)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Some(
        text.lines()
            .map(|line| unquote(line.trim_end_matches('\r')))
            .filter(|line| !line.is_empty())
            .collect(),
    )
}

fn decode_argfile(bytes: &[u8]) -> Option<String> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|chunk| {
                u16::from_le_bytes([
                    chunk[0],
                    chunk.get(1).copied().unwrap_or(0),
                ])
            })
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    String::from_utf8(bytes.to_vec()).ok()
}

fn unquote(line: &str) -> String {
    let Some(inner) = line
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return line.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some(next) => out.push(next),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_capture(args_dir: &Path, rustc_args: &[String]) -> Result<(), String> {
    let crate_name = flag_value(rustc_args, "--crate-name");
    let Some(crate_name) = crate_name else {
        return Ok(());
    };
    if crate_name == "___" {
        return Ok(());
    }

    let suffix = match flag_value(rustc_args, "--crate-type").as_deref() {
        Some("lib" | "rlib") => "lib",
        _ => "bin",
    };

    fs::create_dir_all(args_dir)
        .map_err(|err| format!("create {}: {err}", args_dir.display()))?;
    let path = args_dir.join(format!("{crate_name}.{suffix}.json"));
    let search_paths =
        args_dir.join(format!("{crate_name}.{suffix}.search-paths.rsp"));
    let replay_args = pack_search_paths(rustc_args, &search_paths)
        .map_err(|err| format!("write {}: {err}", search_paths.display()))?;
    let mut file = fs::File::create(&path)
        .map_err(|err| format!("create {}: {err}", path.display()))?;
    write!(file, "{{\"args\":[").map_err(|err| err.to_string())?;
    for (index, arg) in replay_args.iter().enumerate() {
        if index > 0 {
            write!(file, ",").map_err(|err| err.to_string())?;
        }
        write!(file, "\"{}\"", json_escape(arg))
            .map_err(|err| err.to_string())?;
    }
    write!(file, "],\"envs\":[").map_err(|err| err.to_string())?;
    for (index, (key, value)) in env::vars().enumerate() {
        if index > 0 {
            write!(file, ",").map_err(|err| err.to_string())?;
        }
        write!(
            file,
            "[\"{}\",\"{}\"]",
            json_escape(&key),
            json_escape(&value)
        )
        .map_err(|err| err.to_string())?;
    }
    writeln!(file, "]}}").map_err(|err| err.to_string())?;
    Ok(())
}

fn pack_search_paths(
    args: &[String],
    path: &Path,
) -> std::io::Result<Vec<String>> {
    // dx inspects flags such as --out-dir, -Cextra-filename and -Clinker.
    // Leave those visible while moving the bulky -L pairs out of argv.
    let mut replay = Vec::new();
    let mut paths = String::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-L" {
            if let Some(value) = iter.next() {
                if paths.is_empty() {
                    replay.push(format!("@{}", path.display()));
                }
                paths.push_str("-L\n");
                paths.push_str(value);
                paths.push('\n');
                continue;
            }
        }
        replay.push(arg.clone());
    }
    if !paths.is_empty() {
        fs::write(path, paths)?;
    }
    Ok(replay)
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let inline = args.iter().find_map(|arg| {
        arg.strip_prefix(flag)
            .and_then(|rest| rest.strip_prefix('='))
            .map(str::to_string)
    });
    if inline.is_some() {
        return inline;
    }
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                out.push_str(&format!("\\u{:04x}", ch as u32))
            }
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_sccache_in_front_of_rustc() {
        let args = vec![
            r"C:\sccache.exe".to_string(),
            r"C:\rustc.exe".to_string(),
            "--crate-name".to_string(),
            "flowy_ui".to_string(),
        ];
        assert_eq!(
            skip_sccache(&args),
            &[
                r"C:\rustc.exe".to_string(),
                "--crate-name".to_string(),
                "flowy_ui".to_string(),
            ]
        );
    }

    #[test]
    fn expands_cargo_argfile_after_rustc() {
        let dir = std::env::temp_dir()
            .join(format!("dx-capture-wrapper-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let argfile = dir.join("cargo-argfile.txt");
        fs::write(&argfile, "--crate-name\nflowy_app\n--crate-type\nbin\n")
            .unwrap();
        let args = vec![
            r"C:\dx.exe".to_string(),
            r"C:\rustc.exe".to_string(),
            format!("@{}", argfile.display()),
        ];
        let expanded = expand_rustc_args(&args).unwrap();
        assert_eq!(
            expanded,
            vec![
                r"C:\rustc.exe",
                "--crate-name",
                "flowy_app",
                "--crate-type",
                "bin",
            ]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn capture_name_matches_dx() {
        let dir = std::env::temp_dir()
            .join(format!("dx-capture-out-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let args = vec![
            r"C:\rustc.exe".to_string(),
            "--crate-name".to_string(),
            "flowy_app".to_string(),
            "--crate-type".to_string(),
            "bin".to_string(),
        ];
        write_capture(&dir, &args).unwrap();
        let written =
            fs::read_to_string(dir.join("flowy_app.bin.json")).unwrap();
        assert!(written.contains(r#""--crate-name","flowy_app""#));
        assert!(written.contains(r#""envs":["#));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn capture_keeps_replay_metadata_visible_and_packs_search_paths() {
        let dir = std::env::temp_dir()
            .join(format!("dx-capture-packed-{}", std::process::id()));
        let args = [
            r"C:\rustc.exe",
            "--crate-name",
            "flowy_ui",
            "--crate-type",
            "lib",
            "--out-dir",
            r"C:\out",
            "-C",
            "extra-filename=-abc",
            "-L",
            r"native=C:\path with spaces\one",
            "-L",
            r"dependency=C:\two",
            "-Clinker=link.exe",
        ]
        .map(str::to_owned);
        write_capture(&dir, &args).unwrap();
        let capture =
            fs::read_to_string(dir.join("flowy_ui.lib.json")).unwrap();
        let response_path = dir.join("flowy_ui.lib.search-paths.rsp");
        assert!(capture
            .contains(&json_escape(&format!("@{}", response_path.display()))));
        assert!(capture
            .contains(r#""--out-dir","C:\\out","-C","extra-filename=-abc""#));
        assert!(capture.contains(r#""-Clinker=link.exe""#));
        assert!(!capture.contains("path with spaces"));
        assert_eq!(
            read_argfile(response_path.to_str().unwrap()).unwrap(),
            &args[9..13]
        );
        let _ = fs::remove_dir_all(dir);
    }
}
