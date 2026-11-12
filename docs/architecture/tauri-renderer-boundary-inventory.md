# Renderer Direct Tauri Call Inventory

Status: migration complete; protected by `bun run check:tauri-renderer-boundary`

Last verified: 2026-11-12

Scope: `ui/src/renderer/**`

This document records the direct Tauri references that existed in the
Renderer layer before migration. Renderer code now reaches every capability
through `ui/src/common/adapter/**`; the table is retained as the migration
record and regression reference.

The repository boundary is documented in
[`docs/architecture/communication.md`](./communication.md) and
[`docs/contributing/project-structure.md`](../contributing/project-structure.md):
Renderer code should reach host capabilities through the common/platform
adapter rather than importing Tauri directly. The boundary is now enforced by
the `check:tauri-renderer-boundary` static gate.

## Counting rules

- Static imports and dynamic `import('@tauri-apps/...')` expressions both count.
- Each actual Tauri module reference is one record.
- If one source line imports two Tauri modules, each module is recorded as a
  separate record.
- The count is based on `rg -n -F "@tauri-apps" ui/src/renderer`.
- Existing implementations in `ui/src/common/adapter/**` are intentionally
  excluded from the Renderer count.
- Line numbers below are the verified baseline locations. They are expected to
  have moved with the completed migration; every row below is marked `Migrated`.

## Baseline summary

| Measure | Baseline |
| --- | ---: |
| Direct references | 47 |
| Renderer files | 16 |
| `@tauri-apps/api/core` | 4 |
| `@tauri-apps/api/event` | 15 |
| `@tauri-apps/api/window` | 23 |
| `@tauri-apps/api/webview` | 3 |
| `@tauri-apps/api/menu` | 1 |
| `@tauri-apps/plugin-dialog` | 1 |

The 47 rows below are grouped by capability in the `Category` column while
retaining the scan order used to establish the baseline.

## Detailed inventory

