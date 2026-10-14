# Changelog

Flowy 1.0.0 is the first public release. This file records user-facing notes
at a high level rather than a complete commit log.

## Unreleased

- Short-drama shots no longer stretch silent actions to the video model's minimum clip length. A silent action plays at its natural ~2 seconds, silent shots pack by their content, and a held final frame on a shot with no spoken line is trimmed at the film join. Spoken lines are timed as before, and every request still stays within the selected model's clip window.
- Storyboard and screenwriter prompts no longer ask for leftover seconds to be filled with extra shots, and the performance micro-beat applies to emotional turns only, so routine actions no longer read as slow.
- A storyboard row whose action and speech exceed the clip window is split instead of dropping leftover lines, and unquoted SFX/BGM prose is no longer priced as speech.
- Environment plates follow the screenplay's scene headings (built after the script exists), so a scene no longer borrows another location's set from overlapping words.
- While one shot is rendering, other shots keep their node layout movable; switching shots only loads the current take's video, and a layout-only save is no longer rejected as "generating".

## 0.1.0-beta.11 - 2026-10-14

- Market categories and tags are now visible over the wire. A new `store/categories` method returns the category dictionaries of all enabled markets plus each market's top-level tag vocabulary; store items carry `category_id` and a host-resolved `category` (absent when the id misses the dictionary — never guessed); agent summaries now carry localized `tags` and `category_id`, moved up from the agent detail (which drops the two fields — a flattened JSON object cannot carry the same key twice). Skill and connector entries, which ship no plugin.json, take their tags from the marketplace index row, with legacy tag names merged in. Protocol fingerprint `fp-14` → `fp-15` (`56 / 83` methods); clients and hosts must upgrade together.
- The market's category and tag metadata now reaches every remaining face. Team listings carry the package's `tags` and `category_id`, skill listings carry the skill's `tags` (taken from the marketplace index row at import time — a skill ships no `plugin.json` — and read back through the same provenance chain the icon uses, so a later market edit only takes effect through an upgrade, exactly like the skill's own content). Exported expert packs carry the display metadata too and move to `pack_format 3`: the top-level pack from its own package row, each member pack from its own declaration (the opposite scope from the snapshot-level `assets`). Protocol fingerprint `fp-15` → `fp-16`; no method was added or removed, so the wire method count is unchanged and clients only gain fields.

## v1.6.2 - 2026-10-13

- The billing page explains credits from the real account: a check-in calendar with each day's credited amount, today's arrival status, the batches that actually expire (including team-seat credits) with their dates, and the sources behind the balance. Plans, packs, and the payment flow are unchanged.
- Auto model starts on the Economy tier, and a saved model choice is honored while the model catalog is still loading instead of being silently dropped for a system default. Knowledge, Learning, and Eval entry points refuse to start without a resolved model.
- Payments no longer hijack the page: leaving the payment step discards late order/checkout results, an unconfirmed balance is never rendered as 0, account switches drop the previous account's credit reads, and re-entering the billing page from the sidebar scrolls to the requested section again.

## 0.1.0-beta.10 - 2026-10-12

