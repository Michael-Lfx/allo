//! Host-owned Nomi tool policy — the typed form of `~/.agent-store/config.toml`
//! `[tools]` (`docs/agent-store/20-tool-injection-policy.zh.md`).
//!
//! This is **host policy, not client input**: it is injected through the agent
//! factory's process-owned dependencies, never through conversation `extra`
//! JSON, so no request can forge a grant. Every field is therefore either a
//! restriction or a no-op; there is no "force enable" switch that could widen
//! what the engine's own configuration already allows.
//!
//! Layering rule (invariant): the effective tool set is the **intersection** of
//! every layer that constrains it —
//! `(enabled 为空 ? 全放行 : enabled) ∧ ¬disabled`, further intersected with the
//! engine's `builtin_allowlist`/`builtin_denylist` and with any session-scoped
//! allowlist. An empty list means "unconstrained", never "deny everything".
//! [`NomiToolPolicy::overlay`] expresses the same rule between two policies.
//!
//! [`NomiToolPolicy::default`] is deliberately **identical to the behaviour
//! before this policy existed** (every family on, both lists empty), so a host
//! that never writes `[tools]` is byte-for-byte unchanged.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Product-domain switches. These families are wired by host sinks rather than
/// by tool names, so they cannot be expressed as entries in [`NomiToolPolicy::disabled`]:
/// when a sink is not wired the tools do not exist at all, and naming them would
/// be an unmatched (and therefore warning-worthy) entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NomiToolDomains {
    /// Native cron tools (`cron_create` / `cron_list` / `cron_delete`).
    #[serde(default = "default_true")]
    pub cron: bool,
    /// Meeting tools (`meeting.*`).
    #[serde(default = "default_true")]
    pub meeting: bool,
    /// Knowledge retrieval / write-back tools and knowledge mounts.
    #[serde(default = "default_true")]
    pub knowledge: bool,
    /// Learning course generation tools.
    #[serde(default = "default_true")]
    pub learning: bool,
    /// Host `[media]` generation tools.
    #[serde(default = "default_true")]
    pub media: bool,
    /// Companion memory / skill tools and in-session summon.
    #[serde(default = "default_true")]
    pub companion: bool,
    /// AutoWork requirement declaration tools.
    #[serde(default = "default_true")]
    pub requirement: bool,
    /// Goal-driven continuation and the `update_goal` tool.
    #[serde(default = "default_true")]
    pub goal: bool,
}

impl Default for NomiToolDomains {
    fn default() -> Self {
        Self {
            cron: true,
            meeting: true,
            knowledge: true,
            learning: true,
            media: true,
            companion: true,
            requirement: true,
            goal: true,
        }
    }
}

/// The host's tool policy for every Nomi session it builds.
///
/// Field names and matching rules follow the reference implementation
/// (Kimi Code CLI config-file `[tools]`): `enabled` selects, `disabled`
/// subtracts **after** `enabled`, builtin names match exactly and
/// case-sensitively, and only `mcp__…` patterns are treated as globs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NomiToolPolicy {
    /// Global allowlist. **Empty = unconstrained** (not "deny everything").
    /// Non-`mcp__` entries must name a tool exactly; `mcp__<server>__*` may be
    /// used to keep a whole Connector.
    #[serde(default)]
    pub enabled: Vec<String>,
    /// Global denylist, applied after `enabled`. Same matching rules.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Web search / extract tools (`WebSearch`, `WebExtract`).
    #[serde(default = "default_true")]
    pub web: bool,
    /// The `Computer` tool (host desktop control).
    #[serde(default = "default_true")]
    pub computer: bool,
    /// The `Browser` tool family.
    #[serde(default = "default_true")]
    pub browser: bool,
    /// Plan-mode tools (`EnterPlanMode` / `ExitPlanMode`).
    #[serde(default = "default_true")]
    pub plan: bool,
    /// The experimental `Lsp` navigation tool.
    #[serde(default = "default_true")]
    pub lsp: bool,
    /// Product-domain families.
    #[serde(default)]
    pub domains: NomiToolDomains,
}

impl Default for NomiToolPolicy {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
            disabled: Vec::new(),
            web: true,
            computer: true,
            browser: true,
            plan: true,
            lsp: true,
            domains: NomiToolDomains::default(),
        }
    }
}

