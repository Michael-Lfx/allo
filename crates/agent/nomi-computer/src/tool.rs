//! The Computer tool: screenshot, mouse/keyboard synthesis, window control.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_a11y::{
    A11yEngine, A11yError, ElementAction, ElementEntry, ObserveOpts, SnapshotGen, Source, Target,
};
use nomi_config::config::ComputerConfig;
use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::input::{self, ScrollDirection};
use crate::keys::parse_key_combo;
use crate::scale::{map_llm_coord, map_screen_coord};
use crate::screen::{
    CaptureGeometry, CaptureRegion, CapturedScreen, capture_screen, capture_screen_region,
    encode_png,
};
use crate::fallback_backend;

const MAX_WAIT_SECONDS: f64 = 5.0;
const MAX_WAIT_FOR_MS: u64 = 15_000;
const DEFAULT_WAIT_FOR_MS: u64 = 5_000;
const DEFAULT_WAIT_INTERVAL_MS: u64 = 200;
const DEFAULT_SCROLL_AMOUNT: i64 = 3;
const DEFAULT_SESSION_ID: &str = "default";
const MAX_SESSION_STATES: usize = 64;

/// Example key combo for the platform we are compiled for. The accelerator
/// modifier differs by OS (Command on macOS, Control on Windows/Linux), so we
/// steer the model toward the right one instead of always suggesting `cmd`,
/// which is a macOS idiom. `parse_key_combo` still accepts `cmd` everywhere and
/// remaps it per-platform, but a correct example reduces wrong presses.
#[cfg(target_os = "macos")]
const KEY_COMBO_EXAMPLE: &str = "cmd+shift+t";
#[cfg(not(target_os = "macos"))]
const KEY_COMBO_EXAMPLE: &str = "ctrl+shift+t";

const DESCRIPTION: &str = "Control the local desktop: read the accessibility tree, take \
screenshots, move and click the mouse, type text, press keys, scroll, and manage windows.\n\n\
PREFER the accessibility-first flow: call `observe` to get a numbered list of interactable \
elements (with a Set-of-Marks overlay screenshot when available), then act on one with \
`click_element` and its `[ref]`. This is far more reliable than guessing pixel coordinates. \
Fall back to pixel actions only when an element is not in the accessibility tree (e.g. canvas \
or some web content).\n\n\
Actions:\n\
- observe: read the accessibility tree. Omit `window_id` for the foreground window; pass \
`window_id` from `list_windows` to observe that window without raising it. `mode` defaults to \
`auto` (semantic fast path for rich trees, window screenshot for sparse/truncated trees). Use \
`semantic` for no pixels, `visual` for screenshot + Set-of-Marks, or `text` for OCR refs without \
an image. `node_budget`/`max_depth` can be raised for complex Chrome/IDE trees. Re-run after any \
UI change (a `[ref]` is only valid for the latest snapshot).\n\
- click_element: activate an element from the latest `observe` by `ref` or semantic selector \
(role/name/name_contains/source/nth). Stale/disconnected refs fail closed and require reobserve.\n\
- right_click_element / double_click_element: right-click or double-click an element by `ref` or \
semantic selector (pixel gesture at the element center).\n\
- set_element_value: set the `text` value of an element by `ref` or semantic selector \
(accessibility set-value, with a focus-and-type fallback).\n\
- launch: open an application, file, or folder reliably via the OS shell — pass `target` \
(e.g. \"notepad\", a file path) and optionally `app` to open the target WITH a specific \
application. ALWAYS use this to open apps/files; do NOT run `cmd /c \
start`, `Start-Process`, or `explorer` in a shell — those are unreliable here. Web URLs \
(http/https) are not launchable: read or interact with web pages through the managed \
Browser tool instead.\n\
- screenshot: capture the screen (optional `display` index) when you need raw pixels.\n\
- cursor_position: report the mouse cursor position in screenshot coordinates.\n\
- list_windows: list open windows with ids, titles, positions and sizes.\n\
- left_click / right_click / middle_click / double_click / triple_click: click at (`x`, `y`).\n\
- mouse_move: move the cursor to (`x`, `y`) without clicking.\n\
- left_click_drag: press at (`start_x`, `start_y`), drag to (`end_x`, `end_y`), release.\n\
- type: type the `text` string into the focused control.\n\
- key: press a key or combo from `key`, e.g. \"enter\". Use the platform's primary \
accelerator modifier (Command on macOS, Control on Windows/Linux) for shortcuts.\n\
- scroll: scroll in `direction` (up/down/left/right) by `amount` wheel clicks, optionally \
at (`x`, `y`).\n\
- focus_window: bring the window with `window_id` to the front. This steals foreground from \
the user — use only when you must type or click into that window, not to inspect it.\n\
- wait: pause for `seconds` (max 5) to let the UI settle.\n\
- wait_for: poll the accessibility tree until an element/text condition is met or UI changes. This is \
strongly preferred over repeated observe + fixed wait loops.\n\n\
Usage notes:\n\
- Prefer `observe` + `click_element [ref]` over pixel coordinates whenever the element is \
listed.\n\
- Prefer `observe` with `window_id` to inspect a background window; do not `focus_window` just \
to look.\n\
- Take a screenshot before interacting so you can see the screen, and take another after \
acting to verify the effect.\n\
- All coordinates are pixel positions in the most recent screenshot; they are mapped to \
the real screen automatically.\n\
- Prefer key presses (e.g. \"enter\") over clicking buttons when both work.";

pub struct ComputerTool {
    max_screenshot_edge: u32,
    /// Tool description with the session's dynamic capability note appended
    /// (computed once at construction; part of the cacheable tool schema).
    description: String,
    /// Lazily-initialized accessibility engine (a11y-first targeting). `Some(Err)`
    /// caches an unavailable backend (an OS without an a11y engine, or a startup
    /// failure) so we don't retry per call.
    a11y: Mutex<Option<Result<Arc<dyn A11yEngine>, String>>>,
    /// Per-session screenshot geometry and latest accessibility snapshot.
    /// A shared tool may serve multiple agents; session isolation prevents one
    /// agent's refs and coordinate mapping from being clobbered by another.
    sessions: Mutex<HashMap<String, SessionState>>,
    /// Whether the session's model can read images. A text-only model gets the
    /// full accessibility tree plus OCR text instead of a screenshot.
    image_input: bool,
}

struct SessionState {
    last_capture: Option<CaptureGeometry>,
    last_snapshot: Option<SnapshotCache>,
    /// The previous observe for this session needed pixels because the a11y
    /// tree was sparse or truncated. The next `auto` observe can prefetch the
    /// capture in parallel instead of discovering that cost after the tree walk.
    capture_hint: bool,
    last_used: Instant,
}

impl SessionState {
    fn new() -> Self {
        Self {
            last_capture: None,
            last_snapshot: None,
            capture_hint: false,
            last_used: Instant::now(),
        }
    }
}

struct SnapshotCache {
    generation: SnapshotGen,
    entries: Vec<CachedEntry>,
}

#[derive(Clone)]
struct CachedEntry {
    /// What the model sees: display `[ref]`, role/name, pixel-space bounds, source.
    display: ElementEntry,
    target: CachedTarget,
}

#[derive(Clone, Copy)]
enum CachedTarget {
    /// Accessibility element: act via the engine; `screen_center` is the pixel
    /// fallback (screen logical coordinates).
    Ax {
        engine_ref: u32,
        screen_center: (i32, i32),
    },
    /// OCR / pixel-only element: act by clicking `screen_center`.
    Pixel { screen_center: (i32, i32) },
}

#[derive(Debug, Clone, Default)]
struct ElementSelector {
    role: Option<String>,
    name: Option<String>,
    name_contains: Option<String>,
    source: Option<Source>,
    nth: Option<usize>,
}

impl ElementSelector {
    fn is_empty(&self) -> bool {
        self.role.is_none()
            && self.name.is_none()
            && self.name_contains.is_none()
            && self.source.is_none()
            && self.nth.is_none()
    }

    fn matches(&self, entry: &ElementEntry) -> bool {
        if let Some(role) = &self.role
            && !entry.role.eq_ignore_ascii_case(role)
        {
            return false;
        }
        if let Some(source) = self.source
            && entry.source != source
        {
            return false;
        }
        let name = entry.name.as_deref().unwrap_or_default();
        let value = entry.value.as_deref().unwrap_or_default();
        if let Some(expected) = &self.name
            && !name.eq_ignore_ascii_case(expected)
            && !value.eq_ignore_ascii_case(expected)
        {
            return false;
        }
        if let Some(fragment) = &self.name_contains {
            let fragment = fragment.to_lowercase();
            if !name.to_lowercase().contains(&fragment) && !value.to_lowercase().contains(&fragment) {
                return false;
            }
        }
        true
    }
}

/// Run OCR fusion when the accessibility tree yields fewer than this many
/// interactable elements (e.g. Electron/canvas/game windows), to recover text
/// targets the tree does not expose. Keeps OCR off the hot path for a11y-rich
/// native apps.
const OCR_FUSION_AX_THRESHOLD: usize = 6;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObserveMode { Auto, Semantic, Visual, Text }

impl ObserveMode {
    fn parse(input: Option<&Value>) -> Result<Self, String> {
        let Some(value) = input else { return Ok(Self::Auto); };
        let Some(mode) = value.as_str() else {
            return Err("Parameter `mode` must be one of: auto, semantic, visual, text.".to_string());
        };
        match mode {
            "auto" => Ok(Self::Auto),
            "semantic" => Ok(Self::Semantic),
            "visual" => Ok(Self::Visual),
            "text" => Ok(Self::Text),
            _ => Err(format!("Unknown observe mode {mode:?}. Use one of: auto, semantic, visual, text.")),
        }
    }

    fn needs_capture(self, image_input: bool, ax_entries: usize, truncated: bool) -> bool {
        match self {
            Self::Semantic => false,
            Self::Visual | Self::Text => true,
            Self::Auto => !image_input || truncated || ax_entries < OCR_FUSION_AX_THRESHOLD,
        }
    }

    fn should_attach_image(self, image_input: bool) -> bool {
        image_input && matches!(self, Self::Auto | Self::Visual)
    }

