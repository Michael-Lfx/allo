# Frontend

The frontend is a single React 19 SPA in [`ui/`](../../ui/). The Tauri desktop
shell and the `nomifun-web` host load the same Vite build from `ui/dist`; the
renderer talks to the backend through HTTP and WebSocket, with a small Tauri
adapter only for desktop shell operations.

## Stack

| Concern | Current choice |
| --- | --- |
| Framework | React 19 + TypeScript |
| Bundler | Vite 6 |
| Routing | `react-router-dom` v7 with `HashRouter` |
| UI | Arco Design + custom CSS theme layers + UnoCSS |
| Data | SWR plus React contexts for app-shaped state |
| i18n | `i18next` / `react-i18next`; current app locales are `zh-CN` and `en-US` |
| Terminal | `xterm.js` with fit/web-links/webgl addons |
| Markdown | `react-markdown`, GFM, KaTeX, Mermaid |

## Source Layout

```text
ui/src/
├── common/       bridge/API/types/util code shared across hosts
├── platform/     small substrate for storage/logger/theme/runtime bridge
└── renderer/     React app: pages, layout, hooks, services, styles
```

The renderer imports the composite bridge from
`ui/src/common/adapter/ipcBridge.ts`. Most product operations are HTTP calls.
Tauri-specific operations are guarded behind `isTauri()` and implemented in the
adapter layer rather than scattered through pages.

## Backend URL And Trust

Desktop:

- `apps/desktop/src/main.rs` injects `window.__backendPort`.
- It also injects a per-boot `window.__nomiLocalTrust` secret.
- The init script patches `fetch` and `XMLHttpRequest` so requests to the
  embedded loopback backend include `x-nomi-local-trust`.

Web:

- No port is injected.
- The bridge uses same-origin `/api` and `/ws`.
- Authenticated web mode uses the session cookie plus CSRF double-submit header.

## Current Route Map

The source of truth is
[`ui/src/renderer/components/layout/Router.tsx`](../../ui/src/renderer/components/layout/Router.tsx).

| Route | Surface |
| --- | --- |
| `/login` | Login / first-run setup. |
| `/companion` | Desktop companion window route; outside the normal protected app layout. |
| `/guid` | Session start surface. |
| `/conversation/:id` | Conversation runtime. Developer mode can slide the column to Session Logs (`AgentTraceInspector`): capture is always on; list/turn/call HTTP still requires developer mode. See [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md). |
| `/eval` | Developer-mode Agent Eval lab. Sidebar `dev` badge; requires `system.developerMode`. |
| `/terminal-new` | Terminal creation. |
| `/terminal/:id` | Terminal runtime. |
| `/models` | Model and agent management. |
| `/presets` | Reusable preset library. |
| `/skills` | Skills capability library. |
| `/mcp` | MCP server management. |
| `/plugins` | Plugin market and installed extensions. |
| `/open-capabilities` | Remote/public capability exposure. |
| `/scheduled`, `/scheduled/:job_id` | Scheduled tasks. |
| `/requirements`, `/requirements/extensions`, `/requirements/sources` | Requirements Platform, AutoWork, notification/source extensions. |
| `/nomi` | Companion configuration. |
| `/knowledge`, `/knowledge/:id` | Knowledge base list/detail. |
| `/settings/system` and related settings subroutes | System settings page and sub-sections. |

Legacy settings paths such as `/settings/model`, `/settings/agent`,
`/settings/capabilities`, `/settings/skills-hub`, `/settings/tools`,
`/settings/webui` and `/settings/webhook` are
redirects. Do not document them as primary navigation.

Agent collaboration has no standalone route or separate page. Its
AgentExecution projection is rendered inside the owning Conversation, so
navigation does not introduce another product object.

## State And Data

- SWR owns most remote list/detail state.
- `AuthProvider`, theme, feedback, preview, and conversation-history contexts
  own app-shaped state.
- `configService` initializes before i18n/theme consumers so early render reads
  backend-backed preferences.
- Realtime events arrive through a singleton WebSocket and are demuxed by event
  name.

## Desktop Conversation Streaming Presentation

The desktop conversation surface separates an active assistant reply into a
stable Markdown prefix and a lightweight trailing block. The split is local to
[`MessageText.tsx`](../../ui/src/renderer/pages/conversation/Messages/components/MessageText.tsx): completed
paragraphs and closed fenced blocks render through `MarkdownView`, while the
unfinished final paragraph stays plain text. An open fenced block is shown as a
lightweight code preview until it closes, avoiding raw fence markers and a
full-Markdown parse for every token.

Markdown renders in a Shadow DOM so message typography and code controls have
an explicit local style contract:

- message body remains `14px / 22px`; headings use a restrained
  `18 / 16 / 15 / 14px` hierarchy;
- links receive theme-derived normal, hover, and keyboard-focus colors;
- desktop code actions reveal on hover or keyboard focus inside the Shadow DOM;
- process-row text uses a neutral readable color while icons carry running,
  waiting, failed, or canceled state color; reduced-motion disables the
  associated animations.

### Current Scope And Follow-Up

Updated in the current streaming presentation work:

- stable-prefix / trailing-block rendering and open-code preview;
- Markdown hierarchy, themed links, and Shadow-DOM code-toolbar states;
- semantic `MessageThinking` toggles with keyboard focus, labelled detail
  regions, and bounded desktop scrolling;
- Mermaid's Shadow-DOM toolbar and preview/source segmented control;
- code footer states, process status contrast, primary focus rings, and
  icon-only live-step animation;
- focused coverage for the splitter, Markdown typography, thinking controls,
  Mermaid and code toolbars, and process layout.

Not updated by this work:

- mobile touch targets, safe-area spacing, and narrow-screen disclosure layout;
- visual screenshot coverage across desktop themes (the current checks are
  source/behavior tests plus the normal frontend quality gate); the planned
  1440x900 and 1280x800 light/dark visual review is not yet recorded.

## Desktop-Specific UX

Desktop shell behavior is implemented by Tauri commands and plugins:

- updater check,
- companion window reconciliation,
- WebUI LAN listener status/start/stop,
- keep-awake toggle,
- tray label localization,
- deep-link forwarding,
- tray close behavior.

Browser builds no-op or degrade desktop-only affordances in the adapter layer.

### Desktop titlebar and Tooltip boundary

On Windows and Linux, the frameless titlebar root is the native drag plane:
blank menu and toolbar space remains draggable, while buttons, window controls,
mobile actions, and Tooltip anchors are explicit no-drag islands. Double-click
maximize only accepts a target inside the drag plane and outside those islands.
macOS Overlay titlebars keep the menu and toolbar outside the drag plane, and
browser builds do not enable desktop dragging.

Titlebar and sidebar icon hints that use `InstantHoverTooltip` are rendered
through a body Portal and positioned with fixed Floating UI `flip`/`shift`
middleware inside an 8px viewport boundary. Long localized content wraps within
`calc(100vw - 16px)`. The implementation and verification boundary are recorded
in [`Topbar drag and Tooltip fix audit`](../superpowers/audits/2026-08-13-topbar-drag-tooltip-fix.md).
