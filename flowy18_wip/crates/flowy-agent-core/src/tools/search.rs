//! Deferred tool discovery and append-only in-turn activation (`tool_search`).
//!
//! Extension providers with many specialized tools can mark them
//! [`ToolExposure::Deferred`]. Instead of bloating every Worker turn's initial
//! schema payload and invalidating the prompt prefix cache, only a compact
//! `[DeferredToolsAvailable]` catalog and `tool_search` are exposed upfront.
//! Calling `tool_search` activates matching deferred tools for subsequent steps
//! of the same turn via `prepare_next_turn`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::loop_runner::log_preview;
use crate::tools::registry::{
    ToolDefinition, ToolExposure, ToolRegistry, ToolSource,
};
use crate::types::{ToolCallArgs, ToolExecutionResult, ToolRunContext};

/// Canonical host tool name for on-demand deferred tool discovery.
pub const TOOL_SEARCH_NAME: &str = "tool_search";

const DEFAULT_MAX_RESULTS: usize = 8;
const HARD_MAX_RESULTS: usize = 20;
const MAX_CATALOG_LIST_THRESHOLD: usize = 30;
const MAX_CATALOG_DESC_CHARS: usize = 60;
const MAX_CATALOG_TOTAL_CHARS: usize = 1_600;
/// Maximum total deferred tools allowed to become active within a single turn
/// via `tool_search` (host `pre_activated_tools` are always preserved).
pub const MAX_ACTIVATED_TOOLS_PER_TURN: usize = 32;

/// Pure-value metadata snapshot of a discoverable tool captured for `tool_search`.
/// Holds zero `ToolRegistry` or `ToolDefinition` `Arc` references so search
/// closures can never trap tool closures or service handles in a cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchableToolEntry {
    pub name: String,
    pub description: String,
    pub source_label: String,
    pub tags: Vec<String>,
    pub read_only: bool,
    pub exposure: ToolExposure,
}

/// Snapshot searchable tool metadata from `registry` without retaining any
/// `Arc<ToolRegistry>` or `Arc<ToolDefinition>` references.
#[must_use]
pub fn snapshot_searchable_catalog(
    registry: &ToolRegistry,
) -> Arc<[SearchableToolEntry]> {
    let entries: Vec<SearchableToolEntry> = registry
        .definitions()
        .into_iter()
        .filter(|def| def.name != TOOL_SEARCH_NAME)
        .map(|def| SearchableToolEntry {
            name: def.name.clone(),
            description: def.description.clone(),
            source_label: def.source().label().to_owned(),
            tags: def.tags().to_vec(),
            read_only: def.is_read_only(),
            exposure: def.exposure(),
        })
        .collect();
    Arc::from(entries)
}

/// Monotonic, thread-safe set of deferred tool names activated within a turn.
#[derive(Clone, Debug)]
pub struct ActivatedToolSet {
    names: Arc<Mutex<BTreeSet<String>>>,
    max_activated: usize,
}

impl Default for ActivatedToolSet {
    fn default() -> Self {
        Self::new(std::iter::empty::<String>())
    }
}

impl ActivatedToolSet {
    #[must_use]
    pub fn new<I, S>(initial: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_limit(initial, MAX_ACTIVATED_TOOLS_PER_TURN)
    }

