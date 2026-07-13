//! `ModelsDevClient` — the main entry point for the models.dev registry.
//!
//! - tests can inject a custom base URL or pre-seeded cache without
//!   touching the global filesystem
//! - downstream code can hold an `Arc<ModelsDevClient>` for DI
//! - cache state is explicit rather than module-global
//!
//! For the common case where no customisation is needed, the parent module
//! exposes `default_client()` which is a process-wide singleton.

use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value};
use strsim::normalized_levenshtein;
use tracing::debug;

use super::cache;
use super::mapping;
use super::noise::{noise_re, should_hide};
use super::parse;
use super::types::{ModelCapabilities, ModelInfo, ProviderInfo};

/// Public URL of the models.dev API.
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// In-memory cache lifetime — matches Python `_MODELS_DEV_CACHE_TTL = 3600`.
const CACHE_TTL: Duration = Duration::from_secs(3_600);

/// When the network fetch fails and we fall back to the disk cache, treat
/// the cached data as "5 minutes from expiring" so the next call retries
/// the network rather than serving a stale snapshot for a full hour.
const DISK_FALLBACK_REMAINING: Duration = Duration::from_secs(300);

/// HTTP timeout for the network fetch. Python uses `timeout=15`.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Internal cache state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct CacheState {
    /// The full registry: `{ provider_id -> { ..., "models": { model_id -> entry } } }`.
    data: Value,
    /// When the cache was last populated.
    last_refresh: Option<Instant>,
    /// When the cache "should expire" — overrideable so disk-fallback can
    /// look almost-stale.
    expires_at: Option<Instant>,
    /// Last network / decode error message, if any.
    last_error: Option<String>,
}

impl CacheState {
    fn is_fresh(&self) -> bool {
        match (self.last_refresh, self.expires_at) {
            (Some(_), Some(exp)) => Instant::now() < exp,
            _ => false,
        }
    }

    fn is_populated(&self) -> bool {
        self.data.is_object() && !self.data.as_object().map(|m| m.is_empty()).unwrap_or(true)
    }
}

/// Snapshot of registry health for diagnostics / UI.
#[derive(Debug, Clone)]
pub struct RegistryStatus {
    pub populated: bool,
    pub cache_age_secs: Option<u64>,
    pub last_error: Option<String>,
    pub provider_count: usize,
    pub model_count: usize,
    pub cache_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// One search hit from [`ModelsDevClient::search`].
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Nomifun platform ID.
    pub provider: String,
    /// Model ID as reported by models.dev.
    pub model_id: String,
    /// Raw entry — callers may further parse into [`ModelInfo`] if needed.
    pub entry: Value,
}

/// Client for the models.dev registry.
///
/// Construction is cheap; one client per process is normally enough. Use
/// [`crate::default_client`] for the global singleton.
pub struct ModelsDevClient {
    base_url: String,
    cache_path: PathBuf,
    http: reqwest::Client,
    state: RwLock<CacheState>,
    /// Serialises the network stage so concurrent `fetch(false)` calls share
    /// a single in-flight request.
    fetch_lock: tokio::sync::Mutex<()>,
}