impl NomiToolPolicy {
    /// Combine two policies so that **only the more restrictive one survives**:
    /// switches are ANDed and both lists are unioned.
    ///
    /// This is the whole "更严者胜" rule in one place — there is deliberately no
    /// inverse operation, because no layer in this system may widen another's
    /// restriction.
    pub fn overlay(&self, other: &Self) -> Self {
        Self {
            enabled: union_sorted(&self.enabled, &other.enabled),
            disabled: union_sorted(&self.disabled, &other.disabled),
            web: self.web && other.web,
            computer: self.computer && other.computer,
            browser: self.browser && other.browser,
            plan: self.plan && other.plan,
            lsp: self.lsp && other.lsp,
            domains: NomiToolDomains {
                cron: self.domains.cron && other.domains.cron,
                meeting: self.domains.meeting && other.domains.meeting,
                knowledge: self.domains.knowledge && other.domains.knowledge,
                learning: self.domains.learning && other.domains.learning,
                media: self.domains.media && other.domains.media,
                companion: self.domains.companion && other.domains.companion,
                requirement: self.domains.requirement && other.domains.requirement,
                goal: self.domains.goal && other.domains.goal,
            },
        }
    }

    /// True when this policy constrains nothing — i.e. the pre-policy
    /// behaviour. Callers may use it to skip work, never to skip a check.
    pub fn is_unrestricted(&self) -> bool {
        self.enabled.is_empty()
            && self.disabled.is_empty()
            && self.web
            && self.computer
            && self.browser
            && self.plan
            && self.lsp
            && self.domains == NomiToolDomains::default()
    }

    /// False when the denylist removes `ToolSearch`.
    ///
    /// The caller warns (it does not refuse): every MCP/Connector tool is
    /// advertised as a deferred stub that only `ToolSearch` can activate, so
    /// removing it makes the whole Connector catalog unreachable. Refusing
    /// outright would silently fall back to an unrestricted policy, which is
    /// worse than an explicit, warned-for configuration.
    pub fn allows_tool_search(&self) -> bool {
        !self.disabled.iter().any(|pattern| pattern == TOOL_SEARCH)
    }

    /// Non-fatal configuration problems, as caller-rendered messages.
    ///
    /// Only **syntactic** problems are detectable here. Whether an entry
    /// actually matches a registered tool depends on this session's wiring and
    /// is reported by the registry instead (see `ToolRegistry::deny_named`).
    ///
    /// Three cases, mirroring the reference implementation:
    /// 1. a wildcard outside the `mcp__` namespace — `enabled = ["*"]` then
    ///    matches nothing and **removes** every tool, while `disabled = ["*"]`
    ///    subtracts nothing;
    /// 2. an `mcp__` literal with no tool segment (`mcp__github`), which never
    ///    matches — a whole server is `mcp__github__*`;
    /// 3. `ToolSearch` in the denylist (see [`Self::allows_tool_search`]).
    pub fn syntax_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for (list, field) in [(&self.enabled, "enabled"), (&self.disabled, "disabled")] {
            for pattern in list {
                if !pattern.starts_with(MCP_PREFIX) && contains_wildcard(pattern) {
                    warnings.push(format!(
                        "[tools].{field} entry `{pattern}` uses a wildcard outside the `mcp__` \
                         namespace: builtin names match exactly, so this entry matches no tool \
                         ({}).",
                        if field == "enabled" {
                            "in an allowlist that removes every tool"
                        } else {
                            "in a denylist that subtracts nothing"
                        }
                    ));
                    continue;
                }
                if pattern.starts_with(MCP_PREFIX)
                    && !contains_wildcard(pattern)
                    && !pattern[MCP_PREFIX.len()..].contains("__")
                {
                    warnings.push(format!(
                        "[tools].{field} entry `{pattern}` is an `mcp__` name without a tool \
                         segment and can never match; use `{pattern}__*` for the whole server."
                    ));
                }
            }
        }
        if !self.allows_tool_search() {
            warnings.push(format!(
                "[tools].disabled removes `{TOOL_SEARCH}`: MCP/Connector tools are advertised as \
                 deferred stubs, so every Connector tool becomes unreachable."
            ));
        }
        warnings
    }
}

/// `ToolSearch` is the activation path for every deferred (MCP) tool.
const TOOL_SEARCH: &str = "ToolSearch";

/// The reserved prefix for MCP proxy tool names (`nomi-mcp::tool_proxy`).
const MCP_PREFIX: &str = "mcp__";

