# Changelog

This page records what changed in every **published** release of `@flowy-agent-store/*`, and it is the **single announcement surface for breaking changes**.

Release facts (publish times, dist-tag targets, how artifact differences were verified) live in the [Upgrade and migration guide](/en-US/docs/upgrade); this page answers only "what changed in each version".

## 1. Scope of this page

- **Published npm versions only**: `@flowy-agent-store/protocol`, `client`, `sdk`, plus the `@flowy-agent-store/runtime-*` platform packages shipped alongside the sdk.
- **Unpublished work never becomes an entry here**: differences that exist in the working tree but have not shipped in any release are recorded only in §8 of the [Upgrade and migration guide](/en-US/docs/upgrade) and in §4 below.
- **No dates are promised**: this page never says "coming soon" or "planned for"; entries are appended only **after** a release.
- Version-number semantics and the compatibility stance (no backward compatibility during beta, breaking changes take a minor number) are in §1 of the [Upgrade and migration guide](/en-US/docs/upgrade) and in §3 below.

## 2. Published versions (facts)

All three packages and the platform runtime packages currently share one version set. To reproduce:

```bash
npm view @flowy-agent-store/sdk versions dist-tags time --json
```

```json
{
  "versions": ["0.1.0-beta.2", "0.1.0-beta.3", "0.1.0"],
  "dist-tags": { "beta": "0.1.0-beta.3", "latest": "0.1.0-beta.2" },
  "time": {
    "0.1.0": "2026-09-09T09:09:04.795Z",
    "0.1.0-beta.2": "2026-09-09T09:27:36.122Z",
    "0.1.0-beta.3": "2026-09-10T04:44:34.609Z"
  }
}
```

| Version | Released (UTC) | Change type | Current dist-tag |
| --- | --- | --- | --- |
| `0.1.0-beta.3` | 2026-09-10 | Additive (no breaking change) | `beta` |
| `0.1.0-beta.2` | 2026-09-09 | Additive (no breaking change) | `latest` |
| `0.1.0` | 2026-09-09 | First release | none |

The `versions` array is **not** in chronological order: `0.1.0` is listed last but was the **earliest** release (about 18 minutes before `beta.2`), and it carries no dist-tag. It is not a stable release and it is not newer than the beta line — for dist-tag semantics see §3 of the [Upgrade and migration guide](/en-US/docs/upgrade).

### 2.1 `0.1.0-beta.3` — 2026-09-10T04:44:34Z

- **client**: new public declarations `TransportLifecycle`, `onLifecycle`, `connectTimeoutMs`, and `rearm()` on the subscription state machine.
- **sdk**: new public declarations `assertProtocolCompatible`, `SpawnOptions.env` / `cwd` / `onExit`, `SpawnedServer.exited`, `SpawnExitInfo`.
- **`package.json` metadata**: client and sdk gained `engines.node >= 22`, `repository`, `sideEffects`.
- **protocol**: type declarations **unchanged, byte for byte**.
- **Upgrade impact**: moving up from `0.1.0-beta.2` requires **no code changes**; steps are in §6.1 of the [Upgrade and migration guide](/en-US/docs/upgrade).

### 2.2 `0.1.0-beta.2` — 2026-09-09T09:27:36Z

- **sdk**: `optionalDependencies` filled in, pinning five platform runtime packages at the same version — `runtime-win32-x64`, `runtime-linux-x64`, `runtime-linux-arm64`, `runtime-darwin-x64`, `runtime-darwin-arm64`.
- The earlier `0.1.0` lacks that dependency set, so `launchClient` could fail to find the executable (see §6.2 of the [Upgrade and migration guide](/en-US/docs/upgrade)).
- This is the version `latest` currently points at — **it is not the newest one**.

### 2.3 `0.1.0` — 2026-09-09T09:09:04Z

- **First release**: the first public version of all three packages; there was no earlier version to break, so it carries no breaking changes.
- The three packages' **code and type declarations are byte-for-byte identical to `0.1.0-beta.2`**; the only difference is `package.json` — the sdk had **no** `optionalDependencies` at that point.
- Carries no dist-tag.

> Those "substantive differences from the previous version" come from byte-for-byte comparison of artifacts fetched with `npm pack`; the raw records are row R4 of plan doc `16` and the R4 landing note in `21`. This page only summarises them and adds no new evidence.

## 3. Change types and the breaking-change announcement rule (D10=A)

Stance: **no backward compatibility is promised during beta**, and breaking changes **take a minor number** (`0.1.x` → `0.2.0`; there is no stable release to break yet, hence no major numbers).

| Change type | Version number | Action on this page |
| --- | --- | --- |
| First release | the version itself | add an entry marked "first release" |
| Additive (new declarations, new dependencies, metadata) | patch / pre-release counter | add an entry marked "additive (no breaking change)" |
| Breaking (type narrowing, methods added or removed, event-surface changes) | **minor number** | add an entry marked "breaking" with the migration path |

**This page is the only announcement surface for breaking changes**: they are not announced through commit messages, chat history or release pages — only an entry here marked "breaking" with a migration path counts as an announcement (version-number rules in §1 of the [Upgrade and migration guide](/en-US/docs/upgrade)).

Rules for adding and correcting entries:

- **Adding**: appended **after** a version is published to npm; no advance-notice entries.
- **Never rewritten**: the version number and publish date of a published entry are not edited once written.
- **Corrections**: if an entry is wrong, a `Correction (date)` line is appended in place; history is not silently edited.
- **Moving a dist-tag is not an entry**: which version `latest` / `beta` points at is release state, recorded in §3 of the [Upgrade and migration guide](/en-US/docs/upgrade).

## 4. Unpublished changes and release cadence

This page covers published versions only. The working tree (`web/packages/*`) already contains changes that **have not shipped in any release**:

| Item | Status | Evidence |
| --- | --- | --- |
| `event_type` narrowed from an open union (with a `\| string` escape hatch) to the closed type `ConversationEventType`, plus an in-package decoder | in the working tree, **unpublished**: `\| string` is still present in all three published versions | §8 of the [Upgrade and migration guide](/en-US/docs/upgrade); row R1 of `16` |
| Wire-method additions (`run/plan`, `config/get`, `config/set`, `run/answer-decision`, marketplace entry snapshots, …) | in the working tree, **unpublished**: the published protocol artifact contains none of these types | rows R2 / R8 / R10 / R16 of `16` |
| Release rebuild and `beta.4` | **on hold** (stated policy: release-consistency close-out deferred); no date is announced here | row R6 of `16` |

So: **never assume a change has shipped unless it is listed on this page; and never assume the items in §4 have shipped.**

## 5. See also

- [Upgrade and migration guide](/en-US/docs/upgrade): release-fact table, dist-tag semantics, pinning exact versions, per-version upgrade steps and self-check commands.
- [TypeScript SDK guide](/en-US/docs/typescript-sdk): install, API, events and error model; its §1 states the current version status.
- [Quick start](/en-US/docs/quick-start): the installer path for end users.
