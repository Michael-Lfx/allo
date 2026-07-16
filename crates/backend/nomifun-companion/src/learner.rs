//! The scheduled learning loop: every tick, if enabled and due, read new
//! collected events, run one LLM distillation call, and apply the output
//! (memories / reinforcement / supersedes / suggestions / mood / diary).

use std::path::PathBuf;
use std::sync::Arc;

use nomifun_ai_agent::nomi_config;
use nomifun_ai_agent::{one_shot_completion, resolve_provider_config, user_message};
use nomifun_common::{AppError, generate_prefixed_id, now_ms};
use nomifun_db::IProviderRepository;
use tokio::sync::Mutex;

use crate::collector::{SharedConfig, read_events_since};
use crate::events::CompanionEventEmitter;
use crate::prompt::{self, LEARN_MAX_TOKENS};
use crate::registry::CompanionRegistry;
use crate::store::{MemoryFilter, CompanionLearnRun, CompanionStore};

const MAX_EVENTS_PER_RUN: usize = 300;
const TICK_SECONDS: u64 = 60;
/// After this many consecutive scheduled runs fail to parse, the batch is
/// abandoned (cursor advanced) instead of re-burning tokens forever.
const PARSE_FAIL_GIVE_UP_RUNS: i64 = 3;
/// Max suggestions accepted from one learn run (quality-first; the system prompt
/// already asks for 0~2 and permits an empty array).
const MAX_SUGGESTIONS_PER_RUN: usize = 2;

/// Suggestion cadence tunables (internal — NOT user config). The learner still
/// distills memories/mood/diary every run; this gate only throttles *suggestions*
/// so they stay infrequent, high-signal and non-repetitive without losing the
/// memory-side context. Production uses [`SuggestionGate::production`]; tests use
/// [`SuggestionGate::open`] to exercise the apply/dedup pipeline without throttling.
#[derive(Debug, Clone, Copy)]
pub struct SuggestionGate {
    /// Min new events accumulated since the last emitted suggestion before a new
    /// batch of suggestions may be proposed (evidence gate — frequency scales with
    /// real activity, not the wall clock).
    pub min_events: i64,
    /// Wall-clock cooldown between suggestion bursts, in ms (caps burst rate even
    /// when the user is very active).
    pub cooldown_ms: i64,
    /// Max pending (status='new') suggestions; at/above this, propose none
    /// (backpressure — don't pile onto an unhandled backlog).
    pub max_pending: i64,
    /// A recently-decided idea won't be re-raised within this window, in ms
    /// (cross-time dedup). Zero disables the cross-time check.
    pub decided_repeat_cooldown_ms: i64,
}

impl SuggestionGate {
    /// Production cadence: at most one suggestion burst every 4h, gated on ≥15 new
    /// events, capped at 8 pending, and no repeating a decided idea within 3 days.
    pub fn production() -> Self {
        Self {
            min_events: 15,
            cooldown_ms: 4 * 60 * 60 * 1000,
            max_pending: 8,
            decided_repeat_cooldown_ms: 3 * 24 * 60 * 60 * 1000,
        }
    }

    /// Gate-open: never throttles insertion and disables the cross-time window.
    /// Used by tests that assert the apply/dedup pipeline directly.
    pub fn open() -> Self {
        Self { min_events: 0, cooldown_ms: 0, max_pending: i64::MAX, decided_repeat_cooldown_ms: 0 }
    }
}

/// LLM seam so tests can run the learner without a live provider.
/// (Companion chat runs on the real agent engine; this trait only serves
/// the scheduled learning distillation calls.)
#[async_trait::async_trait]
pub trait CompanionCompleter: Send + Sync {
    async fn complete(&self, provider_id: &str, model: &str, system: &str, user: &str, max_tokens: u32)
    -> Result<String, AppError>;
}

/// Production completer: provider row → nomi Config → one-shot completion.
pub struct LiveCompanionCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

impl LiveCompanionCompleter {
    async fn resolve(&self, provider_id: &str, model: &str) -> Result<nomi_config::config::Config, AppError> {
        resolve_provider_config(
            &self.provider_repo,
            &self.encryption_key,
            provider_id,
            model,
            &self.workspace,
        )
        .await
    }
}

