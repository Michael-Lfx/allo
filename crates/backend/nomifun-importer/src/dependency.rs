//! Dependency declarations of the **compatibility layer** (`02` §8, `17` §7).
//!
//! V1 only *registered* `dependencies` as components and never let them take
//! part in an install decision (`17` §10 P3). This module is the decision half:
//! parse the declared SemVer range and say what is wrong with it.
//!
//! ## What counts as a «strong» dependency
//!
//! `02` §11.1 blocks when 「必需依赖无法解析，**且**来源声明为强依赖」, but the
//! declared shape (`{name, version?, marketplace?}`, `02` §8) has no `required`
//! flag. The only in-data signal that the source *asserts* a requirement is a
//! **declared `version` range** — a bare name is a note, not a requirement. So:
//!
//! - `version` present → **strong** (resolvable, therefore blockable);
//! - `version` absent  → **weak** (registered only, never blocks).
//!
//! ## What it resolves against
//!
//! [`CatalogIndex`]: the names/versions the snapshot catalog already knows
//! (imported components plus imported snapshot names). It is deliberately the
//! *whole* catalog and not just the marketplace the plugin came from — a
//! dependency may legitimately be satisfied by anything already imported.
//!
//! Two limitations are deliberate. First, a dependency satisfied *outside* the
//! catalog (a builtin skill, a hand-configured MCP connector) is invisible
//! here. Second, only **plugin snapshot names** come with a trustworthy
//! version: a component row's version is the version of the plugin that shipped
//! it, not the component's own, so a name known only as a component is resolved
//! by **existence** (a range on it is not judged). Both cases stay
//! **unblocked** — a wrong "missing" is worse than a missed block, and this
//! whole check is opt-in (`[import].strict_dependencies`, default off).

use std::collections::HashMap;

/// One dependency row, after `manifest.rs` normalization (`02` §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredDependency {
    pub name: String,
    /// The declared SemVer range. `None` = weak dependency.
    pub version: Option<String>,
    /// `connectors` / `plugins` / `skills` … when the manifest used the
    /// object form (`02` §8). Only informational for the check.
    pub group: Option<String>,
}