- Exporting an expert or a team now carries the package's plugin-level assets (`avatars/`, `bin/`, `scripts/`) alongside the skills. Protocol fingerprint `fp-13` → `fp-14`: two new WebSocket-only methods (`import/assets` lists the snapshot's plugin-level assets with sha256 digests, `import/asset` serves one file base64) and an `assets` / `avatar` pair on the expert pack (`pack_format` 1 → 2), gated by the new `snapshot_assets` capability bit. The SDK's `exportTeam()` writes them at their snapshot-relative paths (`ExportResult.writtenAssets`), so the exported directory is a complete plugin root — a team whose own scripts resolve `PLUGIN_ROOT/bin/init_task.py` two levels above their skill finds it, instead of an export that points at files nobody can produce.
- The materialized export carries executable plugin scripts honestly: `bin/` and `scripts/` files arrive as `kind: "script"` bytes without an executable bit (the host never executes them), and an operator can keep the class out of the pack with `[expert_export] scripts = false` in `config.toml` (avatars unaffected; default remains export, per the subtract-only policy table). The snapshot byte face itself is not policy-gated, the same way `skill/files` never was. Dangling skill declarations are recorded as accepted behaviour (doc 38 F4): the source runtime never fulfills member-declared skills either; this project reports them honestly instead.
- A connector authorization page no longer says "Authorization successful!" before the token exchange has happened: the browser is answered once the credential is stored, and a refused exchange (a throttling gateway answering `slow_down`, for example) now shows a "not completed" page with the sanitized reason. Previously a failed login looked finished in the window, so the user closed it and never retried.
- `connector/auth/status` reports `authorization_pending` while a browser flow for that connector is running — a state the design doc already named but the App Server never reported, so "waiting for the user" and "never started" were the same answer. It outranks the stored credential, is scoped to the connector being authorized (one flow at a time), and disappears with the flow. No field, method, event or fingerprint change: it fills in the existing `state` vocabulary.
- The Agent Store Web UI shows 「认证中」 on a connector while its authorization is in flight and follows the flow to its outcome, instead of leaving 「需要授权」 up and waiting for a manual refresh.
- Installing a store entry from the Web UI goes through the SDK's `store.install()` rather than the bare `store/install-entry` verb, so a connector is switched on and probed the same way the SDK does it (the button therefore waits for readiness, up to the SDK's default budget, and a connector that needs authorization returns immediately with that verdict instead of burning it). A connector installed from the store page used to stay 「已禁用」 until someone found the drawer's component switch.
- `store.findByConnector(connectorId)` finds the `StoreItem` that installed a given connector — by its `id` or its runtime `name` (the entry `fenbi-baokao-decision` installs a server called `fenbi_mcp`, which no catalogue field carries, so `search()` could not answer it), and `StoreOperationOutcome.installedCount` carries the host's own count for `install()` / `update()`.
- An OAuth dynamic client registration announces the host's own name when `MCP_OAUTH_CLIENT_NAME` is set (it wins over the built-in `Nomifun MCP Client`, and an empty value counts as unset). That string is what the provider's consent page shows the user, so a private deployment no longer has to patch the binary to be named correctly; it applies to clients this host registers itself, and a registration that already exists keeps the name it was created with.
- Desktop companion controls follow the theme, English sidebar labels fit narrow layouts, and companion chats keep the Pets tab selected. Expanded chats restore on the current display, and IME confirmation no longer sends a message.
- Startup recreates a missing workspace folder at its configured path and reports it in Settings. A missing owner marker is restored only when the existing dataset binding and generation match; deleted workspace contents are not recovered.

## v1.6.1 - 2026-10-10

- The desktop companion stays fully on a connected display (including multi-monitor gaps and unplugged screens), shows live module status on the pet, and adds the cream-colored Puff figure with richer motion.
- Canvas chrome follows the app theme, missing shot packets are seeded, and Vimax restores the live film from `films/vN` while skipping shot-packet autosave during planning. Timestamp-named stills bind as anonymous cameo plates.
- Exporting an expert team with `exportTeam()` brings the team's own package skills: a team pack's `skills` now comes from the skills that team's package installed, instead of always being empty.
- Session titles stay distinct from search and rename. Interest topics default off. Skills ship themed avatars, and `huashu-art-motion` is renamed to `art-motion`.

## v1.6.0 - 2026-10-08

