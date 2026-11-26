//! MSVC linker wrapper for `dx serve --hot-patch` on Windows.
//!
//! `dioxus-cli 0.7.10` intercepts `rustc`'s link invocation (`-Clinker=dx.exe`),
//! serializes linker arguments one-per-line into a response file without
//! quoting paths that contain spaces, and invokes the real linker with
//! `@<file>`. Under `gpui-pre`, `embed-resource` / `vswhom-sys` emits
//! `-L native=C:\Program Files\Microsoft Visual Studio\...\atlmfc\lib\x64`,
//! which MSVC `link.exe` splits on whitespace and fails with
//! `LNK1104: cannot open file 'Files\Microsoft.obj'`.
//!
//! This wrapper serves three roles:
//! 1. In hot-patch mode (`DX_WRAPPER_NO_LINK=1`), `dx 0.7.10` accidentally
//!    forwards `custom_linker` into `DX_LINK_CUSTOM_LINKER` during `cargo rustc`
//!    (fat) and `rustc` (thin) builds instead of using its own `NoLink` path.
//!    When invoked directly by `dx.exe` (`DX_LINK=1`) with `@<DX_LINK_ARGS_FILE>`,
//!    `dx` has already captured the linker args and will perform the real fat or
//!    patch DLL link itself via `@windows_command_file`. Writing a minimal empty
//!    COFF stub to `/OUT:<file>` matches `dx`'s intended `NoLink` behavior,
//!    avoids a wasted full link on every build, and avoids MSVC `LNK1104` on
//!    `out/flowy_app.exe` during thin patch replay.
//! 2. During Windows thin patch DLL linking (`/DLL` + `stub.o`), `dx 0.7.10`
//!    emits `movabs rax, <addr>; jmp rax` stubs for every undefined function in
//!    `stub.o` — including `__chkstk`. On Windows x64 ABI, `__chkstk` takes the
//!    stack allocation size in `RAX` and must preserve it; clobbering `RAX`
//!    causes an immediate stack overflow in the first patched function with a
//!    frame > 4 KB. Demoting `__chkstk` (and friends) from external to static
//!    in a temporary copy of `stub.o` lets the linker resolve `__chkstk` from
//!    `msvcrt.lib`'s native `chkstk.obj`.
//! 3. For all real linker invocations (build scripts, proc macros, fat binary
//!    linking, and thin patch DLL linking), it rewrites any `@response-file`
//!    with unquoted whitespace paths into a temporary UTF-16LE response file,
//!    leaves the original file untouched, and forwards to `DX_REAL_LINKER`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if should_emit_nolink_stub(&args) {
        if let Some(rsp_path) = single_response_file_arg(&args) {
            match emit_nolink_stub(Path::new(rsp_path)) {
                Ok(()) => exit(0),
                Err(err) => {
                    eprintln!("dx-link-wrapper: NoLink stub failed: {err}");
                    exit(1);
                }
            }
        }
    }

    let real_linker = match env::var("DX_REAL_LINKER") {
        Ok(path) if !path.trim().is_empty() => path,
        _ => {
            eprintln!(
                "dx-link-wrapper: DX_REAL_LINKER is not set; launch via scripts/dev-hotpatch.ps1"
            );
            exit(1);
        }
    };

    let mut forwarded = Vec::with_capacity(args.len());
    let mut temp_files: Vec<PathBuf> = Vec::new();

    for (index, arg) in args.iter().enumerate() {
        if let Some(raw_rsp) = arg.strip_prefix('@') {
            let rsp_path = strip_outer_quotes(raw_rsp);
            match rewrite_response_file(
                Path::new(rsp_path),
                index,
                &mut temp_files,
            ) {
                Ok(Some(quoted_rsp)) => {
                    forwarded.push(format!("@{}", quoted_rsp.display()));
                    temp_files.push(quoted_rsp);
                    continue;
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!(
                        "dx-link-wrapper: failed to rewrite @{rsp_path}: {err}"
                    );
                }
            }
        }
        forwarded.push(arg.clone());
    }

    if needs_no_incremental(&real_linker, &forwarded) {
        forwarded.push("/INCREMENTAL:NO".to_string());
    }

    let status = Command::new(&real_linker).args(&forwarded).status();
    for tmp in temp_files {
        let _ = fs::remove_file(tmp);
    }

    match status {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("dx-link-wrapper: failed to spawn {real_linker}: {err}");
            exit(1);
        }
    }
}

