//! `ReminderService` — the `<system-reminder>` notification channel.
//!
//! Ported from kimi-code's `features/reminder/systemReminder.ts`: state
//! *enforcement* stays in the hard gates (`dispatch_gate`, `on_user_request`),
//! while state *notification* rides a wrapped user message that tells the model
//! what is going on without pretending to be the user's instruction. A model
//! that ignores a reminder is still stopped by the gate.
//!
//! Why this channel exists rather than the turn-tail `[Context]` block:
//!
//! - the turn-tail block mixes data, instructions and evidence under one
//!   "this is context" label, so a long plan-mode instruction block reads as a
//!   second system prompt glued onto the user slot
//!   (`turn-tail-context-investigation.zh.md` P2);
//! - it re-injects on **every provider pass**, so the same text reappears at the
//!   tail of each pass of one turn (P3);
//! - the highest-salience user position ends up holding non-instruction text,
//!   which the investigation records as the main driver of repeated
//!   exploration loops (P1).
//!
//! Reminders are therefore:
//!
//! - **persistent**: appended once to `messages`, never editing an already-sent
//!   prefix, so the provider's prefix cache only grows;
//! - **deduplicated within a turn**: identical text is emitted once, with an
//!   optional periodic refresh (`refresh_after_passes`) so a long tool loop
//!   still re-reads a stale reminder without re-emitting it on every pass;
//! - **re-emitted once per turn**: dedup state resets at each root user
//!   request, which is how a standing goal stays visible without repeating
//!   inside a turn;
//! - **regenerable**: text is derived from feature state, so a compaction that
//!   drops the message is repaired at the next trigger. No bookkeeping marker is
//!   stored on the message, and the wrapper deliberately does **not** start with
//!   `[Context]`, so the turn-tail predicates (`is_turn_tail_context_text`,
//!   `is_context_only_user_content`) used by truncation-restart never match it.

use std::collections::HashMap;

/// Wraps reminder text in the channel's envelope.
///
/// The envelope is the model-visible signal that this is environment/state
/// information and not a new user instruction. It absorbs the wording the
/// turn-tail investigation recommended as its cheapest fix (§5-A).
pub fn wrap_system_reminder(text: &str) -> String {
    format!("<system-reminder>\n{}\n</system-reminder>", text.trim())
}

/// Sentence prepended to every variant's body so the model never mistakes a
/// state notification for a fresh instruction.
///
/// The reference implementation leaves this to each variant's template. nomi
/// states it once here because every nomi variant is a state notification, and
/// one place is easier to audit than nine templates.
pub const SYSTEM_REMINDER_PREAMBLE: &str =
    "This is environment/state information, not a new instruction from the user. \
     Do not restate or acknowledge it; act on it only as far as it changes what you should do next.";

/// Compose a reminder body: the shared preamble, then the variant's text.
pub fn reminder_body(variant_text: &str) -> String {
    format!("{SYSTEM_REMINDER_PREAMBLE}\n\n{}", variant_text.trim())
}

/// When a reminder is being rendered.
///
/// The engine supplies these facts; the service adds no policy of its own
/// beyond deduplication and refresh. The reference passes equivalent facts
/// (`isNewTurn`, `lastInjectedAt`) and lets each provider decide; nomi splits
/// it the other way round — the service owns repeat-suppression, and variants
/// own *what* to say — because every nomi variant wants the same suppression.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReminderCtx {
    /// First provider pass of a new root user request.
    pub turn_start: bool,
    /// A new root user message arrived since the previous rendering.
    pub new_user_message: bool,
    /// A state event outside the per-pass cadence: plan mode was entered or
    /// left, a goal changed status, or the history was spliced by compaction.
    /// Rendered even if the text is unchanged since the turn started.
    pub urgent: bool,
    /// Provider passes already completed in this turn (0-based).
    pub pass_in_turn: usize,
}

/// What a variant decided to emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedReminder {
    pub variant: &'static str,
    /// Raw body (preamble + variant text), not yet wrapped.
    pub body: String,
}