impl DeclaredDependency {
    /// Read one normalized dependency row.
    ///
    /// `manifest.rs` normalizes a bare string into `{"name": …}` and expands
    /// the object form with a `group` marker, so both shapes land here.
    pub fn from_declared(raw: &serde_json::Value) -> Self {
        let text = |key: &str| {
            raw.get(key)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        Self {
            name: text("name").unwrap_or_default(),
            version: text("version"),
            group: text("group"),
        }
    }

    /// A weak dependency asserts no requirement (`02` §11.1: 阻断以「来源声明为
    /// 强依赖」为前提), so it is never a reason to refuse an import.
    pub fn is_strong(&self) -> bool {
        self.version.is_some()
    }
}

/// Names/versions the catalog already holds. Built by the caller from the
/// repository; keyed case-insensitively for matching.
///
/// A name with an **empty** version list is known-but-not-comparable: it exists
/// (so it is not "missing"), but no range can be judged against it. That is how
/// component-only names land here — see the module docs.
#[derive(Debug, Default, Clone)]
pub struct CatalogIndex {
    versions: HashMap<String, Vec<String>>,
}

impl CatalogIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one known name (component name or snapshot name) and, when it is
    /// trustworthy, its version. `None` records the name alone.
    pub fn insert(&mut self, name: &str, version: Option<&str>) {
        let key = normalize_name(name);
        if key.is_empty() {
            return;
        }
        let entry = self.versions.entry(key).or_default();
        let version = version.unwrap_or("").trim().to_owned();
        if !version.is_empty() && !entry.contains(&version) {
            entry.push(version);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// All known versions for a name; `None` when the name is unknown.
    pub fn versions_of(&self, name: &str) -> Option<&[String]> {
        self.versions.get(&normalize_name(name)).map(Vec::as_slice)
    }
}

/// Why a strong dependency cannot be satisfied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyProblem {
    /// The row carries no usable `name`, so nothing can be resolved (`02` §8).
    MissingName { index: usize },
    /// `version` is present but is not a SemVer range (`17` §7).
    UnparsableRange {
        name: String,
        declared: String,
    },
    /// Nothing in the catalog carries this name (`17` §7: 给出缺失项).
    Missing { name: String, required: String },
    /// Something carries the name, but no known version satisfies the range.
    Unsatisfied {
        name: String,
        required: String,
        available: Vec<String>,
    },
}

impl DependencyProblem {
    /// One human-readable line, appended to the blocked import result.
    ///
    /// Never contains a filesystem path (`02` §9) — it names the dependency
    /// instead. Nothing parses this text.
    pub fn message(&self) -> String {
        match self {
            Self::MissingName { index } => format!(
                "第 {} 条依赖缺少 name，无法解析（02 §8）",
                index + 1
            ),
            Self::UnparsableRange { name, declared } => format!(
                "依赖「{name}」的版本范围「{declared}」不是合法 SemVer 范围（17 §7）"
            ),
            Self::Missing { name, required } => format!(
                "缺少依赖「{name}」（要求 {required}）：目录中没有任何已导入的同名组件或插件。\
                 请先导入它，或在 ~/.agent-store/config.toml 关闭 [import].strict_dependencies"
            ),
            Self::Unsatisfied {
                name,
                required,
                available,
            } => format!(
                "依赖「{name}」版本不满足：要求 {required}，已导入的是 {}",
                available.join(" / ")
            ),
        }
    }
}

/// Check every declared dependency against the catalog.
///
/// An empty result means "keep going". Only **strong** rows can produce a
/// problem; weak rows are skipped by design (`02` §11.1).
pub fn check_dependencies(
    declared: &[serde_json::Value],
    catalog: &CatalogIndex,
) -> Vec<DependencyProblem> {
    let mut problems = Vec::new();

    for (index, raw) in declared.iter().enumerate() {
        let dependency = DeclaredDependency::from_declared(raw);
        if !dependency.is_strong() {
            continue; // 弱依赖：只登记，不阻断
        }
        if dependency.name.is_empty() {
            problems.push(DependencyProblem::MissingName { index });
            continue;
        }

        let required = dependency.version.clone().unwrap_or_default();
        // The range itself must be readable before anything can be compared
        // (`17` §7「依赖不可满足时：阻断安装并给出缺失项」的第一种失败).
        if !is_parsable_requirement(&required) {
            problems.push(DependencyProblem::UnparsableRange {
                name: dependency.name.clone(),
                declared: required,
            });
            continue;
        }

        let Some(versions) = catalog.versions_of(&dependency.name) else {
            problems.push(DependencyProblem::Missing {
                name: dependency.name.clone(),
                required,
            });
            continue;
        };

        // Only *parsable* known versions can be compared. A name known without
        // a comparable version (component-only, or an unparsable stored
        // version) is treated as "cannot verify" and does not block — see the
        // module docs for why that is the safe direction.
        let comparable: Vec<&String> =
            versions.iter().filter(|version| parse_version(version).is_some()).collect();
        if comparable.is_empty() {
            continue;
        }
        if comparable
            .iter()
            .any(|version| version_matches(&required, version.as_str()))
        {
            continue;
        }

        problems.push(DependencyProblem::Unsatisfied {
            name: dependency.name.clone(),
            required,
            available: comparable.into_iter().cloned().collect(),
        });
    }

    problems
}

// ---------------------------------------------------------------------------
// Matching (same semantics as the extension layer's `dependency.rs`)
// ---------------------------------------------------------------------------

/// Bare version (`"1.2.3"`) → **exact** match; `^` / `~` ranges as declared.
///
/// Mirrors `nomifun-extension/src/dependency.rs`'s `version_matches`, so both
/// layers of `17` §7 read a requirement the same way.
fn version_matches(requirement: &str, actual: &str) -> bool {
    let Some(version) = parse_version(actual) else {
        return false;
    };
    let Some(req) = parse_requirement(requirement) else {
        return false;
    };
    req.matches(&version)
}

fn is_parsable_requirement(requirement: &str) -> bool {
    parse_requirement(requirement).is_some()
}

fn parse_requirement(requirement: &str) -> Option<semver::VersionReq> {
    let trimmed = requirement.trim();
    if trimmed.is_empty() {
        return None;
    }
    // A bare version means "exactly this one" (not semver's default caret).
    let normalized = if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        format!("={trimmed}")
    } else {
        trimmed.to_owned()
    };
    semver::VersionReq::parse(&normalized).ok()
}