- Appearance adds full-window custom wallpaper scenes.
- The compact update card wraps its description, and Flowy LLM requests stamp the conversation session.
- Short-drama workshop portraits are a standard character design sheet (full-body hero plus face close-up) instead of an equal three-view turnaround.
- Video canvas no longer treats WAV clips as images, keeps reference audio on Seedance/Wan, and opens audio settings above the prompt overlay.
- Buy credits from the sidebar cart, conversation errors, and video failures opens the official website credits tab with the same cloud login token.
- Session context occupancy and the Metrics rail refresh after each model request, not only when the whole turn finishes.
- Session turn credit chips wait for the Agent Run to finish and include Flowy-proxied Anthropic/Responses, MoA, and goal auto-draft model calls.
- Session file-change counts wait until the full turn is in the history window, and the Changes rail keeps its session snapshot across leave/re-enter and app restart.
- The conversation task queue sits in the composer layout instead of covering messages, with a quieter Reminders-style checklist.
- Lesson practice sections present questions one at a time with instant grading feedback ("Next question" after each submit); the stepper is gated per practice section and unlocks past it only once its questions are answered. Answered questions stay reviewable with your own response and the grader's feedback.
- Wan 3.0 video create requests now send DashScope `input`/`parameters` (not Seedance `content[]`), with 2–30s clips and `480P`/`720P`/`1080P` resolution tokens.
- Connector OAuth authorization that fails before the browser opens now reports the real reason instead of claiming a browser window was opened.
- Connector OAuth logins reuse the stored client registration instead of registering a new one with the authorization server on every attempt, so repeated attempts stop reading as "too many OAuth requests" to a throttling gateway.
- The Agent Store host accepts `--log-level` / `NOMI_LOG_LEVEL` (for example `info,nomifun_mcp::oauth_service=debug`), and every backend log opens with its log directory and effective level.
- The connector detail drawer shows why an authorization did not finish (token exchange failure, callback timeout, a throttling gateway) instead of only "not authorized".
- A connector authorization that an authorization server throttles (`slow_down`) now waits instead of retrying at once: the next attempt is refused with the remaining wait, and repeated throttles back off up to 15 minutes.
- Installing a connector no longer probes it on every readiness poll (one real connection per 5s instead of one per 400ms — up to ~10 connections and ~20 token requests per install before), and the SDK's `connectors.waitForAuth` reports why an authorization did not finish.
- Connectors that need a key or token keep the auth headers their marketplace declares (and their `sse` transport), and a credential the URL carries is substituted before the request; a probe that finds a credential missing says which key to fill instead of sending the literal `secret:NAME`.
- A default marketplace source can defer its boot download with `download_on_start = false` (one key per `[default_marketplaces.<id>]`, default unchanged), and a launcher that spawns the host can force the whole process with `AGENT_STORE_MARKET_DOWNLOAD=eager|lazy|none` or point it at its own file with `AGENT_STORE_CONFIG`.
- The Agent Store host's market policy — how often the background sweep runs and which entry kinds it may upgrade — is readable and changeable while the host runs (`market/settings` / `market/settings-set`, written through to `config.toml`), so the interval no longer needs a restart.
- The background sweep can now upgrade the entries installed from a market whose auto-update switch is on, together with the index refresh it already did. Connectors stay excluded unless that kind is opted in (an upgrade re-runs `upsert_server`, which disables a server whose configuration changed), and an entry you switched off by hand, one carrying a blocked reason, or one that was never installed is never touched; `entry_auto_update_kinds = []` restores the index-only behaviour.
- Protocol fingerprint `fp-12` → `fp-13` (the App Server rejects a mismatched client outright, so SDK and host must be updated together).

## v1.3.2 - 2026-09-11

- Video generation adds a blank-canvas entry and outer connect rails for wiring nodes from outside the card.

## v1.3.1 - 2026-09-10

- Desktop surfaces startup diagnostics when the local backend is unreachable.

## v1.3.0 - 2026-09-10

- Canvas adds a craft-shelf handbook and a simpler create-node menu.
- Bottom dock toolbar and toolbar settings are refined. Windows 11 keeps the webview loopback API reachable.

## v1.2.9 - 2026-09-10

- macOS cross-builds bundle ripgrep for the Tauri target architecture.

## v1.2.8 - 2026-09-09

- Vimax cover generation, storyboard truncation, and missing scene reference images are fixed.
- Desktop defers the startup OTA check until after first paint, and reports update-funnel plus CDN quality telemetry.
- Agent eval adds layered suites, pass@k, and a cloud bad-case pipeline.

## v1.2.7 - 2026-09-09

- Vimax fills missing shots at the finished-film level before packing, so storyboards do not duplicate rows or split extra clips.

## v1.2.6 - 2026-09-09

- Release CI no longer replaces crates.io with rsproxy, which was breaking ModelScope builds.

