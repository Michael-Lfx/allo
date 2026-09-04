use std::env;
use std::path::PathBuf;

/// `agent-store` compiles `web/dist` into the binary (release) or serves it
/// from disk (debug) via rust-embed — but only when the `static-webui`
/// feature is enabled. The check is gated on the feature so a fresh clone
/// without `web/dist` (gitignored) can still `cargo check --workspace`:
/// without the feature the binary is API-only and the SPA fallback serves 404.
fn main() {
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    if env::var("CARGO_FEATURE_STATIC_WEBUI").is_err() {
        println!(
            "cargo:warning=agent-store built WITHOUT static-webui: no embedded web frontend \
             (SPA fallback serves 404). Build with `--features static-webui` (or `bun run \
             agent-store:build`) after `bun run --cwd ./web build`."
        );
        return;
    }

    let index = dist.join("index.html");
    if !index.is_file() {
        panic!(
            "embedded web frontend is not built: {} is missing\n\
             run `bun run --cwd ./web build` (or the full `bun run agent-store:build`) before \
             building agent-store with the static-webui feature",
            index.display()
        );
    }
}