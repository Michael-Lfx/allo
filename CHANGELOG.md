# Changelog

Flowy 1.0.0 is the first public release. This file records user-facing notes
at a high level rather than a complete commit log.

## Unreleased

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