## v1.2.2 - 2026-09-05

- Video Look catalog and original still covers share one surface.
- R2V storyboard display stays frozen during generation, and users can name tasks.

## v1.2.1 - 2026-09-05

- Canvas unifies the asset space with short-drama stills, quiets overlay chrome, and collapses the node composer.
- Historical canvas media displays again, and art-critique errors surface instead of failing silently.

## v1.2.0 - 2026-09-04

- SkillHub expert packs now validate package integrity during install and at runtime; expert pack settings live in the local capability center, and Nomi setup stays consistent with pack installs. Missing ACP skills no longer silently degrade mid-session.
- Conversations gain a Skills picker card and improved setup-presentation coverage.
- Image analysis prioritizes DeepSeek vision models with tightened matching, a larger token budget, and graceful fallback on failure.
- Networking: the workspace HTTP client enables HTTP/2 multiplexing, with a new HTTP/2 vs HTTP/3 benchmark probe and documented findings on the current reqwest HTTP/3 ALPN limitation.
- Canvas isolates node jobs from clip recents and hardens the home agent.
- Video generation unifies recents, fixes TV tabs, and tightens the drama pipeline.
- Learning fixes directory deletion graph cleanup and relaxes the startup orphan audit with automatic repair.
- Meeting dock pill and notes-first surface match Granola; capability-center sidebar no longer overflows in English.

## v1.1.10 - 2026-09-03

- Learning courses use a graph/list workspace with on-demand generation, interrupt/resume, and faster first paint.
- Session Trace timeline, folding, and diagnostics are clearer; conversation errors include model context and block mid-stream model switches.
- Video canvas allows connecting video nodes and defaults to a light theme. Vimax adds style catalog, session Cameo, and clearer provider moderation errors.
- Coding recon is bounded by provider round-trips, with corrected turn credits.

## v1.1.9 - 2026-09-02

- News Briefing adds a research-to-voice/stills workflow and starts processing directly after submission.
- Settings navigation now shows loading and error states and prevents stale interactions during transitions.
- MCP, plugin, and skill settings are more consistent across the settings surfaces.
- Video-generation and canvas surfaces integrate briefing controls and improve draft, route, and duration handling.
- First-party telemetry now preserves error messages, while UI async, type, and component-contract fixes improve cross-page stability.

## v1.1.8 - 2026-09-01

- Canvas Agent restores the local coding switch and no longer hits a max-update-depth loop on prompt resize.
- Keep-awake settings stay consistent. Feedback and support chat modals are more stable.
- First-party telemetry reports video-generation events and film terminal status.

## v1.1.7 - 2026-09-01

- Canvas Agent starts faster, persists chats, and uses a simpler panel.

## v1.1.6 - 2026-09-01

- Vimax studio docks the Agent session and persists run history.
- Storyboard rows stay frozen while a video clip is generating.

## v1.1.5 - 2026-08-31

- Long Markdown blockquotes collapse; streaming layout and collapse controls stay stable.
- Media catalog lists TTS models and rewrites relative catalog icons.
- Vimax clip timelines follow model duration windows; narrative clips pack without phantom last shots.

## v1.1.4 - 2026-08-30

- Seedance reference-to-video prompts are shorter and honor the user's aspect ratio.

## v1.1.3 - 2026-08-29

- Video canvas adds a 3D director workbench and canvas agent tools.
- Vimax campaign carousel, TV tab, and submit flow. Storyboard shows I2V spec and concatenates mixed-resolution shots.
- Photographic faces are sanitized for privacy; clip pacing follows speech.

## v1.1.2 - 2026-08-29

- Fix Windows ARM ModelScope publish: NSIS `_arm64-setup.exe` maps to `windows-aarch64`.
- Video home composer attach matches stacked-card UX; custom/skill popovers open faster.
- Meeting recording dock stays pinned. Safari 15.5 WebKit and real macOS versions report correctly.

## v1.0.6 - 2026-08-25