/// `semver::Version` is strict about the three components; real manifests
/// sometimes carry a `v` prefix, so tolerate exactly that much.
fn parse_version(actual: &str) -> Option<semver::Version> {
    let trimmed = actual.trim();
    semver::Version::parse(trimmed)
        .or_else(|_| semver::Version::parse(trimmed.trim_start_matches(['v', 'V'])))
        .ok()
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog(entries: &[(&str, Option<&str>)]) -> CatalogIndex {
        let mut index = CatalogIndex::new();
        for (name, version) in entries {
            index.insert(name, *version);
        }
        index
    }

    #[test]
    fn weak_dependencies_never_produce_a_problem() {
        // A bare name is a note, not a requirement (02 §11.1).
        let declared = vec![json!({ "name": "westock-mcp" }), json!({ "name": "x", "group": "connectors" })];
        assert!(check_dependencies(&declared, &CatalogIndex::new()).is_empty());
    }

    #[test]
    fn strong_dependency_is_skipped_when_it_is_absent_from_an_empty_catalog_only_if_weak() {
        // Sanity: with a version it *is* strong, so an empty catalog reports it.
        let declared = vec![json!({ "name": "base", "version": "^1.0.0" })];
        let problems = check_dependencies(&declared, &CatalogIndex::new());
        assert_eq!(problems.len(), 1);
        assert!(matches!(&problems[0], DependencyProblem::Missing { name, .. } if name == "base"));
    }

    #[test]
    fn satisfiable_range_is_accepted() {
        let declared = vec![json!({ "name": "base", "version": "^1.2.0" })];
        assert!(check_dependencies(&declared, &catalog(&[("base", Some("1.9.0"))])).is_empty());
    }

    #[test]
    fn bare_version_requires_an_exact_match() {
        let declared = vec![json!({ "name": "base", "version": "1.2.3" })];
        assert!(check_dependencies(&declared, &catalog(&[("base", Some("1.2.3"))])).is_empty());
        let problems = check_dependencies(&declared, &catalog(&[("base", Some("1.2.4"))]));
        assert!(matches!(
            problems.as_slice(),
            [DependencyProblem::Unsatisfied { required, .. }] if required == "1.2.3"
        ));
    }

    #[test]
    fn tilde_and_caret_follow_semver() {
        let tilde = vec![json!({ "name": "base", "version": "~1.2.3" })];
        assert!(check_dependencies(&tilde, &catalog(&[("base", Some("1.2.9"))])).is_empty());
        assert_eq!(check_dependencies(&tilde, &catalog(&[("base", Some("1.3.0"))])).len(), 1);
        let caret = vec![json!({ "name": "base", "version": "^1.2.3" })];
        assert_eq!(check_dependencies(&caret, &catalog(&[("base", Some("2.0.0"))])).len(), 1);
    }

    #[test]
    fn an_unparsable_range_blocks_even_when_the_name_is_known() {
        let declared = vec![json!({ "name": "base", "version": "not-a-version" })];
        let problems = check_dependencies(&declared, &catalog(&[("base", Some("1.0.0"))]));
        assert!(matches!(
            problems.as_slice(),
            [DependencyProblem::UnparsableRange { declared, .. }] if declared == "not-a-version"
        ));
    }

    #[test]
    fn a_nameless_strong_row_is_reported_by_position() {
        let declared = vec![json!({ "version": "^1.0.0" })];
        let problems = check_dependencies(&declared, &CatalogIndex::new());
        assert!(matches!(problems.as_slice(), [DependencyProblem::MissingName { index: 0 }]));
    }

    #[test]
    fn names_match_case_insensitively_and_tolerate_a_v_prefix() {
        let declared = vec![json!({ "name": "Base", "version": "^1.0.0" })];
        assert!(check_dependencies(&declared, &catalog(&[("base", Some("v1.2.0"))])).is_empty());
    }

    #[test]
    fn several_candidates_satisfy_when_any_one_of_them_does() {
        let declared = vec![json!({ "name": "base", "version": "^1.0.0" })];
        let index = catalog(&[("base", Some("1.0.0")), ("base", Some("2.0.0"))]);
        assert!(check_dependencies(&declared, &index).is_empty());
    }

    #[test]
    fn an_unparsable_known_version_does_not_block() {
        // Cannot verify ⇒ do not claim "unsatisfiable" (false blocks are worse).
        let declared = vec![json!({ "name": "base", "version": "^1.0.0" })];
        assert!(check_dependencies(&declared, &catalog(&[("base", Some("nightly"))])).is_empty());
    }

    #[test]
    fn messages_name_the_dependency_and_never_a_path() {
        let problems = check_dependencies(
            &[json!({ "name": "base", "version": "^2.0.0" })],
            &catalog(&[("base", Some("1.0.0"))]),
        );
        let message = problems[0].message();
        assert!(message.contains("base"), "{message}");
        assert!(message.contains("^2.0.0"), "{message}");
        assert!(!message.contains('/'), "no path separators: {message}");

        let missing = check_dependencies(
            &[json!({ "name": "gone", "version": "^1.0.0" })],
            &CatalogIndex::new(),
        );
        let message = missing[0].message();
        assert!(message.contains("gone"), "{message}");
        assert!(message.contains("strict_dependencies"), "must mention the way out: {message}");
    }

    #[test]
    fn index_records_names_even_without_a_usable_version() {
        let mut index = CatalogIndex::new();
        index.insert("  ", Some("1.0.0"));
        index.insert("base", Some("  "));
        assert_eq!(index.versions_of("   "), None, "a blank name records nothing");
        assert_eq!(
            index.versions_of("base"),
            Some([].as_slice()),
            "the name is recorded, the blank version is not"
        );
        // A name with no comparable version cannot be judged ⇒ no block.
        let declared = vec![json!({ "name": "base", "version": "^1.0.0" })];
        assert!(check_dependencies(&declared, &index).is_empty());
    }

    #[test]
    fn declared_dependency_reads_the_normalized_shape() {
        let row = json!({ "name": " base ", "version": " ^1.0.0 ", "group": "skills" });
        let parsed = DeclaredDependency::from_declared(&row);
        assert_eq!(parsed.name, "base");
        assert_eq!(parsed.version.as_deref(), Some("^1.0.0"));
        assert_eq!(parsed.group.as_deref(), Some("skills"));
        assert!(parsed.is_strong());

        // Blank strings are absent, not empty values.
        let blank = DeclaredDependency::from_declared(&json!({ "name": "x", "version": "  " }));
        assert!(blank.version.is_none());
        assert!(!blank.is_strong());
    }
}