fn strip_outer_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

/// MSVC `link.exe` turns `/INCREMENTAL` on implicitly whenever `/DEBUG` is
/// passed, exporting `@ILT+...(main)` thunks instead of the real `main` body
/// RVA recorded in the PDB. That skews `subsecond`'s ASLR base calculation and
/// crashes on patch apply. Force `/INCREMENTAL:NO` when falling back to MSVC
/// `link.exe` in hot-patch mode (`lld-link.exe` never emits ILT thunks).
fn needs_no_incremental(real_linker: &str, args: &[String]) -> bool {
    if env::var("DX_WRAPPER_NO_LINK").as_deref() != Ok("1") {
        return false;
    }
    if !is_msvc_link_exe(real_linker) {
        return false;
    }
    !args
        .iter()
        .any(|arg| arg.to_ascii_uppercase().starts_with("/INCREMENTAL"))
}

fn is_msvc_link_exe(linker: &str) -> bool {
    Path::new(linker)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("link.exe"))
}

fn single_response_file_arg(args: &[String]) -> Option<&str> {
    if args.len() != 1 {
        return None;
    }
    args[0].strip_prefix('@').map(strip_outer_quotes)
}

fn should_emit_nolink_stub(args: &[String]) -> bool {
    if env::var("DX_WRAPPER_NO_LINK").as_deref() != Ok("1") {
        return false;
    }
    if env::var("DX_LINK").as_deref() != Ok("1") {
        return false;
    }
    let Ok(link_args_file) = env::var("DX_LINK_ARGS_FILE") else {
        return false;
    };
    let Some(rsp_path) = single_response_file_arg(args) else {
        return false;
    };
    paths_match(Path::new(rsp_path), Path::new(&link_args_file))
}

fn paths_match(left: &Path, right: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (fs::canonicalize(left), fs::canonicalize(right)) {
        return a == b;
    }
    let normalize = |p: &Path| {
        p.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    normalize(left) == normalize(right)
}

fn emit_nolink_stub(link_args_file: &Path) -> std::io::Result<()> {
    let bytes = fs::read(link_args_file)?;
    let text = decode_rsp(&bytes);
    let out_file = extract_out_path(&text).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("missing /OUT: argument in {}", link_args_file.display()),
        )
    })?;
    if let Some(parent) = out_file.parent() {
        fs::create_dir_all(parent)?;
    }
    let triple = env::var("DX_LINK_TRIPLE").unwrap_or_default();
    fs::write(&out_file, empty_coff_stub(&triple))
}

fn extract_out_path(text: &str) -> Option<PathBuf> {
    for raw_line in text.lines() {
        let line = strip_outer_quotes(raw_line.trim());
        if line.len() > 5 && line[..5].eq_ignore_ascii_case("/out:") {
            let value = strip_outer_quotes(&line[5..]);
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

/// Minimal 20-byte COFF `IMAGE_FILE_HEADER` (0 sections, 0 symbols), matching
/// `dioxus-cli`'s `LinkAction` `NoLink` stub so `rustc` emits `emit="link"` and
/// `object::File::parse` reads an empty valid COFF file.
fn empty_coff_stub(triple: &str) -> [u8; 20] {
    let mut header = [0u8; 20];
    let machine: u16 = if triple.to_ascii_lowercase().starts_with("aarch64") {
        0xAA64
    } else {
        0x8664
    };
    let [lo, hi] = machine.to_le_bytes();
    header[0] = lo;
    header[1] = hi;
    header
}

fn rewrite_response_file(
    src: &Path,
    index: usize,
    temp_files: &mut Vec<PathBuf>,
) -> std::io::Result<Option<PathBuf>> {
    let bytes = fs::read(src)?;
    let original = decode_rsp(&bytes);
    let mut current = original;
    let mut changed = false;

    if let Some(quoted) = quote_response_contents(&current) {
        current = quoted;
        changed = true;
    }

    if env::var("DX_WRAPPER_NO_LINK").as_deref() == Ok("1") {
        if let Some(patched) =
            sanitize_patch_stub_in_rsp(&current, index, temp_files)?
        {
            current = patched;
            changed = true;
        }
    }

    if !changed {
        return Ok(None);
    }

    let tmp_path = unique_temp_path("dx-link-quoted", index, "rsp");
    fs::write(&tmp_path, encode_utf16le_with_bom(&current))?;
    Ok(Some(tmp_path))
}

fn unique_temp_path(prefix: &str, index: usize, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}-{index}.{ext}",
        std::process::id()
    ))
}

