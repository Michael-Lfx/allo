//! Presentation capability (host-managed ppt-master Skill): readiness,
//! install, and running its scripts with the private interpreter.
//!
//! Nomi decides by itself when a deck is requested: `status` says whether the
//! capability is ready (and returns the usage contract once it is);
//! `install` takes no arguments, so the agent can trigger the pinned download
//! but never choose what is downloaded. Install and script execution run local
//! code, so they are denied on IM channel and remote surfaces.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nomifun_api_types::PresentationCapabilityStatus;
use nomifun_presentation::{ScriptError, usage_contract};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::deps::{CallerCtx, GatewayDeps};
use crate::registry::{Capability, CapabilityMeta, DangerTier, Surface};
use crate::server::ok;

const LOCAL_ONLY: &[Surface] = &[Surface::Channel, Surface::Remote];

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoParams {}

/// Upper bound for one `wait_seconds` call. Kept below the 90 s default MCP
/// request timeout so a waiting call always returns before the client gives up.
const MAX_STATUS_WAIT_SECS: u64 = 60;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StatusParams {
    /// While an install is running, block up to this many seconds (max 60)
    /// until it finishes, then return. Omit or 0 to return immediately.
    #[serde(default)]
    wait_seconds: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunScriptParams {
    /// Script path relative to SKILL_DIR/scripts, e.g. "project_manager.py" or
    /// "source_to_md/pdf_to_md.py". A leading "scripts/" or "${SKILL_DIR}/scripts/" is accepted.
    script: String,
    /// Arguments exactly as the ppt-master docs show after the script path.
    /// Paths are relative to the working directory (default: the conversation workspace).
    #[serde(default)]
    args: Vec<String>,
    /// Working directory inside the conversation workspace (relative or
    /// absolute). Omit to use the workspace root, where `projects/` lives.
    #[serde(default)]
    cwd: Option<String>,
}

/// Keep the turn open: the user is waiting for the deck, not for a status
/// report, so Nomi waits in-turn and continues on its own once ready.
const WAIT_IN_TURN: &str = "Do NOT end your reply or ask the user to come back. Tell the user once that it is downloading, then call nomi_presentation_status with wait_seconds=60 again (repeat until status changes), and continue making the deck as soon as it is ready.";

async fn status(deps: Arc<GatewayDeps>, _ctx: CallerCtx, p: StatusParams) -> Value {
    let wait = p.wait_seconds.unwrap_or(0).min(MAX_STATUS_WAIT_SECS);
    let view = if wait == 0 {
        deps.presentation_service.status()
    } else {
        deps.presentation_service.wait_until_settled(Duration::from_secs(wait)).await
    };
    let next = match view.status {
        PresentationCapabilityStatus::Ready => {
            "Ready. Continue the user's request now without waiting for another message; follow `usage` exactly.".to_owned()
        }
        PresentationCapabilityStatus::Installing => format!("Still installing. {WAIT_IN_TURN}"),
        PresentationCapabilityStatus::NotInstalled => format!(
            "Not installed. Call nomi_presentation_install (no arguments; ~120 MB first download). {WAIT_IN_TURN}"
        ),
        PresentationCapabilityStatus::Failed => {
            "Install failed. Tell the user the reason from `capability.failure`, call nomi_presentation_install once to retry, then wait the same way; if it fails again, stop and report the failure.".to_owned()
        }
    };
    let usage = view.skill_dir.as_deref().map(usage_contract);
    ok(json!({ "capability": view, "next": next, "usage": usage }))
}

async fn install(deps: Arc<GatewayDeps>, _ctx: CallerCtx, _p: NoParams) -> Value {
    let response = deps.presentation_service.start_install(false);
    let note = if response.accepted {
        "install started in the background; call nomi_presentation_status with wait_seconds=60 (repeat while installing) and continue the deck when ready, in this same reply"
    } else if response.capability.status == PresentationCapabilityStatus::Ready {
        "already installed; call nomi_presentation_status for the usage contract"
    } else {
        "an install is already running; call nomi_presentation_status with wait_seconds=60 (repeat while installing)"
    };
    ok(json!({ "accepted": response.accepted, "capability": response.capability, "note": note }))
}

async fn run_script(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: RunScriptParams) -> Value {
    let Some(conversation_id) = ctx.conversation_id.as_deref() else {
        return json!({ "error": "running ppt-master scripts requires a conversation context" });
    };
    let conversation = match deps.conversation_service.get(ctx.user_id.as_str(), conversation_id).await {
        Ok(conversation) => conversation,
        Err(error) => return json!({ "error": format!("could not resolve the calling conversation: {error}") }),
    };
    let Some(workspace) = conversation
        .extra
        .get("workspace")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|workspace| !workspace.is_empty())
    else {
        return json!({ "error": "the current conversation has no workspace; ppt-master projects need one" });
    };
    match deps
        .presentation_service
        .run_script(&p.script, &p.args, Path::new(workspace), p.cwd.as_deref())
        .await
    {
        Ok(run) => ok(run),
        Err(ScriptError::NotReady(message)) => json!({ "error": message, "hint": "call nomi_presentation_status" }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

pub(crate) fn register(out: &mut Vec<Capability>) {
    out.push(Capability::new::<StatusParams, _, _>(
        CapabilityMeta::new(
            "nomi_presentation_status",
            "presentation",
            "Check the built-in ppt-master presentation Skill before creating or editing a PPT/PPTX/slide deck. Returns readiness, install progress, and once ready the SKILL_DIR + usage rules. While installing, pass wait_seconds=60 to block until the install finishes so you can continue in the same reply.",
            DangerTier::Read,
        ),
        status,
    ));
    out.push(Capability::new::<NoParams, _, _>(
        CapabilityMeta::new(
            "nomi_presentation_install",
            "presentation",
            "Install the ppt-master presentation Skill and its private Python runtime from pinned sources (no arguments). Call when nomi_presentation_status is not ready; returns immediately.",
            DangerTier::Write,
        )
        .deny_on(LOCAL_ONLY),
        install,
    ));
    out.push(Capability::new::<RunScriptParams, _, _>(
        CapabilityMeta::new(
            "nomi_presentation_run_script",
            "presentation",
            "Run one ppt-master script (SKILL_DIR/scripts/*.py) with the Skill's private Python in the conversation workspace. The only supported way to execute ppt-master; returns exit_code and output.",
            DangerTier::Write,
        )
        .deny_on(LOCAL_ONLY),
        run_script,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn tools_are_registered_owner_only_and_local_only() {
        let registry = Registry::global();
        let desktop = registry.tool_specs_for_caller(Surface::Desktop, Some(&["presentation"]), true);
        let mut names: Vec<_> = desktop.iter().map(|spec| spec.name).collect();
        names.sort();
        assert_eq!(names, ["nomi_presentation_install", "nomi_presentation_run_script", "nomi_presentation_status"]);

        let not_owner = registry.tool_specs_for_caller(Surface::Desktop, Some(&["presentation"]), false);
        assert!(not_owner.is_empty(), "installation-wide capability is owner-only");

        let channel: Vec<_> = registry
            .tool_specs_for_caller(Surface::Channel, Some(&["presentation"]), true)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(channel, ["nomi_presentation_status"], "IM sessions may only read status");
    }

    #[test]
    fn install_takes_no_arguments_and_run_script_has_a_closed_schema() {
        assert!(serde_json::from_value::<NoParams>(json!({})).is_ok());
        assert!(serde_json::from_value::<NoParams>(json!({"url": "https://evil.example"})).is_err());
        let status: StatusParams = serde_json::from_value(json!({})).unwrap();
        assert!(status.wait_seconds.is_none());
        let status: StatusParams = serde_json::from_value(json!({"wait_seconds": 60})).unwrap();
        assert_eq!(status.wait_seconds, Some(60));
        assert!(serde_json::from_value::<StatusParams>(json!({"wait_seconds": 5, "force": true})).is_err());
        let run: RunScriptParams = serde_json::from_value(json!({"script": "svg_to_pptx.py"})).unwrap();
        assert!(run.args.is_empty() && run.cwd.is_none());
        assert!(serde_json::from_value::<RunScriptParams>(json!({"script": "x.py", "python": "/bin/sh"})).is_err());
    }
}