#[async_trait::async_trait]
impl CompanionCompleter for LiveCompanionCompleter {
    async fn complete(
        &self,
        provider_id: &str,
        model: &str,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        let cfg = self.resolve(provider_id, model).await?;
        one_shot_completion(&cfg, system, vec![user_message(user)], max_tokens).await
    }
}

pub struct Learner {
    pub companion_dir: PathBuf,
    pub config: SharedConfig,
    pub store: CompanionStore,
    /// Companion roster: learn-run XP is a shared achievement granted to every companion.
    pub registry: Arc<CompanionRegistry>,
    pub completer: Arc<dyn CompanionCompleter>,
    pub emitter: CompanionEventEmitter,
    /// Re-entrancy guard shared between the tick loop and "run now".
    pub run_lock: Arc<Mutex<()>>,
    /// Suggestion cadence gate (internal defaults; see [`SuggestionGate`]).
    pub gate: SuggestionGate,
}

impl Learner {
    /// Spawn the periodic tick loop.
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(TICK_SECONDS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let (enabled, interval_minutes) = {
                    let cfg = self.config.read().await;
                    (cfg.learn.enabled, cfg.learn.interval_minutes.max(5) as i64)
                };
                if !enabled {
                    continue;
                }
                let last_run = self.store.get_state_i64("last_learn_ts").await.unwrap_or(0);
                if now_ms() - last_run < interval_minutes * 60_000 {
                    continue;
                }
                if let Err(e) = self.run_once().await {
                    tracing::warn!(error = %e, "companion scheduled learn run failed");
                }
            }
        });
    }

    /// One learning run. Returns the persisted run record.
    pub async fn run_once(&self) -> Result<CompanionLearnRun, AppError> {
        let Ok(_guard) = self.run_lock.try_lock() else {
            return Err(AppError::Conflict("a learn run is already in progress".into()));
        };
        let started_at = now_ms();
        // Stamp first so a crashed/failed run doesn't hot-loop the scheduler.
        self.store.set_state("last_learn_ts", &started_at.to_string()).await?;

        let model = { self.config.read().await.learn.model.clone() };
        let mut run = CompanionLearnRun {
            id: generate_prefixed_id("plr"),
            started_at,
            finished_at: None,
            status: "ok".into(),
            events_processed: 0,
            memories_added: 0,
            suggestions_added: 0,
            error: None,
            summary: None,
        };

        let Some(model) = model else {
            run.status = "model_unconfigured".into();
            run.finished_at = Some(now_ms());
            self.store.insert_learn_run(&run).await?;
            return Ok(run);
        };

        let cursor = self.store.get_state_i64("learn_cursor_ts").await?;
        let (events, truncated) = read_events_since(&self.companion_dir, cursor, MAX_EVENTS_PER_RUN);
        if events.is_empty() {
            run.status = "no_events".into();
            run.finished_at = Some(now_ms());
            self.store.insert_learn_run(&run).await?;
            return Ok(run);
        }
        run.events_processed = events.len() as i64;
        let new_cursor = events.last().map(|e| e.ts).unwrap_or(cursor);

        // 选项A：共享学习产出只由「默认体」窗口呈现，避免 N 个伙伴窗口同时弹气泡（提示风暴）。
        let target = {
            let did = { self.config.read().await.default_companion_id.clone() };
            self.registry.resolve_default(did.as_deref()).await
        };

        if let Some(target) = target.as_deref() {
            self.emitter.emit_learn_started(target);
        }

        // Existing-memory digest for reinforcement/conflict matching, plus
        // the pending suggestions so the model can avoid re-raising them.
        let existing = self
            .store
            .list_memories(&MemoryFilter {
                status: Some("active".into()),
                limit: 120,
                ..Default::default()
            })
            .await?;
        let pending_suggestions = self.store.list_suggestions(Some("new"), 50).await.unwrap_or_default();

        // Suggestion cadence gate (A: evidence + cooldown; B: pending backpressure).
        // Memories/mood/diary always distill; only *suggestions* are throttled so
        // they stay infrequent and high-signal without losing memory-side context.
        let pending_new = pending_suggestions.len() as i64;
        let last_suggestion_ts = self.store.get_state_i64("last_suggestion_ts").await.unwrap_or(0);
        let prev_events_since = self.store.get_state_i64("events_since_suggestion").await.unwrap_or(0);
        let events_since = prev_events_since + run.events_processed;
        let cooldown_ok = now_ms().saturating_sub(last_suggestion_ts) >= self.gate.cooldown_ms;
        let evidence_ok = events_since >= self.gate.min_events;
        let capacity_ok = pending_new < self.gate.max_pending;
        let suggestions_allowed = cooldown_ok && evidence_ok && capacity_ok;

        // Recently-decided suggestions feed the model cross-time context (C) so it
        // won't re-raise a just-handled idea, restoring holistic 统筹 while throttled.
        let recently_decided = self
            .store
            .list_recently_decided_suggestions(self.gate.decided_repeat_cooldown_ms, 30)
            .await
            .unwrap_or_default();

        let event_lines: Vec<String> = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect();
        let user_prompt = prompt::build_learn_prompt(
            &existing,
            &pending_suggestions,
            &recently_decided,
            &event_lines,
            truncated,
            suggestions_allowed,
        );

        // One retry on parse failure (the model occasionally wraps in prose).
        let mut parsed = None;
        let mut last_err = String::new();
        let mut provider_failed = false;
        for attempt in 0..2 {
            match self
                .completer
                .complete(&model.provider_id, &model.model, prompt::LEARN_SYSTEM, &user_prompt, LEARN_MAX_TOKENS)
                .await
            {
                Ok(raw) => match prompt::parse_learn_output(&raw) {
                    Ok(out) => {
                        parsed = Some(out);
                        break;
                    }
                    Err(e) => {
                        last_err = e;
                        tracing::debug!(attempt, error = %last_err, "companion learn output unparseable");
                    }
                },
                Err(e) => {
                    last_err = e.to_string();
                    provider_failed = true;
                    break; // provider failure: don't burn a retry
                }
            }
        }

        let Some(output) = parsed else {
            run.status = "error".into();
            run.error = Some(last_err);
            run.finished_at = Some(now_ms());
            // Provider failure is transient: keep the cursor so the same
            // events retry once the provider recovers. Parse failure is the
            // model misformatting — retry the batch a few scheduled runs,
            // then advance past it so a consistently-confused model can't
            // re-burn tokens on the same batch forever.
            if !provider_failed {
                let streak = self.store.get_state_i64("learn_parse_fail_streak").await.unwrap_or(0) + 1;
                if streak >= PARSE_FAIL_GIVE_UP_RUNS {
                    self.store.set_state("learn_cursor_ts", &new_cursor.to_string()).await?;
                    self.store.set_state("learn_parse_fail_streak", "0").await?;
                    tracing::warn!(events = run.events_processed, "companion learn batch abandoned after repeated parse failures");
                } else {
                    self.store.set_state("learn_parse_fail_streak", &streak.to_string()).await?;
                }
            }
            self.store.insert_learn_run(&run).await?;
            if let Some(target) = target.as_deref() {
                self.emitter.emit_learn_finished(target, &run);
            }
            return Ok(run);
        };
        let _ = self.store.set_state("learn_parse_fail_streak", "0").await;

        // Apply: decay first, then reinforce/supersede/insert.
        let _ = self.store.decay_memories().await;
        self.store.reinforce_memories(&output.reinforce_ids).await?;
        self.store.archive_memories(&output.supersede_ids).await?;

        let prior_active = self.store.count_memories("active").await.unwrap_or(0);
        for m in &output.memories {
            if self.store.find_similar_active(&m.kind, &m.content).await?.is_some() {
                continue;
            }
            self.store
                .insert_memory(&m.kind, &m.content, &m.tags, m.importance, "learn")
                .await?;
            run.memories_added += 1;
        }
        // First-preference milestone: the moment nomi visibly "gets" you.
        if prior_active == 0 && run.memories_added > 0 {
            let milestone = self
                .store
                .insert_suggestion(
                    "insight",
                    "nomi 学会了关于你的第一条记忆！",
                    "我开始懂你了，快来记忆页看看吧～",
                    Some(&serde_json::json!({"type": "navigate", "to": "/nomi?tab=memories"})),
                )
                .await?;
            run.suggestions_added += 1;
            if let Some(target) = target.as_deref() {
                self.emitter.emit_suggestion_created(target, &milestone);
            }
        }
        let mut emitted_suggestion = false;
        for s in output.suggestions.iter().take(MAX_SUGGESTIONS_PER_RUN) {
            // Insert-side dedup backstop: even when the model ignores the
            // "don't repeat pending suggestions" rule, a similar status='new'
            // suggestion blocks the duplicate. The hit is not silently
            // dropped: the existing suggestion is touched (created_at bumped)
            // so repeated evidence re-floats it instead of vanishing. This
            // runs regardless of the cadence gate (touching is free and keeps
            // the backlog fresh).
            if let Some(existing_id) = self.store.find_similar_suggestion(&s.kind, &s.title, &s.body).await? {
                if let Err(e) = self.store.touch_suggestion(&existing_id).await {
                    tracing::warn!(error = %e, suggestion_id = %existing_id, "companion learn failed to touch duplicate suggestion");
                }
                continue;
            }
            // Cross-time dedup (C): don't re-raise an idea the owner just
            // accepted or dismissed within the repeat-cooldown window.
            if self
                .store
                .find_recent_decided_similar(&s.kind, &s.title, &s.body, self.gate.decided_repeat_cooldown_ms)
                .await?
                .is_some()
            {
                continue;
            }
            // Cadence gate (A/B): a genuinely new suggestion is inserted only
            // when there's enough fresh evidence, the cooldown has elapsed and
            // the pending backlog isn't full. Otherwise skip — evidence keeps
            // accumulating for a later, higher-signal burst.
            if !suggestions_allowed {
                continue;
            }
            // Optimization 9: when a create_skill suggestion carries
            // knowledge_base content, embed it in the action JSON so the
            // service layer can create a KB page when the user accepts.
            let action = if s.kind == "create_skill" && s.knowledge_base.is_some() {
                let mut action = s.action.clone().unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = action.as_object_mut() {
                    obj.insert(
                        "knowledge_base".to_string(),
                        serde_json::Value::String(s.knowledge_base.clone().unwrap_or_default()),
                    );
                    if !obj.contains_key("type") {
                        obj.insert("type".to_string(), serde_json::Value::String("navigate".to_string()));
                    }
                    if !obj.contains_key("to") {
                        obj.insert("to".to_string(), serde_json::Value::String("/nomi?tab=skills".to_string()));
                    }
                }
                Some(action)
            } else {
                s.action.clone()
            };
            let created = self
                .store
                .insert_suggestion(&s.kind, &s.title, &s.body, action.as_ref())
                .await?;
            run.suggestions_added += 1;
            emitted_suggestion = true;
            if let Some(target) = target.as_deref() {
                self.emitter.emit_suggestion_created(target, &created);
            }
        }

        // Advance the suggestion cadence counters: a fresh burst resets both the
        // event tally and the cooldown clock; a throttled/empty run banks the
        // accumulated evidence for next time.
        if emitted_suggestion {
            let _ = self.store.set_state("last_suggestion_ts", &now_ms().to_string()).await;
            let _ = self.store.set_state("events_since_suggestion", "0").await;
        } else {
            let _ = self.store.set_state("events_since_suggestion", &events_since.to_string()).await;
        }

        if let Some(mood) = &output.mood {
            self.store.set_state("mood", mood).await?;
            if let Some(target) = target.as_deref() {
                self.emitter.emit_mood_changed(target, mood);
            }
        }
        run.summary = output.diary;

        // XP: 1 per event + 5 per new memory — a shared achievement, granted
        // to every companion in the roster (spec ruling 2: the family grows
        // together on the shared learning loop).
        let _ = self
            .store
            .add_xp_all(
                &self.registry.ids().await,
                run.events_processed + run.memories_added * 5,
            )
            .await;

        self.store.set_state("learn_cursor_ts", &new_cursor.to_string()).await?;
        run.finished_at = Some(now_ms());
        self.store.insert_learn_run(&run).await?;
        if let Some(target) = target.as_deref() {
            self.emitter.emit_learn_finished(target, &run);
        }
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::{CollectedEvent, append_event};
    use crate::profile::SharedCompanionConfig;
    use nomifun_api_types::WebSocketMessage;
    use nomifun_realtime::BroadcastEventBus;
    use tokio::sync::RwLock;

    struct CannedCompleter(String);

    #[async_trait::async_trait]
    impl CompanionCompleter for CannedCompleter {
        async fn complete(&self, _p: &str, _m: &str, _s: &str, _u: &str, _t: u32) -> Result<String, AppError> {
            Ok(self.0.clone())
        }
    }

    /// Learner over a temp dir with one registered companion (so the shared XP
    /// grant has someone to land on). Returns the learner + that companion's id.
    async fn make_learner(dir: &std::path::Path, reply: &str) -> (Learner, String) {
        let mut config = SharedCompanionConfig::default();
        config.learn.model = Some(nomifun_common::ProviderWithModel {
            provider_id: nomifun_common::ProviderId::new().into_string(),
            model: "test-model".into(),
            use_model: None,
        });
        let registry = Arc::new(CompanionRegistry::scan(dir.join("companions"), dir.join("shared")));
        let companion = registry.create("测试宠", "ink").await.unwrap();
        let learner = Learner {
            companion_dir: dir.to_path_buf(),
            config: Arc::new(RwLock::new(config)),
            store: CompanionStore::open_memory().await.unwrap(),
            registry,
            completer: Arc::new(CannedCompleter(reply.to_owned())),
            emitter: CompanionEventEmitter::new(Arc::new(BroadcastEventBus::new(16)), "owner-a"),
            run_lock: Arc::new(Mutex::new(())),
            gate: SuggestionGate::open(),
        };
        (learner, companion.id)
    }

    fn seed_event(dir: &std::path::Path) {
        append_event(
            dir,
            &CollectedEvent {
                ts: now_ms(),
                source: "chat_user_messages".into(),
                name: "message.userCreated".into(),
                data: serde_json::json!({"content": "帮我看看 Rust 编译错误"}),
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn run_once_applies_learn_output() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let reply = r#"{"memories":[{"kind":"profile","content":"主人是 Rust 工程师","importance":0.9}],
            "suggestions":[{"kind":"insight","title":"洞察","body":"最近常调编译错误"}],
            "mood":"content","diary":"今天陪主人修了 bug～"}"#;
        let (learner, companion_id) = make_learner(dir.path(), reply).await;
        let run = learner.run_once().await.unwrap();
        assert_eq!(run.status, "ok");
        assert_eq!(run.events_processed, 1);
        assert_eq!(run.memories_added, 1);
        // 1 real suggestion + 1 first-memory milestone
        assert_eq!(run.suggestions_added, 2);
        assert_eq!(learner.store.get_state("mood").await.unwrap().unwrap(), "content");
        assert!(learner.store.get_state_i64("learn_cursor_ts").await.unwrap() > 0);
        // Shared XP grant lands on every registered companion (1 event + 1*5).
        assert_eq!(learner.store.get_companion_state_i64(&companion_id, "xp").await.unwrap(), 6);
        assert_eq!(learner.store.get_state_i64("xp").await.unwrap(), 0);
        // Cursor advanced: a second run sees no events.
        let run2 = learner.run_once().await.unwrap();
        assert_eq!(run2.status, "no_events");
    }

    #[tokio::test]
    async fn run_once_skips_duplicate_pending_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let reply = r#"{"suggestions":[{"kind":"insight","title":"最近常调编译错误","body":"建议看看构建脚本"}]}"#;
        let (learner, _) = make_learner(dir.path(), reply).await;

        let run1 = learner.run_once().await.unwrap();
        assert_eq!(run1.suggestions_added, 1);
        assert_eq!(learner.store.count_suggestions("new").await.unwrap(), 1);
        let first = &learner.store.list_suggestions(Some("new"), 10).await.unwrap()[0];
        let (first_id, first_created_at) = (first.id.clone(), first.created_at);

        // Same model output over a new event batch: the pending suggestion
        // blocks the duplicate, and the dedup hit touches it (created_at
        // bumped) instead of silently dropping the repeated evidence.
        // (Sleep keeps the new event's ms timestamp past the advanced
        // cursor and guarantees a strictly larger touch timestamp.)
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        seed_event(dir.path());
        let run2 = learner.run_once().await.unwrap();
        assert_eq!(run2.status, "ok");
        assert_eq!(run2.suggestions_added, 0);
        assert_eq!(learner.store.count_suggestions("new").await.unwrap(), 1);
        let touched = &learner.store.list_suggestions(Some("new"), 10).await.unwrap()[0];
        assert_eq!(touched.id, first_id, "dedup must keep the existing suggestion");
        assert!(
            touched.created_at > first_created_at,
            "dedup hit must touch the existing suggestion ({} -> {})",
            first_created_at,
            touched.created_at
        );

        // Once decided, the same suggestion may be raised again.
        let pending = learner.store.list_suggestions(Some("new"), 10).await.unwrap();
        learner.store.decide_suggestion(&pending[0].id, false).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        seed_event(dir.path());
        let run3 = learner.run_once().await.unwrap();
        assert_eq!(run3.suggestions_added, 1);
    }

    #[tokio::test]
    async fn run_once_records_error_on_garbage_output() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let (learner, _) = make_learner(dir.path(), "我不会输出 JSON").await;
        let run = learner.run_once().await.unwrap();
        assert_eq!(run.status, "error");
        assert!(run.error.is_some());
    }

    #[tokio::test]
    async fn run_once_skips_when_model_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let (learner, _) = make_learner(dir.path(), "{}").await;
        learner.config.write().await.learn.model = Default::default();
        let run = learner.run_once().await.unwrap();
        assert_eq!(run.status, "model_unconfigured");
    }

    /// Evidence gate: a thin batch (below `min_events`) is throttled — no
    /// suggestion is inserted, and the evidence is banked (not lost) so a later,
    /// richer run can fire. This is the core "control frequency" behaviour.
    #[tokio::test]
    async fn run_once_throttles_then_fires_on_accumulated_evidence() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path()); // 1 event only
        // A suggestion but NO memory, so the first-memory milestone can't muddy the count.
        let reply = r#"{"suggestions":[{"kind":"insight","title":"洞察","body":"最近常调编译错误"}]}"#;
        let (mut learner, _) = make_learner(dir.path(), reply).await;
        learner.gate = SuggestionGate { min_events: 5, cooldown_ms: 0, max_pending: i64::MAX, decided_repeat_cooldown_ms: 0 };

        let run1 = learner.run_once().await.unwrap();
        assert_eq!(run1.status, "ok");
        assert_eq!(run1.suggestions_added, 0, "1 event < 5-event threshold → throttled");
        assert_eq!(learner.store.count_suggestions("new").await.unwrap(), 0);
        assert_eq!(
            learner.store.get_state_i64("events_since_suggestion").await.unwrap(),
            1,
            "evidence is banked, not lost"
        );

        // Accumulate past the threshold: a later run now clears the gate.
        for _ in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            seed_event(dir.path());
        }
        let run2 = learner.run_once().await.unwrap();
        assert_eq!(run2.suggestions_added, 1, "banked 1 + 5 new ≥ 5 → suggestion emitted");
        assert_eq!(
            learner.store.get_state_i64("events_since_suggestion").await.unwrap(),
            0,
            "a fresh burst resets the evidence tally"
        );
    }

    /// Backpressure (B): when the pending backlog is already at the cap, no new
    /// suggestion is added regardless of fresh evidence.
    #[tokio::test]
    async fn run_once_backpressure_caps_pending_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let reply = r#"{"suggestions":[{"kind":"insight","title":"全新洞察","body":"全新内容"}]}"#;
        let (mut learner, _) = make_learner(dir.path(), reply).await;
        learner.gate = SuggestionGate { min_events: 0, cooldown_ms: 0, max_pending: 2, decided_repeat_cooldown_ms: 0 };
        learner.store.insert_suggestion("insight", "占位一", "x", None).await.unwrap();
        learner.store.insert_suggestion("insight", "占位二", "y", None).await.unwrap();
        assert_eq!(learner.store.count_suggestions("new").await.unwrap(), 2);

        let run = learner.run_once().await.unwrap();
        assert_eq!(run.suggestions_added, 0, "backlog at cap → no new suggestion");
        assert_eq!(learner.store.count_suggestions("new").await.unwrap(), 2);
    }

    /// Cross-time dedup (C): an idea the owner just dismissed is not re-raised
    /// while it's inside the repeat-cooldown window, even with the cadence gate open.
    #[tokio::test]
    async fn run_once_skips_recently_decided_idea() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let reply = r#"{"suggestions":[{"kind":"insight","title":"最近常调编译错误","body":"看看构建脚本"}]}"#;
        let (mut learner, _) = make_learner(dir.path(), reply).await;
        learner.gate = SuggestionGate {
            min_events: 0,
            cooldown_ms: 0,
            max_pending: i64::MAX,
            decided_repeat_cooldown_ms: 24 * 60 * 60 * 1000,
        };
        // An identical idea, dismissed just now.
        let s = learner
            .store
            .insert_suggestion("insight", "最近常调编译错误", "看看构建脚本", None)
            .await
            .unwrap();
        learner.store.decide_suggestion(&s.id, false).await.unwrap();

        let run = learner.run_once().await.unwrap();
        assert_eq!(run.suggestions_added, 0, "a just-dismissed idea is not re-raised within the window");
        assert_eq!(learner.store.count_suggestions("new").await.unwrap(), 0);
    }

    #[derive(Default)]
    struct RecordingBroadcaster {
        events: std::sync::Mutex<Vec<WebSocketMessage<serde_json::Value>>>,
    }
    impl nomifun_realtime::UserEventSink for RecordingBroadcaster {
        fn send_to_user(&self, _user_id: &str, e: WebSocketMessage<serde_json::Value>) {
            self.events.lock().unwrap().push(e);
        }
    }

    #[tokio::test]
    async fn learn_events_scoped_to_default_companion() {
        let dir = tempfile::tempdir().unwrap();
        seed_event(dir.path());
        let reply = r#"{"memories":[{"kind":"profile","content":"主人是 Rust 工程师","importance":0.9}],
            "suggestions":[{"kind":"insight","title":"洞察","body":"最近常调编译错误"}],
            "mood":"content","diary":"今天陪主人修了 bug～"}"#;

        let mut config = SharedCompanionConfig::default();
        config.learn.model = Some(nomifun_common::ProviderWithModel {
            provider_id: nomifun_common::ProviderId::new().into_string(),
            model: "test-model".into(),
            use_model: None,
        });
        let registry = Arc::new(CompanionRegistry::scan(dir.path().join("companions"), dir.path().join("shared")));
        let _a = registry.create("甲", "ink").await.unwrap();
        let b = registry.create("乙", "ink").await.unwrap();
        config.default_companion_id = Some(b.id.clone()); // 默认体 = 乙

        let bc = Arc::new(RecordingBroadcaster::default());
        let learner = Learner {
            companion_dir: dir.path().to_path_buf(),
            config: Arc::new(RwLock::new(config)),
            store: CompanionStore::open_memory().await.unwrap(),
            registry,
            completer: Arc::new(CannedCompleter(reply.to_owned())),
            emitter: CompanionEventEmitter::new(bc.clone(), "owner-a"),
            run_lock: Arc::new(Mutex::new(())),
            gate: SuggestionGate::open(),
        };
        learner.run_once().await.unwrap();

        let events = bc.events.lock().unwrap().clone();
        for name in [
            "companion.suggestion-created",
            "companion.mood-changed",
            "companion.learn-finished",
            "companion.learn-started",
        ] {
            let evs: Vec<_> = events.iter().filter(|e| e.name == name).collect();
            assert!(!evs.is_empty(), "expected at least one {name} event");
            for e in evs {
                assert_eq!(
                    e.data.get("companion_id").and_then(|v| v.as_str()),
                    Some(b.id.as_str()),
                    "{name} 必须 scope 到默认体 乙"
                );
            }
        }
    }
}
