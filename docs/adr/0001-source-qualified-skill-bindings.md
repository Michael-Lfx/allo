# ADR-0001: Preserve Ambiguous Current-V3 Preset Skill Bindings

Status: Accepted

Date: 2026-07-31

## Context

Preset Skill bindings created before source-qualified catalog identities stored
only a display name. A name can now refer to more than one catalog entry, such
as `builtin:writer` and `user:writer`. Assigning either source during the
identity transition would silently change a user's Preset behavior. Dropping
the binding would also violate the product decision that Presets retain their
Skill bindings independently of the `/` launcher.

The normal v3 data contract rejects historical dataset migrations and legacy
mappers. This decision is a narrowly approved release transition for rows
inside the active v3 dataset; it does not import, retain, or query retired
datasets.

## Decision

The `preset_skill_bindings.skill_name` column is renamed to `skill_id` without
rewriting ambiguous values. A stored bare name is represented in the service
boundary as `legacy:<percent-encoded-name>`.

`legacy:` is not a catalog source and is never returned from
`GET /api/skills/catalog`, offered by `/`, or used to resolve a new explicit
Skill load. It is only a lossless marker for an already-persisted ambiguous
binding. At runtime, it follows the previous name-based resolver and workspace
linking path. Canonical source-qualified IDs always use the immutable
`SKILL.md` snapshot path and are never downgraded to a name.

The compatibility marker remains only until the user replaces that binding
with a catalog selection. That new write persists a canonical source-qualified
ID. The system must not guess a source in the meantime.

## Consequences

- Existing Presets keep their prior name-resolution behavior instead of being
  rebound to an arbitrary same-name Skill.
- New UI selections and explicit loads use only catalog identities.
- The exception is limited to active-v3 Preset rows and has focused migration
  and runtime tests.
- Conversation skill-load history is separate: it stores the exact loaded
  content and hash, so later Skill edits cannot change historical context.
