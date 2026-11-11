//! Computer-use domain capabilities (registry form, feature-gated). Lets a
//! Remote/companion agent drive the local desktop — a thin facade over the
//! in-tree `nomi_computer::ComputerTool`, mirroring the inward
//! `mcp-computer-stdio` bridge (same 14 discrete tools, same action mapping,
//! zero duplicated logic). Only compiled with the `computer-use` feature.
//!
//! DangerTier: observe/screenshot/cursor_position/list_windows/wait are `Read`;
//! input-synthesis actions (click/type/key/scroll/launch/…) are `Write` — which
//! is Allowed on every surface incl. Remote, so an external "外部伙伴" can drive
//! the desktop (same posture as the browser `act` tools).

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::computer_registry::{ComputerRegistry, tool_result_to_value};
use crate::deps::{CallerCtx, GatewayDeps};
use crate::registry::{Capability, CapabilityMeta, DangerTier};

fn registry(deps: &GatewayDeps) -> Result<&ComputerRegistry, Value> {
    deps.computer_registry
        .as_ref()
        .ok_or_else(|| json!({ "error": "computer-use is not available on this host" }))
}

fn caller_session_id(ctx: &CallerCtx) -> String {
    if let Some(conversation_id) = &ctx.conversation_id {
        return format!("conversation:{conversation_id}");
    }
    if let Some(companion_id) = &ctx.companion_id {
        return format!("companion:{companion_id}");
    }
    format!("user:{}", ctx.user_id)
}

async fn run(deps: &GatewayDeps, ctx: &CallerCtx, input: Value) -> Value {
    match registry(deps) {
        Ok(reg) => {
            tool_result_to_value(reg.execute_for_session(&caller_session_id(ctx), input).await)
        }
        Err(e) => e,
    }
}

