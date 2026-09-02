# Compatibility matrix

## Platforms

The single-file runtime covers the platforms below; binaries are distributed on [GitHub Releases](https://github.com/your-org/flowy-agent-store/releases) under per-target names.

| OS | Architecture | Status |
| --- | --- | --- |
| macOS | Apple silicon (aarch64) | Supported |
| macOS | Intel (x86_64) | Supported |
| Windows | x86_64 | Supported |
| Linux | x86_64 | Supported |
| Linux | aarch64 | Supported |

> The download button detects your system automatically; all platforms are also available manually on the releases page.

## Source formats

| Source | Import path | Compatibility |
| --- | --- | --- |
| CodeBuddy Plugin | Importer → PluginSnapshot | compatible / compatible-with-adapter |
| WorkBuddy Skill | Importer → PluginSnapshot | compatible |
| WorkBuddy Connector | Importer → PluginSnapshot | compatible / manual-review |
| Unconfirmed-license resources | marked `pending-legal-review` | excluded from public distribution |

## Connectors

- At least one MCP Connector can discover tools and perform controlled calls.
- OAuth uses standard PKCE Loopback; credentials enter secure storage and are injected at request time.
- When login succeeds but calls don't, the connector is marked `partial` and never shows `connected`.

## Non-goals (V1)

Cloud execution, multi-tenancy, HA, a full Marketplace review backend, a signed-update system, and arbitrary Hook/bin execution are all out of V1 scope.