impl ModelsDevClient {
    /// Build a client with custom endpoint + cache path and an optional HTTP client.
    ///
    /// When `http` is `None`, a fresh `reqwest::Client` with a 15s timeout is built.
    pub fn new(
        base_url: impl Into<String>,
        cache_path: impl Into<PathBuf>,
        http: Option<reqwest::Client>,
    ) -> Self {
        let http = http.unwrap_or_else(|| {
            reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        });
        Self {
            base_url: base_url.into(),
            cache_path: cache_path.into(),
            http,
            state: RwLock::new(CacheState::default()),
            fetch_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Production default — hits `models.dev/api.json`, uses
    /// [`cache::default_cache_path`], and the shared [`nomifun_net::http_client`].
    pub fn default_production() -> Self {
        Self::new(
            MODELS_DEV_URL,
            cache::default_cache_path(),
            Some(nomifun_net::http_client()),
        )
    }

    /// Diagnostic status of the in-memory registry + disk cache path.
    pub fn status(&self) -> RegistryStatus {
        let state = self.state.read().expect("cache lock poisoned");
        let (provider_count, model_count) = match state.data.as_object() {
            Some(obj) => {
                let providers = obj.len();
                let models = obj
                    .values()
                    .filter_map(|p| p.get("models").and_then(Value::as_object))
                    .map(|m| m.len())
                    .sum();
                (providers, models)
            }
            None => (0, 0),
        };
        let cache_age_secs = state
            .last_refresh
            .map(|t| t.elapsed().as_secs())
            .or_else(|| self.disk_cache_age().map(|d| d.as_secs()));
        RegistryStatus {
            populated: state.is_populated(),
            cache_age_secs,
            last_error: state.last_error.clone(),
            provider_count,
            model_count,
            cache_path: self.cache_path.clone(),
        }
    }

    // ---------- disk mtime helper ----------

    /// Return the age of the on-disk cache file, or `None` if missing /
    /// unreadable / clock-skewed.
    fn disk_cache_age(&self) -> Option<Duration> {
        let meta = std::fs::metadata(&self.cache_path).ok()?;
        let modified = meta.modified().ok()?;
        SystemTime::now().duration_since(modified).ok()
    }

    // ---------- test-only seam ----------

    /// Replace the in-memory cache with a pre-built registry. Intended for
    /// tests; the disk cache is **not** touched.
    #[doc(hidden)]
    pub fn seed_cache(&self, data: Value) {
        let mut state = self.state.write().expect("cache lock poisoned");
        state.data = data;
        let now = Instant::now();
        state.last_refresh = Some(now);
        state.expires_at = Some(now + CACHE_TTL);
        state.last_error = None;
    }

    // ---------- core fetch ----------

    /// Fetch the full registry. Resolution order:
    /// 1. Fresh in-memory cache (within [`CACHE_TTL`]).
    /// 2. Fresh-by-mtime disk cache — skips the network on cold-start processes
    ///    when a recent snapshot already exists on disk.
    /// 3. Network fetch (`base_url`); persist to disk on success.
    ///    Concurrent callers share a single-flight lock around this stage.
    /// 4. Disk cache (treated as expiring in [`DISK_FALLBACK_REMAINING`]).
    /// 5. Empty object (caller decides how to handle).
    pub async fn fetch(&self, force_refresh: bool) -> Value {
        if !force_refresh {
            // Stage 1: hot in-memory cache.
            {
                let state = self.state.read().expect("cache lock poisoned");
                if state.is_fresh() && state.is_populated() {
                    return state.data.clone();
                }
            }

            // Stage 2: fresh-by-mtime disk cache — avoids the network on
            // cold-start processes.
            if let Some(age) = self.disk_cache_age() {
                if age < CACHE_TTL {
                    if let Some(disk) = cache::load(&self.cache_path) {
                        let mut state = self.state.write().expect("cache lock poisoned");
                        state.data = disk.clone();
                        let now = Instant::now();
                        state.last_refresh = Some(now);
                        state.expires_at = Some(now + CACHE_TTL.saturating_sub(age));
                        state.last_error = None;
                        drop(state);
                        debug!("Loaded models.dev from fresh disk cache (age={age:.0?})");
                        return disk;
                    }
                }
            }
        }

        // Stage 3: network (single-flight).
        let _guard = self.fetch_lock.lock().await;

        // Re-check freshness after acquiring the lock — another waiter may
        // have already populated the cache.
        if !force_refresh {
            let state = self.state.read().expect("cache lock poisoned");
            if state.is_fresh() && state.is_populated() {
                return state.data.clone();
            }
        }

        match self.http.get(&self.base_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<Value>().await {
                        Ok(v) if v.is_object() && !v.as_object().unwrap().is_empty() => {
                            let mut state = self.state.write().expect("cache lock poisoned");
                            state.data = v.clone();
                            let now = Instant::now();
                            state.last_refresh = Some(now);
                            state.expires_at = Some(now + CACHE_TTL);
                            state.last_error = None;
                            drop(state);
                            if let Err(e) = cache::save(&self.cache_path, &v) {
                                debug!(?self.cache_path, "Failed to persist models.dev cache: {e}");
                            }
                            return v;
                        }
                        Ok(_) => {
                            let msg = "models.dev returned empty / non-object payload".to_string();
                            debug!("{msg}");
                            self.set_last_error(msg);
                        }
                        Err(e) => {
                            let msg = format!("Failed to decode models.dev JSON: {e}");
                            debug!("{msg}");
                            self.set_last_error(msg);
                        }
                    }
                } else {
                    let msg = format!("models.dev returned non-success status {}", resp.status());
                    debug!("{msg}");
                    self.set_last_error(msg);
                }
            }
            Err(e) => {
                let msg = format!("Failed to fetch models.dev: {e}");
                debug!("{msg}");
                self.set_last_error(msg);
            }
        }

        // Try disk fallback only if in-memory cache is empty.
        let already_populated = {
            let state = self.state.read().expect("cache lock poisoned");
            state.is_populated()
        };
        if !already_populated {
            if let Some(disk) = cache::load(&self.cache_path) {
                let mut state = self.state.write().expect("cache lock poisoned");
                state.data = disk.clone();
                let now = Instant::now();
                state.last_refresh = Some(now);
                state.expires_at = Some(now + DISK_FALLBACK_REMAINING);
                drop(state);
                debug!("Loaded models.dev from disk cache");
                return disk;
            }
        }

        // Return whatever we have (possibly empty). Normalise the
        // never-populated `Value::Null` to an empty object so callers can
        // always rely on `is_object()` regardless of failure path.
        let state = self.state.read().expect("cache lock poisoned");
        if state.data.is_null() {
            Value::Object(Map::new())
        } else {
            state.data.clone()
        }
    }