fn sanitize_patch_stub_in_rsp(
    text: &str,
    index: usize,
    temp_files: &mut Vec<PathBuf>,
) -> std::io::Result<Option<String>> {
    let tokens = split_rsp_tokens(text);
    let is_dll = tokens.iter().any(|tok| tok.eq_ignore_ascii_case("/DLL"));
    if !is_dll {
        return Ok(None);
    }

    let Some(stub_token) = tokens.iter().find(|tok| {
        Path::new(tok)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("stub.o"))
    }) else {
        return Ok(None);
    };

    let stub_path = Path::new(stub_token);
    if !stub_path.is_file() {
        return Ok(None);
    }

    let mut stub_bytes = fs::read(stub_path)?;
    if !demote_chkstk_symbols(&mut stub_bytes) {
        return Ok(None);
    }

    let patched_stub = unique_temp_path("dx-stub-patched", index, "o");
    fs::write(&patched_stub, &stub_bytes)?;
    temp_files.push(patched_stub.clone());

    let patched_str = patched_stub.to_string_lossy();
    let quoted_stub = format!("\"{stub_token}\"");
    let rewritten = if text.contains(&quoted_stub) {
        text.replace(&quoted_stub, &format!("\"{patched_str}\""))
    } else if patched_str.contains(' ') || patched_str.contains('\t') {
        text.replace(stub_token, &format!("\"{patched_str}\""))
    } else {
        text.replace(stub_token, &patched_str)
    };
    Ok((rewritten != text).then_some(rewritten))
}

fn split_rsp_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in text.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Demotes `IMAGE_SYM_CLASS_EXTERNAL` (`2`) symbols for Windows stack-probe
/// helpers (`__chkstk`, `_chkstk`, `___chkstk_ms`, `__alloca_probe`) in an
/// x86/x64 COFF object file to `IMAGE_SYM_CLASS_STATIC` (`3`) so the linker
/// pulls the real ABI-compliant helper from `msvcrt.lib` instead of `dx`'s
/// `RAX`-clobbering trampoline.
fn demote_chkstk_symbols(coff: &mut [u8]) -> bool {
    const COFF_HEADER_LEN: usize = 20;
    const COFF_SYMBOL_LEN: usize = 18;
    const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
    const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
    const IMAGE_SYM_CLASS_EXTERNAL: u8 = 2;
    const IMAGE_SYM_CLASS_STATIC: u8 = 3;

    if coff.len() < COFF_HEADER_LEN {
        return false;
    }
    let machine = u16::from_le_bytes([coff[0], coff[1]]);
    if machine != IMAGE_FILE_MACHINE_AMD64 && machine != IMAGE_FILE_MACHINE_I386
    {
        return false;
    }
    let symtab_off =
        u32::from_le_bytes([coff[8], coff[9], coff[10], coff[11]]) as usize;
    let num_syms =
        u32::from_le_bytes([coff[12], coff[13], coff[14], coff[15]]) as usize;
    if symtab_off == 0 || num_syms == 0 {
        return false;
    }
    let Some(symtab_len) = num_syms.checked_mul(COFF_SYMBOL_LEN) else {
        return false;
    };
    let Some(strtab_off) = symtab_off.checked_add(symtab_len) else {
        return false;
    };
    if strtab_off > coff.len() {
        return false;
    }

    let mut changed = false;
    let mut idx = 0usize;
    while idx < num_syms {
        let entry = symtab_off + idx * COFF_SYMBOL_LEN;
        let storage_class = coff[entry + 16];
        let aux_count = coff[entry + 17] as usize;

        if storage_class == IMAGE_SYM_CLASS_EXTERNAL {
            let is_chkstk = coff_symbol_name(coff, entry, strtab_off)
                .is_some_and(is_stack_probe_symbol);
            if is_chkstk {
                coff[entry + 16] = IMAGE_SYM_CLASS_STATIC;
                changed = true;
            }
        }

        idx = match idx.checked_add(1 + aux_count) {
            Some(next) => next,
            None => break,
        };
    }

    changed
}

