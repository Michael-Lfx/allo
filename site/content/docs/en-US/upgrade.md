# Upgrade and migration guide

This page answers three questions: **can I upgrade**, **which version do I take**, and **how do I do it**. It covers `@flowy-agent-store/{protocol,client,sdk}` and the platform runtime packages that ship alongside them.

Release facts and upgrade steps live here; the full method semantics stay in the repo file `docs/agent-store/05-allo-app-server-protocol.md`.

## 1. Compatibility promise (current stance)

**No backward compatibility is promised during beta.** All three packages are `0.1.0-beta.*` pre-releases with an unfrozen API: type narrowing, methods added or dropped, and event-surface changes can all happen inside the beta line.

Two rules go with that (decided as D10=A):

- a breaking change ships under a **minor** number (`0.1.x` → `0.2.0`) — there is no stable release to break yet, so no major number is used;
- every release states its change type **in the [changelog](/en-US/docs/changelog)** (see §9).

Two consequences for your daily work:

- pin an exact version in production (see §5) instead of trusting what a bare `bun add` / `npm install` resolves to (see §4);
- read the [changelog entry](/en-US/docs/changelog) for the target version first, then follow the per-version steps in §6.

## 2. Published release sequence (facts)

The table is derived from registry metadata and from the published artifacts (commands to reproduce are in §8):

| Version | Published (UTC) | dist-tag today | Difference from the previous release |
| --- | --- | --- | --- |
| `0.1.0` | 2026-09-09T09:09:04Z | none | first publish; for all three packages the **code and type declarations are byte-identical to beta.2**, only `package.json` differs: the sdk had no `optionalDependencies` then (no platform runtime packages attached) |
| `0.1.0-beta.2` | 2026-09-09T09:27:36Z | `latest` | adds the sdk `optionalDependencies` (five platform runtime packages for darwin / linux / win32, same version) |
| `0.1.0-beta.3` | 2026-09-10T04:44:34Z | `beta` | adds public declarations to client / sdk (reconnect lifecycle, exit observation, see §6.1); `package.json` gains `engines.node >= 22`, `repository` and `sideEffects`; the protocol declarations are unchanged byte for byte |

Note that `0.1.0` is the **earliest** publish (about 18 minutes before beta.2) and yet no dist-tag points at it; it is neither a stable release nor newer than the beta line.

## 3. What dist-tags mean

A `dist-tag` is a label the publisher can move; it says nothing about version ordering:

| dist-tag | Points at today |
| --- | --- |
| `latest` | `0.1.0-beta.2` |
| `beta` | `0.1.0-beta.3` |

`0.1.0` carries no tag at all. The three packages and `@flowy-agent-store/runtime-*` agree on both tags (verified).

```bash
npm view @flowy-agent-store/sdk versions dist-tags --json
```

```json
{
  "versions": ["0.1.0-beta.2", "0.1.0-beta.3", "0.1.0"],
  "dist-tags": { "beta": "0.1.0-beta.3", "latest": "0.1.0-beta.2" }
}
```

Two traps:

1. `latest` is **not** the newest version — it points at `0.1.0-beta.2`, while the newest beta is `0.1.0-beta.3` under the `beta` tag.
2. `0.1.0` has no tag, but a version range can still resolve to it (see §4).

Also note that the `versions` array is **not** in chronological order: `0.1.0` is listed last and was published first.

## 4. Bare installs and range resolution

A bare install follows `latest`, so it lands on `0.1.0-beta.2`; worse, bun writes a **range** into `package.json` (`^0.1.0-beta.2`), so the next install resolves again — and one range does not resolve the same way in every tool.

Measured in throwaway directories with an empty `node_modules`:

```bash
bun add @flowy-agent-store/sdk                     # → 0.1.0-beta.2 (latest)
bun add @flowy-agent-store/sdk@beta                # → 0.1.0-beta.3
bun add '@flowy-agent-store/sdk@^0.1.0-beta.2'     # → bun resolves 0.1.0-beta.2
npm view '@flowy-agent-store/sdk@^0.1.0-beta.2' version   # → npm resolves 0.1.0-beta.3
npm view '@flowy-agent-store/sdk@>=0.0.0' version          # → 0.1.0 (the untagged early publish)
```

Conclusion: **do not rely on range resolution**. A tag alias is no safer — `latest` moves on the next publish (so does `beta`), so `@beta` written today may install another version tomorrow. Write the exact version into `package.json`.

## 5. Pin the exact version (recommended)

```bash
# Write the exact version; avoid ^ and ~
bun add @flowy-agent-store/sdk@0.1.0-beta.3
bun add @flowy-agent-store/protocol@0.1.0-beta.3   # when you import wire types

# same for npm / pnpm
npm install @flowy-agent-store/sdk@0.1.0-beta.3
```

`package.json` should end up with `"@flowy-agent-store/sdk": "0.1.0-beta.3"` (**no** `^`). The sdk's platform runtime packages are pinned to the same version by its `optionalDependencies`, so they need no separate entry.

Commit the lockfile too: `bun.lock` / `package-lock.json` / `pnpm-lock.yaml` is the only authoritative record of what an install actually pulled.

An upgrade is then: change that one string, reinstall, re-run your tests. Nothing else.

## 6. Per-version upgrade steps

