//! Tool → runtime dependency mapping and install coordination hooks.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::dep_check::{RuntimeDep, description, is_available};

pub type NotifyFn = Arc<dyn Fn(String) + Send + Sync>;
pub type SpawnInstallFn = Box<dyn Fn(Vec<RuntimeDep>) + Send + Sync>;
pub type WaitToolDepsFn =
    Arc<dyn Fn(&str, NotifyFn) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

static SPAWN_INSTALL: OnceLock<SpawnInstallFn> = OnceLock::new();
static WAIT_TOOL_DEPS: OnceLock<WaitToolDepsFn> = OnceLock::new();

pub fn register_hooks(spawn: SpawnInstallFn, wait: WaitToolDepsFn) {
    let _ = SPAWN_INSTALL.set(spawn);
    let _ = WAIT_TOOL_DEPS.set(wait);
}

pub fn spawn_background_install(deps: Vec<RuntimeDep>) {
    if let Some(spawn) = SPAWN_INSTALL.get() {
        spawn(deps);
    }
}

pub fn deps_for_tool(tool_name: &str) -> &'static [RuntimeDep] {
    match tool_name {
        "search_files" => &[RuntimeDep::Ripgrep],
        name if name.starts_with("browser_") => &[RuntimeDep::Browser],
        "computer_use" => &[RuntimeDep::Browser],
        "tts" | "tts_premium" | "video_analyze" | "media_long_video" => &[RuntimeDep::Ffmpeg],
        _ => &[],
    }
}

pub async fn await_tool_deps(tool_name: &str, notify: NotifyFn) -> bool {
    let deps = deps_for_tool(tool_name);
    if deps.is_empty() || deps.iter().all(|dep| is_available(*dep)) {
        return true;
    }
    if let Some(wait) = WAIT_TOOL_DEPS.get() {
        return wait(tool_name, notify).await;
    }
    deps.iter().all(|dep| is_available(*dep))
}

pub fn missing_dep_labels(deps: &[RuntimeDep]) -> String {
    deps.iter()
        .filter(|dep| !is_available(**dep))
        .map(|dep| format!("{} ({})", dep, description(*dep)))
        .collect::<Vec<_>>()
        .join(", ")
}
