//! `CompanionSkillStoreSink` — bridges the companion's skill registry + on-disk
//! SKILL.md bodies to the `nomifun_ai_agent::CompanionSkillSink` trait the agent
//! engine consumes for skill auto-use (design §7).
//!
//! `active_skills` feeds the per-turn `when_to_use` index (the `CompanionSkillContributor`);
//! `load_skill_body` resolves a named skill's SKILL.md on demand (the `companion_skill` tool).
//! Both scope to the default companion (the owner of mined skills) plus shared skills.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_ai_agent::{CompanionSkillSink, SkillListing};
use nomifun_extension::constants::SKILL_MANIFEST_FILE;
use nomifun_extension::skill_service::{self, SkillDraftInput, SkillPaths, SkillScope};

use crate::collector::SharedConfig;
use crate::events::CompanionEventEmitter;
use crate::store::{CompanionSkill, CompanionStore};

pub struct CompanionSkillStoreSink {
    pub store: CompanionStore,
    pub config: SharedConfig,
    pub skill_paths: Arc<SkillPaths>,
    pub emitter: CompanionEventEmitter,
}

impl CompanionSkillStoreSink {
    /// The companion that owns mined skills (default companion).
    async fn owner(&self) -> Option<String> {
        self.config.read().await.default_companion_id.clone()
    }

    fn scope_of(companion_id: Option<&str>) -> SkillScope {
        companion_id
            .map(|id| SkillScope::Companion(id.to_owned()))
            .unwrap_or(SkillScope::Shared)
    }
}

#[async_trait]
impl CompanionSkillSink for CompanionSkillStoreSink {
    async fn active_skills(&self) -> Vec<SkillListing> {
        let Some(owner) = self.owner().await else {
            return Vec::new();
        };
        let skills = self.store.list_skills(&owner, true).await.unwrap_or_default();
        let mut out = Vec::new();
        for s in skills.into_iter().filter(|s| s.status == "active") {
            let scope = Self::scope_of(s.scope_companion_id.as_deref());
            // when_to_use index uses the SKILL.md description (what the skill does).
            if let Ok(dir) = skill_service::skill_dir_for(&self.skill_paths, &scope, &s.skill_name, false) {
                let desc = skill_service::read_skill_info(&dir).await.map(|(_, d)| d).unwrap_or_default();
                out.push(SkillListing { name: s.skill_name, when_to_use: desc });
            }
        }
        out
    }

    async fn load_skill_body(&self, name: &str) -> Option<String> {
        let owner = self.owner().await;
        // Prefer the owner's companion-scoped skill (record usage against the owner),
        // then fall back to the ownerless shared scope.
        if let Some(owner) = owner {
            if let Ok(dir) = skill_service::skill_dir_for(&self.skill_paths, &SkillScope::Companion(owner.clone()), name, false) {
                if let Ok(body) = tokio::fs::read_to_string(dir.join(SKILL_MANIFEST_FILE)).await {
                    let _ = self
                        .store
                        .record_skill_usage(Some(&owner), name, nomifun_common::now_ms())
                        .await;
                    return Some(append_support_files(&dir, &body));
                }
            }
        }
        if let Ok(dir) = skill_service::skill_dir_for(&self.skill_paths, &SkillScope::Shared, name, false) {
            if let Ok(body) = tokio::fs::read_to_string(dir.join(SKILL_MANIFEST_FILE)).await {
                let _ = self.store.record_skill_usage(None, name, nomifun_common::now_ms()).await;
                return Some(append_support_files(&dir, &body));
            }
        }
        None
    }

