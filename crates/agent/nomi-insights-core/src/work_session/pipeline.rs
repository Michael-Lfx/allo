//! Session-end orchestration: POI ingest → resolution → work package upload.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{InsightsContributionConfig, InterestConfig};
use nomi_auxiliary::AuxiliaryClient;
use nomi_poi::{
    InterestSignal, InterestStore, apply_signal_batch, collect_starter_topic_ids,
    extract_signals_from_messages, extract_signals_from_transcript_llm,
    filter_persistable_signals, filter_poi_signals, format_user_transcript_for_llm,
    generate_starters_with_store,
};
use tracing::{info, warn};

use crate::{
    ContributionService, WorkPackageBuildInput, append_audit_event, drain_session_skills,
    find_skill_dir_by_slug, set_active_session,
};

use super::domain::{candidate_to_poi, extract_domain_candidate_for_work_package_with_source};
use super::metrics::build_work_metrics;
use super::resolution::{analyze_session, resolve_session_verdict};

pub fn spawn_session_end_pipeline(
    data_dir: PathBuf,
    interest_cfg: InterestConfig,
    insights_cfg: InsightsContributionConfig,
    session_id: String,
    messages: Vec<serde_json::Value>,
    buffered: Vec<InterestSignal>,
    auxiliary: Option<Arc<AuxiliaryClient>>,
) {
    if !interest_cfg.enabled && !insights_cfg.enabled {
        return;
    }
    tokio::spawn(async move {
        info!(
            session_id = %session_id,
            message_count = messages.len(),
            interest_enabled = interest_cfg.enabled,
            insights_enabled = insights_cfg.enabled,
            "work_session: session-end pipeline started"
        );

        if interest_cfg.enabled {
            run_poi_ingest(
                &data_dir,
                &interest_cfg,
                &messages,
                buffered,
                auxiliary.as_ref(),
            )
            .await;
        }

        if !insights_cfg.enabled {
            info!(session_id = %session_id, "work_session: insights contribution disabled — skipping work packages");
            return;
        }

        // Optimization 1: skill mining for normal conversations. When enabled,
        // extract the tool-call sequence from this session and log it as a skill
        // candidate. This bridges the normal-dialog → evolution gap: normal
        // conversations can now directly produce skill suggestions, not just
        // serve as a data source for the companion system.
        if insights_cfg.skill_mining_enabled {
            if let Some(candidate) = mine_session_tools(&messages, &session_id) {
                info!(
                    session_id = %session_id,
                    tool_count = candidate.tool_sequence.len(),
                    tools = ?candidate.tool_sequence,
                    "work_session: skill mining detected a candidate tool pattern"
                );
                append_audit_event(
                    &data_dir,
                    "skill_mining_candidate",
                    &format!(
                        "session_id={session_id} tools={:?} steps={}",
                        candidate.tool_sequence,
                        candidate.tool_sequence.len()
                    ),
                );
            }
        }

        let packages = build_work_packages(
            &data_dir,
            &insights_cfg,
            interest_cfg.enabled,
            &session_id,
            &messages,
            auxiliary.as_ref(),
        )
        .await;
        if packages.is_empty() {
            warn!(
                session_id = %session_id,
                audit_path = %crate::audit_path(&data_dir).display(),
                "work_session: no domain work packages built — see prior skip logs and audit.jsonl"
            );
            return;
        }
        info!(
            count = packages.len(),
            session_id = %session_id,
            "work_session: enqueue domain work packages"
        );
        ContributionService::spawn_work_packages(data_dir, insights_cfg, packages);
    });
}