    fn set_last_error(&self, msg: String) {
        let mut state = self.state.write().expect("cache lock poisoned");
        state.last_error = Some(msg);
    }

    /// Synchronous read of the in-memory cache without triggering network.
    /// Returns an empty `Value::Object` if the cache is unpopulated.
    pub fn snapshot(&self) -> Value {
        let state = self.state.read().expect("cache lock poisoned");
        if state.data.is_null() {
            Value::Object(Map::new())
        } else {
            state.data.clone()
        }
    }

    // ---------- look-ups (sync, off the in-memory snapshot) ----------

    /// Look up the context window for a Nomifun `(platform, model)` pair.
    ///
    /// Resolution order:
    /// 1. Exact match, then case-insensitive fallback.
    /// 2. `:cloud` / `-cloud` suffix variants — some providers store model IDs
    ///    with these suffixes while the live API returns bare names.
    pub fn lookup_context(&self, provider: &str, model: &str) -> Option<u64> {
        let models = self.provider_models_map(provider)?;

        if let Some((_, entry)) = find_entry_case_insensitive(&models, model) {
            if let Some(ctx) = parse::extract_context(entry) {
                return Some(ctx);
            }
        }

        for suffix in [":cloud", "-cloud"] {
            let suffixed = format!("{model}{suffix}");
            if let Some((_, entry)) = find_entry_case_insensitive(&models, &suffixed) {
                if let Some(ctx) = parse::extract_context(entry) {
                    return Some(ctx);
                }
            }
        }

        None
    }

    /// Resolve the compact capability struct for a Nomifun `(platform, model)` pair.
    pub fn capabilities(&self, provider: &str, model: &str) -> Option<ModelCapabilities> {
        let entry = self.find_model_entry(provider, model)?;
        Some(parse::parse_model_capabilities(&entry))
    }