    fn always_capture(self, image_input: bool) -> bool {
        matches!(self, Self::Visual | Self::Text) || (self == Self::Auto && !image_input)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Semantic => "semantic",
            Self::Visual => "visual",
            Self::Text => "text",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitCondition {
    ElementPresent,
    ElementAbsent,
    TextPresent,
    TextAbsent,
    Changed,
}

impl WaitCondition {
    fn parse(value: Option<&Value>) -> Result<Self, String> {
        match value.and_then(Value::as_str) {
            Some("element_present") => Ok(Self::ElementPresent),
            Some("element_absent") => Ok(Self::ElementAbsent),
            Some("text_present") => Ok(Self::TextPresent),
            Some("text_absent") => Ok(Self::TextAbsent),
            Some("changed") => Ok(Self::Changed),
            Some(other) => Err(format!(
                "Unknown wait condition {other:?}. Use element_present, element_absent, text_present, text_absent, or changed."
            )),
            None => Err("Parameter `condition` is required for wait_for.".to_string()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ElementPresent => "element_present",
            Self::ElementAbsent => "element_absent",
            Self::TextPresent => "text_present",
            Self::TextAbsent => "text_absent",
            Self::Changed => "changed",
        }
    }
}

const TEXT_ONLY_NOTE: &str = "\n- This session's model cannot read images: `observe` returns \
text only (the full accessibility tree plus OCR text lines, each with a [ref]) and \
`screenshot` returns the same text. Act with `click_element` and [ref]; pixel coordinates \
are not usable.\n";

impl ComputerTool {
    pub fn new(config: &ComputerConfig) -> Self {
        Self {
            max_screenshot_edge: config.max_screenshot_edge,
            description: format!("{DESCRIPTION}{}", capabilities_note()),
            a11y: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            image_input: true,
        }
    }

    pub fn with_image_input(mut self, image_input: bool) -> Self {
        self.image_input = image_input;
        if !image_input && !self.description.contains(TEXT_ONLY_NOTE) {
            self.description.push_str(TEXT_ONLY_NOTE);
        }
        self
    }

    /// Lazily construct (and cache) the accessibility engine. The error string
    /// is cached too, so an unavailable backend is reported without retrying.
    fn engine(&self) -> Result<Arc<dyn A11yEngine>, String> {
        let mut guard = self.a11y.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_none() {
            *guard = Some(nomi_a11y::create_engine().map_err(|e| e.to_string()));
        }
        guard.as_ref().unwrap().clone()
    }

    fn with_session_state<R>(
        &self,
        session_id: &str,
        f: impl FnOnce(&mut SessionState) -> R,
    ) -> R {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !sessions.contains_key(session_id) && sessions.len() >= MAX_SESSION_STATES {
            if let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, state)| state.last_used)
                .map(|(id, _)| id.clone())
            {
                sessions.remove(&oldest);
            }
        }
        let state = sessions
            .entry(session_id.to_string())
            .or_insert_with(SessionState::new);
        state.last_used = Instant::now();
        f(state)
    }

    fn set_last_capture(&self, session_id: &str, geometry: CaptureGeometry) {
        self.with_session_state(session_id, |state| state.last_capture = Some(geometry));
    }

    fn last_capture(&self, session_id: &str) -> Option<CaptureGeometry> {
        self.with_session_state(session_id, |state| state.last_capture)
    }

    fn set_last_snapshot(&self, session_id: &str, snapshot: SnapshotCache) {
        self.with_session_state(session_id, |state| state.last_snapshot = Some(snapshot));
    }

    fn capture_hint(&self, session_id: &str) -> bool {
        self.with_session_state(session_id, |state| state.capture_hint)
    }

    fn set_capture_hint(&self, session_id: &str, needs_pixels: bool) {
        self.with_session_state(session_id, |state| state.capture_hint = needs_pixels);
    }

    /// a11y-first "look": read the focused window's accessibility tree, return a
    /// numbered element list, and (when screen capture is available) a
    /// Set-of-Marks overlay screenshot. Needs only the Accessibility grant for
    /// the element list; the overlay additionally needs Screen Recording.
    /// Pass `window_id` to scope the tree to that window without raising it.
    async fn do_observe(&self, session_id: &str, input: &Value) -> ToolResult {
        let started = Instant::now();
        let mode = match ObserveMode::parse(input.get("mode")) {
            Ok(mode) => mode,
            Err(error) => return ToolResult::error(error),
        };
        let request = match observe_opts_from_input(input).await {
            Ok(request) => request,
            Err(e) => return ToolResult::error(e),
        };
        let scoped = request.opts.window_id;
        let target_window = request.window;
        let max_edge = self.max_screenshot_edge;

        // For modes that always need pixels, capture the target window while
        // the accessibility tree is being read. `auto` deliberately does not
        // start an unnecessary multi-second capture for a11y-rich native apps.
        let prefetch_auto = mode == ObserveMode::Auto
            && self.image_input
            && self.capture_hint(session_id);
        let capture_task = if mode.always_capture(self.image_input) || prefetch_auto {
            let window = resolve_capture_window(target_window.clone()).await;
            Some(spawn_capture(window, max_edge))
        } else {
            None
        };
        let a11y_started = Instant::now();
        let opts = request.opts;
        let engine = match self.engine() {
            Ok(e) => e,
            Err(msg) => {
                return ToolResult::error(format!(
                    "Accessibility engine unavailable: {msg} Use the pixel actions \
                     (screenshot + click with x,y) instead."
                ));
            }
        };
        let eng = engine.clone();
        let snap = tokio::task::spawn_blocking(move || eng.observe(&opts))
            .await
            .unwrap_or_else(|e| Err(A11yError::Backend(format!("observe task failed: {e}"))));
        let a11y_ms = a11y_started.elapsed().as_millis();
        let snap = match snap {
            Ok(s) => s,
            Err(e) => return ToolResult::error(format!("Accessibility observe failed: {e}")),
        };
        let needs_capture = mode.needs_capture(self.image_input, snap.entries.len(), snap.truncated);
        self.set_capture_hint(
            session_id,
            snap.truncated || snap.entries.len() < OCR_FUSION_AX_THRESHOLD,
        );
        tracing::debug!(
            mode = mode.as_str(),
            entries = snap.entries.len(),
            truncated = snap.truncated,
            needs_capture,
            a11y_ms,
            "Computer observe accessibility phase completed"
        );
        let app_note = snap
            .app_name
            .as_deref()
            .map(|a| format!(" in {a}"))
            .unwrap_or_default();
        let ax_note = if snap.truncated {
            " (a11y tree truncated to the node budget)"
        } else {
            ""
        };
        let scope_note = if scoped.is_some() {
            " Window-scoped (did not raise/focus the window)."
        } else {
            ""
        };
        // The engine numbers controls 1..N (grouped per window, then reading
        // order); OCR-fused targets continue after this so refs never collide.
        let max_ax_ref = snap.entries.iter().map(|e| e.r#ref).max().unwrap_or(0);

        // Capture a screenshot for the overlay + OCR fusion + pixel mapping. If
        // it is denied, fall back to an AX-only text list (a11y needs only the
        // Accessibility grant) — the core a11y-first win.
        let capture_started = Instant::now();
        let captured: Option<Result<CapturedScreen, String>> = if needs_capture {
            match capture_task {
                Some(task) => Some(
                    task.await
                        .unwrap_or_else(|e| Err(format!("Screenshot task failed: {e}"))),
                ),
                None => {
                    let window = resolve_capture_window(target_window).await;
                    Some(
                        spawn_capture(window, max_edge)
                            .await
                            .unwrap_or_else(|e| Err(format!("Screenshot task failed: {e}"))),
                    )
                }
            }
        } else {
            None
        };
        let capture_ms = capture_started.elapsed().as_millis();

        let mut shot = match captured {
            Some(Ok(shot)) => {
                self.set_last_capture(session_id, shot.geometry);
                shot
            }
            Some(Err(error)) => {
                let (cached, count) = cached_ax_entries(&snap);
                self.set_last_snapshot(session_id, SnapshotCache {
                    generation: snap.generation,
                    entries: cached,
                });
                tracing::debug!(
                    mode = mode.as_str(),
                    entries = count,
                    a11y_ms,
                    capture_ms,
                    total_ms = started.elapsed().as_millis(),
                    "Computer observe completed without pixels"
                );
                return ToolResult::text(format!(
                    "Accessibility snapshot (gen {}, mode {}): {count} element(s){app_note}{ax_note}\
                     {scope_note} No Set-of-Marks overlay (screen capture unavailable: {error}). \
                     Still actionable by [ref] with `click_element`.\n\n{}",
                    snap.generation.0,
                    mode.as_str(),
                    snap.text
                ));
            }
            None => {
                let (cached, count) = cached_ax_entries(&snap);
                self.set_last_snapshot(session_id, SnapshotCache {
                    generation: snap.generation,
                    entries: cached,
                });
                tracing::debug!(
                    mode = mode.as_str(),
                    entries = count,
                    a11y_ms,
                    capture_ms,
                    total_ms = started.elapsed().as_millis(),
                    "Computer observe completed without pixels"
                );
                return ToolResult::text(format!(
                    "Accessibility snapshot (gen {}, mode {}): {count} element(s){app_note}{ax_note}\
                     {scope_note} Semantic-only observe: Set-of-Marks omitted by mode. Still \
                     actionable by [ref] with `click_element`.\n\n{}",
                    snap.generation.0,
                    mode.as_str(),
                    snap.text
                ));
            }
        };
        let geom = shot.geometry;

        // AX entries keep their engine [ref] (so the overlay numbers, the semantic
        // tree, and click_element all agree); bounds are mapped to pixel space for
        // the overlay.
        let mut cached: Vec<CachedEntry> = Vec::with_capacity(snap.entries.len());
        for e in &snap.entries {
            let (cx, cy) = e.bounds.center();
            let mut display = e.clone();
            display.bounds = ax_rect_to_pixel(e.bounds, &geom);
            cached.push(CachedEntry {
                display,
                target: CachedTarget::Ax {
                    engine_ref: e.r#ref,
                    screen_center: (cx as i32, cy as i32),
                },
            });
        }

        // OCR fusion when the accessibility tree is thin (Electron/canvas/games):
        // recover on-screen text Vision can read but the tree does not expose.
        // OCR targets are numbered AFTER the AX refs and listed in an appendix.
        let mut ocr_appendix: Vec<String> = Vec::new();
        let mut next_ref = max_ax_ref;
        // OCR is a text-model / explicit-text-mode path. Vision models get the
        // Set-of-Marks overlay instead; running OCR as well roughly doubles
        // observe latency on complex windows and duplicates what the image
        // already shows.
        let should_ocr = mode == ObserveMode::Text || !self.image_input;
        if should_ocr {
            let img = shot.image.clone();
            let langs = vec!["zh-Hans".to_string(), "en-US".to_string()];
            let ocr = tokio::task::spawn_blocking(move || nomi_a11y::ocr_screenshot(&img, &langs))
                .await
                .unwrap_or_else(|e| Err(A11yError::Backend(format!("OCR task failed: {e}"))));
            if let Ok(lines) = ocr {
                for line in lines {
                    let (lcx, lcy) = line.bounds.center();
                    // Skip text already covered by an accessibility element.
                    let covered = cached.iter().any(|c| {
                        let b = c.display.bounds;
                        matches!(c.target, CachedTarget::Ax { .. })
                            && lcx >= b.x
                            && lcx <= b.x + b.w
                            && lcy >= b.y
                            && lcy <= b.y + b.h
                    });
                    if covered {
                        continue;
                    }
                    next_ref += 1;
                    let (sx, sy) = self.to_screen(session_id, lcx as i32, lcy as i32);
                    ocr_appendix.push(format!("[{next_ref}] text {:?}", line.text));
                    cached.push(CachedEntry {
                        display: ElementEntry {
                            r#ref: next_ref,
                            role: "text".to_string(),
                            name: Some(line.text),
                            value: None,
                            states: Vec::new(),
                            bounds: line.bounds,
                            source: Source::Ocr,
                        },
                        target: CachedTarget::Pixel {
                            screen_center: (sx, sy),
                        },
                    });
                }
            }
        }

        let display_entries: Vec<ElementEntry> = cached.iter().map(|c| c.display.clone()).collect();
        // Display the engine's hierarchical semantic tree; append OCR targets.
        let ocr_count = ocr_appendix.len();
        let mut text = snap.text.clone();
        if !ocr_appendix.is_empty() {
            text.push_str("\n\nAdditional on-screen text (OCR — click by [ref]):\n");
            text.push_str(&ocr_appendix.join("\n"));
        }
        let ocr_note = if ocr_count > 0 {
            format!(" (+{ocr_count} via OCR for a11y-thin content)")
        } else {
            String::new()
        };
        let header = format!(
            "Accessibility snapshot (gen {}, mode {}): {} element(s){app_note}{ax_note}{ocr_note}\
             {scope_note} The tree below is desktop → window → controls; act on a control with \
             the `click_element` action and its [ref]. Re-run `observe` after any UI change (a \
             [ref] is only valid for the latest snapshot).\n\n{text}",
            snap.generation.0,
            mode.as_str(),
            display_entries.len()
        );

        self.set_last_snapshot(session_id, SnapshotCache {
            generation: snap.generation,
            entries: cached,
        });

        tracing::debug!(
            mode = mode.as_str(),
            entries = display_entries.len(),
            ocr_entries = ocr_count,
            a11y_ms,
            capture_ms,
            total_ms = started.elapsed().as_millis(),
            "Computer observe completed"
        );

        if !mode.should_attach_image(self.image_input) {
            return ToolResult::text(header);
        }
        nomi_a11y::overlay::draw_set_of_marks(&mut shot.image, &display_entries);
        match encode_png(&shot.image) {
            Ok(encoded) => {
                let mut geometry = shot.geometry;
                geometry.img_w = encoded.width;
                geometry.img_h = encoded.height;
                self.set_last_capture(session_id, geometry);
                ToolResult::text(header).with_images(vec![encoded.image])
            }
            Err(_) => ToolResult::text(header),
        }
    }

    /// Look up a `[ref]` in the latest snapshot, returning its generation and a
    /// clone of the cached entry (with its action target).
    fn resolve_ref(&self, session_id: &str, r: u32) -> Result<(SnapshotGen, CachedEntry), String> {
        self.with_session_state(session_id, |state| match state.last_snapshot.as_ref() {
            Some(cache) => match cache.entries.iter().find(|c| c.display.r#ref == r) {
                Some(c) => Ok((cache.generation, c.clone())),
                None => Err(format!(
                    "No element [{r}] in the latest snapshot. Run `observe` and use a ref it lists."
                )),
            },
            None => Err("No accessibility snapshot yet. Run the `observe` action first.".to_string()),
        })
    }

    fn resolve_action_target(
        &self,
        session_id: &str,
        input: &Value,
    ) -> Result<(SnapshotGen, CachedEntry), ToolResult> {
        let ref_value = match input.get("ref") {
            None | Some(Value::Null) => None,
            Some(value) => match value.as_u64() {
                Some(value) if value <= u64::from(u32::MAX) => Some(value as u32),
                _ => {
                    return Err(structured_error(
                        "INVALID_REF",
                        "`ref` must be an element number from the latest observe snapshot.",
                        "reobserve",
                        true,
                    ));
                }
            },
        };
        let selector = match selector_from_input(input) {
            Ok(selector) => selector,
            Err(message) => {
                return Err(structured_error("INVALID_SELECTOR", message, "retry", false));
            }
        };
        if ref_value.is_some() && selector.is_some() {
            return Err(structured_error(
                "AMBIGUOUS_TARGET",
                "Provide either `ref` or selector fields, not both.",
                "retry",
                false,
            ));
        }
        if let Some(r#ref) = ref_value {
            return self.resolve_ref(session_id, r#ref).map_err(|message| {
                let code = if message.contains("No accessibility snapshot yet") {
                    "NO_SNAPSHOT"
                } else {
                    "STALE_REF"
                };
                structured_error(code, message, "reobserve", true)
            });
        }
        let Some(selector) = selector else {
            return Err(structured_error(
                "MISSING_TARGET",
                "Provide `ref`, or a selector such as `role` + `name`.",
                "retry",
                false,
            ));
        };
        self.resolve_selector(session_id, &selector).map_err(|message| {
            let code = if message.starts_with("Ambiguous") {
                "AMBIGUOUS_TARGET"
            } else if message.contains("No accessibility snapshot") {
                "NO_SNAPSHOT"
            } else {
                "TARGET_NOT_FOUND"
            };
            structured_error(code, message, "reobserve", true)
        })
    }

    fn resolve_selector(
        &self,
        session_id: &str,
        selector: &ElementSelector,
    ) -> Result<(SnapshotGen, CachedEntry), String> {
        self.with_session_state(session_id, |state| {
            let Some(cache) = state.last_snapshot.as_ref() else {
                return Err("No accessibility snapshot yet. Run the `observe` action first.".to_string());
            };
            let mut matches: Vec<&CachedEntry> = cache
                .entries
                .iter()
                .filter(|entry| selector.matches(&entry.display))
                .collect();
            if matches.is_empty() {
                return Err(format!(
                    "No element matched selector role={:?} name={:?} contains={:?} source={:?}.",
                    selector.role, selector.name, selector.name_contains, selector.source
                ));
            }
            let index = selector.nth.unwrap_or(1);
            if index == 0 {
                return Err("Selector `nth` is 1-based and must be at least 1.".to_string());
            }
            if matches.len() > 1 && selector.nth.is_none() {
                let candidates = matches
                    .iter()
                    .take(8)
                    .map(|entry| {
                        format!(
                            "[{}] {} {:?}",
                            entry.display.r#ref,
                            entry.display.role,
                            entry.display.name.as_deref().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "Ambiguous selector matched {} elements: {candidates}. Add `nth` (1-based) or a more specific selector.",
                    matches.len()
                ));
            }
            if index > matches.len() {
                return Err(format!(
                    "Selector matched {} element(s), but `nth`={index} was requested.",
                    matches.len()
                ));
            }
            matches.sort_by_key(|entry| entry.display.r#ref);
            Ok((cache.generation, matches[index - 1].clone()))
        })
    }

    /// Act on an element by its `[ref]` from the latest `observe` snapshot.
    /// Accessibility elements use AXPress with an automatic pixel-click
    /// fallback; OCR/pixel-only elements click their center directly.
    async fn do_click_element(&self, session_id: &str, input: &Value) -> ToolResult {
        let (generation, entry) = match self.resolve_action_target(session_id, input) {
            Ok(v) => v,
            Err(error) => return error,
        };
        let r = entry.display.r#ref;

        match entry.target {
            CachedTarget::Ax {
                engine_ref,
                screen_center,
            } => {
                let engine = match self.engine() {
                    Ok(e) => e,
                    Err(msg) => {
                        return ToolResult::error(format!("Accessibility engine unavailable: {msg}"));
                    }
                };
                let eng = engine.clone();
                let result = tokio::task::spawn_blocking(move || {
                    eng.invoke(&Target::Ref(engine_ref), generation, ElementAction::LeftClick)
                })
                .await
                .unwrap_or_else(|e| Err(A11yError::Backend(format!("invoke task failed: {e}"))));
                match result {
                    Ok(eff) => action_effect_result(&eff),
                    Err(e) => {
                        if let Some((code, retry, safe_to_retry)) = blocking_a11y_error(&e) {
                            return structured_error(code, e.to_string(), retry, safe_to_retry);
                        }
                        let (sx, sy) = screen_center;
                        match input::click(sx, sy, enigo::Button::Left, 1).await {
                            Ok(()) => ToolResult::text(format!(
                                "The accessibility action on [{r}] did not succeed ({e}); fell \
                                 back to a pixel click at the element center. Re-run `observe` to verify."
                            )),
                            Err(pe) => structured_error(
                                "ACTION_FAILED",
                                format!(
                                    "Accessibility action on [{r}] failed ({e}) and the pixel fallback also failed: {pe}"
                                ),
                                "reobserve",
                                true,
                            ),
                        }
                    }
                }
            }
            CachedTarget::Pixel { screen_center } => {
                let (sx, sy) = screen_center;
                match input::click(sx, sy, enigo::Button::Left, 1).await {
                    Ok(()) => ToolResult::text(format!(
                        "Clicked element [{r}] (OCR/pixel target) at its center. Run `observe` to verify."
                    )),
                    Err(pe) => ToolResult::error(format!("Click on [{r}] failed: {pe}")),
                }
            }
        }
    }

    /// Set the text value of an element by `[ref]`. Accessibility elements use
    /// AXValue with a focus-then-type fallback; OCR/pixel-only elements click
    /// then type.
    async fn do_set_element_value(&self, session_id: &str, input: &Value) -> ToolResult {
        let Some(text) = input.get("text").and_then(|v| v.as_str()) else {
            return ToolResult::error("Missing required parameter `text` for set_element_value.");
        };
        let text = text.to_string();
        let (generation, entry) = match self.resolve_action_target(session_id, input) {
            Ok(v) => v,
            Err(error) => return error,
        };
        let r = entry.display.r#ref;

        // For accessibility elements, try the semantic set-value first.
        if let CachedTarget::Ax { engine_ref, .. } = entry.target {
            let engine = match self.engine() {
                Ok(e) => e,
                Err(msg) => {
                    return ToolResult::error(format!("Accessibility engine unavailable: {msg}"));
                }
            };
            let eng = engine.clone();
            let value = text.clone();
            let result = tokio::task::spawn_blocking(move || {
                eng.invoke(
                    &Target::Ref(engine_ref),
                    generation,
                    ElementAction::SetValue(value),
                )
            })
            .await
            .unwrap_or_else(|e| Err(A11yError::Backend(format!("invoke task failed: {e}"))));
            match result {
                Ok(eff) => return action_effect_result(&eff),
                Err(error) => {
                    if let Some((code, retry, safe_to_retry)) = blocking_a11y_error(&error) {
                        return structured_error(
                            code,
                            error.to_string(),
                            retry,
                            safe_to_retry,
                        );
                    }
                }
            }
        }

        // Fallback (OCR/pixel target, or AX set-value did not take): click the
        // element to focus it, then type.
        let (sx, sy) = match entry.target {
            CachedTarget::Ax { screen_center, .. } => screen_center,
            CachedTarget::Pixel { screen_center } => screen_center,
        };
        if let Err(pe) = input::click(sx, sy, enigo::Button::Left, 1).await {
            return ToolResult::error(format!(
                "Could not focus element [{r}] to type into it: {pe}"
            ));
        }
        match input::type_text(text).await {
            Ok(()) => ToolResult::text(format!(
                "Set element [{r}] by clicking the field and typing the text. Run `observe` to verify."
            )),
            Err(te) => ToolResult::error(format!("Typing into element [{r}] failed: {te}")),
        }
    }

    /// Perform a pixel mouse gesture (right-click / double-click) on the element
    /// addressed by `[ref]`. These are pointer gestures with no UIA semantic
    /// equivalent, so they click the element's center directly.
    async fn do_element_gesture(
        &self,
        session_id: &str,
        input: &Value,
        button: enigo::Button,
        count: u32,
        verb: &str,
    ) -> ToolResult {
        let entry = match self.resolve_action_target(session_id, input) {
            Ok((_, entry)) => entry,
            Err(error) => return error,
        };
        let r = entry.display.r#ref;
        let (sx, sy) = match entry.target {
            CachedTarget::Ax { screen_center, .. } => screen_center,
            CachedTarget::Pixel { screen_center } => screen_center,
        };
        match input::click(sx, sy, button, count).await {
            Ok(()) => ToolResult::text(format!(
                "Performed {verb} on element [{r}] at its center. Run `observe` (or take a \
                 screenshot) to verify the result."
            )),
            Err(e) => ToolResult::error(format!("{verb} on element [{r}] failed: {e}")),
        }
    }

    /// Reliably open an application, file, or folder via the OS shell
    /// (ShellExecute on Windows). The dependable way to launch things — never
    /// shell out to `cmd /c start` / `Start-Process`, which fail and pop a
    /// "Windows cannot find" dialog on this host. Web URLs fail closed toward
    /// the managed Browser tool (see `launch::validate_agent_web_target`).
    async fn do_launch(&self, input: &Value) -> ToolResult {
        let Some(target) = input.get("target").and_then(|v| v.as_str()) else {
            return ToolResult::error(
                "Missing required parameter `target` for launch (a file/folder path, or an \
                 application name like \"notepad\"). Web URLs are opened with the managed \
                 Browser tool, not launch.",
            );
        };
        let app = input.get("app").and_then(|v| v.as_str());
        match crate::launch::launch(target, app).await {
            Ok(msg) => ToolResult::text(format!(
                "{msg} Take a screenshot or run `observe` to see the result."
            )),
            Err(e) => ToolResult::error(e),
        }
    }

    /// Map model-provided screenshot coordinates to absolute screen
    /// coordinates. Identity when no screenshot has been taken yet.
    fn to_screen(&self, session_id: &str, x: i32, y: i32) -> (i32, i32) {
        match self.last_capture(session_id) {
            Some(g) => {
                let (lx, ly) = map_llm_coord(x, y, g.img_w, g.img_h, g.logical_w, g.logical_h);
                (g.origin_x + lx, g.origin_y + ly)
            }
            None => (x, y),
        }
    }

    /// Map an absolute screen coordinate into the most recent screenshot's
    /// pixel space (for reporting the cursor to the model).
    fn to_image(&self, session_id: &str, x: i32, y: i32) -> (i32, i32) {
        match self.last_capture(session_id) {
            Some(g) => map_screen_coord(
                x - g.origin_x,
                y - g.origin_y,
                g.img_w,
                g.img_h,
                g.logical_w,
                g.logical_h,
            ),
            None => (x, y),
        }
    }

    async fn do_screenshot(&self, session_id: &str, input: &Value) -> ToolResult {
        let display = match input.get("display") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_u64() {
                Some(d) => Some(d as usize),
                None => {
                    return ToolResult::error(
                        "Parameter `display` must be a non-negative integer display index.",
                    );
                }
            },
        };

        let max_edge = self.max_screenshot_edge;
        let captured = tokio::task::spawn_blocking(move || capture_screen(display, max_edge))
            .await
            .unwrap_or_else(|e| Err(format!("Screenshot task failed: {e}")));

        match captured {
            Ok(shot) => {
                match encode_png(&shot.image) {
                    Ok(encoded) => {
                        let mut geometry = shot.geometry;
                        geometry.img_w = encoded.width;
                        geometry.img_h = encoded.height;
                        self.set_last_capture(session_id, geometry);
                        let text = format!(
                            "Screenshot captured: {}x{} (display {}, scaled from {}x{} physical). \
                             Coordinates you provide will be mapped back to the screen automatically.",
                            encoded.width,
                            encoded.height,
                            shot.display_index,
                            shot.physical_w,
                            shot.physical_h
                        );
                        ToolResult::text(text).with_images(vec![encoded.image])
                    }
                    Err(e) => ToolResult::error(e),
                }
            }
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_cursor_position(&self, session_id: &str) -> ToolResult {
        match input::cursor_position().await {
            Ok((sx, sy)) => {
                let has_capture = self.last_capture(session_id).is_some();
                if has_capture {
                    let (ix, iy) = self.to_image(session_id, sx, sy);
                    ToolResult::text(format!(
                        "Cursor position: ({ix}, {iy}) in screenshot coordinates \
                         (screen: ({sx}, {sy}))."
                    ))
                } else {
                    ToolResult::text(format!(
                        "Cursor position: ({sx}, {sy}) in screen coordinates \
                         (no screenshot taken yet)."
                    ))
                }
            }
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_list_windows(&self) -> ToolResult {
        let listed = tokio::task::spawn_blocking(fallback_backend::list_windows)
            .await
            .unwrap_or_else(|e| Err(format!("Window listing task failed: {e}")));
        match listed {
            Ok(list) => ToolResult::text(fallback_backend::format_window_list(&list)),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_click(&self, session_id: &str, input: &Value, button: enigo::Button, count: u32) -> ToolResult {
        let (x, y) = match require_xy(input, "x", "y") {
            Ok(xy) => xy,
            Err(e) => return ToolResult::error(e),
        };
        let (sx, sy) = self.to_screen(session_id, x, y);
        match input::click(sx, sy, button, count).await {
            Ok(()) => ToolResult::text(format!(
                "Clicked at ({x}, {y}) (screen ({sx}, {sy})). Take a screenshot to verify the \
                 result."
            )),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_mouse_move(&self, session_id: &str, input: &Value) -> ToolResult {
        let (x, y) = match require_xy(input, "x", "y") {
            Ok(xy) => xy,
            Err(e) => return ToolResult::error(e),
        };
        let (sx, sy) = self.to_screen(session_id, x, y);
        match input::mouse_move(sx, sy).await {
            Ok(()) => ToolResult::text(format!("Moved cursor to ({x}, {y}) (screen ({sx}, {sy}))." )),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_drag(&self, session_id: &str, input: &Value) -> ToolResult {
        let (start_x, start_y) = match require_xy(input, "start_x", "start_y") {
            Ok(xy) => xy,
            Err(e) => return ToolResult::error(e),
        };
        let (end_x, end_y) = match require_xy(input, "end_x", "end_y") {
            Ok(xy) => xy,
            Err(e) => return ToolResult::error(e),
        };
        let (sx, sy) = self.to_screen(session_id, start_x, start_y);
        let (ex, ey) = self.to_screen(session_id, end_x, end_y);
        match input::drag(sx, sy, ex, ey).await {
            Ok(()) => ToolResult::text(format!(
                "Dragged from ({start_x}, {start_y}) to ({end_x}, {end_y}). Take a screenshot \
                 to verify the result."
            )),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_type(&self, input: &Value) -> ToolResult {
        let Some(text) = input.get("text").and_then(|v| v.as_str()) else {
            return ToolResult::error(
                "Missing required parameter `text` for the type action.",
            );
        };
        let char_count = text.chars().count();
        match input::type_text(text.to_string()).await {
            Ok(()) => ToolResult::text(format!("Typed {char_count} character(s).")),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_key(&self, input: &Value) -> ToolResult {
        let Some(combo) = input.get("key").and_then(|v| v.as_str()) else {
            return ToolResult::error(format!(
                "Missing required parameter `key` for the key action, e.g. \"enter\" or \
                 \"{KEY_COMBO_EXAMPLE}\"."
            ));
        };
        let keys = match parse_key_combo(combo) {
            Ok(keys) => keys,
            Err(e) => return ToolResult::error(e),
        };
        match input::key_combo(keys).await {
            Ok(()) => ToolResult::text(format!("Pressed {combo:?}.")),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_scroll(&self, session_id: &str, input: &Value) -> ToolResult {
        let Some(direction_str) = input.get("direction").and_then(|v| v.as_str()) else {
            return ToolResult::error(
                "Missing required parameter `direction` for the scroll action \
                 (up, down, left or right).",
            );
        };
        let direction = match ScrollDirection::parse(direction_str) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(e),
        };
        let amount = input
            .get("amount")
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_SCROLL_AMOUNT)
            .clamp(1, 100) as i32;
        let at = match (
            input.get("x").and_then(|v| v.as_i64()),
            input.get("y").and_then(|v| v.as_i64()),
        ) {
            (Some(x), Some(y)) => Some(self.to_screen(session_id, x as i32, y as i32)),
            _ => None,
        };
        match input::scroll(at, direction, amount).await {
            Ok(()) => ToolResult::text(format!(
                "Scrolled {direction_str} by {amount}. Take a screenshot to see the result."
            )),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_focus_window(&self, input: &Value) -> ToolResult {
        let Some(window_id) = input.get("window_id").and_then(|v| v.as_u64()) else {
            return ToolResult::error(
                "Missing required parameter `window_id` for the focus_window action. \
                 Use list_windows to get window ids.",
            );
        };
        match fallback_backend::focus_window(window_id as u32).await {
            Ok(msg) => ToolResult::text(msg),
            Err(e) => ToolResult::error(e),
        }
    }

    async fn do_wait(&self, input: &Value) -> ToolResult {
        let requested = input
            .get("seconds")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        let seconds = requested.clamp(0.0, MAX_WAIT_SECONDS);
        tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
        ToolResult::text(format!("Waited {seconds} second(s)."))
    }

    async fn do_wait_for(&self, session_id: &str, input: &Value) -> ToolResult {
        let condition = match WaitCondition::parse(input.get("condition")) {
            Ok(condition) => condition,
            Err(error) => return structured_error("INVALID_ARGUMENT", error, "retry", false),
        };
        let selector = match selector_from_input(input) {
            Ok(selector) => selector,
            Err(message) => {
                return structured_error("INVALID_SELECTOR", message, "retry", false);
            }
        };
        if matches!(
            condition,
            WaitCondition::ElementPresent | WaitCondition::ElementAbsent
        ) && selector.is_none()
        {
            return structured_error(
                "MISSING_TARGET",
                "Element wait conditions require selector fields such as `role` + `name`.",
                "retry",
                false,
            );
        }
        let text = input.get("text").and_then(Value::as_str).map(str::to_string);
        if matches!(condition, WaitCondition::TextPresent | WaitCondition::TextAbsent)
            && text.as_deref().unwrap_or_default().is_empty()
        {
            return structured_error(
                "MISSING_TEXT",
                "Text wait conditions require a non-empty `text`.",
                "retry",
                false,
            );
        }
        let timeout_ms = input
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_WAIT_FOR_MS)
            .clamp(100, MAX_WAIT_FOR_MS);
        let interval_ms = input
            .get("interval_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_WAIT_INTERVAL_MS)
            .clamp(50, 2_000);
        let request = match observe_opts_from_input(input).await {
            Ok(request) => request,
            Err(error) => return structured_error("INVALID_ARGUMENT", error, "retry", false),
        };
        let engine = match self.engine() {
            Ok(engine) => engine,
            Err(error) => {
                return structured_error("ACCESSIBILITY_UNAVAILABLE", error, "retry", true);
            }
        };

        let started = Instant::now();
        let deadline = started + Duration::from_millis(timeout_ms);
        let mut baseline: Option<String> = None;
        let mut polls = 0_u32;
        loop {
            polls += 1;
            let eng = engine.clone();
            let opts = request.opts.clone();
            let observed = tokio::task::spawn_blocking(move || eng.observe(&opts))
                .await
                .unwrap_or_else(|error| {
                    Err(A11yError::Backend(format!("wait_for observe task failed: {error}")))
                });
            let snapshot = match observed {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return structured_error(
                        "ACCESSIBILITY_OBSERVE_FAILED",
                        error.to_string(),
                        "retry",
                        true,
                    );
                }
            };
            let (cached, _) = cached_ax_entries(&snapshot);
            self.set_last_snapshot(session_id, SnapshotCache {
                generation: snapshot.generation,
                entries: cached,
            });

            let matched_elements = selector.as_ref().map(|selector| {
                snapshot
                    .entries
                    .iter()
                    .filter(|entry| selector.matches(entry))
                    .count()
            });
            let text_matches = text.as_ref().map(|needle| {
                snapshot
                    .text
                    .to_lowercase()
                    .contains(&needle.to_lowercase())
            });
            let fingerprint = snapshot_fingerprint(&snapshot);
            let satisfied = match condition {
                WaitCondition::ElementPresent => matched_elements.unwrap_or(0) > 0,
                WaitCondition::ElementAbsent => matched_elements.unwrap_or(0) == 0,
                WaitCondition::TextPresent => text_matches.unwrap_or(false),
                WaitCondition::TextAbsent => !text_matches.unwrap_or(false),
                WaitCondition::Changed => baseline
                    .as_ref()
                    .is_some_and(|baseline| baseline != &fingerprint),
            };
            if condition == WaitCondition::Changed && baseline.is_none() {
                baseline = Some(fingerprint);
            }
            if satisfied {
                let elapsed = started.elapsed().as_millis();
                return ToolResult::text(format!(
                    "Wait condition `{}` met after {elapsed} ms ({polls} poll(s)).\n\n{}",
                    condition.as_str(),
                    snapshot.text
                ));
            }
            if Instant::now() >= deadline {
                let elapsed = started.elapsed().as_millis();
                let tail = tail_lines(&snapshot.text, 40);
                return structured_error(
                    "WAIT_TIMEOUT",
                    format!(
                        "Condition `{}` was not met within {timeout_ms} ms ({polls} poll(s), {elapsed} ms elapsed).\nLast observed tree:\n{tail}",
                        condition.as_str()
                    ),
                    "reobserve",
                    true,
                );
            }
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    }

    /// Execute an action against an isolated session state (latest snapshot +
    /// screenshot geometry). Shared hosts use this to keep parallel agents from
    /// resolving refs against each other's observations.
    pub async fn execute_for_session(&self, session_id: &str, input: Value) -> ToolResult {
        let Some(action) = input.get("action").and_then(|v| v.as_str()) else {
            return ToolResult::error(
                "Missing required parameter `action`. See the tool description for the supported actions.",
            );
        };

        tracing::debug!(action = %action, session = %session_id, "ComputerTool executing");

        match action {
            "observe" => self.do_observe(session_id, &input).await,
            "click_element" => self.do_click_element(session_id, &input).await,
            "set_element_value" => self.do_set_element_value(session_id, &input).await,
            "right_click_element" => {
                self.do_element_gesture(session_id, &input, enigo::Button::Right, 1, "right-click").await
            }
            "double_click_element" => {
                self.do_element_gesture(session_id, &input, enigo::Button::Left, 2, "double-click").await
            }
            "launch" => self.do_launch(&input).await,
            "screenshot" if !self.image_input => {
                let mut observe_input = input.clone();
                observe_input["mode"] = Value::String("text".to_string());
                self.do_observe(session_id, &observe_input).await
            },
            "screenshot" => self.do_screenshot(session_id, &input).await,
            "cursor_position" => self.do_cursor_position(session_id).await,
            "list_windows" => self.do_list_windows().await,
            "left_click" => self.do_click(session_id, &input, enigo::Button::Left, 1).await,
            "right_click" => self.do_click(session_id, &input, enigo::Button::Right, 1).await,
            "middle_click" => self.do_click(session_id, &input, enigo::Button::Middle, 1).await,
            "double_click" => self.do_click(session_id, &input, enigo::Button::Left, 2).await,
            "triple_click" => self.do_click(session_id, &input, enigo::Button::Left, 3).await,
            "mouse_move" => self.do_mouse_move(session_id, &input).await,
            "left_click_drag" => self.do_drag(session_id, &input).await,
            "type" => self.do_type(&input).await,
            "key" => self.do_key(&input).await,
            "scroll" => self.do_scroll(session_id, &input).await,
            "focus_window" => self.do_focus_window(&input).await,
            "wait" => self.do_wait(&input).await,
            "wait_for" => self.do_wait_for(session_id, &input).await,
            other => ToolResult::error(format!(
                "Unknown action {other:?}. Supported actions: observe, click_element, set_element_value, right_click_element, double_click_element, launch, screenshot, cursor_position, list_windows, left_click, right_click, middle_click, double_click, triple_click, mouse_move, left_click_drag, type, key, scroll, focus_window, wait, wait_for."
            )),
        }
    }
}

/// A session capability note appended to the tool description so the model
/// knows its real abilities up front (a11y availability + the two macOS TCC
/// grants). Computed once at construction from the platform + a live permission
/// probe — it lives in the (cacheable) tool schema, not the system prompt, so it
/// never thrashes the prompt cache.
fn capabilities_note() -> String {
    let mut s = String::from("\n\nThis session's desktop capabilities:\n");
    #[cfg(target_os = "macos")]
    {
        s.push_str(
            "- Accessibility-first targeting (observe / click_element / set_element_value): \
             available, with OCR fusion for accessibility-thin apps.\n",
        );
    }
    #[cfg(target_os = "windows")]
    {
        s.push_str(
            "- Accessibility-first targeting (observe / click_element / set_element_value): \
             available (UI Automation), with OCR fusion for accessibility-thin apps.\n",
        );
    }
    #[cfg(target_os = "linux")]
    {
        s.push_str(
            "- Accessibility-first targeting (observe / click_element / set_element_value): \
             available (AT-SPI).\n",
        );
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        s.push_str(
            "- Accessibility-first targeting (observe): not yet available on this OS — use the \
             pixel actions (screenshot + click x,y).\n",
        );
    }
    let status = crate::permissions::permission_status();
    let fmt = |g: Option<bool>| match g {
        Some(true) => "granted",
        Some(false) => "NOT granted",
        None => "not applicable on this OS",
    };
    s.push_str(&format!(
        "- Accessibility permission (needed for `observe` and input synthesis): {}.\n",
        fmt(status.accessibility)
    ));
    s.push_str(&format!(
        "- Screen Recording permission (needed for screenshots and the Set-of-Marks overlay; \
         `observe`'s element list works without it): {}.\n",
        fmt(status.screen_recording)
    ));
    s
}

fn cached_ax_entries(snapshot: &nomi_a11y::Snapshot) -> (Vec<CachedEntry>, usize) {
    let mut cached = Vec::with_capacity(snapshot.entries.len());
    for entry in &snapshot.entries {
        let (cx, cy) = entry.bounds.center();
        cached.push(CachedEntry {
            display: entry.clone(),
            target: CachedTarget::Ax {
                engine_ref: entry.r#ref,
                screen_center: (cx as i32, cy as i32),
            },
        });
    }
    let count = cached.len();
    (cached, count)
}

fn snapshot_fingerprint(snapshot: &nomi_a11y::Snapshot) -> String {
    let mut out = String::new();
    out.push_str(if snapshot.truncated { "1" } else { "0" });
    for entry in &snapshot.entries {
        out.push('|');
        out.push_str(&entry.role);
        out.push('\u{1f}');
        out.push_str(entry.name.as_deref().unwrap_or_default());
        out.push('\u{1f}');
        out.push_str(entry.value.as_deref().unwrap_or_default());
        out.push('\u{1f}');
        out.push_str(&entry.states.join(","));
        out.push('\u{1f}');
        out.push_str(&format!(
            "{:.0},{:.0},{:.0},{:.0}",
            entry.bounds.x, entry.bounds.y, entry.bounds.w, entry.bounds.h
        ));
    }
    out
}

fn tail_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

/// Convert an element rectangle from OS accessibility coordinates (screen
/// logical points, top-left origin) into the most recent screenshot's pixel
/// space, so the Set-of-Marks overlay aligns with the captured image. This is
/// the AX-points→pixel conversion (per-monitor origin + logical→pixel scale);
/// it is NOT a plain reuse of the LLM-coordinate mapping.
fn ax_rect_to_pixel(r: nomi_a11y::Rect, g: &CaptureGeometry) -> nomi_a11y::Rect {
    let sx = g.img_w as f64 / g.logical_w.max(1) as f64;
    let sy = g.img_h as f64 / g.logical_h.max(1) as f64;
    nomi_a11y::Rect {
        x: (r.x - g.origin_x as f64) * sx,
        y: (r.y - g.origin_y as f64) * sy,
        w: r.w * sx,
        h: r.h * sy,
    }
}

fn action_effect_result(effect: &nomi_a11y::Effect) -> ToolResult {
    let envelope = json!({
        "ok": true,
        "changed": effect.changed,
        "needs_reobserve": true,
    });
    ToolResult::text(format!("{envelope}\n{}", effect.message))
}

fn blocking_a11y_error(error: &A11yError) -> Option<(&'static str, &'static str, bool)> {
    match error {
        A11yError::Stale(_) => Some(("STALE_REF", "reobserve", true)),
        A11yError::NotFound(_) => Some(("ELEMENT_NOT_FOUND", "reobserve", true)),
        A11yError::Permission(_) => Some(("PERMISSION_REQUIRED", "permission", false)),
        A11yError::Unsupported { .. } => None,
        A11yError::Backend(message) => {
            let lower = message.to_lowercase();
            let disconnected = [
                "stale",
                "disconnected",
                "not found",
                "no longer exists",
                "invalid handle",
                "找不到成员",
                "元素不可用",
                "已断开",
            ]
            .iter()
            .any(|needle| lower.contains(needle));
            disconnected.then_some(("STALE_REF", "reobserve", true))
        }
    }
}

fn selector_from_input(input: &Value) -> Result<Option<ElementSelector>, String> {
    let role = input.get("role").and_then(Value::as_str).map(str::to_string);
    let name = input.get("name").and_then(Value::as_str).map(str::to_string);
    let name_contains = input
        .get("name_contains")
        .and_then(Value::as_str)
        .map(str::to_string);
    let source = match input.get("source").and_then(Value::as_str) {
        None => None,
        Some("a11y") => Some(Source::A11y),
        Some("ocr") => Some(Source::Ocr),
        Some("vision") => Some(Source::Vision),
        Some(other) => {
            return Err(format!(
                "Unknown selector source {other:?}; use a11y, ocr, or vision."
            ));
        }
    };
    let nth = match input.get("nth") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_u64() {
            Some(value) if value >= 1 && value <= usize::MAX as u64 => Some(value as usize),
            _ => return Err("Selector `nth` must be a 1-based positive integer.".to_string()),
        },
    };
    let selector = ElementSelector {
        role,
        name,
        name_contains,
        source,
        nth,
    };
    Ok((!selector.is_empty()).then_some(selector))
}

fn structured_error(
    code: &str,
    message: impl Into<String>,
    retry: &str,
    safe_to_retry: bool,
) -> ToolResult {
    let message = message.into();
    let envelope = json!({
        "ok": false,
        "error": {
            "code": code,
            "message": message,
            "retry": retry,
            "safe_to_retry": safe_to_retry,
        }
    });
    ToolResult::error(format!("{envelope}\n{message}"))
}

struct ObserveRequest {
    opts: ObserveOpts,
    window: Option<fallback_backend::WindowInfo>,
}

fn capture_region_for_window(window: &fallback_backend::WindowInfo) -> Option<CaptureRegion> {
    (window.width > 0 && window.height > 0).then_some(CaptureRegion {
        x: window.x,
        y: window.y,
        width: window.width,
        height: window.height,
    })
}

async fn resolve_capture_window(
    explicit: Option<fallback_backend::WindowInfo>,
) -> Option<fallback_backend::WindowInfo> {
    if explicit.is_some() {
        return explicit;
    }
    tokio::task::spawn_blocking(fallback_backend::foreground_window)
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
}

fn spawn_capture(
    window: Option<fallback_backend::WindowInfo>,
    max_edge: u32,
) -> tokio::task::JoinHandle<Result<CapturedScreen, String>> {
    tokio::task::spawn_blocking(move || {
        let region = window.as_ref().and_then(capture_region_for_window);
        capture_screen_region(None, region, max_edge)
    })
}

/// Resolve `observe` targeting. A `window_id` is looked up through xcap and
/// mapped to `ObserveOpts` without raising the window. Invalid ids fail here
/// so the a11y engine is not started for a stale handle.
fn optional_bounded_usize(
    input: &Value,
    name: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    match input.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => {
            let Some(raw) = value.as_u64() else {
                return Err(format!("Parameter `{name}` must be a positive integer."));
            };
            Ok((raw as usize).clamp(min, max))
        }
    }
}

async fn observe_opts_from_input(input: &Value) -> Result<ObserveRequest, String> {
    let defaults = ObserveOpts::default();
    let node_budget = optional_bounded_usize(input, "node_budget", defaults.node_budget, 20, 1_000)?;
    let max_depth = optional_bounded_usize(input, "max_depth", defaults.max_depth, 1, 64)?;
    let window_id = match input.get("window_id") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let Some(id) = value.as_u64() else {
                return Err(
                    "Parameter `window_id` must be an integer from list_windows.".to_string(),
                );
            };
            if id > u64::from(u32::MAX) {
                return Err(
                    "Parameter `window_id` is out of range. Use list_windows to get current ids."
                        .to_string(),
                );
            }
            Some(id as u32)
        }
    };
    let Some(window_id) = window_id else {
        return Ok(ObserveRequest {
            opts: ObserveOpts {
                node_budget,
                max_depth,
                ..defaults
            },
            window: None,
        });
    };
    let info = tokio::task::spawn_blocking(move || fallback_backend::find_window(window_id))
        .await
        .map_err(|e| format!("Window lookup task failed: {e}"))??;
    Ok(ObserveRequest {
        opts: ObserveOpts {
            pid: info.pid,
            window_id: Some(window_id),
            node_budget,
            max_depth,
        },
        window: Some(info),
    })
}

/// Extract a required (x, y)-style coordinate pair, naming the missing
/// parameters in the error.
fn require_xy(input: &Value, x_name: &str, y_name: &str) -> Result<(i32, i32), String> {
    let x = input.get(x_name).and_then(|v| v.as_i64());
    let y = input.get(y_name).and_then(|v| v.as_i64());
    match (x, y) {
        (Some(x), Some(y)) => Ok((x as i32, y as i32)),
        (None, Some(_)) => Err(format!("Missing required parameter `{x_name}`.")),
        (Some(_), None) => Err(format!("Missing required parameter `{y_name}`.")),
        (None, None) => Err(format!(
            "Missing required parameters `{x_name}` and `{y_name}`."
        )),
    }
}

#[async_trait]
impl Tool for ComputerTool {
    fn name(&self) -> &str {
        "Computer"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "observe", "click_element", "set_element_value",
                        "right_click_element", "double_click_element",
                        "launch",
                        "screenshot", "cursor_position", "list_windows",
                        "left_click", "right_click", "middle_click",
                        "double_click", "triple_click", "mouse_move",
                        "left_click_drag", "type", "key", "scroll",
                        "focus_window", "wait", "wait_for"
                    ],
                    "description": "The desktop operation to perform"
                },
                "ref": { "type": "integer", "description": "Element number from the latest `observe` snapshot. Use this or selector fields (role/name/name_contains/source/nth), not both." },
                "role": { "type": "string", "description": "Semantic selector: exact accessibility role, e.g. button, edit, text, list_item." },
                "name": { "type": "string", "description": "Semantic selector: exact case-insensitive accessible name or value." },
                "name_contains": { "type": "string", "description": "Semantic selector: case-insensitive substring of accessible name/value." },
                "source": { "type": "string", "enum": ["a11y", "ocr", "vision"], "description": "Semantic selector: restrict to one observation source." },
                "nth": { "type": "integer", "minimum": 1, "description": "Semantic selector: choose the 1-based matching element when several match." },
                "target": { "type": "string", "description": "What to open (launch action): a file/folder path or an application name (e.g. \"notepad\"). Web URLs (http/https) are rejected — use the managed Browser tool for web pages" },
                "app": { "type": "string", "description": "Optional application to open the `target` WITH (launch action), e.g. open a file with a specific editor" },
                "x": { "type": "integer", "description": "X coordinate in pixels of the most recent screenshot" },
                "y": { "type": "integer", "description": "Y coordinate in pixels of the most recent screenshot" },
                "start_x": { "type": "integer", "description": "Drag start X (left_click_drag)" },
                "start_y": { "type": "integer", "description": "Drag start Y (left_click_drag)" },
                "end_x": { "type": "integer", "description": "Drag end X (left_click_drag)" },
                "end_y": { "type": "integer", "description": "Drag end Y (left_click_drag)" },
                "text": { "type": "string", "description": "Text to type (type action)" },
                "key": { "type": "string", "description": format!("Key or combo to press, e.g. \"enter\" or \"{KEY_COMBO_EXAMPLE}\" (key action)") },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Scroll direction (scroll action)"
                },
                "amount": { "type": "integer", "description": "Scroll wheel clicks, default 3 (scroll action)" },
                "display": { "type": "integer", "description": "Display index to capture, default primary (screenshot action)" },
                "window_id": { "type": "integer", "description": "Window id from list_windows. For observe: snapshot that window without raising it. For focus_window: bring it to the front." },
                "mode": { "type": "string", "enum": ["auto", "semantic", "visual", "text"], "description": "Observe mode: auto uses the fast semantic path for complete trees and a window screenshot otherwise; semantic returns only the tree; visual always includes screenshot + Set-of-Marks; text runs OCR and returns no image." },
                "node_budget": { "type": "integer", "minimum": 20, "maximum": 1000, "description": "Maximum interactable elements for observe (default 120; raise for complex Chrome/IDE trees)." },
                "max_depth": { "type": "integer", "minimum": 1, "maximum": 64, "description": "Maximum accessibility tree depth for observe (default 12)." },
                "seconds": { "type": "number", "description": "Seconds to wait, max 5 (wait action)" },
                "condition": { "type": "string", "enum": ["element_present", "element_absent", "text_present", "text_absent", "changed"], "description": "Condition for wait_for." },
                "timeout_ms": { "type": "integer", "minimum": 100, "maximum": 15000, "description": "Maximum wait_for duration in milliseconds (default 5000)." },
                "interval_ms": { "type": "integer", "minimum": 50, "maximum": 2000, "description": "wait_for polling interval in milliseconds (default 200)." }
            },
            "required": ["action"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        self.execute_for_session(DEFAULT_SESSION_ID, input).await
    }

    fn category(&self) -> ToolCategory {
        // Conservative default; per-action classification in category_for.
        ToolCategory::Exec
    }

    fn category_for(&self, input: &Value) -> ToolCategory {
        match input.get("action").and_then(|v| v.as_str()) {
            Some("observe" | "screenshot" | "cursor_position" | "list_windows" | "wait" | "wait_for") => {
                ToolCategory::Info
            }
            _ => ToolCategory::Exec,
        }
    }

    fn describe(&self, input: &Value) -> String {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("?");
        let detail = match action {
            "observe" => match input.get("window_id").and_then(|v| v.as_u64()) {
                Some(id) => format!("observe window {id}"),
                None => "observe (accessibility snapshot)".to_string(),
            },
            "click_element" => {
                let r = input.get("ref").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("click element [{r}]")
            }
            "set_element_value" => {
                let r = input.get("ref").and_then(|v| v.as_u64()).unwrap_or(0);
                let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("");
                format!("set element [{r}] = {:?}", nomi_tools::truncate_utf8(text, 40))
            }
            "right_click_element" | "double_click_element" => {
                let r = input.get("ref").and_then(|v| v.as_u64()).unwrap_or(0);
                let verb = if action == "right_click_element" { "right-click" } else { "double-click" };
                format!("{verb} element [{r}]")
            }
            "launch" => {
                let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
                match input.get("app").and_then(|v| v.as_str()) {
                    Some(app) => format!("launch {:?} with {app:?}", nomi_tools::truncate_utf8(target, 60)),
                    None => format!("launch {:?}", nomi_tools::truncate_utf8(target, 60)),
                }
            }
            "screenshot" => match input.get("display").and_then(|v| v.as_u64()) {
                Some(d) => format!("screenshot of display {d}"),
                None => "screenshot".to_string(),
            },
            "left_click" | "right_click" | "middle_click" | "double_click" | "triple_click"
            | "mouse_move" => {
                let x = input.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
                let y = input.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
                format!("{action} at ({x}, {y})")
            }
            "left_click_drag" => {
                let sx = input.get("start_x").and_then(|v| v.as_i64()).unwrap_or(0);
                let sy = input.get("start_y").and_then(|v| v.as_i64()).unwrap_or(0);
                let ex = input.get("end_x").and_then(|v| v.as_i64()).unwrap_or(0);
                let ey = input.get("end_y").and_then(|v| v.as_i64()).unwrap_or(0);
                format!("drag from ({sx}, {sy}) to ({ex}, {ey})")
            }
            "type" => {
                let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("");
                format!("type {:?}", nomi_tools::truncate_utf8(text, 40))
            }
            "key" => {
                let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
                format!("press {key:?}")
            }
            "scroll" => {
                let dir = input.get("direction").and_then(|v| v.as_str()).unwrap_or("?");
                let amount = input
                    .get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_SCROLL_AMOUNT);
                format!("scroll {dir} by {amount}")
            }
            "focus_window" => {
                let id = input.get("window_id").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("focus window {id}")
            }
            "wait" => {
                let secs = input.get("seconds").and_then(|v| v.as_f64()).unwrap_or(1.0);
                format!("wait {secs}s")
            }
            "wait_for" => {
                let condition = input.get("condition").and_then(Value::as_str).unwrap_or("?");
                let timeout = input.get("timeout_ms").and_then(Value::as_u64).unwrap_or(DEFAULT_WAIT_FOR_MS);
                format!("wait for {condition} (up to {timeout} ms)")
            }
            other => other.to_string(),
        };
        format!("Computer: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> ComputerTool {
        ComputerTool::new(&ComputerConfig::default())
    }

    #[test]
    fn text_only_model_is_told_observe_is_text_and_ocr_always_runs() {
        let vision = tool();
        assert!(!vision.description().contains("cannot read images"));

        let text_only = tool().with_image_input(false);
        assert!(text_only.description().contains("cannot read images"));
        let twice = tool().with_image_input(false).with_image_input(false);
        assert_eq!(twice.description().matches("cannot read images").count(), 1);

        assert!(ObserveMode::Text.needs_capture(true, 500, false));
        assert!(ObserveMode::Auto.needs_capture(true, OCR_FUSION_AX_THRESHOLD - 1, false));
        assert!(!ObserveMode::Auto.needs_capture(true, OCR_FUSION_AX_THRESHOLD, false));
        assert!(ObserveMode::Auto.needs_capture(true, 500, true));
        assert!(!ObserveMode::Semantic.should_attach_image(true));
        assert!(ObserveMode::Visual.should_attach_image(true));
    }

    // --- schema ---

    #[test]
    fn schema_is_valid_object_with_required_action() {
        let schema = tool().input_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["action"]));
        let actions = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        for expected in [
            "observe",
            "screenshot",
            "cursor_position",
            "list_windows",
            "left_click",
            "right_click",
            "middle_click",
            "double_click",
            "triple_click",
            "mouse_move",
            "left_click_drag",
            "type",
            "key",
            "scroll",
            "focus_window",
            "wait",
            "wait_for",
        ] {
            assert!(
                actions.iter().any(|a| a == expected),
                "schema enum missing {expected}"
            );
        }
        let window_id = schema["properties"]["window_id"]["description"]
            .as_str()
            .expect("window_id description");
        assert!(window_id.contains("observe"), "{window_id}");
        assert!(window_id.contains("focus_window"), "{window_id}");
        assert!(schema["properties"]["mode"]["enum"].is_array());
        assert_eq!(schema["properties"]["node_budget"]["minimum"], 20);
        assert_eq!(schema["properties"]["max_depth"]["minimum"], 1);
        // Round-trips through serde_json without loss.
        let text = serde_json::to_string(&schema).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, schema);
    }

    #[test]
    fn name_and_metadata() {
        let t = tool();
        assert_eq!(t.name(), "Computer");
        assert!(!t.description().is_empty());
        assert!(!t.is_concurrency_safe(&json!({})));
        assert_eq!(t.category(), ToolCategory::Exec);
    }

    // --- category_for ---

    #[test]
    fn category_for_info_actions() {
        let t = tool();
        for action in ["observe", "screenshot", "cursor_position", "list_windows", "wait", "wait_for"] {
            assert_eq!(
                t.category_for(&json!({"action": action})),
                ToolCategory::Info,
                "{action} should be Info"
            );
        }
    }

    #[test]
    fn click_element_is_exec_and_observe_is_info() {
        let t = tool();
        assert_eq!(
            t.category_for(&json!({"action": "click_element", "ref": 3})),
            ToolCategory::Exec
        );
        assert_eq!(
            t.category_for(&json!({"action": "observe"})),
            ToolCategory::Info
        );
    }

    #[tokio::test]
    async fn click_element_without_ref_is_error() {
        let result = tool().execute(json!({"action": "click_element"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("ref"), "{}", result.content);
    }

    #[tokio::test]
    async fn click_element_without_snapshot_is_error() {
        // No observe has run, so there is no snapshot to resolve the ref against.
        // (On macOS the engine initializes; the error is about the missing
        // snapshot, surfaced as a non-panicking ToolResult error.)
        let result = tool()
            .execute(json!({"action": "click_element", "ref": 1}))
            .await;
        assert!(result.is_error);
    }

    #[test]
    fn describe_observe_and_click_element() {
        let t = tool();
        assert_eq!(
            t.describe(&json!({"action": "observe"})),
            "Computer: observe (accessibility snapshot)"
        );
        assert_eq!(
            t.describe(&json!({"action": "click_element", "ref": 7})),
            "Computer: click element [7]"
        );
    }

    #[test]
    fn category_for_exec_actions() {
        let t = tool();
        for action in [
            "left_click",
            "right_click",
            "middle_click",
            "double_click",
            "triple_click",
            "mouse_move",
            "left_click_drag",
            "type",
            "key",
            "scroll",
            "focus_window",
        ] {
            assert_eq!(
                t.category_for(&json!({"action": action})),
                ToolCategory::Exec,
                "{action} should be Exec"
            );
        }
    }

    #[test]
    fn category_for_unknown_or_missing_action_is_exec() {
        let t = tool();
        assert_eq!(
            t.category_for(&json!({"action": "bogus"})),
            ToolCategory::Exec
        );
        assert_eq!(t.category_for(&json!({})), ToolCategory::Exec);
    }

    // --- execute error paths (no real screen/input needed) ---

    #[tokio::test]
    async fn unknown_action_is_error() {
        let result = tool().execute(json!({"action": "fly"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("fly"), "{}", result.content);
    }

    #[tokio::test]
    async fn missing_action_is_error() {
        let result = tool().execute(json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("action"), "{}", result.content);
    }

    #[tokio::test]
    async fn click_without_coordinates_is_error_naming_params() {
        let result = tool().execute(json!({"action": "left_click"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("x"), "{}", result.content);
        assert!(result.content.contains("y"), "{}", result.content);
    }

    #[tokio::test]
    async fn click_with_only_x_is_error_naming_y() {
        let result = tool()
            .execute(json!({"action": "left_click", "x": 10}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("`y`"), "{}", result.content);
    }

    #[tokio::test]
    async fn drag_without_end_is_error_naming_params() {
        let result = tool()
            .execute(json!({"action": "left_click_drag", "start_x": 1, "start_y": 2}))
            .await;
        assert!(result.is_error);
        assert!(
            result.content.contains("end_x") && result.content.contains("end_y"),
            "{}",
            result.content
        );
    }

    #[tokio::test]
    async fn type_without_text_is_error() {
        let result = tool().execute(json!({"action": "type"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("text"), "{}", result.content);
    }

    #[tokio::test]
    async fn key_without_key_is_error() {
        let result = tool().execute(json!({"action": "key"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("key"), "{}", result.content);
    }

    #[tokio::test]
    async fn key_with_unknown_combo_is_error() {
        let result = tool()
            .execute(json!({"action": "key", "key": "cmd+notakey"}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("notakey"), "{}", result.content);
    }

    #[tokio::test]
    async fn scroll_without_direction_is_error() {
        let result = tool().execute(json!({"action": "scroll"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("direction"), "{}", result.content);
    }

    #[tokio::test]
    async fn scroll_with_bad_direction_is_error() {
        let result = tool()
            .execute(json!({"action": "scroll", "direction": "sideways"}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("sideways"), "{}", result.content);
    }

    #[tokio::test]
    async fn focus_window_without_id_is_error() {
        let result = tool().execute(json!({"action": "focus_window"})).await;
        assert!(result.is_error);
        assert!(result.content.contains("window_id"), "{}", result.content);
    }

    #[tokio::test]
    async fn observe_rejects_non_integer_window_id() {
        let result = tool()
            .execute(json!({"action": "observe", "window_id": "front"}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("window_id"), "{}", result.content);
    }

    #[tokio::test]
    async fn observe_unknown_window_id_is_error() {
        let result = tool()
            .execute(json!({"action": "observe", "window_id": u32::MAX}))
            .await;
        assert!(result.is_error);
        assert!(
            result.content.contains("not found") || result.content.contains("Window"),
            "{}",
            result.content
        );
    }

    #[tokio::test]
    async fn screenshot_with_bad_display_type_is_error() {
        let result = tool()
            .execute(json!({"action": "screenshot", "display": "main"}))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("display"), "{}", result.content);
    }

    // --- wait ---

    #[tokio::test(start_paused = true)]
    async fn wait_clamps_to_five_seconds() {
        let start = tokio::time::Instant::now();
        let result = tool()
            .execute(json!({"action": "wait", "seconds": 60}))
            .await;
        assert!(!result.is_error, "{}", result.content);
        // Paused-clock runtime: the virtual elapsed time is the slept time.
        assert_eq!(start.elapsed(), Duration::from_secs(5));
        assert!(result.content.contains('5'), "{}", result.content);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_default_is_one_second() {
        let start = tokio::time::Instant::now();
        let result = tool().execute(json!({"action": "wait"})).await;
        assert!(!result.is_error);
        assert_eq!(start.elapsed(), Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn wait_negative_clamps_to_zero() {
        let start = tokio::time::Instant::now();
        let result = tool()
            .execute(json!({"action": "wait", "seconds": -3}))
            .await;
        assert!(!result.is_error);
        assert_eq!(start.elapsed(), Duration::from_secs(0));
    }

    #[tokio::test]
    async fn wait_is_info_and_not_error() {
        let t = tool();
        let input = json!({"action": "wait", "seconds": 0});
        assert_eq!(t.category_for(&input), ToolCategory::Info);
        let result = t.execute(input).await;
        assert!(!result.is_error);
    }

    // --- describe ---

    #[test]
    fn describe_click() {
        let d = tool().describe(&json!({"action": "left_click", "x": 120, "y": 340}));
        assert_eq!(d, "Computer: left_click at (120, 340)");
    }

    #[test]
    fn describe_screenshot_and_key_and_type() {
        let t = tool();
        assert_eq!(
            t.describe(&json!({"action": "screenshot"})),
            "Computer: screenshot"
        );
        assert_eq!(
            t.describe(&json!({"action": "key", "key": "cmd+shift+t"})),
            "Computer: press \"cmd+shift+t\""
        );
        let typed = t.describe(&json!({"action": "type", "text": "hello"}));
        assert!(typed.contains("hello"), "{typed}");
    }

    #[test]
    fn describe_drag_scroll_focus_wait() {
        let t = tool();
        assert_eq!(
            t.describe(&json!({
                "action": "left_click_drag",
                "start_x": 1, "start_y": 2, "end_x": 3, "end_y": 4
            })),
            "Computer: drag from (1, 2) to (3, 4)"
        );
        assert_eq!(
            t.describe(&json!({"action": "scroll", "direction": "down", "amount": 5})),
            "Computer: scroll down by 5"
        );
        assert_eq!(
            t.describe(&json!({"action": "observe", "window_id": 7})),
            "Computer: observe window 7"
        );
        assert_eq!(
            t.describe(&json!({"action": "observe"})),
            "Computer: observe (accessibility snapshot)"
        );
        assert_eq!(
            t.describe(&json!({"action": "focus_window", "window_id": 7})),
            "Computer: focus window 7"
        );
        assert_eq!(
            t.describe(&json!({"action": "wait", "seconds": 2})),
            "Computer: wait 2s"
        );
    }

    fn cached_entry(ref_id: u32, role: &str, name: &str) -> CachedEntry {
        CachedEntry {
            display: ElementEntry {
                r#ref: ref_id,
                role: role.to_string(),
                name: Some(name.to_string()),
                value: None,
                states: Vec::new(),
                bounds: nomi_a11y::Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 30.0,
                },
                source: Source::A11y,
            },
            target: CachedTarget::Ax {
                engine_ref: ref_id,
                screen_center: (60, 35),
            },
        }
    }

    #[test]
    fn snapshot_state_is_isolated_per_session() {
        let t = tool();
        t.set_last_snapshot(
            "agent-a",
            SnapshotCache {
                generation: SnapshotGen(1),
                entries: vec![cached_entry(1, "button", "Save")],
            },
        );
        t.set_last_snapshot(
            "agent-b",
            SnapshotCache {
                generation: SnapshotGen(2),
                entries: vec![cached_entry(1, "button", "Cancel")],
            },
        );

        let (gen_a, entry_a) = t.resolve_ref("agent-a", 1).expect("agent A ref");
        let (gen_b, entry_b) = t.resolve_ref("agent-b", 1).expect("agent B ref");
        assert_eq!(gen_a, SnapshotGen(1));
        assert_eq!(gen_b, SnapshotGen(2));
        assert_eq!(entry_a.display.name.as_deref(), Some("Save"));
        assert_eq!(entry_b.display.name.as_deref(), Some("Cancel"));
    }

    #[test]
    fn coordinate_geometry_is_isolated_per_session() {
        let t = tool();
        t.set_last_capture(
            "agent-a",
            CaptureGeometry {
                img_w: 1000,
                img_h: 1000,
                logical_w: 1000,
                logical_h: 1000,
                origin_x: 0,
                origin_y: 0,
            },
        );
        t.set_last_capture(
            "agent-b",
            CaptureGeometry {
                img_w: 1000,
                img_h: 1000,
                logical_w: 1000,
                logical_h: 1000,
                origin_x: 2000,
                origin_y: 100,
            },
        );

        assert_eq!(t.to_screen("agent-a", 10, 10), (10, 10));
        assert_eq!(t.to_screen("agent-b", 10, 10), (2010, 110));
    }

    #[test]
    fn selector_resolves_unique_element_but_reports_ambiguity() {
        let t = tool();
        t.set_last_snapshot(
            "selector",
            SnapshotCache {
                generation: SnapshotGen(3),
                entries: vec![
                    cached_entry(1, "button", "Save"),
                    cached_entry(2, "button", "Save as"),
                    cached_entry(3, "edit", "Document title"),
                ],
            },
        );

        let selector = ElementSelector {
            role: Some("button".to_string()),
            name_contains: Some("save as".to_string()),
            ..ElementSelector::default()
        };
        let (generation, entry) = t
            .resolve_selector("selector", &selector)
            .expect("unique selector");
        assert_eq!(generation, SnapshotGen(3));
        assert_eq!(entry.display.r#ref, 2);

        let ambiguous = ElementSelector {
            role: Some("button".to_string()),
            ..ElementSelector::default()
        };
        let error = match t.resolve_selector("selector", &ambiguous) {
            Ok(_) => panic!("two buttons should be ambiguous"),
            Err(error) => error,
        };
        assert!(error.starts_with("Ambiguous selector"), "{error}");
    }

    #[test]
    fn selector_nth_is_one_based() {
        let t = tool();
        t.set_last_snapshot(
            "selector",
            SnapshotCache {
                generation: SnapshotGen(4),
                entries: vec![
                    cached_entry(1, "button", "Save"),
                    cached_entry(2, "button", "Save"),
                ],
            },
        );
        let selector = ElementSelector {
            role: Some("button".to_string()),
            name: Some("save".to_string()),
            nth: Some(2),
            ..ElementSelector::default()
        };
        let (_, entry) = t.resolve_selector("selector", &selector).expect("nth match");
        assert_eq!(entry.display.r#ref, 2);
    }

    #[tokio::test]
    async fn wait_for_validates_condition_and_target_before_touching_the_screen() {
        let missing_condition = tool().execute(json!({"action": "wait_for"})).await;
        assert!(missing_condition.is_error);
        assert!(missing_condition.content.contains("INVALID_ARGUMENT"));

        let missing_selector = tool()
            .execute(json!({"action": "wait_for", "condition": "element_present"}))
            .await;
        assert!(missing_selector.is_error);
        assert!(missing_selector.content.contains("MISSING_TARGET"));

        let missing_text = tool()
            .execute(json!({"action": "wait_for", "condition": "text_present"}))
            .await;
        assert!(missing_text.is_error);
        assert!(missing_text.content.contains("MISSING_TEXT"));
    }

    #[test]
    fn disconnected_backend_errors_are_classified_as_stale() {
        for message in ["找不到成员", "element disconnected", "invalid handle"] {
            let classified = blocking_a11y_error(&A11yError::Backend(message.to_string()));
            assert_eq!(classified, Some(("STALE_REF", "reobserve", true)), "{message}");
        }
    }

    // --- coordinate mapping through stored geometry ---

    #[test]
    fn to_screen_identity_without_capture() {
        assert_eq!(tool().to_screen(DEFAULT_SESSION_ID, 123, 456), (123, 456));
    }

    #[test]
    fn to_screen_maps_with_capture_geometry() {
        let t = tool();
        t.set_last_capture(DEFAULT_SESSION_ID, crate::screen::CaptureGeometry {
            img_w: 1568,
            img_h: 980,
            logical_w: 1440,
            logical_h: 900,
            origin_x: 0,
            origin_y: 0,
        });
        assert_eq!(t.to_screen(DEFAULT_SESSION_ID, 0, 0), (0, 0));
        assert_eq!(t.to_screen(DEFAULT_SESSION_ID, 1567, 979), (1439, 899));
    }

    #[test]
    fn to_screen_applies_monitor_origin() {
        let t = tool();
        t.set_last_capture(DEFAULT_SESSION_ID, crate::screen::CaptureGeometry {
            img_w: 1000,
            img_h: 800,
            logical_w: 1000,
            logical_h: 800,
            origin_x: 1440,
            origin_y: -100,
        });
        assert_eq!(t.to_screen(DEFAULT_SESSION_ID, 10, 20), (1450, -80));
    }

    // --- capability note (model contract) ---

    #[test]
    fn capabilities_note_advertises_a11y_where_an_engine_exists() {
        let note = capabilities_note();
        // On every OS that create_engine() supports (macOS / Windows / Linux),
        // the model must be told the accessibility-first path is available —
        // never that it is "not yet available on this OS", which would steer it
        // onto fragile pixel guessing even though observe/click_element work.
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            assert!(
                note.contains("observe"),
                "the a11y-first path should be advertised: {note}"
            );
            assert!(
                !note.contains("not yet available on this OS"),
                "must not tell the model the a11y path is unavailable on a \
                 supported OS: {note}"
            );
        }
        // The macOS wording must stay byte-for-byte as before so the macOS
        // contract (and prompt cache) does not regress.
        #[cfg(target_os = "macos")]
        assert!(
            note.contains(
                "Accessibility-first targeting (observe / click_element / set_element_value): \
                 available, with OCR fusion for accessibility-thin apps."
            ),
            "macOS capability note changed: {note}"
        );
    }

    // --- real-device tests ---

    // Requires a display and Screen Recording permission.
    #[tokio::test]
    #[ignore]
    async fn screenshot_real() {
        let result = tool().execute(json!({"action": "screenshot"})).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].media_type, "image/png");
        assert!(result.content.contains("Screenshot captured"));
    }

    // Requires Accessibility permission.
    #[tokio::test]
    #[ignore]
    async fn cursor_position_real() {
        let result = tool().execute(json!({"action": "cursor_position"})).await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("Cursor position"));
    }
}