async fn run_poi_ingest(
    data_dir: &PathBuf,
    config: &InterestConfig,
    messages: &[serde_json::Value],
    buffered: Vec<InterestSignal>,
    auxiliary: Option<&Arc<AuxiliaryClient>>,
) {
    let db_path = data_dir.join("interest.db");
    let Ok(store) = InterestStore::open(&db_path, config.clone()) else {
        warn!(path = %db_path.display(), "interest: failed to open interest.db for session-end ingest");
        return;
    };
    let transcript = format_user_transcript_for_llm(messages);
    let transcript_chars = transcript.chars().count();
    let mut all_signals = buffered;
    let buffered_n = all_signals.len();
    let mut llm_attempted = false;
    let signals_before_llm = all_signals.len();
    if config.session_end_llm_enabled() {
        if let Some(aux) = auxiliary {
            if transcript_chars == 0 {
                warn!("interest: session-end LLM skipped — empty user transcript");
            } else {
                llm_attempted = true;
                let existing_labels = store.top_labels_for_llm(5).unwrap_or_default();
                let llm_signals =
                    extract_signals_from_transcript_llm(aux, &transcript, &existing_labels).await;
                let llm_n = llm_signals.len();
                all_signals.extend(llm_signals);
                info!(
                    transcript_chars,
                    llm_n, "interest: session-end LLM extraction"
                );
            }
        } else {
            warn!("interest: session-end LLM enabled but auxiliary client unavailable");
        }
    }
    let llm_produced = all_signals.len() > signals_before_llm;
    if config.uses_rules() {
        let rules = extract_signals_from_messages(messages);
        let rules_n = rules.len();
        all_signals.extend(rules);
        info!(
            transcript_chars,
            buffered_n, rules_n, "interest: session-end rule supplement"
        );
    } else if llm_attempted && !llm_produced && transcript_chars > 0 {
        warn!(
            transcript_chars,
            "interest: LLM extraction produced no signals — falling back to rules"
        );
        let rules = extract_signals_from_messages(messages);
        let rules_n = rules.len();
        all_signals.extend(rules);
        info!(
            transcript_chars,
            buffered_n, rules_n, "interest: session-end rule fallback after LLM miss"
        );
    }
    let pre_filter_n = all_signals.len();
    let all_signals = filter_persistable_signals(filter_poi_signals(all_signals));
    if all_signals.is_empty() {
        info!(
            transcript_chars,
            buffered_n,
            pre_filter_n,
            "interest: session-end POI pipeline — no persistable signals after gates"
        );
        return;
    }
    let _ = store.apply_decay();
    let report = match apply_signal_batch(&store, config, all_signals) {
        Ok(report) => {
            if report.inserted + report.reinforced + report.merged > 0 {
                info!(
                    inserted = report.inserted,
                    reinforced = report.reinforced,
                    merged = report.merged,
                    promoted = report.promoted,
                    skipped = report.skipped,
                    starter_topics = report.starter_topic_ids.len(),
                    "interest: session-end POI pipeline applied"
                );
            } else {
                info!(
                    skipped = report.skipped,
                    "interest: session-end POI pipeline — signals present but compare/update made no changes"
                );
            }
            report
        }
        Err(err) => {
            warn!("interest: session-end pipeline failed: {err}");
            return;
        }
    };

    if !config.starter_enabled {
        return;
    }
    let starter_ids = collect_starter_topic_ids(&store, report.starter_topic_ids);
    if starter_ids.is_empty() {
        return;
    }
    let Some(aux) = auxiliary else {
        warn!(
            count = starter_ids.len(),
            "interest starters: need generation but auxiliary client unavailable"
        );
        return;
    };
    // Reuse the ingest connection and await inline — nested spawn + reopen was
    // racing / silently skipping after upstream merges on Windows.
    let store = std::sync::Arc::new(std::sync::Mutex::new(store));
    generate_starters_with_store(store, config, &starter_ids, aux).await;
}

fn skip_work_package(data_dir: &Path, reason: &str, detail: &str) {
    warn!(reason, detail, "work_session: domain work package skipped");
    append_audit_event(data_dir, reason, detail);
}

