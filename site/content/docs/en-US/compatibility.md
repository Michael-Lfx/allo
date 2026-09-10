# Compatibility matrix

## Platforms

Only **Windows x64** builds are published today; every other platform is not available yet and needs a separate decision before it opens up. Binaries are distributed on the [download center](http://111.170.173.22:10014/downloads/) under per-target names.

| OS | Architecture | Status |
| --- | --- | --- |
| Windows | x86_64 | Published |
| macOS | Apple silicon (aarch64) | Not available |
| macOS | Intel (x86_64) | Not available |
| Linux | x86_64 | Not available |
| Linux | aarch64 | Not available |

> The download button detects your system: only Windows x64 gets a direct link; other platforms are routed to the download center.

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