fn coff_symbol_name(
    coff: &[u8],
    entry: usize,
    strtab_off: usize,
) -> Option<&str> {
    if coff[entry..entry + 4] == [0, 0, 0, 0] {
        let str_offset = u32::from_le_bytes([
            coff[entry + 4],
            coff[entry + 5],
            coff[entry + 6],
            coff[entry + 7],
        ]) as usize;
        let start = strtab_off.checked_add(str_offset)?;
        if start >= coff.len() {
            return None;
        }
        let rest = &coff[start..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        std::str::from_utf8(&rest[..end]).ok()
    } else {
        let short = &coff[entry..entry + 8];
        let end = short.iter().position(|&b| b == 0).unwrap_or(short.len());
        std::str::from_utf8(&short[..end]).ok()
    }
}

fn is_stack_probe_symbol(name: &str) -> bool {
    matches!(
        name,
        "__chkstk" | "_chkstk" | "___chkstk_ms" | "__alloca_probe"
    )
}

fn quote_response_contents(text: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len() + 128);
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if needs_quoting(line) {
            changed = true;
            out.push('"');
            out.push_str(line);
            if line.ends_with('\\') {
                // Prevent a trailing backslash from escaping the closing quote.
                out.push('\\');
            }
            out.push('"');
        } else {
            out.push_str(line);
        }
        out.push_str("\r\n");
    }
    changed.then_some(out)
}

fn needs_quoting(line: &str) -> bool {
    if !(line.contains(' ') || line.contains('\t')) {
        return false;
    }
    if line.contains('"') {
        return false;
    }
    true
}

fn decode_rsp(bytes: &[u8]) -> String {
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
        return String::from_utf16_lossy(&units);
    }
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_string()
}