- Meetings record with switchable local or cloud STT, live captions, speaker labels, and notes that land in bound chats and tasks. Tray and global shortcuts start or join a session; Agent listen mode keeps a rolling transcript.
- Cloud login returns as soon as the session is saved. Expired cloud tokens open a global re-login modal.
- Desktop completion toasts sit at the bottom-right (Open / Dismiss) and are not blocked by Windows Focus Assist banners. The Windows taskbar shows a count badge until you focus the main window.
- Video canvas honors resolution and retry state, keeps local videos, and opens projects faster.
- SkillHub expert packages install more reliably. Capability hub chrome matches other settings pages.
- Home conversation follow stays pinned through wraps and thinking collapse. Windows titlebar min/max clicks work again.
- Packaging: one `v1.0.6` tag builds Windows, macOS, and Linux in parallel.

## v1.0.5 - 2026-08-21

- Buy a USD personal plan or credit pack in the app with Airwallex. The credits + button opens checkout; card details stay with the payment provider.
- Desktop notifications fire when a conversation turn or requirement finishes. Click the toast to jump back into Flowy.
- Video canvas adds follow-up tools (batch connect, folders, timeline, and subtitles) and full zh-CN / en-US UI.
- Vimax Agent video can optionally set a 5–300s film duration; when the switch is off, planning still chooses length from the story.
- Session Logs keep the original user-message preview in summaries.

## v1.0.4 - 2026-08-20

- Session Logs reclaim disk with quota-only GC (about 1 GiB / 800 MiB, no age TTL) and show request `messages` / `tools` as scan lists by default.

## v1.0.3 - 2026-08-20

- Workspace Files, Changes, Shell, and the collaboration canvas open as mixed tabs in the preview strip; closing a tab restores the previous view.
- Vimax splits long scripts into scenes, resumes from the real checkpoint, and keeps recent projects when the session index is damaged.
- Capability hubs use quieter Discover/Installed chrome. Scheduled tasks open faster on first visit.

## v1.0.2 - 2026-08-19

- Session Logs replaces Agent Trace. Developer mode inspects conversations; closing it hides the inspector but recording continues.
- Vimax can imitate actions from a character still and a reference video.
- MiniMax-H3 video generation chains continuity across scenes.
- Context is compacted automatically before the model window overflows.
- The skill market shows package icons and installs SkillHub COS zips.

## v1.0.1 - 2026-08-18

- Settings keep presets, skills, MCP, and plugins in one hub with shared search. The hub also opens faster by deferring Markdown and market work.
- Flowy Cloud TTS and new companions default to qwen3-tts.
- Vimax keeps a shared production look across portraits, environments, and props, and the storyboard studio is easier to preview and edit.
- Home model switching works again. Streaming chat stays pinned while thinking or tool output grows.
- Memory citation protocol blocks stay out of the visible answer.
- Failed turns show a clearer error card and diagnostic dialog. You can copy diagnostics, see whether to retry or fix setup, and edit-then-resend without losing the draft or attachments. Secrets stay redacted.
- Knowledge tag management reports color and reorder failures instead of failing silently.
- Windows coding shells start over pipes instead of ConPTY, so noninteractive jobs are quieter and faster.
- OpenAI-compatible gateways that reject tool schemas now trigger the sanitization retry. Read/Edit path errors no longer look like a broken Windows drive letter.

## v1.0.0 - 2026-08-18

- Fixed Windows/Linux frameless titlebar drag and maximize. Titlebar and sidebar tooltips stay inside the viewport.
- Home and Settings layout, theme, and navigation are more stable. Capability markets open details in-app.
- Web search cold start is less likely to fail early. Usable evidence is labeled separately from untrusted embedded instructions.
- Editing or retrying a message keeps drafts and attachments after timeouts, dropped streams, and remounts.
- Fixed duplicate plain-text paste after switching conversations. Desktop streaming Markdown stays visually stable while new tokens arrive.
- AutoWork uses the conservative rule when sidecar confidence is low, instead of applying the bypass decision as-is.

## Release Note Policy

Every public release should include:

- User-facing changes.
- Breaking configuration or data migration notes.
- Security-relevant changes.
- Packaging and updater notes.
- Known limitations.

Use semantic versions consistently (`vX.Y.Z`) for public releases.