| # | Category | Location | Tauri module / API | Current use | Runtime condition and Web fallback | Target adapter | Risk | Status / migration notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Core command | `hooks/useCompanionWindowsSync.ts:62` | `api/core` · `invoke('sync_companion_windows', { specs })` | Reconcile native `companion-{id}` windows after companion/profile changes. | Hook is desktop-gated through `isTauriRuntime()`; failure is caught and retried on a later event. Web has no native window set. | `tauriWindows.ts` · `syncCompanionWindows` | High | Migrated. Preserve debounce, command payload, and warning-only failure behavior. |
| 2 | Native drag-drop | `hooks/file/useDragUpload.ts:151` | `api/webview` · `getCurrentWebview().onDragDropEvent` | Receive host file paths and hit-test the active attachment drop zone. | Effect exits when `isTauriRuntime()` is false; HTML5/browser drop path remains active. Listener and RAF are cleaned up on unmount. | `tauriDragDrop.ts` · `registerNativeFileDrop` | Medium | Migrated. Preserve `enter/over/leave/cancel/drop` handling and cancellation race. |
| 3 | Theme event | `utils/theme/themeBroadcast.ts:33` | `api/event` · `emit(THEME_SYNC_EVENT, { theme })` | Broadcast light/dark theme changes to companion windows. | Explicit `isTauriRuntime()` guard; Web is a no-op because it has one document. Emit is best-effort and errors are swallowed. | `tauriEvents.ts` · `broadcastThemeSync` | Low | Migrated. Event payload is `ThemeSyncPayload`; no unsubscribe. |
| 4 | Theme event | `utils/theme/themeBroadcast.ts:48` | `api/event` · `emit(THEME_SYNC_EVENT, { customCss })` | Broadcast custom CSS changes to standalone companion windows. | Explicit Tauri guard; Web no-op. Missed events are healed by the next config load. | `tauriEvents.ts` · `broadcastCustomCssSync` | Low | Migrated. Preserve shared event name and payload field. |
| 5 | Theme event | `utils/theme/themeBroadcast.ts:58` | `api/event` · `emit(THEME_SYNC_EVENT, payload)` | Broadcast wallpaper token state without sending bitmap data. | Explicit Tauri guard; Web no-op; best-effort emit. | `tauriEvents.ts` · `broadcastWallpaperSync` | Low | Migrated. Preserve `wallpaperTokens` and `wallpaperEnabled` fields. |
| 6 | Native drag-drop | `pages/conversation/Workspace/hooks/useWorkspaceDragImport.ts:215` | `api/webview` · `getCurrentWebview().onDragDropEvent` | Import absolute host paths into the workspace, yielding to registered SendBox drop zones. | Effect exits outside Tauri; browser files use the existing HTML5/temp-file path. Listener is removed on unmount or cancellation. | `tauriDragDrop.ts` · `registerWorkspaceFileDrop` | Medium | Migrated. Preserve drop-zone arbitration and `paths` payload validation. |
| 7 | Window state | `pages/memoryPanel/index.tsx:111` | `api/window` · `getCurrentWindow().isFocused()` | Check focus after the memory panel becomes visible before focusing or closing. | Memory panel is Tauri-only; failed focus probe is contained by the surrounding async flow. | `tauriWindows.ts` · `isCurrentWindowFocused` | Medium | Migrated. Keep request-id and phase checks around the probe. |
| 8 | Window state | `pages/memoryPanel/index.tsx:130` | `api/window` · `getCurrentWindow().isFocused()` | Re-check focus after an activation acknowledgement and close on blur when needed. | Tauri-only panel path; no browser equivalent. Failure is handled by the existing close flow. | `tauriWindows.ts` · `isCurrentWindowFocused` | Medium | Migrated. Preserve `requestId` matching and `open` phase guard. |
| 9 | Window event | `pages/memoryPanel/index.tsx:141` | `api/window` · `getCurrentWindow().onFocusChanged` | Close the memory panel when its native window loses focus. | Listener is installed only in Tauri memory-panel mode and unsubscribed during cleanup. | `tauriWindows.ts` · `subscribeWindowFocus` | High | Migrated. Adapter must return an unsubscribe function and retain focus payload typing. |
| 10 | Cross-window event | `pages/meetingCaptions/index.tsx:29` | `api/event` · `listen('meeting-captions://update', ...)` | Render native meeting-caption updates in the caption window. | Explicit Tauri guard; Web renders the empty state. Listener unsubscribe is retained and called on cleanup. | `tauriEvents.ts` · `subscribeMeetingCaptions` | Medium | Migrated. Preserve `MeetingCaptionsPayload` validation and cleanup race. |
| 11 | Cross-window event | `components/layout/Router.tsx:256` | `api/event` · `listen('companion-boot-error', ...)` | Show a localized error when a companion window fails to boot. | Explicit Tauri guard; Web has no companion window. Listener is removed on unmount; errors are swallowed. | `tauriEvents.ts` · `subscribeCompanionBootError` | Medium | Migrated. Preserve notification translation and one listener per Router mount. |
| 12 | Cross-window event | `components/layout/Router.tsx:265` | `api/event` · `listen('companion-navigate', ...)` | Navigate the main router from a companion suggestion/context-menu action. | Explicit Tauri guard; Web no-op. Only string payloads beginning with `/` are accepted; unsubscribe on cleanup. | `tauriEvents.ts` · `subscribeCompanionNavigate` | High | Migrated. Keep route payload validation and async listener race handling. |
| 13 | Cross-window event | `components/layout/Router.tsx:295` | `api/event` · `listen('meeting-open', ...)` | Open `/meeting` from tray/global-shortcut events, honoring a valid path payload. | Explicit Tauri guard; Web no-op. Invalid payload falls back to `/meeting`; unsubscribe on cleanup. | `tauriEvents.ts` · `subscribeMeetingOpen` | Medium | Migrated. Preserve default route and listener teardown. |
| 14 | Cross-window event | `pages/completionToast/index.tsx:23` | `api/event` · `listen('completion-toast://show', ...)` | Receive native completion-toast content and render it. | Explicit Tauri guard; Web renders an empty surface. Unsubscribe on unmount. | `tauriEvents.ts` · `subscribeCompletionToast` | Medium | Migrated. Preserve `CompletionToastPayload` shape and generation identity. |
| 15 | Core command | `pages/completionToast/index.tsx:36` | `api/core` · `invoke(cmd, { generation })` | Activate or dismiss a native completion toast. | Only called by the Tauri toast surface; command errors are handled by caller cleanup. Web never reaches the page. | `tauriSystem.ts` · `activateCompletionToast` / `dismissCompletionToast` | Medium | Migrated. Replace free-form command string with a typed command method. |
| 16 | Core command | `pages/companion/companionLocalPointer.ts:91` | `api/core` · `invoke('get_companion_local_pointer')` | Sample pointer coordinates from the native companion window. | Called from the desktop click-through controller; unsupported/error results are parsed into the existing fallback sample. | `tauriWindows.ts` · `getCompanionLocalPointer` | High | Migrated. Preserve parse normalization and unsupported fallback behavior. |
| 17 | Window state | `pages/companion/useDetachedMemoryPanel.ts:83` | `api/window` · `getCurrentWindow().label` | Capture the companion owner window label for memory-panel protocol payloads. | Hook exits outside Tauri; browser fallback handles the non-detached path. | `tauriWindows.ts` · `getCurrentWindowLabel` | Medium | Migrated. Keep owner label in snapshot and request correlation. |
| 18 | Window and screen | `pages/companion/useDetachedMemoryPanel.ts:135` | `api/window` · `availableMonitors`, `getCurrentWindow`, position/size reads | Measure owner window and monitors to place the detached memory panel. | Tauri-only detached panel; layout failure invokes the existing fallback and cleanup. | `tauriWindows.ts` · `getMonitorLayout` / `getCurrentWindowRect` | High | Migrated. Preserve monitor mapping, scale factor, and request-id checks. |
| 19 | Window event | `pages/companion/useDetachedMemoryPanel.ts:218` | `api/window` · `getCurrentWindow().onMoved/onResized/onScaleChanged` | Close the detached memory panel when owner geometry becomes invalid. | Effect exits outside Tauri; all three listeners are unsubscribed, including the async-dispose race. | `tauriWindows.ts` · `subscribeOwnerGeometry` | High | Migrated. Return a grouped unsubscribe and preserve `closeForGeometryChange`. |
| 20 | Window state | `pages/companion/useCompanionClickThrough.ts:89` | `api/window` · `getCurrentWindow().setIgnoreCursorEvents(false)` | Restore direct pointer capture when local pointer sampling cannot continue. | Hook is enabled only in Tauri; fallback uses native window capture and warns once on failure. | `tauriWindows.ts` · `setIgnoreCursorEvents` | High | Migrated. Preserve ownership guard and idempotent reset. |
| 21 | Window state | `pages/companion/useCompanionClickThrough.ts:124` | `api/window` · `getCurrentWindow()`, `setIgnoreCursorEvents` | Toggle click-through state from the pointer-hit controller. | Tauri-only; unsupported/error path falls back to warning and full-window capture behavior. | `tauriWindows.ts` · `setIgnoreCursorEvents` | High | Migrated. Keep `captureAll`, dragging freeze, and owner-token semantics. |
| 22 | Cross-window event | `pages/companion/messages/channel.ts:43` | `api/event` · `emit(PET_MESSAGE_EVENT, payload)` | Publish local companion status messages to other Tauri windows. | `isTauriRuntime()` guard; Web still dispatches same-window listeners only. Emit is best-effort. | `tauriEvents.ts` · `publishPetMessage` | Low | Migrated. Preserve local dispatch before native broadcast. |
| 23 | Cross-window event | `pages/companion/messages/channel.ts:57` | `api/event` · `listen(PET_MESSAGE_EVENT, ...)` | Project native pet messages into the companion-local channel. | Returns a no-op unsubscribe in Web; async listener setup handles disposal before resolution. | `tauriEvents.ts` · `subscribePetMessageBroadcast` | Medium | Migrated. Preserve listener envelope and late-unsubscribe behavior. |
| 24 | Core command | `pages/companion/memoryPanelShell.ts:7` | `api/core` · generic `invoke` helper | Back the memory-panel prepare/place/show/hide command wrappers. | Every exported operation checks `isTauriRuntime()`; Web returns no-op or `false`. | `tauriWindows.ts` · memory-panel window methods | High | Migrated. Replace generic command helper with typed domain methods; this is the main companion protocol seam. |
| 25 | Cross-window event | `pages/companion/memoryPanelShell.ts:25` | `api/event` · `emitTo(MEMORY_PANEL_LABEL, event, payload)` | Send protocol messages to the detached memory-panel window. | Tauri guard; Web no-op. Callers await the native event send. | `tauriEvents.ts` · `emitToMemoryPanel` | High | Migrated. Keep label, event names, and typed payloads. |
| 26 | Cross-window event | `pages/companion/memoryPanelShell.ts:30` | `api/event` · `emitTo(label, event, payload)` | Send a typed event to an arbitrary named Tauri window. | Tauri guard; Web no-op. | `tauriEvents.ts` · `emitToWindow` | High | Migrated. Limit the public adapter to approved domain events instead of exposing arbitrary labels. |
| 27 | Window event | `pages/companion/memoryPanelShell.ts:35` | `api/window` · `getCurrentWindow().listen` | Subscribe to typed memory-panel events in the current window. | Tauri guard returns a no-op unsubscribe on Web. | `tauriEvents.ts` · `listenCurrentWindow` | High | Migrated. Preserve payload unwrapping and unsubscribe contract. |
| 28 | Window and screen | `pages/companion/index.tsx:361` | `api/window` · `getCurrentWindow`, `PhysicalPosition`, `availableMonitors` | Apply companion visibility and restore/clamp saved position. | `isTauriRuntime()` guard; browser branch renders a desktop-only hint. Errors are logged and optionally rethrown in strict boot mode. | `tauriWindows.ts` · `applyCompanionWindowState` | High | Migrated. Preserve hidden/visible focus behavior and monitor clamping. |
| 29 | Window and screen | `pages/companion/index.tsx:427` | `api/window` · `getCurrentWindow`, `PhysicalPosition`, `PhysicalSize`, `availableMonitors` | Resize the companion to the character desk spec and anchor it. | Tauri-only and visibility-gated; strict mode rethrows, normal config echoes log and continue. | `tauriWindows.ts` · `resizeCompanionWindow` | High | Migrated. Preserve scale-factor calculations, read-back, and anchoring. |
| 30 | Window and screen | `pages/companion/index.tsx:483` | `api/window` · `getCurrentWindow`, `PhysicalPosition`, `PhysicalSize`, `availableMonitors` | Queue expanded companion window geometry for chat/composer surfaces. | Tauri-only; queued operation has existing retry/restore behavior and catches failures at the caller. | `tauriWindows.ts` · `syncExpandedCompanionWindow` | High | Migrated. Keep serialized queue, monitor mapping, restore retries, and internal-layout guard. |
| 31 | Cross-window event | `pages/companion/index.tsx:768` | `api/event` · `emitTo('main', 'companion-boot-error', companionId)` | Report terminal companion boot failure to the main window. | Only after retry exhaustion and Tauri guard; emit failure is swallowed. | `tauriEvents.ts` · `emitCompanionBootError` | Medium | Migrated. Preserve retry threshold, main label, event name, and companion id payload. |
| 32 | Window state | `pages/companion/index.tsx:884` | `api/window` · `getCurrentWindow().close()` | Self-close an orphaned companion window after deletion. | Triggered by backend deletion event; errors are logged. Web branch never mounts native companion UI. | `tauriWindows.ts` · `closeCurrentWindow` | Medium | Migrated. Keep autonomous close behavior when main window is unavailable. |
| 33 | Window event | `pages/companion/index.tsx:1164` | `api/window` · `getCurrentWindow().onMoved` | Debounce companion drag position persistence after native moves. | Tauri and companion-id gated; listener/timer are cleaned up on unmount. | `tauriWindows.ts` · `subscribeWindowMoved` | High | Migrated. Preserve internal-layout and expanded-session guards plus 600 ms debounce. |
| 34 | Window event | `pages/companion/index.tsx:1207` | `api/window` · `getCurrentWindow().onFocusChanged/onScaleChanged` | Re-snap the companion after refocus or DPI/scale changes. | Tauri-gated; also listens to document visibility and browser focus. Native listeners are removed on cleanup. | `tauriWindows.ts` · `subscribeWindowEnvironmentChanges` | High | Migrated. Preserve idle/dragging guards and disposal race. |
| 35 | Cross-window event | `pages/companion/index.tsx:1284` | `api/event` · `listen(THEME_SYNC_EVENT, ...)` | Apply theme, custom CSS, and wallpaper token updates in the standalone companion. | Tauri-gated; initial config/cache load remains the fallback for missed broadcasts. Unsubscribe on cleanup. | `tauriEvents.ts` · `subscribeThemeSync` | Medium | Migrated. Preserve payload field checks and style injection calls. |
| 36 | Window state | `pages/companion/index.tsx:1318` | `api/window` · `getCurrentWindow().setTitle(name)` | Keep the native companion window title synchronized with the profile name. | Tauri-gated; missing names are ignored and failures are swallowed. | `tauriWindows.ts` · `setCurrentWindowTitle` | Low | Migrated. Preserve trimmed-name guard. |
| 37 | Window state | `pages/companion/index.tsx:1397` | `api/window` · `getCurrentWindow().onMoved/startDragging` | Start native companion dragging and detect drag completion. | Tauri-only pointer interaction; failure calls the existing `end()` cleanup path. `onMoved` unsubscribe is retained. | `tauriWindows.ts` · `startCompanionDrag` | High | Migrated. Preserve pointerup/mouseup/silence/safety termination behavior. |
| 38 | Cross-window event | `pages/companion/index.tsx:1412` | `api/event` · `emitTo('main', 'companion-navigate', path)` | Ask the main window to navigate from companion actions. | Tauri-gated; errors are logged and the caller continues best-effort. | `tauriEvents.ts` · `emitCompanionNavigate` | Medium | Migrated. Keep absolute-path validation before this call. |
| 39 | Window state | `pages/companion/index.tsx:1413` | `api/window` · `Window.getByLabel('main')` plus show/unminimize/focus | Bring the main window forward after companion navigation. | Tauri-only; each operation is best-effort so one failure does not block the rest. | `tauriWindows.ts` · `revealMainWindow` | Medium | Migrated. Do not expose `Window` objects to Renderer code. |
| 40 | Native drag-drop | `pages/companion/index.tsx:1505` | `api/webview` · `getCurrentWebview().onDragDropEvent` | Attach image paths dropped onto the companion composer. | Tauri-gated; browser uses paste/file-dialog paths. Listener cleanup handles async setup cancellation. | `tauriDragDrop.ts` · `registerCompanionImageDrop` | Medium | Migrated. Preserve `enter/over/leave/cancel/drop` state transitions. |
| 41 | Menu | `pages/companion/index.tsx:1942` | `api/menu` · `Menu.new` | Construct the companion native context menu. | Called only from the Tauri companion context-menu handler; errors are logged. | `tauriSystem.ts` · `createCompanionContextMenu` | Medium | Migrated. Keep localized entries and action callbacks; do not expose `Menu`. |
| 42 | Menu / window | `pages/companion/index.tsx:1943` | `api/window` · `getCurrentWindow()` | Supply the current native window to `menu.popup`. | Same Tauri-only context-menu path; error is contained by caller. | `tauriSystem.ts` · `showCompanionContextMenu` | Medium | Migrated. Combine with row 41 into one business-level method during migration. |
| 43 | Window state | `pages/companion/index.tsx:2030` | `api/window` · `getCurrentWindow().setIgnoreCursorEvents(false)` | Synchronously restore pointer capture before opening the right-click menu. | Explicit Tauri guard; failure is swallowed and normal menu flow continues. | `tauriWindows.ts` · `setIgnoreCursorEvents` | Medium | Migrated. Preserve ordering before `openNativeContextMenu`. |
| 44 | Window and screen | `pages/companion/companionScreenClamp.ts:42` | `api/window` · `availableMonitors()` | Load current monitor work areas for companion clamping. | Caller-gated by desktop companion paths; errors propagate to existing snap fallback. | `tauriWindows.ts` · `getMonitorWorkAreas` | Medium | Migrated. Preserve work-area vs full-display fallback. |
| 45 | Window and screen | `pages/companion/companionScreenClamp.ts:70` | `api/window` · `getCurrentWindow`, `PhysicalPosition`, position/size reads and `setPosition` | Snap/dock the companion fully inside a connected work area. | Caller-gated by Tauri; no browser equivalent. Existing geometry helpers remain pure and testable. | `tauriWindows.ts` · `snapCompanionWindow` | High | Migrated. Keep docking threshold, force behavior, and physical coordinates. |
| 46 | Window and screen | `pages/companion/companionScreenClamp.ts:101` | `api/window` · `getCurrentWindow`, `PhysicalPosition`, position/size reads and `setPosition` | Undock an edge-docked companion back into the visible work area. | Caller-gated by Tauri; returns `null` when no dock state exists. | `tauriWindows.ts` · `undockCompanionWindow` | Medium | Migrated. Preserve current dock side and monitor selection logic. |
| 47 | File dialog | `pages/nomi/workspace/tabs/OtherTab/bundleIo.ts:26` | `plugin-dialog` · `save({ defaultPath, filters })` | Pick a native ZIP save path for companion export. | Every caller is documented as `isTauriRuntime()`-gated; browser uses the existing non-native path. | `tauriSystem.ts` · `saveBundleFile` | Low | Migrated. Preserve ZIP filter and nullable path result. |