async fn build_work_packages(
    data_dir: &PathBuf,
    insights_cfg: &InsightsContributionConfig,
    interest_enabled: bool,
    session_id: &str,
    messages: &[serde_json::Value],
    auxiliary: Option<&Arc<AuxiliaryClient>>,
) -> Vec<WorkPackageBuildInput> {
    let skill_summary = drain_session_skills(data_dir, session_id);
    info!(
        session_id,
        skill_slugs = ?skill_summary.slugs,
        patch_count = skill_summary.patch_count,
        skill_created = skill_summary.skill_created,
        message_count = messages.len(),
        "work_session: drained session skill binding"
    );
    if insights_cfg.require_skill_binding && skill_summary.slugs.is_empty() {
        skip_work_package(
            data_dir,
            "skill_binding_missing",
            &format!("session_id={session_id}"),
        );
        return Vec::new();
    }

    let signals = analyze_session(messages, &skill_summary);
    if signals.user_turns < insights_cfg.min_work_turns {
        skip_work_package(
            data_dir,
            "insufficient_user_turns",
            &format!(
                "session_id={session_id} user_turns={} min={}",
                signals.user_turns, insights_cfg.min_work_turns
            ),
        );
        return Vec::new();
    }

    let Some((candidate, domain_source)) = extract_domain_candidate_for_work_package_with_source(
        data_dir,
        interest_enabled,
        messages,
        &skill_summary.slugs,
    ) else {
        skip_work_package(
            data_dir,
            "domain_poi_missing",
            &format!("session_id={session_id} message_count={}", messages.len()),
        );
        return Vec::new();
    };

    let resolution = resolve_session_verdict(insights_cfg, auxiliary, messages, &signals).await;
    let domain_poi = candidate_to_poi(&candidate);
    info!(
        session_id,
        user_turns = signals.user_turns,
        tool_failures = signals.tool_failures,
        tool_successes = signals.tool_successes,
        domain_source = ?domain_source,
        verdict = %resolution.verdict,
        evidence_tier = %resolution.evidence_tier,
        "work_session: session signals analyzed"
    );
    let session_id_hash = crate::types::sha256_hex(session_id.as_bytes());
    let work_metrics = build_work_metrics(
        signals.user_turns,
        signals.tool_failures,
        skill_summary.patch_count,
    );

    let skills_root = data_dir.join("skills");
    let Some((skill_dir, slug)) = resolve_bound_skill_dir(&skills_root, &skill_summary, messages)
    else {
        skip_work_package(
            data_dir,
            "skill_dir_not_found",
            &format!("session_id={session_id} slugs={:?}", skill_summary.slugs),
        );
        return Vec::new();
    };

    let binding_role = if skill_summary.skill_created {
        "primary".to_string()
    } else if resolution.recovery_attempted {
        "recovery".to_string()
    } else {
        "primary".to_string()
    };

    info!(
        session_id,
        skill_slug = %slug,
        domain_key = %domain_poi.domain_key,
        verdict = %resolution.verdict,
        evidence_tier = %resolution.evidence_tier,
        "work_session: built domain work package candidate"
    );

    vec![WorkPackageBuildInput {
        work_id: uuid::Uuid::new_v4().to_string(),
        session_id_hash,
        domain_poi,
        resolution,
        skill_dir,
        skills_root,
        binding_role,
        include_body: insights_cfg.redacted_body,
        work_metrics,
    }]
}

fn resolve_bound_skill_dir(
    skills_root: &Path,
    skill_summary: &crate::SessionSkillSummary,
    messages: &[serde_json::Value],
) -> Option<(PathBuf, String)> {
    let mut slugs: Vec<String> = skill_summary.slugs.clone();
    slugs.sort();
    slugs.dedup();
    for slug in &slugs {
        if let Some(skill_dir) = find_skill_dir_by_slug(skills_root, slug) {
            return Some((skill_dir, slug.clone()));
        }
    }
    let fallback_slug = messages.iter().find_map(|m| {
        m.get("tool_calls")?.as_array()?.iter().find_map(|tc| {
            let name = tc.get("function")?.get("name")?.as_str()?;
            if name == "skill_manage" {
                tc.get("function")?
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
            } else {
                None
            }
        })
    })?;
    find_skill_dir_by_slug(skills_root, &fallback_slug).map(|dir| (dir, fallback_slug))
}

pub fn touch_active_session(data_dir: &PathBuf, session_id: &str) {
    set_active_session(data_dir, session_id);
}

/// A tool-call pattern mined from a single session's messages (optimization 1).
#[derive(Debug)]
pub struct MinedSessionTools {
    /// Ordered sequence of tool names invoked in the session.
    pub tool_sequence: Vec<String>,
    /// The session these tools were mined from.
    pub session_id: String,
}

/// Extract the tool-call sequence from a session's messages and return it as a
/// mining candidate if the sequence is long enough to suggest a reusable
/// pattern (≥ 4 distinct tool calls). This is the normal-dialog counterpart of
/// the companion EvolutionEngine's `miner.rs` — it lets regular conversations
/// produce skill suggestions, not just feed data to the companion system.
///
/// Only tool **names** are extracted (never arguments) — the same privacy
/// red line as the collector's `tool_calls` source.
pub fn mine_session_tools(
    messages: &[serde_json::Value],
    session_id: &str,
) -> Option<MinedSessionTools> {
    let mut tool_sequence: Vec<String> = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) else {
            continue;
        };
        for tc in tool_calls {
            if let Some(name) = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
            {
                // Deduplicate consecutive same-tool calls (e.g. multiple Read calls).
                if tool_sequence.last().is_none_or(|last: &String| last != name) {
                    tool_sequence.push(name.to_string());
                }
            }
        }
    }
    // Require at least 4 steps to be worth suggesting as a skill.
    if tool_sequence.len() < 4 {
        return None;
    }
    Some(MinedSessionTools {
        tool_sequence,
        session_id: session_id.to_string(),
    })
}
