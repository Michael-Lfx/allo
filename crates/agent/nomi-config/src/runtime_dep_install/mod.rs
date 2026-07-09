//! Silent runtime dependency installation for Allo (ffmpeg for long-video concat).

mod coordinator;
mod ffmpeg;
mod probe;

use crate::dep_check::{RuntimeDep, is_available};
use crate::gateway::env_var_enabled_default_true;
use tracing::{debug, info, warn};

pub use coordinator::register_dep_gate_hooks;
pub use ffmpeg::ensure_ffmpeg;

const AUTO_ENSURE_ENV: &str = "NOMIFUN_AUTO_ENSURE_DEPS";

/// Whether Allo should attempt silent dependency installation (default: on).
pub fn auto_ensure_enabled() -> bool {
    env_var_enabled_default_true(AUTO_ENSURE_ENV)
}

/// Install a single runtime dependency when missing (`quiet` suppresses info logs).
pub async fn ensure_runtime_dep(dep: RuntimeDep, quiet: bool) -> bool {
    if is_available(dep) {
        debug!(%dep, "runtime dependency already available");
        return true;
    }

    let result = match dep {
        RuntimeDep::Ffmpeg => ensure_ffmpeg(quiet).await.map(|_| ()),
        other => {
            warn!(%other, "auto-install is not implemented for this dependency in Allo");
            return false;
        }
    };

    match result {
        Ok(()) if is_available(dep) => {
            if !quiet {
                info!(%dep, "runtime dependency installed");
            }
            true
        }
        Ok(()) => {
            warn!(%dep, "install finished but dependency still not detected");
            false
        }
        Err(e) => {
            if quiet {
                warn!(%dep, error = %e, "runtime dependency auto-install failed");
            } else {
                warn!(%dep, error = %e, "Failed to install runtime dependency");
            }
            false
        }
    }
}

/// Ensure all missing deps when [`auto_ensure_enabled`] is true.
pub async fn ensure_missing_runtime_deps(
    deps: &[RuntimeDep],
    quiet: bool,
) -> Vec<(RuntimeDep, bool)> {
    let mut results = Vec::new();
    for &dep in deps {
        if is_available(dep) {
            results.push((dep, true));
            continue;
        }
        if !auto_ensure_enabled() {
            debug!(%dep, "{AUTO_ENSURE_ENV} disabled; skipping auto install");
            results.push((dep, false));
            continue;
        }
        let ok = ensure_runtime_dep(dep, quiet).await;
        results.push((dep, ok));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_ensure_defaults_on() {
        let prior = std::env::var(AUTO_ENSURE_ENV).ok();
        unsafe { std::env::remove_var(AUTO_ENSURE_ENV) };
        assert!(auto_ensure_enabled());
        if let Some(v) = prior {
            unsafe { std::env::set_var(AUTO_ENSURE_ENV, v) };
        }
    }
}
