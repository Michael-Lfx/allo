# Configuration file (config.toml)

Agent Store keeps all long-lived preferences in `~/.agent-store/config.toml` (TOML): the default model, API providers, model aliases and default marketplace sources. Change it once, it applies on every start.

- **Default location**: `~/.agent-store/config.toml` (`%USERPROFILE%\.agent-store\config.toml` on Windows).
- **Optional file**: without it the App Server runs fine, but model calls need providers created manually in the Web UI first; with it, declared providers, model defaults and marketplaces are auto-registered on startup.

> This file follows the user-level `config.toml` convention of Claude Code / Codex / Kimi Code (`[providers.<name>]` + `[models."<provider>/<model>"]`). **Unknown top-level and nested keys are tolerated** — the file is yours and may carry settings for other tools (Kimi Code's `thinking`, `permission`, `hooks`, …) without breaking parsing.

## Full example

```toml
default_model = "opencode/mimo-v2.5-free"

[providers.opencode]
type = "openai"
api_key = "sk-..."
base_url = "https://opencode.ai/zen/v1"

[providers.mimo]
type = "openai"
base_url = "https://api.xiaomimimo.com/v1"

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
display_name = "MiMo V2.5 Free"
max_context_size = 200000
max_output_size = 8000
capabilities = ["thinking", "tool_use"]

[models."opencode/laguna-s-2.1-free"]
provider = "opencode"
model = "laguna-s-2.1-free"
max_context_size = 256000
display_name = "Laguna S 2.1 Free"

# Default marketplace sources (auto-registered on first store/market call)
[default_marketplaces.experts]
source_kind = "url"
source = "http://111.170.173.22:10072/experts/.codebuddy-plugin/marketplace.json"
```

## Top-level fields

| Field | Type | Description |
| --- | --- | --- |
| `default_model` | `string` | Default model alias in `"<provider>/<model>"` form; must resolve through `[models]`; used when the caller sends no model |
| `providers` | `table` | API provider table → `providers` |
| `models` | `table` | Model alias table → `models` |
| `default_marketplaces` | `table` | Marketplace sources auto-registered on startup → `default_marketplaces` |

## `providers`

A table keyed by unique provider name. Agent Store reads credentials **from here only** — no shell environment fallback.

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `type` | `string` | no | Provider type (maps to the runtime platform), e.g. `openai`, `anthropic` |
| `api_key` | `string` | no | API key, written in plain text in the config file |
| `base_url` | `string` | no | API base URL |
| `enabled` | `boolean` | no | Enables auto-registration; treated as enabled when omitted |

> Key safety: `api_key` is read only for the encrypted provider registration, then dropped — it never appears in logs or tracing payloads. Consider tightening file permissions yourself (`chmod 600`).

## `models`

A table keyed by `"<provider>/<model>"`. `provider` must point to a key declared in `[providers]`.

| Field | Type | Description |
| --- | --- | --- |
| `provider` | `string` | Owning provider key (required) |
| `model` | `string` | Model name sent upstream (required) |
| `display_name` | `string` | Display name in the UI |
| `max_context_size` | `integer` | Context window limit (tokens) |
| `max_output_size` | `integer` | Max output per response (tokens) |
| `capabilities` | `array<string>` | Capability flags, e.g. `["thinking", "tool_use", "image_in"]` |
| `reasoning_key` | `string` | Field name of the reasoning content in responses, e.g. `reasoning_content` |

## `default_marketplaces`

Default marketplace sources (winget-style software sources). The App Server auto-registers them on the first `store` / `market` call; an existing id is reactivated with the new source.

| Field | Type | Description |
| --- | --- | --- |
| `source_kind` | `string` | `url` \| `github` \| `git` \| `directory` |
| `source` | `string` | Concrete address; for `url` kinds also provide a directory listing (`_files.txt`) so entry trees can mirror over HTTP |

```toml
[default_marketplaces.experts]
source_kind = "url"
source = "http://111.170.173.22:10072/experts/.codebuddy-plugin/marketplace.json"

[default_marketplaces.workbuddy-skills]
source_kind = "url"
source = "http://111.170.173.22:10072/skills/.codebuddy-skill/marketplace.json"

[default_marketplaces.connectors]
source_kind = "url"
source = "http://111.170.173.22:10072/connectors/.codebuddy-connector/connectors.json"
```

## Differences from Kimi Code / Claude Code configs

| Dimension | Agent Store | Kimi Code etc. |
| --- | --- | --- |
| Location | `~/.agent-store/config.toml` | `~/.kimi-code/config.toml` etc. (per-tool directories) |
| `[providers]` / `[models]` | Same shape: `type`/`api_key`/`base_url`, `provider`/`model`/`max_context_size`/`capabilities` | Same shape |
| `default_model` | `"<provider>/<model>"` alias | Same |
| Unknown keys | Tolerated, no error | Tolerated |
| Env fallback | **None** — credentials come from the file only | Some tools support `env`-subtables / env fallbacks |
| Agent Store only | `default_marketplaces` | — |

If your config already has `[providers]` / `[models]` sections from Kimi Code or another tool, you can **copy them directly** into `~/.agent-store/config.toml` (as long as the provider speaks an OpenAI/Anthropic-compatible protocol); unrelated sections (`thinking`, `permission`, `hooks`, …) can stay or go — Agent Store ignores them.