## Target adapter boundaries

The migration should expose business-level functions from the common adapter
layer and keep raw Tauri objects, module types, event callbacks, and window
labels out of Renderer business code.

| Capability | Planned adapter | Boundary rule |
| --- | --- | --- |
| Window and screen | `ui/src/common/adapter/tauriWindows.ts` | Expose typed operations such as `getCurrentWindowBounds`, `setIgnoreCursorEvents`, `subscribeWindowFocus`, `revealMainWindow`, and companion geometry methods. |
| Cross-window events | `ui/src/common/adapter/tauriEvents.ts` | Expose typed subscriptions and domain emitters such as `subscribeCompanionNavigate`, `emitCompanionBootError`, and `subscribeThemeSync`; every subscription returns `() => void`. |
| Native drag-drop | `ui/src/common/adapter/tauriDragDrop.ts` | Expose typed registration helpers for host paths and drop payloads; preserve cancellation and cleanup semantics. |
| Files and menus | `ui/src/common/adapter/tauriSystem.ts` | Expose `saveBundleFile`, `showCompanionContextMenu`, and typed completion-toast commands without returning `Menu`, `Window`, or plugin objects. |
| Companion protocol | Domain methods built on the adapters | Keep memory-panel command/event names and payloads in one typed protocol module; callers should not pass arbitrary labels or command strings. |

## Verification and migration hand-off

The baseline can be rechecked with:

```powershell
$matches = rg -n -F "@tauri-apps" ui/src/renderer
$matches.Count                         # expected: 0
$matches | ForEach-Object { ($_ -split ':', 3)[0] } |
  Sort-Object -Unique | Measure-Object # expected Count: 0
```

Batch 2 should implement the smallest `tauriWindows.ts`, `tauriEvents.ts`,
and `tauriDragDrop.ts` surfaces first, then migrate low-risk rows and mark
each row with the new adapter symbol and commit/PR reference. File and menu
operations follow through `tauriSystem.ts`. The static
`check-tauri-renderer-boundary.mjs` gate should be added only after the rows
are migrated, so the gate protects the completed boundary rather than blocking
the migration itself.

This inventory is a migration baseline. It does not remove duplicate
dependencies, change the event bus, rename storage keys, or alter any runtime
behavior.
