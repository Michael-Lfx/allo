use std::collections::HashMap;
use std::path::Path;

use nomi_coding::{
    TaskProfile, coding_intro, coding_overlay_instructions, office_intro, office_overlay_instructions,
};
use nomi_memory::prompt::build_memory_prompt_minimal;
use nomi_skills::prompt::format_skills_within_budget;
use nomi_skills::types::SkillMetadata;

pub use crate::prompt_graph::{
    AdvertisedToolSet, SessionToolSurface, SystemPromptInput, advertised_tools_for_session,
    insert_before_language_section, skills_for_profile, tool_usage_guidance,
};

/// Session-scoped cache for system prompt sections.
///
/// Each section (intro, tool guidance, AGENTS.md, memory, skills) is cached
/// independently. The `joined` field holds the pre-joined full prompt string
/// and is invalidated whenever any section changes.
///
/// Cache-first design: the system prompt built here is the **cache-stable
/// prefix** — it must stay byte-stable across turns so DeepSeek's automatic
/// prefix cache stays warm. Dynamic content (date, plan mode, RAG injections)
/// is NOT included here; it rides the turn tail (prepended to the last user
/// message) in the engine instead. See `engine.rs` turn-tail injection.
pub struct SystemPromptCache {
    /// Cached section strings, keyed by section name.
    pub(crate) sections: HashMap<&'static str, String>,
    /// Pre-joined full prompt. Invalidated on any section change.
    pub(crate) joined: Option<String>,
    /// Track last toon_enabled value to detect changes.
    pub(crate) last_toon_enabled: bool,
    /// Track last browser_enabled value to detect changes.
    pub(crate) last_browser_enabled: bool,
    last_profile: TaskProfile,
    last_advertised: String,
    last_has_deferred: bool,
    last_language: String,
    /// When true, skip the unrestricted `# Using your tools` block.
    /// Restricted sessions (`read_only` / `read_shell` allowlists) set this so
    /// the model is not told about Bash and other tools it cannot call.
    omit_generic_tool_guidance: bool,
}

impl SystemPromptCache {
    pub fn new() -> Self {
        Self {
            sections: HashMap::new(),
            joined: None,
            last_toon_enabled: false,
            last_browser_enabled: false,
            last_profile: TaskProfile::Office,
            last_advertised: String::new(),
            last_has_deferred: false,
            last_language: String::new(),
            omit_generic_tool_guidance: false,
        }
    }

    /// Drop the cache-stable generic tool-guidance section for this session.
    ///
    /// Restricted Agent Execution attempts advertise a narrowed tool list.
    /// The unrestricted guidance still names Bash/Write/Browser, which those
    /// sessions cannot call. The step brief and per-tool schemas remain.
    pub fn omit_generic_tool_guidance(&mut self) {
        self.omit_generic_tool_guidance = true;
        self.sections.remove("tool_guidance");
        self.joined = None;
    }

    /// Invalidate a specific section by name.
    pub fn invalidate(&mut self, section: &str) {
        self.sections.remove(section);
        self.joined = None;
    }

    /// Invalidate all cached sections (e.g., on /compact).
    pub fn invalidate_all(&mut self) {
        self.sections.clear();
        self.joined = None;
    }

    /// Install the immutable AGENTS.md snapshot resolved by session bootstrap.
    pub fn set_agents_md(&mut self, instructions: String) {
        self.sections.insert("agents_md", instructions);
        self.joined = None;
    }

    /// Override the environment section. Used when the default local cwd
    /// rendering would be wrong — an SSH-bound session has no local cwd.
    pub fn set_environment(&mut self, environment: String) {
        self.sections.insert("environment", environment);
        self.joined = None;
    }
}

impl Default for SystemPromptCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the browser-use preset nudge for the system prompt.
///
/// Intentionally a single sentence (默认①, 省 token): it only points the model at
/// the `Browser` tool and the observe→act→verify loop. The detailed action
/// semantics live in `BrowserTool::DESCRIPTION` (its CORE LOOP section), which the
/// model already sees per-call — so this preset deliberately does NOT restate the
/// per-action vocabulary the way the longer `[Controlling the desktop]` computer
/// nudge does.
///
/// Only emitted when the `browser-use` feature is built AND
/// `config.tools.browser.enabled` is true (threaded in as `browser_enabled`).
#[cfg(feature = "browser-use")]
fn browser_preset() -> &'static str {
    "[Browsing the web] When `web_search` is available, use it to discover URLs; when \
`web_extract` is available, use it directly for known public URLs, including PDF and \
JavaScript-shell pages. \
Use the `Browser` tool when a page must be opened, rendered, inspected, or operated \
interactively. Do not ask the user for permission to browse. After each Browser \
navigation or interaction run `observe` for fresh refs before acting again."
}