fn contains_wildcard(pattern: &str) -> bool {
    pattern.contains(['*', '?', '['])
}

fn union_sorted(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = left.iter().chain(right).cloned().collect();
    merged.sort();
    merged.dedup();
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_the_pre_policy_behaviour() {
        let policy = NomiToolPolicy::default();
        assert!(policy.is_unrestricted());
        assert!(policy.enabled.is_empty() && policy.disabled.is_empty());
        assert!(policy.web && policy.computer && policy.browser && policy.plan && policy.lsp);
        assert!(policy.allows_tool_search());
        assert!(policy.syntax_warnings().is_empty());
    }

    #[test]
    fn absent_fields_deserialize_to_the_permissive_default() {
        // A `[tools]` table that only subtracts must not switch anything else off.
        let policy: NomiToolPolicy = serde_json::from_str(r#"{"disabled":["remember"]}"#).unwrap();
        assert_eq!(policy.disabled, vec!["remember".to_owned()]);
        assert!(policy.web && policy.computer && policy.browser && policy.plan && policy.lsp);
        assert_eq!(policy.domains, NomiToolDomains::default());
        // An empty table is explicitly the permissive default, not "deny all".
        let empty: NomiToolPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, NomiToolPolicy::default());
    }

    #[test]
    fn overlay_only_ever_narrows() {
        let broad = NomiToolPolicy::default();
        let narrow = NomiToolPolicy {
            disabled: vec!["remember".to_owned()],
            browser: false,
            domains: NomiToolDomains {
                cron: false,
                ..NomiToolDomains::default()
            },
            ..NomiToolPolicy::default()
        };

        // Overlaying in either direction yields the narrower policy.
        assert_eq!(broad.overlay(&narrow), narrow);
        assert_eq!(narrow.overlay(&broad), narrow);
        // Idempotent.
        assert_eq!(narrow.overlay(&narrow), narrow);
        // There is no way to widen: a restriction survives the overlay.
        assert!(!narrow.overlay(&broad).browser);
        assert!(!narrow.overlay(&broad).domains.cron);
    }

    #[test]
    fn overlay_unions_lists_and_keeps_them_canonical() {
        let left = NomiToolPolicy {
            enabled: vec!["Read".to_owned(), "Grep".to_owned()],
            disabled: vec!["remember".to_owned()],
            ..NomiToolPolicy::default()
        };
        let right = NomiToolPolicy {
            enabled: vec!["Glob".to_owned(), "Read".to_owned()],
            disabled: vec!["update_plan".to_owned(), "remember".to_owned()],
            ..NomiToolPolicy::default()
        };

        let merged = left.overlay(&right);
        assert_eq!(merged.enabled, vec!["Glob", "Grep", "Read"]);
        assert_eq!(merged.disabled, vec!["remember", "update_plan"]);
    }

    #[test]
    fn wildcard_outside_the_mcp_namespace_is_warned_for_both_lists() {
        let enabled = NomiToolPolicy {
            enabled: vec!["*".to_owned()],
            ..NomiToolPolicy::default()
        };
        let warnings = enabled.syntax_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("removes every tool"), "{warnings:?}");

        let disabled = NomiToolPolicy {
            disabled: vec!["*".to_owned()],
            ..NomiToolPolicy::default()
        };
        let warnings = disabled.syntax_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("subtracts nothing"), "{warnings:?}");
    }

    #[test]
    fn mcp_patterns_are_not_warned_but_a_literal_without_a_tool_segment_is() {
        let ok = NomiToolPolicy {
            enabled: vec!["mcp__github__*".to_owned(), "mcp__*".to_owned()],
            disabled: vec!["mcp__notion__search__*".to_owned()],
            ..NomiToolPolicy::default()
        };
        assert!(ok.syntax_warnings().is_empty(), "{:?}", ok.syntax_warnings());

        let bad = NomiToolPolicy {
            disabled: vec!["mcp__github".to_owned()],
            ..NomiToolPolicy::default()
        };
        let warnings = bad.syntax_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("mcp__github__*"), "{warnings:?}");
    }

    #[test]
    fn removing_tool_search_is_warned_but_reported() {
        let policy = NomiToolPolicy {
            disabled: vec![TOOL_SEARCH.to_owned()],
            ..NomiToolPolicy::default()
        };
        assert!(!policy.allows_tool_search());
        let warnings = policy.syntax_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("unreachable"), "{warnings:?}");
    }
}