    /// Resolve full [`ModelInfo`] for a Nomifun / models.dev `(platform, model)` pair.
    pub fn model_info(&self, provider: &str, model: &str) -> Option<ModelInfo> {
        let mdev_id = mapping::resolve_models_dev_id(provider).to_string();
        let snap = self.snapshot();
        let provider_data = snap.get(&mdev_id)?;
        let models = provider_data.get("models")?.as_object()?;
        let (mid, entry) = find_entry_case_insensitive(models, model)?;
        Some(parse::parse_model_info(mid, entry, &mdev_id))
    }

    /// Resolve full [`ProviderInfo`] for a Nomifun / models.dev platform ID.
    pub fn provider_info(&self, provider: &str) -> Option<ProviderInfo> {
        let mdev_id = mapping::resolve_models_dev_id(provider).to_string();
        let snap = self.snapshot();
        let raw = snap.get(&mdev_id)?;
        if !raw.is_object() {
            return None;
        }
        Some(parse::parse_provider_info(&mdev_id, raw))
    }

    /// All model IDs for a Nomifun platform (any tool_call value).
    ///
    /// Excludes noise models hidden from the catalog (e.g. low-TPM Gemma
    /// variants and retired Gemini slugs for the Google/Gemini provider).
    pub fn list_provider_models(&self, provider: &str) -> Vec<String> {
        let Some(models) = self.provider_models_map(provider) else {
            return Vec::new();
        };
        models
            .keys()
            .filter(|mid| !should_hide(provider, mid))
            .cloned()
            .collect()
    }

    /// Model IDs suitable for agentic use: `tool_call=true`, not matching
    /// the noise patterns (see [`noise_re`]), and not hidden from catalog.
    pub fn list_agentic_models(&self, provider: &str) -> Vec<String> {
        let Some(models) = self.provider_models_map(provider) else {
            return Vec::new();
        };
        models
            .iter()
            .filter(|(mid, entry)| {
                !should_hide(provider, mid)
                    && entry
                        .as_object()
                        .map(|o| o.get("tool_call").and_then(Value::as_bool).unwrap_or(false))
                        .unwrap_or(false)
                    && !noise_re().is_match(mid)
            })
            .map(|(mid, _)| mid.clone())
            .collect()
    }

    /// Fuzzy search across the registry.
    ///
    /// Algorithm:
    /// 1. Substring matches first (case-insensitive), in declaration order.
    /// 2. Fill remaining slots via normalized-levenshtein matches with a
    ///    similarity ≥ 0.4.
    pub fn search(&self, query: &str, provider: Option<&str>, limit: usize) -> Vec<SearchHit> {
        if limit == 0 {
            return Vec::new();
        }

        let snap = self.snapshot();
        let snap_obj = match snap.as_object() {
            Some(o) if !o.is_empty() => o,
            _ => return Vec::new(),
        };

        let mut candidates: Vec<(String, String, Value)> = Vec::new();
        if let Some(p) = provider {
            let Some(mdev_id) = mapping::to_models_dev(p) else {
                return Vec::new();
            };
            if let Some(provider_data) = snap_obj.get(mdev_id) {
                if let Some(models) = provider_data.get("models").and_then(Value::as_object) {
                    for (mid, mdata) in models {
                        candidates.push((p.to_string(), mid.clone(), mdata.clone()));
                    }
                }
            }
        } else {
            for (platform, mdev) in mapping_pairs() {
                let Some(provider_data) = snap_obj.get(*mdev) else {
                    continue;
                };
                let Some(models) = provider_data.get("models").and_then(Value::as_object) else {
                    continue;
                };
                for (mid, mdata) in models {
                    candidates.push((platform.to_string(), mid.clone(), mdata.clone()));
                }
            }
        }
        if candidates.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let mut seen: std::collections::HashSet<(String, String)> = Default::default();
        let mut results: Vec<SearchHit> = Vec::new();

        for (prov, mid, entry) in &candidates {
            if mid.to_lowercase().contains(&query_lower) {
                let key = (prov.clone(), mid.clone());
                if seen.insert(key) {
                    results.push(SearchHit {
                        provider: prov.clone(),
                        model_id: mid.clone(),
                        entry: entry.clone(),
                    });
                    if results.len() >= limit {
                        return results;
                    }
                }
            }
        }

        let mut scored: Vec<(f64, &(String, String, Value))> = candidates
            .iter()
            .filter(|(prov, mid, _)| !seen.contains(&(prov.clone(), mid.clone())))
            .map(|c| {
                let score = normalized_levenshtein(&query_lower, &c.1.to_lowercase());
                (score, c)
            })
            .filter(|(score, _)| *score >= 0.4)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        for (_, (prov, mid, entry)) in scored {
            let key = (prov.clone(), mid.clone());
            if seen.insert(key) {
                results.push(SearchHit {
                    provider: prov.clone(),
                    model_id: mid.clone(),
                    entry: entry.clone(),
                });
                if results.len() >= limit {
                    break;
                }
            }
        }

        results
    }