/// Office-default wrapper. Production sessions should call
/// [`build_system_prompt_with`] so the routing table matches advertised tools.
#[allow(clippy::too_many_arguments)]
pub fn build_system_prompt(
    cache: &mut SystemPromptCache,
    custom_prompt: Option<&str>,
    cwd: &str,
    skills: &[SkillMetadata],
    context_window_tokens: Option<usize>,
    memory_dir: Option<&Path>,
    toon_enabled: bool,
    browser_enabled: bool,
) -> String {
    build_system_prompt_with(
        cache,
        SystemPromptInput::office_defaults(
            custom_prompt,
            cwd,
            skills,
            context_window_tokens,
            memory_dir,
            toon_enabled,
            browser_enabled,
        ),
    )
}

/// Build the **cache-stable** system prompt from typed inputs.
///
/// Order: intro → constitution → clipped tool routing → browser (if advertised)
/// → custom → AGENTS.md → memory (office only) → toon → skills → environment
/// → language LAST.
///
/// Dynamic content (date, plan mode, RAG) is NOT included here; it rides the
/// turn tail. This prefix must stay byte-stable across turns.
pub fn build_system_prompt_with(
    cache: &mut SystemPromptCache,
    input: SystemPromptInput<'_>,
) -> String {
    let advertised_fp = input.advertised.fingerprint();
    let lang_fp = input.language_directive.unwrap_or("").to_string();

    if let Some(ref joined) = cache.joined
        && cache.last_toon_enabled == input.toon_enabled
        && cache.last_browser_enabled == input.browser_enabled
        && cache.last_profile == input.profile
        && cache.last_advertised == advertised_fp
        && cache.last_has_deferred == input.has_deferred_tools
        && cache.last_language == lang_fp
    {
        return joined.clone();
    }

    if cache.last_profile != input.profile || cache.last_advertised != advertised_fp {
        cache.sections.remove("intro");
        cache.sections.remove("constitution");
        cache.sections.remove("tool_guidance");
        cache.sections.remove("skills");
        cache.sections.remove("browser_preset");
        cache.joined = None;
    }
    if cache.last_has_deferred != input.has_deferred_tools {
        cache.sections.remove("tool_guidance");
        cache.joined = None;
    }

    let mut parts = Vec::new();

    // Intro excludes cwd and date so the stable core is a reusable cache prefix.
    let intro = cache.sections.entry("intro").or_insert_with(|| {
        if input.profile.is_coding() {
            coding_intro().to_string()
        } else {
            office_intro().to_string()
        }
    });
    parts.push(intro.clone());

    let constitution = cache.sections.entry("constitution").or_insert_with(|| {
        if input.profile.is_coding() {
            coding_overlay_instructions().to_string()
        } else {
            office_overlay_instructions().to_string()
        }
    });
    if !constitution.is_empty() {
        parts.push(constitution.clone());
    }

    if !cache.omit_generic_tool_guidance {
        let advertised = &input.advertised;
        let has_deferred = input.has_deferred_tools;
        let guidance = cache
            .sections
            .entry("tool_guidance")
            .or_insert_with(|| tool_usage_guidance(advertised, has_deferred));
        parts.push(guidance.clone());
    }

    #[cfg(feature = "browser-use")]
    if input.browser_enabled && input.advertised.contains("Browser") {
        let browser_section = cache
            .sections
            .entry("browser_preset")
            .or_insert_with(|| browser_preset().to_string());
        parts.push(browser_section.clone());
    }

    if let Some(custom) = input.custom_prompt {
        let custom_cached = cache
            .sections
            .entry("custom")
            .or_insert_with(|| custom.to_string());
        parts.push(custom_cached.clone());
    }

    if let Some(agents_section) = cache.sections.get("agents_md")
        && !agents_section.is_empty()
    {
        parts.push(agents_section.clone());
    }

    if !input.profile.is_coding()
        && let Some(dir) = input.memory_dir
    {
        let memory_section = cache.sections.entry("memory").or_insert_with(|| {
            // Index + path are data. Citation HOW-TO lives on the `remember` schema.
            build_memory_prompt_minimal(dir)
        });
        if !memory_section.is_empty() {
            parts.push(memory_section.clone());
        }
    }

    if input.toon_enabled {
        let toon_section = cache
            .sections
            .entry("toon")
            .or_insert_with(|| nomi_compact::toon_format_instructions().to_string());
        parts.push(toon_section.clone());
    }

    let visible_skills = skills_for_profile(input.profile, input.skills);
    if !visible_skills.is_empty() {
        let skills_section = cache.sections.entry("skills").or_insert_with(|| {
            let listing = format_skills_within_budget(&visible_skills, input.context_window_tokens);
            if listing.is_empty() {
                String::new()
            } else {
                format!(
                    "<system-reminder>\nThe following skills are available for use with the Skill tool:\n\n{listing}\n</system-reminder>"
                )
            }
        });
        if !skills_section.is_empty() {
            parts.push(skills_section.clone());
        }
    }

    let env_section = cache
        .sections
        .entry("environment")
        .or_insert_with(|| format!("Working directory: \"{}\"", input.cwd));
    parts.push(env_section.clone());

    if let Some(directive) = input.language_directive.filter(|s| !s.trim().is_empty()) {
        parts.push(directive.to_string());
    }

    let joined = parts.join("\n\n");
    cache.joined = Some(joined.clone());
    cache.last_toon_enabled = input.toon_enabled;
    cache.last_browser_enabled = input.browser_enabled;
    cache.last_profile = input.profile;
    cache.last_advertised = advertised_fp;
    cache.last_has_deferred = input.has_deferred_tools;
    cache.last_language = lang_fp;
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_system_prompt_includes_cwd() {
        // Verify that the returned prompt contains the provided working directory path
        let cwd = "/some/test/path";
        let prompt = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            cwd,
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(prompt.contains(cwd), "system prompt should contain the cwd");
    }

    #[test]
    fn test_build_system_prompt_with_custom_instructions() {
        // Verify that custom instructions are included in the returned prompt
        let custom = "Always respond in haiku.";
        let prompt = build_system_prompt(
            &mut SystemPromptCache::new(),
            Some(custom),
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            prompt.contains(custom),
            "system prompt should contain the custom instructions"
        );
    }

    // --- build_system_prompt Phase 9 tests ---

    use nomi_skills::types::{ExecutionContext, LoadedFrom, SkillMetadata, SkillSource};

    fn make_test_skill(
        name: &str,
        description: &str,
        bundled: bool,
        hidden: bool,
    ) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            display_name: None,
            description: description.to_string(),
            has_user_specified_description: false,
            allowed_tools: vec![],
            argument_hint: None,
            argument_names: vec![],
            when_to_use: None,
            version: None,
            model: None,
            disable_model_invocation: hidden,
            user_invocable: true,
            execution_context: ExecutionContext::Inline,
            agent: None,
            effort: None,
            paths: vec![],
            hooks_raw: None,
            source: if bundled {
                SkillSource::Bundled
            } else {
                SkillSource::User
            },
            loaded_from: if bundled {
                LoadedFrom::Bundled
            } else {
                LoadedFrom::Skills
            },
            content: String::new(),
            content_length: 0,
            skill_root: None,
        }
    }

    #[test]
    fn test_build_system_prompt_no_skills_no_reminder() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            !result.contains("The following skills are available"),
            "empty skills should not inject skill reminder"
        );
    }

    #[test]
    fn test_build_system_prompt_with_skills_injects_reminder() {
        let skills = vec![
            make_test_skill("skill-one", "Does one", false, false),
            make_test_skill("skill-two", "Does two", false, false),
        ];
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &skills,
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("<system-reminder>"),
            "result should contain <system-reminder>"
        );
        assert!(
            result.contains("The following skills are available for use with the Skill tool:"),
            "result should contain skills header"
        );
        assert!(
            result.contains("</system-reminder>"),
            "result should close <system-reminder>"
        );
        assert!(result.contains("skill-one"), "result should list skill-one");
        assert!(result.contains("skill-two"), "result should list skill-two");
    }

    #[test]
    fn test_build_system_prompt_hidden_skill_filtered() {
        let skills = vec![
            make_test_skill("visible-skill", "Visible", false, false),
            make_test_skill("hidden-skill", "Hidden", false, true),
        ];
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &skills,
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("visible-skill"),
            "visible skill should appear"
        );
        assert!(
            !result.contains("hidden-skill"),
            "hidden skill should be filtered out"
        );
    }

    #[test]
    fn test_build_system_prompt_all_hidden_no_reminder() {
        let skills = vec![
            make_test_skill("hidden-a", "Hidden A", false, true),
            make_test_skill("hidden-b", "Hidden B", false, true),
        ];
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &skills,
            None,
            None,
            false,
            false,
        );
        assert!(
            !result.contains("The following skills are available"),
            "all-hidden skills should not inject reminder"
        );
    }

    #[test]
    fn test_build_system_prompt_custom_prompt_and_skills() {
        let skills = vec![make_test_skill("my-skill", "My desc", false, false)];
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            Some("Custom instructions here"),
            "/tmp",
            &skills,
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("Custom instructions here"),
            "custom prompt should appear"
        );
        assert!(
            result.contains("The following skills are available for use with the Skill tool:"),
            "skills reminder should also appear"
        );
    }

    #[test]
    fn test_build_system_prompt_skills_reminder_after_custom_prompt() {
        let skills = vec![make_test_skill("my-skill", "My desc", false, false)];
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            Some("Custom text"),
            "/tmp",
            &skills,
            None,
            None,
            false,
            false,
        );
        let custom_pos = result.find("Custom text").unwrap();
        let reminder_pos = result.rfind("<system-reminder>").unwrap();
        assert!(
            reminder_pos > custom_pos,
            "skills reminder should appear after custom prompt"
        );
    }

    #[test]
    fn test_build_system_prompt_small_budget_triggers_minimal_mode() {
        // context_window_tokens = 50 → budget = 2 chars, triggers minimal mode for non-bundled
        let skill = make_test_skill("nb-skill", &"x".repeat(100), false, false);
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[skill],
            Some(50),
            None,
            false,
            false,
        );
        // Minimal mode: skill appears as name only, no ': '
        assert!(
            result.contains("- nb-skill"),
            "skill name should appear in minimal mode"
        );
        assert!(
            !result.contains("- nb-skill: "),
            "non-bundled should not have description in minimal mode"
        );
    }

    #[test]
    fn test_build_system_prompt_cwd_in_prompt() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/workspace/my-project",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("/workspace/my-project"),
            "cwd should appear in the system prompt"
        );
    }

    #[test]
    fn test_build_system_prompt_loads_agents_md_not_claude_md() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cwd = tmp.path();

        // Create both AGENTS.md and CLAUDE.md
        std::fs::write(cwd.join("AGENTS.md"), "AGENTS_CONTENT_HERE").unwrap();
        std::fs::write(cwd.join("CLAUDE.md"), "CLAUDE_CONTENT_HERE").unwrap();

        let snapshot = crate::agents_md::resolve_agents_md(
            cwd,
            &nomi_config::config::ProjectInstructionsConfig::default(),
        );
        let mut cache = SystemPromptCache::new();
        cache.set_agents_md(snapshot.formatted);
        let result = build_system_prompt(
            &mut cache,
            None,
            &cwd.to_string_lossy(),
            &[],
            None,
            None,
            false,
            false,
        );

        assert!(
            result.contains("AGENTS_CONTENT_HERE"),
            "should load AGENTS.md content"
        );
        assert!(
            !result.contains("CLAUDE_CONTENT_HERE"),
            "should NOT load CLAUDE.md content"
        );
        assert!(
            result.contains("(project instructions)"),
            "header should indicate project instructions"
        );
        assert!(
            result.contains("AGENTS.md"),
            "header should contain AGENTS.md filename"
        );
    }

    #[test]
    fn test_build_system_prompt_no_agents_md_no_injection() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cwd = tmp.path();

        // Only CLAUDE.md exists, no AGENTS.md
        std::fs::write(cwd.join("CLAUDE.md"), "SHOULD_NOT_APPEAR").unwrap();

        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            &cwd.to_string_lossy(),
            &[],
            None,
            None,
            false,
            false,
        );

        assert!(
            !result.contains("SHOULD_NOT_APPEAR"),
            "CLAUDE.md should be ignored"
        );
        assert!(
            !result.contains("(project instructions)"),
            "no project instructions should be injected"
        );
    }

    #[test]
    fn pre_resolved_agents_are_composed_after_custom_prompt_before_environment() {
        let mut cache = SystemPromptCache::new();
        cache.set_agents_md("PRE_RESOLVED_PROJECT_RULE".to_owned());

        let result = build_system_prompt(
            &mut cache,
            Some("CUSTOM_PROMPT_MARKER"),
            "/workspace/project",
            &[],
            None,
            None,
            false,
            false,
        );

        let custom = result.find("CUSTOM_PROMPT_MARKER").unwrap();
        let agents = result.find("PRE_RESOLVED_PROJECT_RULE").unwrap();
        let environment = result.find("Working directory:").unwrap();
        assert!(custom < agents);
        assert!(agents < environment);
    }

    // --- Memory integration tests ---

    #[test]
    fn memory_none_dir_no_injection() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            !result.contains("auto memory"),
            "no memory content when memory_dir is None"
        );
    }

    #[test]
    fn memory_with_dir_injects_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(
            mem_dir.join("MEMORY.md"),
            "- [Role](user_role.md) \u{2014} senior engineer\n",
        )
        .unwrap();

        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            Some(&mem_dir),
            false,
            false,
        );

        assert!(
            result.contains("auto memory"),
            "should contain memory system display name"
        );
        assert!(
            result.contains("Memory types:"),
            "should contain compact memory type summary"
        );
        assert!(
            result.contains("user_role.md"),
            "should contain MEMORY.md content"
        );
        assert!(
            !result.contains("<nomi-mem-citation>"),
            "citation HOW-TO belongs on the remember schema, not the system prefix"
        );
    }

    #[test]
    fn memory_nonexistent_dir_graceful_degradation() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            Some(Path::new("/nonexistent/memory/dir")),
            false,
            false,
        );

        // Should not panic and should show empty state
        assert!(
            result.contains("currently empty"),
            "nonexistent memory dir should show empty state"
        );
    }

    #[test]
    fn memory_empty_dir_shows_empty_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        // No MEMORY.md

        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            Some(&mem_dir),
            false,
            false,
        );

        assert!(
            result.contains("currently empty"),
            "empty memory dir should show empty state"
        );
    }

    #[test]
    fn memory_appears_after_agents_md_before_skills() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cwd = tmp.path();

        // Create AGENTS.md
        std::fs::write(cwd.join("AGENTS.md"), "PROJECT_RULES_HERE").unwrap();

        // Create memory dir with content
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(mem_dir.join("MEMORY.md"), "- [A](a.md) \u{2014} test\n").unwrap();

        let skills = vec![make_test_skill("test-skill", "A skill", false, false)];

        let snapshot = crate::agents_md::resolve_agents_md(
            cwd,
            &nomi_config::config::ProjectInstructionsConfig::default(),
        );
        let mut cache = SystemPromptCache::new();
        cache.set_agents_md(snapshot.formatted);
        let result = build_system_prompt(
            &mut cache,
            None,
            &cwd.to_string_lossy(),
            &skills,
            None,
            Some(&mem_dir),
            false,
            false,
        );

        let agents_pos = result.find("PROJECT_RULES_HERE").unwrap();
        let memory_pos = result.find("auto memory").unwrap();
        let skills_pos = result.find("test-skill").unwrap();

        assert!(
            agents_pos < memory_pos,
            "AGENTS.md should appear before memory"
        );
        assert!(
            memory_pos < skills_pos,
            "memory should appear before skills"
        );
    }

    #[test]
    fn memory_no_bb_brand_in_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(
            mem_dir.join("MEMORY.md"),
            "- [Test](test.md) \u{2014} entry\n",
        )
        .unwrap();

        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            Some(&mem_dir),
            false,
            false,
        );

        assert!(
            !result.contains("~/.claude"),
            "should not contain bb brand path"
        );
        assert!(
            !result.contains("CLAUDE.md"),
            "should not reference CLAUDE.md"
        );
    }

    // --- Tool usage guidance tests (task 4.3) ---

    #[test]
    fn tool_guidance_section_exists() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("# Using your tools"),
            "system prompt should contain the tool guidance heading"
        );
    }

    #[test]
    fn tool_guidance_contains_bash_prohibition_list() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("Glob"),
            "should mention Glob as find/ls replacement"
        );
        assert!(
            result.contains("Grep"),
            "should mention Grep as grep/rg replacement"
        );
        assert!(
            result.contains("Read"),
            "should mention Read as cat/head/tail replacement"
        );
        assert!(
            result.contains("Edit"),
            "should mention Edit as sed/awk replacement"
        );
        assert!(
            result.contains("Write"),
            "should mention Write as echo/heredoc replacement"
        );
    }

    #[test]
    fn tool_guidance_routes_public_pdf_urls_to_web_extract() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(result.contains("Public URL reading"));
        assert!(result.contains("direct PDF and JavaScript-shell URLs"));
        assert!(result.contains("Do not use Browser or"));
        assert!(result.contains("web_extract` directly"));
        assert!(result.contains("download or save the original file"));
        let routing_start = result
            .find(" - Public URL reading:")
            .expect("public URL routing guidance must be present");
        let routing_end = result[routing_start..]
            .find(" - You can call")
            .map(|offset| routing_start + offset)
            .expect("public URL routing guidance must have a following section");
        let routing_guidance = &result[routing_start..routing_end];
        for internal_name in ["MCP", "Provider", "web_fetch", "final_url"] {
            assert!(
                !routing_guidance.contains(internal_name),
                "generic tool guidance must not expose {internal_name}"
            );
        }
    }

    #[test]
    fn tool_guidance_contains_parallel_call_rules() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("parallel"),
            "should contain parallel call guidance"
        );
        assert!(
            result.contains("sequentially"),
            "should explain when to run sequentially"
        );
    }

    #[test]
    fn tool_guidance_contains_edit_over_write_preference() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("Prefer Edit over Write"),
            "should contain Edit-over-Write preference"
        );
    }

    #[test]
    fn tool_guidance_contains_read_before_edit_rule() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("Read each unread range once before the first Edit"),
            "should contain Read-before-Edit rule"
        );
    }

    #[test]
    fn tool_guidance_leaves_update_plan_how_to_in_the_schema() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            !result.contains("final all-completed update_plan"),
            "long update_plan HOW-TO belongs in the tool schema, not the system prompt"
        );
        assert!(
            !result.contains("never the answer itself"),
            "plan-step semantics belong in the update_plan schema"
        );
        assert!(
            result.contains("hard checkpoint"),
            "short routing still requires a failure checkpoint"
        );
    }

    #[test]
    fn tool_guidance_after_intro_before_custom_prompt() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            Some("CUSTOM_MARKER_43"),
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        let intro_pos = result.find("You are Allo").unwrap();
        let guidance_pos = result.find("# Using your tools").unwrap();
        let custom_pos = result.find("CUSTOM_MARKER_43").unwrap();
        assert!(
            guidance_pos > intro_pos,
            "tool guidance should appear after intro"
        );
        assert!(
            guidance_pos < custom_pos,
            "tool guidance should appear before custom prompt"
        );
    }

    #[test]
    fn tool_guidance_before_skills_reminder() {
        let skills = vec![make_test_skill("guide-test-skill", "A skill", false, false)];
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &skills,
            None,
            None,
            false,
            false,
        );
        let guidance_pos = result.find("# Using your tools").unwrap();
        let skills_pos = result.find("guide-test-skill").unwrap();
        assert!(
            guidance_pos < skills_pos,
            "tool guidance should appear before skills reminder"
        );
    }

    #[test]
    fn tool_guidance_present_in_plan_mode() {
        // Plan mode is no longer in the system prompt (it rides the turn tail
        // to keep the prefix cache-stable). Tool guidance must still be present.
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            result.contains("# Using your tools"),
            "tool guidance should always be present"
        );
    }

    #[test]
    fn tool_guidance_contains_deferred_instruction() {
        let mut cache = SystemPromptCache::new();
        let mut input = SystemPromptInput::office_defaults(
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        input.has_deferred_tools = true;
        let result = build_system_prompt_with(&mut cache, input);
        assert!(
            result.contains("deferred"),
            "tool guidance should mention deferred tools"
        );
        assert!(
            result.contains("ToolSearch"),
            "tool guidance should mention ToolSearch"
        );
        assert!(
            result.contains("subsequent model turn"),
            "tool guidance should make deferred activation timing explicit"
        );
    }

    #[test]
    fn tool_guidance_before_memory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(mem_dir.join("MEMORY.md"), "- [X](x.md) \u{2014} test\n").unwrap();

        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            Some(&mem_dir),
            false,
            false,
        );
        let guidance_pos = result.find("# Using your tools").unwrap();
        let memory_pos = result.find("auto memory").unwrap();
        assert!(
            guidance_pos < memory_pos,
            "tool guidance should appear before memory section"
        );
    }

    // --- SystemPromptCache tests ---

    #[test]
    fn cache_new_is_empty() {
        let cache = SystemPromptCache::new();
        assert!(cache.joined.is_none());
        assert!(cache.sections.is_empty());
    }

    #[test]
    fn cache_stores_and_retrieves_section() {
        let mut cache = SystemPromptCache::new();
        cache.sections.insert("intro", "Hello world".to_string());
        assert_eq!(cache.sections.get("intro").unwrap(), "Hello world");
    }

    // --- Cache integration tests ---

    #[test]
    fn build_system_prompt_uses_cache_on_second_call() {
        let mut cache = SystemPromptCache::new();
        let first = build_system_prompt(
            &mut cache,
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(cache.joined.is_some());

        let second = build_system_prompt(
            &mut cache,
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert_eq!(first, second);
    }

    #[test]
    fn build_system_prompt_plan_mode_does_not_affect_prefix() {
        // Plan mode is no longer in the system prompt — it rides the turn tail
        // to keep the prefix cache-stable. Toggling plan mode must NOT change
        // the system prompt (this is the cache-stability invariant).
        let mut cache = SystemPromptCache::new();
        let without_plan = build_system_prompt(
            &mut cache,
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        // Same args — plan mode is now injected in the turn tail, not here.
        // The key invariant: the system prompt is identical regardless of
        // plan mode state. (Plan mode toggle happens in engine.rs turn tail.)
        let second = build_system_prompt(
            &mut cache,
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert_eq!(
            without_plan, second,
            "system prompt must be byte-stable — plan mode no longer affects it"
        );
    }

    // --- TOON format injection tests ---

    #[test]
    fn toon_enabled_injects_format_instructions() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            true,
            false,
        );
        assert!(
            result.contains("TOON"),
            "toon_enabled should inject TOON format instructions"
        );
        assert!(
            result.contains("Token-Oriented Object Notation"),
            "should contain full TOON description"
        );
    }

    #[test]
    fn toon_disabled_no_format_instructions() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            !result.contains("TOON"),
            "toon_disabled should not inject TOON format instructions"
        );
    }

    // --- Browser-use preset injection tests (P3-P1) ---
    //
    // The preset is feature-gated (`browser-use`) AND runtime-gated
    // (`browser_enabled`). These tests only run in the `browser-use` build —
    // in a build without the feature, the section is compiled out entirely, so
    // there is nothing meaningful to assert about its presence.

    #[cfg(feature = "browser-use")]
    #[test]
    fn browser_enabled_injects_preset() {
        let preset = browser_preset();
        assert!(preset.contains("including PDF and JavaScript-shell pages"));
        assert!(preset.contains("When `web_search` is available, use it to discover URLs"));
        assert!(preset.contains("when `web_extract` is available"));

        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            true, // browser_enabled
        );
        assert!(
            result.contains("[Browsing the web]"),
            "browser_enabled should inject the browser preset heading"
        );
        assert!(
            result.contains("`Browser` tool"),
            "preset should name the Browser tool"
        );
        assert!(
            result.contains("Do not ask the user for permission to browse"),
            "preset should make ordinary browsing low-friction"
        );
        assert!(
            !result.contains("For web tasks, use the `Browser` tool"),
            "preset should not route every web task to Browser"
        );
        assert!(
            result.contains("observe"),
            "preset should mention the observe step of the loop"
        );
        assert!(
            result.contains("direct PDF and JavaScript-shell URLs"),
            "preset should make PDF and JavaScript extraction explicit"
        );
        assert!(
            result.contains("When `web_search` is available, use it to discover URLs"),
            "preset should distinguish URL discovery from known direct URLs"
        );
        assert!(
            result.contains("When `web_search` is available")
                && result.contains("when `web_extract` is available"),
            "preset should not route to unavailable web tools"
        );
    }

    #[cfg(feature = "browser-use")]
    #[test]
    fn browser_disabled_no_preset() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false, // browser_enabled = false
        );
        assert!(
            !result.contains("[Browsing the web]"),
            "browser disabled should not inject the browser preset"
        );
    }

    #[cfg(feature = "browser-use")]
    #[test]
    fn browser_preset_is_concise_single_sentence_nudge() {
        // 默认①: the preset must stay a short nudge, NOT a restatement of the
        // full per-action vocabulary. Guard against accidental token bloat and
        // against copying the long `[Controlling the desktop]` computer nudge.
        let preset = browser_preset();
        assert!(
            preset.len() < 500,
            "browser preset should stay a concise nudge (got {} chars)",
            preset.len()
        );
        assert!(
            !preset.contains("[Controlling the desktop]"),
            "browser preset must not copy the computer-use nudge"
        );
    }

    // --- Prefix stability regression tests (cache-stability invariants) ---
    //
    // These tests guard the core DeepSeek prefix-cache invariant: the system
    // prompt must be byte-stable across turns so the automatic prefix cache
    // stays warm. Any change that introduces dynamic content (date, plan mode,
    // RAG injections) into the system prompt will break the cache and cause
    // full re-computation on every turn.

    /// Scan for a YYYY-MM-DD date pattern without pulling in regex as a dep.
    fn contains_date_pattern(s: &str) -> bool {
        let b = s.as_bytes();
        if b.len() < 10 {
            return false;
        }
        for i in 0..=(b.len() - 10) {
            if b[i].is_ascii_digit()
                && b[i + 1].is_ascii_digit()
                && b[i + 2].is_ascii_digit()
                && b[i + 3].is_ascii_digit()
                && b[i + 4] == b'-'
                && b[i + 5].is_ascii_digit()
                && b[i + 6].is_ascii_digit()
                && b[i + 7] == b'-'
                && b[i + 8].is_ascii_digit()
                && b[i + 9].is_ascii_digit()
            {
                return true;
            }
        }
        false
    }

    #[test]
    fn prefix_stability_no_date_in_system_prompt() {
        // The date was previously in the intro section (chrono::Local::now).
        // It must NOT appear anywhere in the system prompt — it rides the
        // turn tail instead (injected by engine.rs).
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            !contains_date_pattern(&result),
            "system prompt must NOT contain a date — it breaks the prefix cache daily"
        );
        assert!(
            !result.contains("Current date"),
            "system prompt must NOT contain 'Current date' — it rides the turn tail"
        );
    }

    #[test]
    fn prefix_stability_byte_identical_across_calls() {
        // The system prompt must be byte-identical across repeated calls with
        // the same inputs. This is the fundamental cache-stability invariant.
        let mut cache = SystemPromptCache::new();
        let prompts: Vec<String> = (0..5)
            .map(|_| {
                build_system_prompt(
                    &mut cache,
                    None,
                    "/tmp",
                    &[],
                    None,
                    None,
                    false,
                    false,
                )
            })
            .collect();
        for i in 1..5 {
            assert_eq!(
                prompts[0].as_bytes(),
                prompts[i].as_bytes(),
                "system prompt must be byte-identical across calls (call {} differs)",
                i
            );
        }
    }

    #[test]
    fn prefix_stability_no_plan_mode_keywords() {
        // Plan mode instructions must NOT appear in the system prompt — they
        // ride the turn tail. This catches regressions where plan mode is
        // accidentally re-injected into the system prompt.
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(
            !result.contains("# Plan Mode"),
            "system prompt must NOT contain plan mode heading"
        );
        assert!(
            !result.contains("ExitPlanMode"),
            "system prompt must NOT reference ExitPlanMode tool"
        );
        assert!(
            !result.contains("Submit for review"),
            "system prompt must NOT contain plan mode workflow phases"
        );
    }

    #[test]
    fn prefix_stability_cache_survives_invalidation_cycle() {
        // After /compact (invalidate_all), the rebuilt prompt must be
        // byte-identical to the pre-compact prompt — compact is the only
        // valid cache reset, and the rebuilt prefix must match.
        let mut cache = SystemPromptCache::new();
        let before = build_system_prompt(
            &mut cache,
            Some("Custom instructions"),
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        // Simulate /compact
        cache.invalidate_all();
        let after = build_system_prompt(
            &mut cache,
            Some("Custom instructions"),
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert_eq!(
            before.as_bytes(),
            after.as_bytes(),
            "system prompt must be byte-identical before and after /compact"
        );
    }

    #[test]
    fn office_prompt_uses_office_constitution_and_omits_coding_overlay() {
        let result = build_system_prompt(
            &mut SystemPromptCache::new(),
            None,
            "/tmp",
            &[],
            None,
            None,
            false,
            false,
        );
        assert!(result.contains("You are Allo, a desktop assistant"));
        assert!(result.contains("# Office mode"));
        assert!(!result.contains("# Coding mode"));
        assert!(!result.contains("knowledge_search"));
        assert!(!result.contains("nomi_delegate"));
        assert!(!result.contains("image_generate"));
        assert!(!result.contains("media_workflow"));
        assert!(!result.contains("ApplyPatch"));
        assert!(!result.contains("nomi-mem-citation"));
    }

    #[test]
    fn coding_prompt_closes_office_surfaces_and_keeps_language_last() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(mem_dir.join("MEMORY.md"), "- [X](x.md) — note\n").unwrap();

        let advertised = advertised_tools_for_session(
            TaskProfile::Coding,
            &SessionToolSurface {
                computer: true,
                browser: true,
                web: true,
                memory: true,
                ssh: false,
                lsp: true,
                has_deferred: true,
                allowlist: Vec::new(),
            },
        );
        let result = build_system_prompt_with(
            &mut SystemPromptCache::new(),
            SystemPromptInput {
                custom_prompt: Some("persona blob"),
                cwd: "/ws",
                skills: &[],
                context_window_tokens: None,
                memory_dir: Some(&mem_dir),
                toon_enabled: false,
                browser_enabled: true,
                profile: TaskProfile::Coding,
                advertised,
                language_directive: Some("【输出语言】请用中文"),
                has_deferred_tools: true,
            },
        );

        assert!(result.contains("You are a coding agent"));
        assert!(result.contains("# Coding mode"));
        assert!(result.contains("persona blob"));
        assert!(!result.contains("# Office mode"));
        assert!(!result.contains("auto memory"));
        assert!(!result.contains("knowledge_search"));
        assert!(!result.contains("nomi_delegate"));
        assert!(!result.contains("image_generate"));
        assert!(!result.contains("Computer"));
        assert!(!result.contains("ToolSearch"));
        assert!(!result.contains("remember"));
        assert!(!result.contains("[Browsing the web]"));

        let coding = result.find("# Coding mode").unwrap();
        let lang = result.find("【输出语言】").unwrap();
        let env = result.find("Working directory").unwrap();
        assert!(coding < env);
        assert!(env < lang, "language directive must be last");
    }

    #[test]
    fn coding_prompt_keeps_project_skills_and_drops_user_skills() {
        let project = make_test_skill("repo-skill", "Project skill", false, false);
        let mut project = project;
        project.source = SkillSource::Project;
        let user = make_test_skill("user-skill", "User skill", false, false);
        let bundled = make_test_skill("bundled-skill", "Bundled skill", true, false);

        let advertised = AdvertisedToolSet::from_names(["Read", "Skill"]);
        let result = build_system_prompt_with(
            &mut SystemPromptCache::new(),
            SystemPromptInput {
                custom_prompt: None,
                cwd: "/ws",
                skills: &[project, user, bundled],
                context_window_tokens: None,
                memory_dir: None,
                toon_enabled: false,
                browser_enabled: false,
                profile: TaskProfile::Coding,
                advertised,
                language_directive: None,
                has_deferred_tools: false,
            },
        );
        assert!(result.contains("repo-skill"));
        assert!(!result.contains("user-skill"));
        assert!(!result.contains("bundled-skill"));
    }
}