// ---- parameter structs (lifted from the inward computer_stdio bridge) -------

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoParams {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RefParams {
    /// Element number `[ref]` from the most recent `nomi_computer_snapshot`.
    #[serde(default)]
    r#ref: Option<u32>,
    /// Exact accessibility role, e.g. button or edit.
    #[serde(default)]
    role: Option<String>,
    /// Exact case-insensitive accessible name or value.
    #[serde(default)]
    name: Option<String>,
    /// Case-insensitive accessible name/value substring.
    #[serde(default)]
    name_contains: Option<String>,
    /// Restrict to one source: a11y, ocr, or vision.
    #[serde(default)]
    source: Option<String>,
    /// 1-based match when multiple elements match the selector.
    #[serde(default)]
    nth: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetValueParams {
    /// Element number `[ref]` from the most recent snapshot.
    #[serde(default)]
    r#ref: Option<u32>,
    /// Exact accessibility role, e.g. button or edit.
    #[serde(default)]
    role: Option<String>,
    /// Exact case-insensitive accessible name or value.
    #[serde(default)]
    name: Option<String>,
    /// Case-insensitive accessible name/value substring.
    #[serde(default)]
    name_contains: Option<String>,
    /// Restrict to one source: a11y, ocr, or vision.
    #[serde(default)]
    source: Option<String>,
    /// 1-based match when multiple elements match the selector.
    #[serde(default)]
    nth: Option<u64>,
    /// The text to set into the element.
    text: String,
}

#[allow(clippy::too_many_arguments)]
fn apply_target_fields(
    input: &mut Value,
    ref_id: Option<u32>,
    role: Option<&str>,
    name: Option<&str>,
    name_contains: Option<&str>,
    source: Option<&str>,
    nth: Option<u64>,
) {
    if let Some(ref_id) = ref_id {
        input["ref"] = json!(ref_id);
    }
    for (key, value) in [
        ("role", role.map(|value| json!(value))),
        ("name", name.map(|value| json!(value))),
        ("name_contains", name_contains.map(|value| json!(value))),
        ("source", source.map(|value| json!(value))),
        ("nth", nth.map(|value| json!(value))),
    ] {
        if let Some(value) = value {
            input[key] = value;
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct XyParams {
    /// X coordinate in pixels of the most recent screenshot.
    x: i64,
    /// Y coordinate in pixels of the most recent screenshot.
    y: i64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TypeParams {
    /// The text to type into the focused control.
    text: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct KeyParams {
    /// Key or combo to press, e.g. "enter" or "ctrl+a".
    key: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScrollParams {
    /// Scroll direction: up, down, left, or right.
    direction: String,
    /// Wheel clicks (default 3).
    #[serde(default)]
    amount: Option<i64>,
    /// Optional X to scroll at (screenshot pixels).
    #[serde(default)]
    x: Option<i64>,
    /// Optional Y to scroll at (screenshot pixels).
    #[serde(default)]
    y: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LaunchParams {
    /// What to open: a file/folder path or an app name. Web URLs (http/https)
    /// are rejected — use the managed Browser tools for web pages.
    target: String,
    /// Optional application to open the target WITH (e.g. a specific editor).
    #[serde(default)]
    app: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScreenshotParams {
    /// Optional display index to capture (default: primary).
    #[serde(default)]
    display: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WaitParams {
    /// Seconds to wait (max 5).
    #[serde(default)]
    seconds: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WaitForParams {
    /// Condition: element_present, element_absent, text_present, text_absent, changed.
    condition: String,
    /// Optional window id from `list_windows`.
    #[serde(default)]
    window_id: Option<u64>,
    /// Element selector: exact accessibility role.
    #[serde(default)]
    role: Option<String>,
    /// Element selector: exact accessible name or value.
    #[serde(default)]
    name: Option<String>,
    /// Element selector: case-insensitive name/value substring.
    #[serde(default)]
    name_contains: Option<String>,
    /// Element selector: a11y, ocr, or vision.
    #[serde(default)]
    source: Option<String>,
    /// Element selector: 1-based match when several elements match.
    #[serde(default)]
    nth: Option<u64>,
    /// Text to look for in text_present/text_absent.
    #[serde(default)]
    text: Option<String>,
    /// Maximum wait in milliseconds (default 5000, max 15000).
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// Polling interval in milliseconds (default 200).
    #[serde(default)]
    interval_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SnapshotParams {
    /// Optional window id from `list_windows`. When set, snapshot that window
    /// without raising it.
    #[serde(default)]
    window_id: Option<u64>,
    /// Observe mode: auto, semantic, visual, or text.
    #[serde(default)]
    mode: Option<String>,
    /// Maximum interactable elements (default 120).
    #[serde(default)]
    node_budget: Option<u64>,
    /// Maximum accessibility tree depth (default 12).
    #[serde(default)]
    max_depth: Option<u64>,
}

// ---- handlers (forward to the shared tool's action dispatcher) --------------

async fn snapshot(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: SnapshotParams) -> Value {
    let mut input = json!({ "action": "observe" });
    if let Some(window_id) = p.window_id {
        input["window_id"] = json!(window_id);
    }
    if let Some(mode) = p.mode {
        input["mode"] = json!(mode);
    }
    if let Some(node_budget) = p.node_budget {
        input["node_budget"] = json!(node_budget);
    }
    if let Some(max_depth) = p.max_depth {
        input["max_depth"] = json!(max_depth);
    }
    run(&deps, &ctx, input).await
}
async fn screenshot(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: ScreenshotParams) -> Value {
    run(&deps, &ctx, json!({ "action": "screenshot", "display": p.display })).await
}
async fn click(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: RefParams) -> Value {
    let mut input = json!({ "action": "click_element" });
    apply_target_fields(
        &mut input,
        p.r#ref,
        p.role.as_deref(),
        p.name.as_deref(),
        p.name_contains.as_deref(),
        p.source.as_deref(),
        p.nth,
    );
    run(&deps, &ctx, input).await
}
async fn right_click(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: RefParams) -> Value {
    let mut input = json!({ "action": "right_click_element" });
    apply_target_fields(
        &mut input,
        p.r#ref,
        p.role.as_deref(),
        p.name.as_deref(),
        p.name_contains.as_deref(),
        p.source.as_deref(),
        p.nth,
    );
    run(&deps, &ctx, input).await
}
async fn double_click(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: RefParams) -> Value {
    let mut input = json!({ "action": "double_click_element" });
    apply_target_fields(
        &mut input,
        p.r#ref,
        p.role.as_deref(),
        p.name.as_deref(),
        p.name_contains.as_deref(),
        p.source.as_deref(),
        p.nth,
    );
    run(&deps, &ctx, input).await
}
async fn set_value(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: SetValueParams) -> Value {
    let mut input = json!({ "action": "set_element_value", "text": p.text });
    apply_target_fields(
        &mut input,
        p.r#ref,
        p.role.as_deref(),
        p.name.as_deref(),
        p.name_contains.as_deref(),
        p.source.as_deref(),
        p.nth,
    );
    run(&deps, &ctx, input).await
}
async fn click_xy(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: XyParams) -> Value {
    run(&deps, &ctx, json!({ "action": "left_click", "x": p.x, "y": p.y })).await
}
async fn type_text(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: TypeParams) -> Value {
    run(&deps, &ctx, json!({ "action": "type", "text": p.text })).await
}
async fn key(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: KeyParams) -> Value {
    run(&deps, &ctx, json!({ "action": "key", "key": p.key })).await
}
async fn scroll(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: ScrollParams) -> Value {
    run(
        &deps,
        &ctx,
        json!({ "action": "scroll", "direction": p.direction, "amount": p.amount, "x": p.x, "y": p.y }),
    )
    .await
}
async fn launch(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: LaunchParams) -> Value {
    run(&deps, &ctx, json!({ "action": "launch", "target": p.target, "app": p.app })).await
}
async fn list_windows(deps: Arc<GatewayDeps>, ctx: CallerCtx, _p: NoParams) -> Value {
    run(&deps, &ctx, json!({ "action": "list_windows" })).await
}
async fn cursor_position(deps: Arc<GatewayDeps>, ctx: CallerCtx, _p: NoParams) -> Value {
    run(&deps, &ctx, json!({ "action": "cursor_position" })).await
}
async fn wait(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: WaitParams) -> Value {
    run(&deps, &ctx, json!({ "action": "wait", "seconds": p.seconds })).await
}

async fn wait_for(deps: Arc<GatewayDeps>, ctx: CallerCtx, p: WaitForParams) -> Value {
    let mut input = json!({ "action": "wait_for", "condition": p.condition });
    for (key, value) in [
        ("window_id", p.window_id.map(|v| json!(v))),
        ("role", p.role.map(|v| json!(v))),
        ("name", p.name.map(|v| json!(v))),
        ("name_contains", p.name_contains.map(|v| json!(v))),
        ("source", p.source.map(|v| json!(v))),
        ("nth", p.nth.map(|v| json!(v))),
        ("text", p.text.map(|v| json!(v))),
        ("timeout_ms", p.timeout_ms.map(|v| json!(v))),
        ("interval_ms", p.interval_ms.map(|v| json!(v))),
    ] {
        if let Some(value) = value {
            input[key] = value;
        }
    }
    run(&deps, &ctx, input).await
}

pub(crate) fn register(out: &mut Vec<Capability>) {
    out.push(Capability::new::<SnapshotParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_snapshot",
            "computer",
            "Read the accessibility tree (windows → controls, numbered [ref]). Optional `window_id` observes that window without raising it. `mode=auto|semantic|visual|text`; `node_budget`/`max_depth` tune complex trees. Re-run after any UI change. Read-only.",
            DangerTier::Read,
        ),
        snapshot,
    ));
    out.push(Capability::new::<ScreenshotParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_screenshot",
            "computer",
            "Capture the screen as a PNG (optional `display` index).",
            DangerTier::Read,
        ),
        screenshot,
    ));
    out.push(Capability::new::<RefParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_click",
            "computer",
            "Activate the element with the given `ref` from the latest snapshot.",
            DangerTier::Write,
        ),
        click,
    ));
    out.push(Capability::new::<RefParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_right_click",
            "computer",
            "Right-click the element with the given `ref` (opens its context menu).",
            DangerTier::Write,
        ),
        right_click,
    ));
    out.push(Capability::new::<RefParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_double_click",
            "computer",
            "Double-click the element with the given `ref`.",
            DangerTier::Write,
        ),
        double_click,
    ));
    out.push(Capability::new::<SetValueParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_set_value",
            "computer",
            "Set the `text` value of the element with the given `ref` (good for text fields).",
            DangerTier::Write,
        ),
        set_value,
    ));
    out.push(Capability::new::<XyParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_click_xy",
            "computer",
            "Left-click at pixel coordinates (`x`, `y`) of the most recent screenshot. Prefer click-by-ref when possible.",
            DangerTier::Write,
        ),
        click_xy,
    ));
    out.push(Capability::new::<TypeParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_type",
            "computer",
            "Type the `text` string into the focused control.",
            DangerTier::Write,
        ),
        type_text,
    ));
    out.push(Capability::new::<KeyParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_key",
            "computer",
            "Press a key or combo, e.g. \"enter\" or \"ctrl+a\".",
            DangerTier::Write,
        ),
        key,
    ));
    out.push(Capability::new::<ScrollParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_scroll",
            "computer",
            "Scroll in `direction` (up/down/left/right) by `amount` wheel clicks, optionally at (`x`, `y`).",
            DangerTier::Write,
        ),
        scroll,
    ));
    out.push(Capability::new::<LaunchParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_launch",
            "computer",
            "Open an application, file, or folder via the OS shell. Always use this instead of shell `start`/`Start-Process`. Web URLs (http/https) are rejected: use the managed Browser tools for web pages.",
            DangerTier::Write,
        ),
        launch,
    ));
    out.push(Capability::new::<NoParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_list_windows",
            "computer",
            "List open windows with ids, titles, positions and sizes.",
            DangerTier::Read,
        ),
        list_windows,
    ));
    out.push(Capability::new::<NoParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_cursor_position",
            "computer",
            "Report the mouse cursor position in screenshot coordinates.",
            DangerTier::Read,
        ),
        cursor_position,
    ));
    out.push(Capability::new::<WaitParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_wait",
            "computer",
            "Pause for `seconds` (max 5) to let the UI settle.",
            DangerTier::Read,
        ),
        wait,
    ));
    out.push(Capability::new::<WaitForParams, _, _>(
        CapabilityMeta::new(
            "nomi_computer_wait_for",
            "computer",
            "Poll the accessibility tree until an element/text condition is met or the UI changes. Prefer this over repeated snapshot + fixed wait loops.",
            DangerTier::Read,
        ),
        wait_for,
    ));
}