    #[must_use]
    pub fn with_limit<I, S>(initial: I, max_activated: usize) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let set: BTreeSet<String> = initial
            .into_iter()
            .map(Into::into)
            .filter(|name| !name.trim().is_empty())
            .collect();
        Self {
            names: Arc::new(Mutex::new(set)),
            max_activated,
        }
    }

    #[must_use]
    pub const fn max_activated(&self) -> usize {
        self.max_activated
    }

    /// Record `names` as active for subsequent steps of the turn up to
    /// `max_activated`, returning `(newly_activated, skipped_due_to_limit)` in
    /// ascending alphabetical order.
    pub fn activate_bounded<I, S>(&self, names: I) -> (Vec<String>, Vec<String>)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut guard = self
            .names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut added = Vec::new();
        let mut skipped = BTreeSet::new();
        for raw in names {
            let name = raw.into();
            if name.trim().is_empty() || guard.contains(&name) {
                continue;
            }
            if guard.len() >= self.max_activated {
                skipped.insert(name);
                continue;
            }
            if guard.insert(name.clone()) {
                added.push(name);
            }
        }
        drop(guard);
        added.sort();
        (added, skipped.into_iter().collect())
    }

    /// Record `names` as active for subsequent steps of the turn, returning the
    /// subset that became newly active (in ascending alphabetical order).
    pub fn activate<I, S>(&self, names: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.activate_bounded(names).0
    }

    /// Sorted snapshot of all currently activated deferred tool names.
    #[must_use]
    pub fn snapshot(&self) -> BTreeSet<String> {
        self.names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(name)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolSearchArgs {
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
}

/// Build the host `tool_search` tool using an immutable metadata snapshot of
/// `registry` so the tool closure never retains an `Arc<ToolRegistry>`.
#[must_use]
pub fn tool_search_definition(
    registry: &ToolRegistry,
    activated: ActivatedToolSet,
) -> Arc<ToolDefinition> {
    let catalog = snapshot_searchable_catalog(registry);
    let parameters = json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "description": "Search keywords matching deferred tool names, tags, or descriptions (e.g. 'cron schedule'), or `select:tool_a,tool_b` for exact activation."
            },
            "max_results": {
                "type": "integer",
                "minimum": 1,
                "maximum": HARD_MAX_RESULTS,
                "description": "Maximum deferred tools to activate and return (default 8, max 20)."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    });
    Arc::new(
        ToolDefinition::new(
            TOOL_SEARCH_NAME,
            "Discover and activate deferred tools for the rest of this turn. Supports keyword search or exact `select:name1,name2` syntax.",
            parameters,
            move |args: ToolCallArgs, _ctx: ToolRunContext| {
                let catalog = Arc::clone(&catalog);
                let activated = activated.clone();
                Box::pin(async move {
                    execute_tool_search(&catalog, &activated, args.arguments)
                })
            },
        )
        .with_read_only(true)
        .with_source(ToolSource::Host)
        .with_exposure(ToolExposure::Eager),
    )
}

/// Build a bounded `[DeferredToolsAvailable]` system prompt section listing
/// deferred tools in `registry`. Returns `None` when no deferred tools exist so
/// ordinary turns pay zero prompt overhead.
#[must_use]
pub fn format_deferred_tool_catalog(registry: &ToolRegistry) -> Option<String> {
    let deferred = registry.deferred_definitions();
    if deferred.is_empty() {
        return None;
    }
    let mut out = String::from(
        "[DeferredToolsAvailable]\nAdditional specialized tools are registered in deferred mode and not loaded into the active schema yet. Call `tool_search` with keywords or `select:name1,name2` to unlock matching tools before invoking them:\n",
    );
    if deferred.len() <= MAX_CATALOG_LIST_THRESHOLD {
        for def in &deferred {
            let line = format_catalog_entry(def);
            if out.chars().count() + line.chars().count()
                > MAX_CATALOG_TOTAL_CHARS
            {
                break;
            }
            out.push_str(&line);
        }
    } else {
        let mut by_source: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for def in &deferred {
            by_source
                .entry(def.source().label())
                .or_default()
                .push(def.name.as_str());
        }
        for (source, names) in by_source {
            let sample: Vec<&str> = names.iter().copied().take(5).collect();
            let extra = names.len().saturating_sub(sample.len());
            let suffix = if extra > 0 {
                format!(", +{extra} more")
            } else {
                String::new()
            };
            let line = format!(
                "- {source} ({} tools): {}{suffix}\n",
                names.len(),
                sample.join(", ")
            );
            if out.chars().count() + line.chars().count()
                > MAX_CATALOG_TOTAL_CHARS
            {
                break;
            }
            out.push_str(&line);
        }
    }
    Some(log_preview(out.trim_end(), MAX_CATALOG_TOTAL_CHARS))
}