/// A registered reminder variant.
pub struct ReminderVariant {
    pub variant: &'static str,
    /// Render the variant's body, or `None` when it has nothing to say.
    pub render: Box<dyn Fn(&ReminderCtx) -> Option<String> + Send + Sync>,
    /// Re-emit unchanged text once this many passes have elapsed in the turn.
    /// `None` never refreshes within a turn.
    pub refresh_after_passes: Option<usize>,
}

/// Registry of reminder variants plus the per-turn deduplication state.
///
/// Registration order is emission order, so a variant that needs to be read
/// first is registered first.
#[derive(Default)]
pub struct ReminderService {
    variants: Vec<ReminderVariant>,
    /// Last body emitted per variant **this turn**, and the pass it was emitted
    /// on. Cleared by [`Self::begin_turn`].
    emitted: HashMap<&'static str, (String, usize)>,
}

impl ReminderService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a variant. Re-registering the same name replaces the earlier
    /// one in place, so a double bootstrap cannot emit twice.
    pub fn register(
        &mut self,
        variant: &'static str,
        render: impl Fn(&ReminderCtx) -> Option<String> + Send + Sync + 'static,
        refresh_after_passes: Option<usize>,
    ) {
        let entry = ReminderVariant {
            variant,
            render: Box::new(render),
            refresh_after_passes,
        };
        match self
            .variants
            .iter_mut()
            .find(|existing| existing.variant == variant)
        {
            Some(slot) => *slot = entry,
            None => self.variants.push(entry),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    pub fn len(&self) -> usize {
        self.variants.len()
    }

    pub fn variants(&self) -> Vec<&'static str> {
        self.variants.iter().map(|v| v.variant).collect()
    }

    /// Start a new root user request. Clears the within-turn dedup state so a
    /// standing reminder is re-stated once per turn.
    pub fn begin_turn(&mut self) {
        self.emitted.clear();
    }

    /// Render every variant that has something to say right now.
    ///
    /// A variant is skipped when it returns `None`, or when it produces the
    /// exact body already emitted this turn and the refresh interval has not
    /// elapsed.
    pub fn collect(&mut self, ctx: &ReminderCtx) -> Vec<RenderedReminder> {
        let mut out = Vec::new();
        for variant in &self.variants {
            let Some(body) = (variant.render)(ctx) else {
                continue;
            };
            let body = body.trim();
            if body.is_empty() {
                continue;
            }
            let body = reminder_body(body);

            if let Some((last, last_pass)) = self.emitted.get(variant.variant) {
                let unchanged = *last == body;
                let refresh_due = variant
                    .refresh_after_passes
                    .is_some_and(|every| ctx.pass_in_turn.saturating_sub(*last_pass) >= every);
                if unchanged && !ctx.urgent && !refresh_due {
                    continue;
                }
            }
            self.emitted
                .insert(variant.variant, (body.clone(), ctx.pass_in_turn));
            out.push(RenderedReminder {
                variant: variant.variant,
                body,
            });
        }
        out
    }

    /// For tests and diagnostics: the body last emitted for a variant this turn.
    pub fn last_emitted(&self, variant: &str) -> Option<&str> {
        self.emitted.get(variant).map(|(body, _)| body.as_str())
    }
}

