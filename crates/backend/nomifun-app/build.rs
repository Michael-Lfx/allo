use std::{path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let ts = std::env::var("SOURCE_DATE_EPOCH").unwrap_or_else(|_| {
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock must be after the Unix epoch")
                .as_secs()
                .to_string()
        } else {
            "0".to_owned()
        }
    });
    println!("cargo:rustc-env=BUILD_TIME={ts}");
    println!("cargo:rerun-if-changed=build.rs");

    emit_git_identity();
}

fn emit_git_identity() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let manifest_dir = Path::new(&manifest_dir);

    let sha = git(manifest_dir, &["rev-parse", "--short=12", "HEAD"]);
    let dirty = sha.as_ref().map(|_| {
        git(
            manifest_dir,
            &["status", "--porcelain", "--untracked-files=no"],
        )
        .is_some_and(|status| !status.is_empty())
    });

    println!(
        "cargo:rustc-env=FLOWY_GIT_SHA={}",
        sha.as_deref().unwrap_or("unknown")
    );
    println!(
        "cargo:rustc-env=FLOWY_GIT_DIRTY={}",
        dirty.map_or("unknown", |dirty| if dirty { "true" } else { "false" })
    );
    println!(
        "cargo:rustc-env=FLOWY_BUILD_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned())
    );

    // Rerun whenever HEAD moves or tracked sources change, so the stamped
    // identity does not outlive the checkout it was built from.
    for path in ["HEAD", "index"] {
        if let Some(git_path) = git(manifest_dir, &["rev-parse", "--git-path", path]) {
            println!(
                "cargo:rerun-if-changed={}",
                manifest_dir.join(git_path).display()
            );
        }
    }
    if let Some(head_ref) = git(manifest_dir, &["symbolic-ref", "-q", "HEAD"])
        && let Some(git_path) = git(manifest_dir, &["rev-parse", "--git-path", &head_ref])
    {
        println!(
            "cargo:rerun-if-changed={}",
            manifest_dir.join(git_path).display()
        );
    }
    println!("cargo:rerun-if-changed=../../../crates");
    println!("cargo:rerun-if-changed=../../../Cargo.lock");
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
