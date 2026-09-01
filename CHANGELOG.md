# Changelog

Flowy 1.0.0 is the first public release. This file records user-facing notes
at a high level rather than a complete commit log.

## Unreleased

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