impl std::fmt::Debug for ReminderService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReminderService")
            .field("variants", &self.variants())
            .field("emitted_this_turn", &self.emitted.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn wrap_produces_the_envelope() {
        assert_eq!(
            wrap_system_reminder("  hello  "),
            "<system-reminder>\nhello\n</system-reminder>"
        );
    }

    #[test]
    fn wrapper_is_not_mistakable_for_a_turn_tail_block() {
        // truncation-restart drops `[Context]`-only user messages; a reminder
        // must never look like one.
        let wrapped = wrap_system_reminder("plan mode is active");
        assert!(!crate::context_contributor::is_turn_tail_context_text(&wrapped));
        let content = vec![nomi_types::message::ContentBlock::Text { text: wrapped }];
        assert!(!crate::context_contributor::is_context_only_user_content(
            &content
        ));
    }

    #[test]
    fn body_carries_the_not_an_instruction_preamble() {
        let body = reminder_body("Plan mode is active.");
        assert!(body.starts_with(SYSTEM_REMINDER_PREAMBLE));
        assert!(body.contains("Plan mode is active."));
    }

    #[test]
    fn an_empty_service_renders_nothing() {
        let mut service = ReminderService::new();
        assert!(service.is_empty());
        assert!(service.collect(&ReminderCtx::default()).is_empty());
    }

    #[test]
    fn variants_emit_in_registration_order() {
        let mut service = ReminderService::new();
        service.register("first", |_| Some("one".into()), None);
        service.register("second", |_| Some("two".into()), None);

        let rendered = service.collect(&ReminderCtx::default());
        assert_eq!(
            rendered.iter().map(|r| r.variant).collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn a_variant_returning_none_is_silent() {
        let mut service = ReminderService::new();
        service.register("quiet", |_| None, None);
        service.register("loud", |_| Some("here".into()), None);

        let rendered = service.collect(&ReminderCtx::default());
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].variant, "loud");
    }

    #[test]
    fn an_all_whitespace_body_is_not_emitted() {
        let mut service = ReminderService::new();
        service.register("blank", |_| Some("   \n ".into()), None);
        assert!(service.collect(&ReminderCtx::default()).is_empty());
    }

    #[test]
    fn identical_text_is_emitted_once_per_turn() {
        // P3: the same reminder must not reappear at the tail of every provider
        // pass. Three passes, one emission.
        let mut service = ReminderService::new();
        service.register("plan", |_| Some("plan mode is active".into()), None);

        let first = service.collect(&ReminderCtx {
            turn_start: true,
            pass_in_turn: 0,
            ..Default::default()
        });
        assert_eq!(first.len(), 1);

        for pass in 1..3 {
            let again = service.collect(&ReminderCtx {
                pass_in_turn: pass,
                ..Default::default()
            });
            assert!(again.is_empty(), "pass {pass} must not re-emit");
        }
    }

    #[test]
    fn changed_text_is_emitted_even_within_a_turn() {
        let mut service = ReminderService::new();
        let state = Arc::new(AtomicUsize::new(0));
        let render_state = Arc::clone(&state);
        service.register(
            "plan",
            move |_| {
                Some(match render_state.load(Ordering::SeqCst) {
                    0 => "exploring".to_string(),
                    _ => "awaiting approval".to_string(),
                })
            },
            None,
        );

        assert_eq!(service.collect(&ReminderCtx::default()).len(), 1);
        state.store(1, Ordering::SeqCst);
        let rendered = service.collect(&ReminderCtx {
            pass_in_turn: 1,
            ..Default::default()
        });
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].body.contains("awaiting approval"));
    }

    #[test]
    fn a_new_turn_re_emits_the_same_text() {
        // A standing goal must be visible on every turn, not only the first.
        let mut service = ReminderService::new();
        service.register("goal", |_| Some("goal is active".into()), None);

        assert_eq!(service.collect(&ReminderCtx::default()).len(), 1);
        assert!(service.collect(&ReminderCtx::default()).is_empty());

        service.begin_turn();
        assert_eq!(
            service.collect(&ReminderCtx::default()).len(),
            1,
            "the next root user request re-states the standing state"
        );
    }

    #[test]
    fn an_urgent_event_re_emits_within_a_turn() {
        let mut service = ReminderService::new();
        service.register("plan", |_| Some("plan mode is active".into()), None);

        assert_eq!(service.collect(&ReminderCtx::default()).len(), 1);
        assert!(service.collect(&ReminderCtx::default()).is_empty());

        let urgent = service.collect(&ReminderCtx {
            urgent: true,
            pass_in_turn: 2,
            ..Default::default()
        });
        assert_eq!(urgent.len(), 1, "entering plan mode mid-turn must be told");
    }

    #[test]
    fn periodic_refresh_re_emits_unchanged_text_after_the_interval() {
        // Risk: a long tool loop makes the reminder stale. The refresh keeps it
        // near the tail without re-emitting it on every pass.
        let mut service = ReminderService::new();
        service.register("plan", |_| Some("plan mode is active".into()), Some(4));

        assert_eq!(
            service.collect(&ReminderCtx {
                turn_start: true,
                pass_in_turn: 0,
                ..Default::default()
            }).len(),
            1
        );

        for pass in 1..4 {
            assert!(
                service
                    .collect(&ReminderCtx {
                        pass_in_turn: pass,
                        ..Default::default()
                    })
                    .is_empty(),
                "pass {pass} is before the refresh interval"
            );
        }

        assert_eq!(
            service
                .collect(&ReminderCtx {
                    pass_in_turn: 4,
                    ..Default::default()
                })
                .len(),
            1,
            "the interval elapsed, so the same body is re-stated"
        );
    }

    #[test]
    fn compaction_splice_rearms_within_the_same_turn() {
        // The reference re-injects every variant after a compaction splice,
        // because the spliced history may have dropped the reminder message.
        // `urgent` is that rearm.
        let mut service = ReminderService::new();
        service.register("plan", |_| Some("plan mode is active".into()), None);

        assert_eq!(
            service
                .collect(&ReminderCtx {
                    turn_start: true,
                    pass_in_turn: 0,
                    ..Default::default()
                })
                .len(),
            1
        );
        assert!(
            service
                .collect(&ReminderCtx {
                    pass_in_turn: 1,
                    ..Default::default()
                })
                .is_empty()
        );

        let after_splice = service.collect(&ReminderCtx {
            urgent: true,
            pass_in_turn: 1,
            ..Default::default()
        });
        assert_eq!(
            after_splice.len(),
            1,
            "the spliced history needs the reminder back"
        );
        assert!(after_splice[0].body.contains("plan mode is active"));
    }

    #[test]
    fn re_registering_a_variant_replaces_it() {
        let mut service = ReminderService::new();
        service.register("plan", |_| Some("old".into()), None);
        service.register("plan", |_| Some("new".into()), None);

        assert_eq!(service.len(), 1);
        let rendered = service.collect(&ReminderCtx::default());
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].body.contains("new"));
        assert!(!rendered[0].body.contains("old"));
    }

    #[test]
    fn compaction_drop_is_repaired_at_the_next_trigger() {
        // The message is regenerable: dropping it (compaction) and asking again
        // on the next turn reproduces it, with no marker on the message.
        let mut service = ReminderService::new();
        service.register("plan", |_| Some("plan mode is active".into()), None);

        let first = service.collect(&ReminderCtx {
            turn_start: true,
            ..Default::default()
        });
        let body = first[0].body.clone();

        // Compaction drops the message; the service's state is untouched
        // because nothing was recorded on the message itself.
        service.begin_turn();
        let rebuilt = service.collect(&ReminderCtx {
            turn_start: true,
            ..Default::default()
        });
        assert_eq!(rebuilt[0].body, body);
    }

    #[test]
    fn last_emitted_reports_the_current_turn_body() {
        let mut service = ReminderService::new();
        service.register("goal", |_| Some("goal is active".into()), None);
        assert!(service.last_emitted("goal").is_none());

        service.collect(&ReminderCtx::default());
        assert!(service.last_emitted("goal").unwrap().contains("goal is active"));

        service.begin_turn();
        assert!(service.last_emitted("goal").is_none());
    }

    #[test]
    fn a_session_with_no_plan_or_goal_emits_nothing() {
        // The reminder channel is opt-in per feature: registering no variant
        // keeps a plain session's messages byte-identical.
        let mut service = ReminderService::new();
        assert!(service.collect(&ReminderCtx {
            turn_start: true,
            ..Default::default()
        }).is_empty());
        assert!(service.variants().is_empty());
    }
}