fn encode_utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2 + text.len() * 2);
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_unquoted_msvc_libpath_with_spaces() {
        let input = "/NOLOGO\n/LIBPATH:C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207\\atlmfc\\lib\\x64\n/OUT:flowy-app.exe\n";
        let rewritten = quote_response_contents(input).expect("should rewrite");
        assert_eq!(
            rewritten,
            "/NOLOGO\r\n\"/LIBPATH:C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207\\atlmfc\\lib\\x64\"\r\n/OUT:flowy-app.exe\r\n"
        );
    }

    #[test]
    fn skips_rewrite_when_already_quoted_or_no_spaces() {
        let input = "/NOLOGO\r\n\"/LIBPATH:C:\\Program Files\\SDK\"\r\n/LIBPATH:\"C:\\Program Files\\MSVC\"\r\nfoo.obj\r\n";
        assert!(quote_response_contents(input).is_none());
    }

    #[test]
    fn escapes_trailing_backslash_before_closing_quote() {
        let input = "/LIBPATH:C:\\Program Files\\SDK\\\n";
        let rewritten = quote_response_contents(input).expect("should rewrite");
        assert_eq!(rewritten, "\"/LIBPATH:C:\\Program Files\\SDK\\\\\"\r\n");
    }

    #[test]
    fn roundtrips_utf16le_with_bom_and_unicode_paths() {
        let original = "/LIBPATH:C:\\Users\\测试 用户\\lib\r\n";
        let encoded = encode_utf16le_with_bom(original);
        assert_eq!(&encoded[..2], &[0xFF, 0xFE]);
        assert_eq!(decode_rsp(&encoded), original);
    }

    #[test]
    fn rewrite_preserves_original_response_file() {
        let dir = env::temp_dir()
            .join(format!("dx-link-wrapper-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("link_args.json");
        let raw = "/NOLOGO\n/LIBPATH:C:\\Program Files\\MSVC\\lib\n";
        fs::write(&src, raw).unwrap();

        let mut temps = Vec::new();
        let tmp = rewrite_response_file(&src, 0, &mut temps)
            .unwrap()
            .expect("should produce temp file");
        assert_eq!(fs::read_to_string(&src).unwrap(), raw);
        let decoded = decode_rsp(&fs::read(&tmp).unwrap());
        assert!(decoded.contains("\"/LIBPATH:C:\\Program Files\\MSVC\\lib\""));

        let _ = fs::remove_file(tmp);
        for t in temps {
            let _ = fs::remove_file(t);
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn extracts_out_path_and_writes_valid_coff_stub() {
        let dir = env::temp_dir()
            .join(format!("dx-link-nolink-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let out_exe = dir.join("nested").join("flowy_app.exe");
        let link_args = dir.join("link_args.json");
        fs::write(
            &link_args,
            format!("/NOLOGO\n/OUT:{}\n/DEBUG\n", out_exe.display()),
        )
        .unwrap();

        emit_nolink_stub(&link_args).unwrap();
        let stub = fs::read(&out_exe).unwrap();
        assert_eq!(stub.len(), 20);
        assert_eq!(&stub[..2], &[0x64, 0x86]);

        let arm_stub = empty_coff_stub("aarch64-pc-windows-msvc");
        assert_eq!(&arm_stub[..2], &[0x64, 0xAA]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn paths_match_ignores_slash_and_case_differences() {
        assert!(paths_match(
            Path::new(r"C:\Temp\Sub/link_args.json"),
            Path::new(r"c:\temp\sub\LINK_ARGS.JSON")
        ));
        assert!(!paths_match(
            Path::new(r"C:\Temp\windows_command_file.txt"),
            Path::new(r"C:\Temp\link_args.json")
        ));
    }

    #[test]
    fn detects_msvc_link_exe_vs_lld_link() {
        assert!(is_msvc_link_exe(
            r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe"
        ));
        assert!(!is_msvc_link_exe(
            r"C:\Users\dev\.rustup\toolchains\1.100\lib\rustlib\x86_64-pc-windows-msvc\bin\gcc-ld\lld-link.exe"
        ));
    }

    fn build_test_coff(symbols: &[(&str, u8)]) -> Vec<u8> {
        let symtab_off = 20u32;
        let num_syms = symbols.len() as u32;
        let mut out = vec![0u8; 20];
        out[0..2].copy_from_slice(&0x8664u16.to_le_bytes());
        out[8..12].copy_from_slice(&symtab_off.to_le_bytes());
        out[12..16].copy_from_slice(&num_syms.to_le_bytes());

        let mut strtab = vec![0u8; 4]; // 4-byte total size placeholder
        for (name, storage_class) in symbols {
            let mut rec = [0u8; 18];
            if name.len() <= 8 {
                rec[..name.len()].copy_from_slice(name.as_bytes());
            } else {
                let offset = strtab.len() as u32;
                rec[4..8].copy_from_slice(&offset.to_le_bytes());
                strtab.extend_from_slice(name.as_bytes());
                strtab.push(0);
            }
            rec[16] = *storage_class;
            out.extend_from_slice(&rec);
        }
        let strtab_len = strtab.len() as u32;
        strtab[0..4].copy_from_slice(&strtab_len.to_le_bytes());
        out.extend_from_slice(&strtab);
        out
    }

    #[test]
    fn demotes_chkstk_symbols_and_preserves_other_externals() {
        let mut coff = build_test_coff(&[
            ("memcpy", 2),
            ("__chkstk", 2),
            ("___chkstk_ms", 2),
            ("memset", 2),
        ]);
        assert!(demote_chkstk_symbols(&mut coff));
        // memcpy stays EXTERNAL (2)
        assert_eq!(coff[20 + 16], 2);
        // __chkstk demoted to STATIC (3)
        assert_eq!(coff[20 + 18 + 16], 3);
        // ___chkstk_ms (long string-table name) demoted to STATIC (3)
        assert_eq!(coff[20 + 36 + 16], 3);
        // memset stays EXTERNAL (2)
        assert_eq!(coff[20 + 54 + 16], 2);
        // Second pass is a no-op
        assert!(!demote_chkstk_symbols(&mut coff));
    }

    #[test]
    fn sanitizes_stub_o_in_dll_patch_response_file() {
        let dir = env::temp_dir()
            .join(format!("dx-link-stub-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let stub = dir.join("stub.o");
        let original_coff = build_test_coff(&[("__chkstk", 2), ("memcpy", 2)]);
        fs::write(&stub, &original_coff).unwrap();

        let rsp = format!(
            "\"foo.o\" \"{}\" \"/DLL\" \"/OUT:patch.dll\"",
            stub.display()
        );
        let mut temps = Vec::new();
        let rewritten = sanitize_patch_stub_in_rsp(&rsp, 0, &mut temps)
            .unwrap()
            .expect("should rewrite stub.o in DLL patch rsp");
        assert_eq!(temps.len(), 1);
        assert!(rewritten.contains(&temps[0].display().to_string()));
        assert!(!rewritten.contains(&stub.display().to_string()));
        // Original stub.o stays untouched; temp copy has __chkstk demoted.
        assert_eq!(fs::read(&stub).unwrap()[20 + 16], 2);
        assert_eq!(fs::read(&temps[0]).unwrap()[20 + 16], 3);

        for t in temps {
            let _ = fs::remove_file(t);
        }
        let _ = fs::remove_dir_all(dir);
    }
}