    async fn create_skill_draft(
        &self,
        name: &str,
        description: &str,
        when_to_use: &str,
        body: &str,
    ) -> Result<String, String> {
        let Some(owner) = self.owner().await else {
            return Err("未找到当前伙伴".into());
        };
        // Program sanitizes the name — LLM cannot bypass this.
        let name = sanitize_skill_name(name);
        if name.is_empty() {
            return Err("技能名无效（需包含至少一个字母或数字）".into());
        }
        // Dedup: if a similar active/draft skill already exists, skip creation.
        if let Ok(Some(existing)) = self.store.find_similar_skill(&owner, &name).await {
            return Ok(format!("技能「{existing}」已存在，无需重复创建。"));
        }
        // Dedup: if a similar pending suggestion already exists, touch it instead of duplicating.
        let title = format!("我整理了一个新技能：{name}");
        let body_text = if when_to_use.is_empty() {
            format!("「{description}」")
        } else {
            format!("「{description}」— {when_to_use}")
        };
        if let Ok(Some(existing_id)) = self
            .store
            .find_similar_suggestion("create_skill", &title, &body_text)
            .await
        {
            let _ = self.store.touch_suggestion(&existing_id).await;
            return Ok(format!("已有类似的技能建议在等待审阅，已重新浮到顶部。"));
        }
        // Program builds the SKILL.md content — LLM cannot write files directly.
        let input = SkillDraftInput {
            name: name.clone(),
            description: description.to_owned(),
            when_to_use: if when_to_use.is_empty() { None } else { Some(when_to_use.to_owned()) },
            allowed_tools: None,
            paths: None,
            body: body.to_owned(),
        };
        let scope = SkillScope::Companion(owner.clone());
        skill_service::create_skill(&self.skill_paths, &scope, true, &input)
            .await
            .map_err(|e| format!("写入技能文件失败: {e}"))?;

        // Program writes DB row — status forced to draft, source tagged assistant.
        let now = nomifun_common::now_ms();
        self.store
            .insert_skill(&CompanionSkill {
                skill_name: name.clone(),
                scope_kind: "companion".into(),
                scope_companion_id: Some(owner.clone()),
                status: "draft".into(),
                source: "assistant".into(),
                confidence: 0.5,
                provenance: vec![],
                strength: 1.0,
                version: 1,
                superseded_by: None,
                usage_count: 0,
                last_used_at: None,
                created_at: now,
                updated_at: now,
                signature: String::new(),
            })
            .await
            .map_err(|e| format!("写入技能记录失败: {e}"))?;

        // Program creates the review suggestion card — user must accept to activate.
        let action = serde_json::json!({
            "type": "create_skill",
            "name": name,
            "companion_id": owner,
            "signature": ""
        });
        if let Ok(created) = self
            .store
            .insert_suggestion("create_skill", &title, &body_text, Some(&action))
            .await
        {
            self.emitter.emit_suggestion_created(&owner, &created);
        }
        self.emitter.emit_skill_drafted(&owner, &name);

        Ok(format!("技能「{name}」已创建为草案，等待主人审阅。"))
    }
}

/// Optimization 6: append support file listings from `references/`, `templates/`,
/// and `scripts/` subdirectories to the skill body. This gives the agent context
/// about what supporting files exist without inlining their full content — the
/// agent can then read specific files on demand.
fn append_support_files(skill_dir: &std::path::Path, body: &str) -> String {
    let mut out = body.to_string();
    for sub in &["references", "templates", "scripts"] {
        let sub_dir = skill_dir.join(sub);
        if let Ok(entries) = std::fs::read_dir(&sub_dir) {
            let mut files: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name == ".gitkeep" || name.starts_with('.') {
                        None
                    } else {
                        Some(format!("{sub}/{name}"))
                    }
                })
                .collect();
            if !files.is_empty() {
                files.sort();
                out.push_str(&format!("\n\n## {}\n", sub));
                for f in &files {
                    out.push_str(&format!("- `{f}`\n"));
                }
            }
        }
    }
    out
}

/// Normalize a skill name into a kebab-case valid directory name.
/// Mirrors `evolution::engine::sanitize_skill_name` — kept here as a private
/// copy so `skill_sink` doesn't depend on the evolution engine crate.
fn sanitize_skill_name(raw: &str) -> String {
    let mut s: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').chars().take(64).collect()
}
