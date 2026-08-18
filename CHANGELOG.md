# Changelog

Flowy 1.0.0 is the first public release. This file records user-facing notes
at a high level rather than a complete commit log.

## Unreleased

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