fn format_catalog_entry(def: &ToolDefinition) -> String {
    let collapsed = def
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let summary = log_preview(&collapsed, MAX_CATALOG_DESC_CHARS);
    let ro = if def.is_read_only() {
        ", read-only"
    } else {
        ""
    };
    format!(
        "- {} ({}{}): {summary}\n",
        def.name,
        def.source().label(),
        ro
    )
}

fn execute_tool_search(
    catalog: &[SearchableToolEntry],
    activated: &ActivatedToolSet,
    arguments: Value,
) -> ToolExecutionResult {
    let Ok(parsed) = serde_json::from_value::<ToolSearchArgs>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected `{ query: string, max_results?: integer }`",
        );
    };
    let trimmed = parsed.query.trim();
    if trimmed.is_empty() {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "`query` must be a non-empty string",
        );
    }
    let limit = parsed
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, HARD_MAX_RESULTS);

    if let Some(select_body) = strip_select_prefix(trimmed) {
        return execute_exact_select(catalog, activated, select_body, limit);
    }

    let query_lower = trimmed.to_ascii_lowercase();
    let deferred: Vec<&SearchableToolEntry> = catalog
        .iter()
        .filter(|entry| entry.exposure == ToolExposure::Deferred)
        .collect();
    let matched_defs: Vec<&SearchableToolEntry> = if trimmed == "*" {
        deferred.into_iter().take(limit).collect()
    } else {
        let tokens = tokenize_query(&query_lower);
        let mut scored: Vec<(usize, &SearchableToolEntry)> = deferred
            .into_iter()
            .filter_map(|def| {
                let score = score_tool(def, &query_lower, &tokens);
                (score > 0).then_some((score, def))
            })
            .collect();
        scored.sort_by(|(sa, da), (sb, db)| {
            (Reverse(*sa), da.name.as_str())
                .cmp(&(Reverse(*sb), db.name.as_str()))
        });
        scored.into_iter().take(limit).map(|(_, def)| def).collect()
    };

    let (newly_activated, skipped_due_to_limit) = activated
        .activate_bounded(matched_defs.iter().map(|def| def.name.clone()));
    build_search_response(
        trimmed,
        &matched_defs,
        &newly_activated,
        &skipped_due_to_limit,
        &[],
        activated.max_activated(),
    )
}

fn execute_exact_select(
    catalog: &[SearchableToolEntry],
    activated: &ActivatedToolSet,
    select_body: &str,
    limit: usize,
) -> ToolExecutionResult {
    let requested: Vec<String> = select_body
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if requested.is_empty() {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "`select:` requires at least one tool name",
        );
    }
    let mut matched = Vec::new();
    let mut to_activate = Vec::new();
    let mut not_found = Vec::new();
    let mut seen = BTreeSet::new();
    for name in requested {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(def) = catalog.iter().find(|entry| entry.name == name) else {
            not_found.push(name);
            continue;
        };
        if matched.len() < limit {
            if def.exposure == ToolExposure::Deferred {
                to_activate.push(def.name.clone());
            }
            matched.push(def);
        }
    }
    let (newly_activated, skipped_due_to_limit) =
        activated.activate_bounded(to_activate);
    build_search_response(
        "select",
        &matched,
        &newly_activated,
        &skipped_due_to_limit,
        &not_found,
        activated.max_activated(),
    )
}