    // ---------- private helpers ----------

    fn provider_models_map(&self, provider: &str) -> Option<Map<String, Value>> {
        let mdev_id = mapping::resolve_models_dev_id(provider);
        let snap = self.snapshot();
        snap.get(mdev_id)?.get("models")?.as_object().cloned()
    }

    /// Find a model entry honouring exact + case-insensitive match.
    fn find_model_entry(&self, provider: &str, model: &str) -> Option<Value> {
        let models = self.provider_models_map(provider)?;
        let (_id, entry) = find_entry_case_insensitive(&models, model)?;
        Some(entry.clone())
    }
}

impl Default for ModelsDevClient {
    fn default() -> Self {
        Self::default_production()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn find_entry_case_insensitive<'a>(
    models: &'a Map<String, Value>,
    model: &str,
) -> Option<(&'a String, &'a Value)> {
    if let Some((k, v)) = models.get_key_value(model) {
        if v.is_object() {
            return Some((k, v));
        }
    }
    let lower = model.to_lowercase();
    models
        .iter()
        .find(|(k, v)| k.to_lowercase() == lower && v.is_object())
}

/// Re-expose the static pairs from `mapping` without making them public.
fn mapping_pairs() -> &'static [(&'static str, &'static str)] {
    static PAIRS: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    PAIRS
        .get_or_init(|| {
            let mut v: Vec<_> = mapping::forward_map()
                .iter()
                .map(|(h, m)| (*h, *m))
                .collect();
            v.sort_by(|a, b| a.0.cmp(b.0));
            v
        })
        .as_slice()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "anthropic": {
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "api": "https://api.anthropic.com/v1",
                "doc": "https://docs.anthropic.com",
                "models": {
                    "claude-sonnet-4-5": {
                        "name": "Claude Sonnet 4.5",
                        "family": "claude",
                        "tool_call": true,
                        "attachment": true,
                        "limit": {"context": 200000, "output": 8192},
                        "cost": {"input": 3.0, "output": 15.0}
                    },
                    "claude-haiku-3-5": {
                        "tool_call": true,
                        "limit": {"context": 200000}
                    },
                    "claude-3-tts": {
                        "tool_call": true,
                        "limit": {"context": 100000}
                    }
                }
            },
            "google": {
                "name": "Google",
                "env": ["GOOGLE_API_KEY"],
                "api": "https://generativelanguage.googleapis.com",
                "models": {
                    "gemini-2.0-flash": {
                        "tool_call": true,
                        "attachment": true,
                        "limit": {"context": 1048576, "output": 8192}
                    },
                    "embedding-001": {
                        "tool_call": false
                    },
                    "gemini-live-001": {
                        "tool_call": true
                    }
                }
            },
            "deepseek": {
                "name": "DeepSeek",
                "env": ["DEEPSEEK_API_KEY"],
                "api": "https://api.deepseek.com",
                "models": {
                    "deepseek-chat": {
                        "tool_call": true,
                        "limit": {"context": 64000}
                    }
                }
            }
        })
    }

    fn client_with_fixture() -> ModelsDevClient {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("cache.json");
        let c = ModelsDevClient::new("http://invalid.invalid/api.json", cache_path, None);
        c.seed_cache(fixture());
        std::mem::forget(dir);
        c
    }

    #[test]
    fn lookup_context_returns_value_for_known_models() {
        let c = client_with_fixture();
        assert_eq!(
            c.lookup_context("anthropic", "claude-sonnet-4-5"),
            Some(200_000)
        );
        assert_eq!(
            c.lookup_context("gemini", "gemini-2.0-flash"),
            Some(1_048_576)
        );
    }

    #[test]
    fn lookup_context_case_insensitive_fallback() {
        let c = client_with_fixture();
        assert_eq!(
            c.lookup_context("anthropic", "CLAUDE-SONNET-4-5"),
            Some(200_000)
        );
    }

    #[test]
    fn lookup_context_returns_none_for_unknown() {
        let c = client_with_fixture();
        assert_eq!(c.lookup_context("anthropic", "nonexistent"), None);
        assert_eq!(c.lookup_context("unknown-provider", "any"), None);
    }

    #[test]
    fn capabilities_uses_models_dev_data() {
        let c = client_with_fixture();
        let caps = c.capabilities("anthropic", "claude-sonnet-4-5").unwrap();
        assert!(caps.supports_tools);
        assert!(caps.supports_vision);
        assert!(!caps.supports_reasoning);
        assert_eq!(caps.context_window, 200_000);
        assert_eq!(caps.max_output_tokens, 8_192);
        assert_eq!(caps.model_family, "claude");
    }

    #[test]
    fn model_info_returns_full_struct() {
        let c = client_with_fixture();
        let info = c.model_info("anthropic", "claude-sonnet-4-5").unwrap();
        assert_eq!(info.id, "claude-sonnet-4-5");
        assert_eq!(info.name, "Claude Sonnet 4.5");
        assert_eq!(info.cost_input, 3.0);
        assert!(info.has_cost_data());
        assert!(info.supports_vision());
    }

    #[test]
    fn provider_info_returns_metadata() {
        let c = client_with_fixture();
        let info = c.provider_info("anthropic").unwrap();
        assert_eq!(info.id, "anthropic");
        assert_eq!(info.env, vec!["ANTHROPIC_API_KEY"]);
        assert_eq!(info.model_count, 3);
    }

    #[test]
    fn provider_info_resolves_alias_through_mapping() {
        let c = client_with_fixture();
        let info = c.provider_info("gemini").unwrap();
        assert_eq!(info.id, "google");
        let info = c.provider_info("claude").unwrap();
        assert_eq!(info.id, "anthropic");
    }

    #[test]
    fn list_provider_models_returns_all() {
        let c = client_with_fixture();
        let mut models = c.list_provider_models("anthropic");
        models.sort();
        assert_eq!(
            models,
            vec!["claude-3-tts", "claude-haiku-3-5", "claude-sonnet-4-5"]
        );
    }

    #[test]
    fn list_provider_models_accepts_models_dev_provider_ids() {
        let c = client_with_fixture();
        // Unmapped models.dev IDs fall back to themselves via resolve_models_dev_id.
        let mut via_models_dev = c.list_provider_models("deepseek");
        via_models_dev.sort();
        assert_eq!(via_models_dev, vec!["deepseek-chat"]);
    }

    #[test]
    fn list_agentic_models_filters_noise_and_non_tool_call() {
        let c = client_with_fixture();
        let mut models = c.list_agentic_models("anthropic");
        models.sort();
        assert_eq!(models, vec!["claude-haiku-3-5", "claude-sonnet-4-5"]);

        let g_models = c.list_agentic_models("gemini");
        assert!(g_models.is_empty());
    }

    #[test]
    fn search_substring_match_first() {
        let c = client_with_fixture();
        let hits = c.search("sonnet", None, 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].model_id, "claude-sonnet-4-5");
    }

    #[test]
    fn search_falls_back_to_fuzzy() {
        let c = client_with_fixture();
        let hits = c.search("haiku", Some("anthropic"), 3);
        assert!(hits.iter().any(|h| h.model_id == "claude-haiku-3-5"));
    }

    #[test]
    fn search_respects_limit() {
        let c = client_with_fixture();
        let hits = c.search("claude", None, 2);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn search_unknown_provider_returns_empty() {
        let c = client_with_fixture();
        let hits = c.search("anything", Some("not-a-provider"), 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn search_empty_registry_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let c = ModelsDevClient::new("http://invalid/", dir.path().join("c.json"), None);
        let hits = c.search("claude", None, 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn lookup_context_cloud_suffix_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let c = ModelsDevClient::new("http://invalid/", dir.path().join("c.json"), None);
        c.seed_cache(json!({
            "moonshot": {
                "models": {
                    "kimi-k2.6:cloud": {"limit": {"context": 131072}},
                    "kimi-k2.6-cloud": {"limit": {"context": 65536}}
                }
            }
        }));
        assert_eq!(c.lookup_context("moonshot-cn", "kimi-k2.6"), Some(131_072));
    }

    #[test]
    fn lookup_context_dash_cloud_suffix_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let c = ModelsDevClient::new("http://invalid/", dir.path().join("c.json"), None);
        c.seed_cache(json!({
            "moonshot": {
                "models": {
                    "kimi-k2.6-cloud": {"limit": {"context": 65536}}
                }
            }
        }));
        assert_eq!(
            c.lookup_context("moonshot-global", "kimi-k2.6"),
            Some(65_536)
        );
    }

    #[test]
    fn google_hidden_models_excluded_from_list_provider_models() {
        let c = client_with_fixture();
        let models = c.list_provider_models("gemini");
        assert!(!models.contains(&"gemini-2.0-flash".to_string()));
        assert!(models.contains(&"embedding-001".to_string()));
    }

    #[test]
    fn google_hidden_models_excluded_from_list_agentic_models() {
        let c = client_with_fixture();
        let models = c.list_agentic_models("gemini");
        assert!(!models.contains(&"gemini-2.0-flash".to_string()));
    }

    #[test]
    fn status_reports_populated_fixture() {
        let c = client_with_fixture();
        let s = c.status();
        assert!(s.populated);
        assert!(s.provider_count >= 2);
        assert!(s.model_count >= 4);
        assert!(s.last_error.is_none());
        assert!(s.cache_age_secs.is_some());
    }

    #[tokio::test]
    async fn fetch_uses_disk_cache_when_fresh_by_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("c.json");
        cache::save(&cache_path, &fixture()).unwrap();
        let c = ModelsDevClient::new("http://127.0.0.1:1/api.json", cache_path, None);
        let v = c.fetch(false).await;
        assert!(v.get("anthropic").is_some());
    }

    #[tokio::test]
    async fn fetch_uses_in_memory_cache_when_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let c = ModelsDevClient::new(
            "http://127.0.0.1:1/api.json",
            dir.path().join("c.json"),
            None,
        );
        c.seed_cache(fixture());
        let v = c.fetch(false).await;
        assert!(v.is_object());
        assert!(v.get("anthropic").is_some());
    }

    #[tokio::test]
    async fn fetch_falls_back_to_disk_when_network_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("c.json");
        cache::save(&cache_path, &fixture()).unwrap();

        let c = ModelsDevClient::new("http://127.0.0.1:1/api.json", cache_path.clone(), None);
        let v = c.fetch(false).await;
        assert!(v.is_object());
        assert!(v.get("anthropic").is_some());
    }

    #[tokio::test]
    async fn fetch_returns_empty_when_everything_fails() {
        let dir = tempfile::tempdir().unwrap();
        let c = ModelsDevClient::new(
            "http://127.0.0.1:1/api.json",
            dir.path().join("missing.json"),
            None,
        );
        let v = c.fetch(false).await;
        assert!(v.is_object());
        assert!(v.as_object().unwrap().is_empty());
        let s = c.status();
        assert!(!s.populated);
        assert!(s.last_error.is_some());
    }
}
