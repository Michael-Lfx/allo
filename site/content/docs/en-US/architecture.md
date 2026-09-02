# Architecture

Flowy Agent Store is a **local-first** layered system: the cloud manages *definitions*, your machine owns *execution*.

## Layers

```text
CodeBuddy / WorkBuddy source directories
        ↓ Importer (path / digest / compatibility checks)
PluginSnapshot (versioned, immutable)
        ↓ Catalog
Agent / Team / Skill / Connector definitions
        ↓ Runtime Adapter (domain → allo execution semantics)
allo Runtime (Rust, the only execution engine)
        ↓ Versioned App Server Protocol
TS SDK · CLI · Web UI (protocol clients)
```

## Key boundaries

- **Runtime**: `allo` is the only execution engine — single file, small footprint, low resource use.
- **App Server protocol**: SDK, CLI and Web all depend on it; it exposes versioned, typed lifecycle and event messages. Public IDs are opaque and never expose internal execution IDs.
- **Runtime Adapter**: the sole boundary from domain models to `allo` internals; a Team's Planning Context is derived at runtime and never contains full member prompts or real credentials.
- **Credentials**: stored only in local secure storage; Web / SDK see status, account identifiers and expiry — never the real token.

## Agent Team (V1)

Fixed members + Leader planning-context driven **planned DAG**:

- The Leader is a planning role; it does not own a separate conversation.
- Independent Steps support local parallelism, retries and replan.
- Events, Artifacts and terminal state are persisted, queryable and replayable.

## Principles

- Verify a single Agent before building Teams.
- The event log is the single source of truth; projections are derived.
- Capabilities that fail Runtime verification are never marked runnable.