fn build_search_response(
    query: &str,
    matched: &[&SearchableToolEntry],
    newly_activated: &[String],
    skipped_due_to_limit: &[String],
    not_found: &[String],
    max_activated: usize,
) -> ToolExecutionResult {
    let entries: Vec<Value> = matched
        .iter()
        .map(|def| {
            json!({
                "name": def.name,
                "description": def.description,
                "source": def.source_label,
                "tags": def.tags,
                "read_only": def.read_only,
                "exposure": match def.exposure {
                    ToolExposure::Eager => "eager",
                    ToolExposure::Deferred => "deferred",
                },
            })
        })
        .collect();
    let limit_reached = !skipped_due_to_limit.is_empty();
    let summary = if matched.is_empty() {
        format!("No deferred tools matched `{query}`.")
    } else if newly_activated.is_empty() && limit_reached {
        format!(
            "Per-turn deferred tool activation limit ({max_activated}) reached; skipped: {}.",
            skipped_due_to_limit.join(", ")
        )
    } else if newly_activated.is_empty() {
        let names: Vec<&str> =
            matched.iter().map(|d| d.name.as_str()).collect();
        format!(
            "Matched tools (already active for this turn): {}.",
            names.join(", ")
        )
    } else if limit_reached {
        format!(
            "Activated {} tool(s) ({}); per-turn activation limit ({max_activated}) reached, skipped: {}.",
            newly_activated.len(),
            newly_activated.join(", "),
            skipped_due_to_limit.join(", ")
        )
    } else {
        format!(
            "Activated {} tool(s) for the remainder of this turn: {}. You can call them in your next step.",
            newly_activated.len(),
            newly_activated.join(", ")
        )
    };
    ToolExecutionResult::success_text(
        summary,
        json!({
            "query": query,
            "matched": entries,
            "newly_activated": newly_activated,
            "skipped_due_to_limit": skipped_due_to_limit,
            "limit_reached": limit_reached,
            "max_activated": max_activated,
            "not_found": not_found,
        }),
    )
}

fn strip_select_prefix(trimmed: &str) -> Option<&str> {
    let prefix: String = trimmed.chars().take(7).collect();
    prefix
        .eq_ignore_ascii_case("select:")
        .then(|| &trimmed[prefix.len()..])
}

fn tokenize_query(query_lower: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw in query_lower.split(|c: char| {
        c.is_whitespace()
            || matches!(c, '_' | '-' | '/' | ',' | ':' | '.' | ';')
    }) {
        if !raw.is_empty() && !tokens.iter().any(|existing| existing == raw) {
            tokens.push(raw.to_owned());
        }
    }
    tokens
}