### 6.1 From 0.1.0-beta.2 to 0.1.0-beta.3

This is the only real beta-to-beta jump so far and it needs **no code change**: the measured difference is added declarations only (client: `TransportLifecycle` / `onLifecycle` / `connectTimeoutMs` / subscription `rearm()`; sdk: `assertProtocolCompatible`, `SpawnOptions.env` / `cwd` / `onExit`, `SpawnedServer.exited`, `SpawnExitInfo`) plus `package.json` metadata; the protocol declarations are unchanged byte for byte.

```bash
# 1) see what is actually installed
bun pm ls | grep '@flowy-agent-store'          # npm projects: npm ls @flowy-agent-store/sdk
# 2) pin the target version (upgrade the packages you use, on one version)
bun add @flowy-agent-store/sdk@0.1.0-beta.3
bun add @flowy-agent-store/protocol@0.1.0-beta.3
# 3) confirm what got resolved
node -p "require('@flowy-agent-store/sdk/package.json').version"
# 4) re-run your own type check and tests
bun run typecheck && bun run test
```

If you point `AGENT_STORE_BIN` at a self-built binary, note that the SDK **checks the readiness line's `protocol_version` against its own** (see the TypeScript SDK guide §2): after upgrading the SDK an older binary is rejected, so update the binary in the same step.

### 6.2 From 0.1.0 back onto the beta line

`0.1.0` was the first publish and carries no dist-tag; for all three packages its **code and type declarations are byte-identical to `0.1.0-beta.2`**, and the only difference is `package.json` — in particular the sdk at that time had **no** `optionalDependencies`, so it does not bring `@flowy-agent-store/runtime-win32-x64` or the other platform runtime packages, and `launchClient` may fail to find an executable.

```bash
# move off the untagged 0.1.0 onto the current beta line
bun add @flowy-agent-store/sdk@0.1.0-beta.3
bun pm ls | grep '@flowy-agent-store'   # confirm 0.1.0 is gone
```

This move is additive as well: between `0.1.0` and `beta.3` the public declaration surface only grew (the runtime packages added in `beta.2`, the reconnect and exit observation added in `beta.3`) — nothing was removed or renamed.

## 7. Check which version you actually have

```bash
# what the project resolved (start here)
bun pm ls | grep '@flowy-agent-store'      # npm: npm ls @flowy-agent-store/sdk
# read the installed package.json (all three packages export ./package.json)
node -p "require('@flowy-agent-store/protocol/package.json').version"
# what the lockfile pinned
grep -o '@flowy-agent-store/sdk@[0-9][^"]*' bun.lock | head -1
# what the registry offers, and where the tags point
npm view @flowy-agent-store/sdk versions dist-tags --json
```

When the three disagree, trust the **lockfile and the installed `package.json`**: the range in `package.json` states intent, what landed in `node_modules` is the fact.

## 8. Unpublished differences (working tree) and how to check them

The working tree (`web/packages/*`) reports version `0.1.0-beta.3` as well, but it already contains an **unpublished breaking change**: `event_type` is narrowed from an open union with a `| string` escape hatch into the closed type `ConversationEventType`, plus the in-package decoder `decodeConversationEvent`.

Verified boundary: **none of the three published versions has that narrowing** — reproduce it from the registry artifacts:

```bash
npm pack @flowy-agent-store/protocol@0.1.0-beta.3 --silent
tar xzf flowy-agent-store-protocol-0.1.0-beta.3.tgz
grep -n 'event_type' package/dist/index.d.mts
# 0.1.0 / 0.1.0-beta.2 / 0.1.0-beta.3 all ship an index.d.mts of 20049 bytes, identical byte for byte
# event_type: "message.created" | ... | "context.usage" | string;   ← the | string is still there
```

What that means: code that treats `event_type` as `string` (building its own string comparisons, or relying on a `switch` default to absorb unknown kinds) compiles and runs today; once the narrowing ships, exhaustive switches and type guards start failing type check and you must handle the closed union. It will **not** happen silently — it is a breaking change, so it ships under a minor number with a changelog entry per §1. No release date is announced here.

## 9. Changelog and release notes boundary

| Item | Status | Basis |
| --- | --- | --- |
| Versioning of breaking changes | minor number, stated in the changelog | the compatibility stance D10=A (no backward compatibility during beta) |
| Standalone changelog / release notes page | ✅ **built**: [Changelog](/en-US/docs/changelog) | item R6 of plan doc `16` (shipped in batch 4) |
| This page's job | record published release facts and verified unpublished differences | no invented release history |
| Division of labour | this page explains **how to upgrade**; the changelog records **what changed in each version** and is the only announcement surface for breaking changes | the two pages cross-reference instead of duplicating |

So this page states **what happened** (publish times, dist-tags, artifact differences) and **what the working tree has confirmed but not shipped** (§8). For any change not listed here, do not assume it has happened, and do not assume it cannot happen.

## 10. See also

- [TypeScript SDK guide](/en-US/docs/typescript-sdk): install, API, events, error model and examples.
- [Quick start](/en-US/docs/quick-start): the installer path for end users.
- [Compatibility matrix](/en-US/docs/compatibility): supported platforms and source formats.
- [Changelog](/en-US/docs/changelog): what changed in each published version, and the announcement surface for breaking changes.
