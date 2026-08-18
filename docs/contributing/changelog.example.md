# CHANGELOG format example

This is a **format guide**, not the project history. The file you maintain is
[`CHANGELOG.md`](../../CHANGELOG.md) at the repo root. Do not copy the sample
bullets below into the real changelog.

What belongs in a user-facing note is defined in
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) under "Changelog And Release Notes".
How the updater picks up this text is documented in
[`BUILD_RELEASE.zh-CN.md`](../../BUILD_RELEASE.zh-CN.md).

## What this answers

| Question | Answer |
| --- | --- |
| Where to write | Root `CHANGELOG.md` only |
| During a PR | Add one bullet under `## Unreleased` |
| Before a release | Fold Unreleased into `## vX.Y.Z - YYYY-MM-DD` |
| What the update dialog shows | Everything under that version heading until the next `##` |
| Can I skip it when packaging | Yes. OTA still works. The dialog then shows `Flowy vX.Y.Z` or the previous released section |

## Skeleton (annotated)

The notes on the right are for this example only. Do not put them in the real
`CHANGELOG.md`.

```markdown
# Changelog                          ← file title; already present, leave it

A short intro paragraph.             ← optional. The real file has a pre-1.0 note.

## Unreleased                        ← always keep this heading.
                                     ← Daily entries go here. No version in the title.
                                     ← make:latest skips this section.

- Fixed duplicate plain-text paste   ← PR-time: one short, user-visible sentence.
  after switching conversations.
- Stabilized Home and Settings layout.

## v0.0.4 - 2026-08-18               ← release heading. Must contain the Cargo.toml
                                     ← version (0.0.4 here). Date is recommended,
                                     ← not required for extraction.

- Fixed Windows/Linux titlebar drag. ← user-visible change, one topic per bullet.
- Made web-search cold start less likely to fail early.
- **Breaking:** public companions    ← only if something breaks. Otherwise omit.
  are replaced by customer service.
  Existing configs are not migrated.
- Packaging note: Windows-only this  ← desktop OTA / installer caveats. Omit if none.
  time.

## v0.3.3 - 2026-07-30               ← history. Append new sections; do not rewrite
                                     ← old meaning.
```

## What each part means

| Part | Required? | Meaning |
| --- | --- | --- |
| `# Changelog` | Already in the file | Document title |
| `## Unreleased` | **Keep it during development** | Changes not yet assigned to a version. Move them before tagging; the heading can stay empty |
| `## vX.Y.Z - YYYY-MM-DD` | **Required at release** | The update notes for that version. `X.Y.Z` must appear in the heading and match `[workspace.package].version` in root `Cargo.toml` |
| Normal `-` bullets | Required when the version has user-visible changes | Features, behavior changes, noticeable bugfixes |
| `**Breaking**` / `**BREAKING**` | Only when something breaks | Config, data, API, or workflow the user must migrate |
| `Packaging note` | Recommended for desktop releases | Which platforms shipped, signing / SmartScreen, install limits |
| Security bullets | Only when relevant | Fixes or tightening operators/users should know |
| Known limitations | Only when relevant | What this shipped version explicitly does not do |

Do **not** title a release `## Unreleased 0.0.4` or `## Update notes`. Extraction only accepts a heading that:

1. starts with `##`;
2. contains the current version string (for example `0.0.4`);
3. is not `## Unreleased`.

If no heading matches, the script falls back to the **first non-Unreleased `##` section** — usually the previous release, which is the wrong text.

## How complete it has to be

Three levels. **Packaging does not check the changelog.**

### Level 0 — package and OTA still work (dialog may be wrong)

You can write nothing. `make:latest` fills `notes` in this order:

1. `--notes` / `--notes-file` if you passed them (CI does not)
2. existing `notes` on the same-version manifest
3. the matching `CHANGELOG.md` section
4. otherwise `Flowy vX.Y.Z`

### Level 1 — the dialog shows notes for *this* version (minimum for correct OTA copy)

Before tagging, `CHANGELOG.md` needs at least:

```markdown
## v0.0.4 - 2026-08-18

- One sentence the user can understand.
```

Requirements:

- the heading contains the version, matching the tag / `Cargo.toml` version;
- the body under that heading is non-empty (one bullet is enough);
- do not tag while the new copy still sits only under `## Unreleased` — that section is skipped.

### Level 2 — recommended for a public release (repo policy)

Include what exists; do not invent empty sections:

1. User-visible features / behavior / bugfixes (short bullets, not a dump of Unreleased)
2. Breaking config, data, API, or workflow (say what the user must do)
3. Security-relevant changes
4. Packaging / updater notes (which OS shipped, signing)
5. Known limitations

Done when a user can tell in ten seconds **whether to update and whether they must migrate**. Not when every implementation detail is listed.

## How to write one bullet

| | Guidance |
| --- | --- |
| Length | One or two sentences. The dialog shows the whole section |
| Subject | User-visible result ("fixed duplicate paste"), not filenames, PR numbers, or modules |
| One topic | Do not pack five subsystems into one bullet |
| Language | The real `CHANGELOG.md` is currently English; the dialog shows the text as-is. Either language is fine; keep one language per section |
| Avoid | Commit lists, `feat:` prefixes, function names, test names, review discussion |

**Too detailed:**

```markdown
- AutoWork/IDMM: bypass-model (sidecar) decisions with confidence below 0.4
  now fall back to the conservative rule action. Previously the floor was 0.0.
```

**Enough:**

```markdown
- AutoWork now uses the conservative rule when sidecar confidence is low,
  instead of applying the bypass decision as-is.
```

## Moving Unreleased at release time

1. Confirm the `Cargo.toml` version, for example `0.0.4`.
2. Insert `## v0.0.4 - 2026-08-18` **below** `## Unreleased`.
3. Move the bullets that ship in this version under the new heading and shorten them.
4. Leave `## Unreleased` empty (or keep only items meant for a later version).
5. Then push the platform tag, for example `v0.4.2-windows`. The `X.Y.Z` in the tag must match this heading and Cargo.

CI `make:latest` will not copy `## Unreleased` into `latest.json`.