fn score_tool(
    def: &SearchableToolEntry,
    query_lower: &str,
    tokens: &[String],
) -> usize {
    let name = def.name.to_ascii_lowercase();
    let desc = def.description.to_ascii_lowercase();
    let source = def.source_label.to_ascii_lowercase();
    let tags: Vec<String> =
        def.tags.iter().map(|t| t.to_ascii_lowercase()).collect();

    let mut score = 0_usize;
    if name == query_lower {
        score = score.saturating_add(100);
    } else if name.contains(query_lower) {
        score = score.saturating_add(40);
    }
    if source == query_lower
        || (!query_lower.is_empty() && source.contains(query_lower))
    {
        score = score.saturating_add(15);
    }
    if desc.contains(query_lower) {
        score = score.saturating_add(10);
    }
    for tag in &tags {
        if tag == query_lower {
            score = score.saturating_add(30);
        } else if tag.contains(query_lower) {
            score = score.saturating_add(15);
        }
    }
    for token in tokens {
        if name.contains(token.as_str()) {
            score = score.saturating_add(20);
        }
        if tags.iter().any(|tag| tag.contains(token.as_str())) {
            score = score.saturating_add(15);
        }
        if source.contains(token.as_str()) {
            score = score.saturating_add(10);
        }
        if desc.contains(token.as_str()) {
            score = score.saturating_add(5);
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::{
        ActivatedToolSet, TOOL_SEARCH_NAME, format_deferred_tool_catalog,
        tool_search_definition,
    };
    use crate::tools::registry::{
        ToolDefinition, ToolExposure, ToolRegistry, ToolSource,
    };
    use crate::types::{CancelToken, ToolExecutionResult, ToolRunContext};
    use serde_json::json;
    use std::sync::Arc;

    fn run_ctx() -> ToolRunContext {
        ToolRunContext {
            signal: CancelToken::new(),
            agent_id: Some("w-test".into()),
            cwd: None,
            todos: None,
            submission_id: None,
            session_origin: crate::session::SessionOrigin::Unknown,
            output_root: None,
            artifact_delivery_available: false,
        }
    }

    fn deferred_def(
        name: &str,
        desc: &str,
        source: &str,
        tags: &[&str],
        read_only: bool,
    ) -> Arc<ToolDefinition> {
        Arc::new(
            ToolDefinition::new(
                name,
                desc,
                json!({"type": "object", "properties": {}}),
                |_args, _ctx| {
                    Box::pin(async {
                        ToolExecutionResult::success_text("ok", json!({}))
                    })
                },
            )
            .with_exposure(ToolExposure::Deferred)
            .with_source(ToolSource::Provider(source.to_owned()))
            .with_tags(tags.iter().copied())
            .with_read_only(read_only),
        )
    }

    #[tokio::test]
    async fn keyword_and_cjk_search_scores_and_activates_deferred_tools() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(ToolDefinition::new(
            "eager_helper",
            "eager cron helper that must not dominate deferred results",
            json!({"type": "object"}),
            |_args, _ctx| {
                Box::pin(async {
                    ToolExecutionResult::success_text("ok", json!({}))
                })
            },
        )));
        registry.register(deferred_def(
            "automation_list",
            "列出所有定时任务规则与状态",
            "automation",
            &["cron", "定时任务"],
            true,
        ));
        registry.register(deferred_def(
            "automation_create",
            "Create a scheduled cron job",
            "automation",
            &["cron", "schedule"],
            false,
        ));
        registry.register(deferred_def(
            "weather_lookup",
            "Query forecast data",
            "weather",
            &["forecast"],
            true,
        ));

        let activated = ActivatedToolSet::default();
        registry.register(tool_search_definition(&registry, activated.clone()));

        let res = registry
            .execute(
                "s1".into(),
                TOOL_SEARCH_NAME.into(),
                json!({"query": "定时任务"}),
                run_ctx(),
            )
            .await;
        let ToolExecutionResult::Success { value, .. } = res else {
            panic!("expected search success, got {res:?}");
        };
        assert_eq!(value["newly_activated"], json!(["automation_list"]));
        assert!(activated.contains("automation_list"));
        assert!(!activated.contains("eager_helper"));
        assert!(!activated.contains("weather_lookup"));
    }

    #[tokio::test]
    async fn exact_select_and_wildcard_respect_limit_and_report_missing() {
        let registry = ToolRegistry::new();
        for idx in 0..5 {
            registry.register(deferred_def(
                &format!("auto_{idx}"),
                "Automation action",
                "automation",
                &["auto"],
                false,
            ));
        }
        let activated = ActivatedToolSet::default();
        registry.register(tool_search_definition(&registry, activated.clone()));

        let sel = registry
            .execute(
                "s2".into(),
                TOOL_SEARCH_NAME.into(),
                json!({"query": "SELECT: auto_2, missing_tool, auto_0"}),
                run_ctx(),
            )
            .await;
        let ToolExecutionResult::Success { value, .. } = sel else {
            panic!("select failed: {sel:?}");
        };
        assert_eq!(value["newly_activated"], json!(["auto_0", "auto_2"]));
        assert_eq!(value["not_found"], json!(["missing_tool"]));

        let wild = registry
            .execute(
                "s3".into(),
                TOOL_SEARCH_NAME.into(),
                json!({"query": "*", "max_results": 2}),
                run_ctx(),
            )
            .await;
        let ToolExecutionResult::Success { value, .. } = wild else {
            panic!("wildcard failed: {wild:?}");
        };
        assert_eq!(value["matched"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn deferred_catalog_skips_when_empty_and_summarizes_large_sets() {
        let eager_only = ToolRegistry::new();
        eager_only.register(Arc::new(ToolDefinition::new(
            "read_file",
            "read",
            json!({"type": "object"}),
            |_args, _ctx| {
                Box::pin(async {
                    ToolExecutionResult::success_text("ok", json!({}))
                })
            },
        )));
        assert!(format_deferred_tool_catalog(&eager_only).is_none());

        let small = ToolRegistry::new();
        small.register(deferred_def(
            "cron_create",
            "Create recurring job",
            "automation",
            &[],
            false,
        ));
        let catalog = format_deferred_tool_catalog(&small).unwrap();
        assert!(catalog.contains("[DeferredToolsAvailable]"));
        assert!(
            catalog.contains("cron_create (automation): Create recurring job")
        );

        let large = ToolRegistry::new();
        for idx in 0..35 {
            large.register(deferred_def(
                &format!("mcp_tool_{idx:02}"),
                "Remote capability endpoint",
                "mcp_srv",
                &[],
                false,
            ));
        }
        let grouped = format_deferred_tool_catalog(&large).unwrap();
        assert!(grouped.contains("mcp_srv (35 tools):"));
        assert!(grouped.contains("+30 more"));
        assert!(grouped.chars().count() <= 1_605);
    }

    #[test]
    fn tool_search_snapshot_drops_registry_and_tool_closures() {
        let probe = Arc::new(());
        let weak = Arc::downgrade(&probe);
        let registry = ToolRegistry::new();
        let captured = Arc::clone(&probe);
        registry.register(Arc::new(
            ToolDefinition::new(
                "deferred_probe",
                "Deferred tool holding a probe Arc",
                json!({"type": "object", "properties": {}}),
                move |_args, _ctx| {
                    let _keep = Arc::clone(&captured);
                    Box::pin(async {
                        ToolExecutionResult::success_text("ok", json!({}))
                    })
                },
            )
            .with_exposure(ToolExposure::Deferred)
            .with_source(ToolSource::Provider("probe".into())),
        ));
        drop(probe);

        let activated = ActivatedToolSet::default();
        registry.register(tool_search_definition(&registry, activated));
        drop(registry);

        assert!(
            weak.upgrade().is_none(),
            "tool_search must not trap ToolRegistry/ToolDefinition closures in an Arc reference cycle"
        );
    }

    #[tokio::test]
    async fn tool_search_enforces_per_turn_cumulative_activation_limit_and_preserves_pre_activated()
     {
        let registry = ToolRegistry::new();
        for idx in 0..40 {
            registry.register(deferred_def(
                &format!("cap_tool_{idx:02}"),
                "Capability under limit test",
                "cap",
                &["cap"],
                true,
            ));
        }
        registry.register(deferred_def(
            "pre_approved_tool",
            "Host pre-activated tool",
            "cap",
            &["pre"],
            false,
        ));

        let activated = ActivatedToolSet::with_limit(
            ["pre_approved_tool".to_owned(), "cap_tool_00".to_owned()],
            5,
        );
        registry.register(tool_search_definition(&registry, activated.clone()));

        let first = registry
            .execute(
                "lim-1".into(),
                TOOL_SEARCH_NAME.into(),
                json!({"query": "*", "max_results": 4}),
                run_ctx(),
            )
            .await;
        let ToolExecutionResult::Success { value: v1, .. } = first else {
            panic!("first search failed: {first:?}");
        };
        assert_eq!(v1["limit_reached"], json!(false));
        assert_eq!(
            v1["newly_activated"],
            json!(["cap_tool_01", "cap_tool_02", "cap_tool_03"])
        );
        assert!(activated.contains("pre_approved_tool"));

        // Quota (5 total) is now full; activating additional tools must stop cleanly.
        let second = registry
            .execute(
                "lim-2".into(),
                TOOL_SEARCH_NAME.into(),
                json!({"query": "select:cap_tool_20,cap_tool_21"}),
                run_ctx(),
            )
            .await;
        let ToolExecutionResult::Success { value: v2, .. } = second else {
            panic!("second search failed: {second:?}");
        };
        assert_eq!(v2["limit_reached"], json!(true));
        assert_eq!(v2["newly_activated"], json!([]));
        assert_eq!(
            v2["skipped_due_to_limit"],
            json!(["cap_tool_20", "cap_tool_21"])
        );
        assert_eq!(activated.snapshot().len(), 5);
        assert!(!activated.contains("cap_tool_20"));

        // Default limit matches MAX_ACTIVATED_TOOLS_PER_TURN (= 32).
        let default_set = ActivatedToolSet::default();
        assert_eq!(
            default_set.max_activated(),
            super::MAX_ACTIVATED_TOOLS_PER_TURN
        );
    }
}
