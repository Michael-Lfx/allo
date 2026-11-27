//! Workspace tool registry, path confinement, skill scoping, and shell guard hooks.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use flowy_agent_core::{
    SessionOrigin, ToolDefinition, ToolPolicyContext, ToolProviderContext,
    ToolSource, WorkAccessMode,
    tools::{ShellPolicyDecision, evaluate_shell_command},
};
use flowy_skills::{InstalledSkill, SkillId};

use super::{
    AgentWorkspace, Arc, Path, PathBuf, ProviderApprovalBinding,
    RegisteredProvider, ToolCallArgs, ToolGuard, ToolRegistry, lock,
    log_storage_io_error, pi_shell_definition, register_builtin_tools,
};

/// Canonical host / built-in / Router tool names that extension providers may
/// never register or shadow.
const RESERVED_HOST_TOOLS: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "glob",
    "grep",
    "todo_write",
    "bash",
    "powershell",
    "create_deck",
    "verify_deck",
    "verify_artifact",
    "publish_file",
    flowy_agent_core::TOOL_SEARCH_NAME,
    "dispatch_work",
    "query_work_status",
    "control_work",
    "ask_user_choice",
];

const MAX_PROVIDER_WORKER_HINT_CHARS: usize = 240;
const MAX_TOTAL_WORKER_HINTS_CHARS: usize = 1_200;

/// Compute a deterministic SHA-256 hex digest over `(tool_name, canonical(args))`
/// so `PendingCard::ToolApproval` approvals are pinned to exact invocation args.
#[must_use]
pub(super) fn canonical_tool_args_hash(
    tool_name: &str,
    args: &serde_json::Value,
) -> String {
    let canonical = flowy_agent_core::canonical_tool_args_json(tool_name, args);
    crate::delivery::hash_bytes(canonical.as_bytes())
}

/// Canonical normalized directory key used for comparing `cwd` across Windows
/// casing and slash styles without mutating filesystems.
#[must_use]
pub(super) fn normalize_cwd_key(cwd: &Path) -> String {
    let resolved = resolve_deepest_ancestor(cwd);
    #[cfg(windows)]
    {
        resolved.to_string_lossy().replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        resolved.to_string_lossy().into_owned()
    }
}

/// Deterministic SHA-256 fingerprint over a provider tool's schema, exposure,
/// read-only metadata, tags, and optional provider version.
#[must_use]
pub(super) fn tool_definition_fingerprint(
    def: &ToolDefinition,
    provider_version: Option<&str>,
) -> String {
    let exposure = match def.exposure() {
        flowy_agent_core::ToolExposure::Eager => "eager",
        flowy_agent_core::ToolExposure::Deferred => "deferred",
    };
    let canonical_params =
        flowy_agent_core::canonical_tool_args_json("__schema", &def.parameters);
    let payload = serde_json::json!({
        "name": def.name,
        "description": def.description,
        "canonical_params": canonical_params,
        "read_only": def.is_read_only(),
        "exposure": exposure,
        "tags": def.tags(),
        "version": provider_version.unwrap_or(""),
    });
    let canonical =
        flowy_agent_core::canonical_tool_args_json("__tool_fp", &payload);
    crate::delivery::hash_bytes(canonical.as_bytes())
}

/// Canonical 64-hex approval digest pinned to process nonce, live provider
/// identity, tool schema/version fingerprint, normalized `cwd`, and invocation args.
#[must_use]
pub(super) fn provider_approval_hash(
    process_nonce: &str,
    binding: &ProviderApprovalBinding,
    args: &serde_json::Value,
) -> String {
    let canonical_args =
        flowy_agent_core::canonical_tool_args_json(&binding.tool_name, args);
    let payload = serde_json::json!({
        "kind": "provider_approval_v1",
        "process_nonce": process_nonce,
        "provider_id": binding.provider_id,
        "registration_id": binding.registration_id,
        "tool_name": binding.tool_name,
        "tool_fingerprint": binding.tool_fingerprint,
        "cwd_key": binding.cwd_key,
        "canonical_args": canonical_args,
    });
    let canonical =
        flowy_agent_core::canonical_tool_args_json("__prov_approval", &payload);
    crate::delivery::hash_bytes(canonical.as_bytes())
}

/// Read-only inspection and verification tools that remain permitted even
/// after a user denies a risky mutating action in the current turn.
const READ_ONLY_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "ls",
    "list_dir",
    "find_files",
    "verify_deck",
    "verify_artifact",
    flowy_agent_core::TOOL_SEARCH_NAME,
];

/// Parameters bundled for [`build_pre_tool_hook`] to keep hook wiring explicit.
pub(super) struct PreToolHookSpec {
    pub(super) session_id: String,
    pub(super) cwd: PathBuf,
    pub(super) origin: SessionOrigin,
    pub(super) state: Arc<super::UiState>,
    pub(super) registry: ToolRegistry,
    pub(super) providers: Vec<RegisteredProvider>,
    pub(super) mounted_provenance: HashMap<String, MountedToolProvenance>,
    pub(super) process_nonce: String,
    pub(super) force_reconfirm_tools: BTreeSet<String>,
}

#[derive(Clone)]
pub(super) struct StagedProviderTool {
    pub(super) definition: Arc<ToolDefinition>,
    pub(super) provider_id: String,
    pub(super) registration_id: u64,
    pub(super) tool_fingerprint: String,
}

#[derive(Clone, Default)]
pub(super) struct StagedProviders {
    pub(super) tools: Vec<StagedProviderTool>,
    pub(super) hints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MountedToolProvenance {
    pub(super) provider_id: String,
    pub(super) registration_id: u64,
    pub(super) tool_fingerprint: String,
}

#[derive(Default)]
pub(super) struct MountedProviders {
    pub(super) hints: Vec<String>,
    pub(super) provenance: HashMap<String, MountedToolProvenance>,
}

/// Snapshot of Worker `ToolProvider::register_tools()` collected outside the
/// global `dispatch` lock and reused inside `dispatch` when metadata matches.
pub(super) struct PreStagedWorkerProviders {
    pub(super) session_id: String,
    pub(super) cwd_key: String,
    pub(super) origin: SessionOrigin,
    pub(super) access_mode: WorkAccessMode,
    pub(super) provider_signatures: Vec<(String, u64)>,
    pub(super) staged: StagedProviders,
}

impl PreStagedWorkerProviders {
    #[must_use]
    pub(super) fn matches(
        &self,
        session_id: &str,
        cwd_key: &str,
        origin: &SessionOrigin,
        access_mode: WorkAccessMode,
        providers: &[RegisteredProvider],
    ) -> bool {
        self.session_id == session_id
            && self.cwd_key == cwd_key
            && &self.origin == origin
            && self.access_mode == access_mode
            && self.provider_signatures.len() == providers.len()
            && self.provider_signatures.iter().zip(providers.iter()).all(
                |((sid, reg_id), entry)| {
                    *reg_id == entry.registration_id
                        && sid == entry.provider.id().trim()
                },
            )
    }
}

/// Invoke `ToolProvider::register_tools()` and `worker_prompt_hint()` with zero
/// host locks held, deterministically rejecting duplicate tool names registered
/// more than once within the same provider.
pub(super) fn stage_tool_providers(
    providers: &[RegisteredProvider],
    ctx: &ToolProviderContext<'_>,
) -> StagedProviders {
    let mut staged = StagedProviders::default();
    let mut total_hint_chars = 0_usize;
    for entry in providers {
        let provider_id = entry.provider.id().trim();
        if provider_id.is_empty() {
            continue;
        }
        let staging = ToolRegistry::new_rejecting_duplicates();
        entry.provider.register_tools(ctx, &staging);
        let dup_names: HashSet<String> =
            staging.duplicate_tool_names().into_iter().collect();
        for dup in &dup_names {
            tracing::warn!(
                target: "flowy.prompt",
                provider = provider_id,
                tool = %dup,
                "refused duplicate tool name registered multiple times within the same provider"
            );
        }
        let version = entry.provider.version();
        for def in staging.definitions() {
            if dup_names.contains(&def.name) {
                continue;
            }
            let tool_fingerprint = tool_definition_fingerprint(&def, version);
            staged.tools.push(StagedProviderTool {
                definition: def,
                provider_id: provider_id.to_owned(),
                registration_id: entry.registration_id,
                tool_fingerprint,
            });
        }
        if let Some(raw_hint) = entry.provider.worker_prompt_hint(ctx) {
            let cleaned =
                raw_hint.split_whitespace().collect::<Vec<_>>().join(" ");
            if cleaned.is_empty() {
                continue;
            }
            let remaining =
                MAX_TOTAL_WORKER_HINTS_CHARS.saturating_sub(total_hint_chars);
            if remaining == 0 {
                continue;
            }
            let clamped = flowy_agent_core::log_preview(
                &cleaned,
                MAX_PROVIDER_WORKER_HINT_CHARS.min(remaining),
            );
            total_hint_chars =
                total_hint_chars.saturating_add(clamped.chars().count());
            staged.hints.push(clamped);
        }
    }
    staged
}

/// Pure in-memory merge of pre-staged provider tools into `registry` (zero
/// provider callbacks), enforcing `RESERVED_HOST_TOOLS`, cross-provider/host
/// shadowing protection, and stripping `artifact_delivery`.
pub(super) fn merge_staged_providers(
    registry: &ToolRegistry,
    staged: &StagedProviders,
) -> MountedProviders {
    let mut mounted = MountedProviders {
        hints: staged.hints.clone(),
        provenance: HashMap::new(),
    };
    for item in &staged.tools {
        let def = &item.definition;
        let trimmed = def.name.trim();
        if trimmed.is_empty() || trimmed != def.name {
            tracing::warn!(
                target: "flowy.prompt",
                provider = %item.provider_id,
                tool = %def.name,
                "refused provider tool with empty or whitespace-padded name"
            );
            continue;
        }
        if RESERVED_HOST_TOOLS.contains(&def.name.as_str())
            || registry.get(&def.name).is_some()
        {
            tracing::warn!(
                target: "flowy.prompt",
                provider = %item.provider_id,
                tool = %def.name,
                "refused provider tool shadowing reserved host tool or duplicate name"
            );
            continue;
        }
        let sanitized: ToolDefinition = (**def)
            .clone()
            .without_artifact_delivery()
            .with_source(ToolSource::Provider(item.provider_id.clone()));
        registry.register(Arc::new(sanitized));
        mounted.provenance.insert(
            def.name.clone(),
            MountedToolProvenance {
                provider_id: item.provider_id.clone(),
                registration_id: item.registration_id,
                tool_fingerprint: item.tool_fingerprint.clone(),
            },
        );
    }
    mounted
}

/// Mount all registered extension [`ToolProvider`]s into `registry` for a
/// `Worker (ReadWrite)` or legacy `Standard` turn.
pub(super) fn mount_tool_providers(
    registry: &ToolRegistry,
    providers: &[RegisteredProvider],
    ctx: &ToolProviderContext<'_>,
) -> MountedProviders {
    let staged = stage_tool_providers(providers, ctx);
    merge_staged_providers(registry, &staged)
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal approval consumer shares shell and provider Ask paths"
)]
fn consume_or_request_approval(
    state: &super::UiState,
    session_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    process_nonce: &str,
    provider_binding: Option<ProviderApprovalBinding>,
    target_summary: String,
    risk_reason: String,
    command_preview: String,
) -> flowy_agent_core::PreToolDecision {
    let args_hash = match &provider_binding {
        Some(binding) => provider_approval_hash(process_nonce, binding, args),
        None => canonical_tool_args_hash(tool_name, args),
    };
    let consumed_grant = {
        let mut grants = lock(&state.approved_tool_calls);
        if let Some(list) = grants.get_mut(session_id)
            && let Some(pos) = list.iter().position(|grant| {
                grant.tool_name == tool_name
                    && grant.args_hash == args_hash
                    && grant.provider_binding == provider_binding
            })
        {
            list.remove(pos);
            true
        } else {
            false
        }
    };
    if consumed_grant {
        return flowy_agent_core::PreToolDecision::Allow;
    }
    if let Some(binding) = provider_binding {
        lock(&state.pending_provider_approvals)
            .insert((session_id.to_owned(), args_hash.clone()), binding);
    }
    flowy_agent_core::PreToolDecision::Ask {
        target_summary,
        args_hash,
        risk_reason,
        command_preview,
    }
}

/// Build the host `PreToolHook` wired into `rpi_agent`'s `before_tool_call`
/// enforcing turn-scoped `UserDenied` anti-bypass, 3-way `shell_policy`
/// (`Allow | Ask | Deny`), provider-scoped pre-tool policies, and one-shot
/// bound approval grants.
pub(super) fn build_pre_tool_hook(
    spec: PreToolHookSpec,
) -> flowy_agent_core::PreToolHook {
    let PreToolHookSpec {
        session_id,
        cwd,
        origin,
        state,
        registry,
        providers,
        mounted_provenance,
        process_nonce,
        force_reconfirm_tools,
    } = spec;
    let cwd_key = normalize_cwd_key(&cwd);
    Arc::new(move |tool_name: &str, args: &serde_json::Value| {
        let is_read_only = READ_ONLY_TOOLS.contains(&tool_name)
            || registry.is_read_only(tool_name);
        if !is_read_only {
            let denied =
                lock(&state.denied_tool_turns).get(&session_id).cloned();
            if let Some(reason) = denied {
                return flowy_agent_core::PreToolDecision::Deny {
                    reason: format!(
                        "USER_DENIED: user denied a risky action in this turn ({reason}); bypassing with {tool_name} is forbidden"
                    ),
                };
            }
        }
        if matches!(tool_name, "bash" | "powershell")
            && let Some(command) =
                args.get("command").and_then(serde_json::Value::as_str)
        {
            match evaluate_shell_command(command, &cwd) {
                ShellPolicyDecision::Allow => {
                    return flowy_agent_core::PreToolDecision::Allow;
                }
                ShellPolicyDecision::Deny { code, reason } => {
                    return flowy_agent_core::PreToolDecision::Deny {
                        reason: format!("{code}: {reason}"),
                    };
                }
                ShellPolicyDecision::Ask { reason, .. } => {
                    let preview =
                        flowy_agent_core::log_preview(command.trim(), 160);
                    return consume_or_request_approval(
                        &state,
                        &session_id,
                        tool_name,
                        args,
                        &process_nonce,
                        None,
                        format!("{tool_name} ({})", cwd.display()),
                        reason,
                        preview,
                    );
                }
            }
        }
        if let Some(def) = registry.get(tool_name)
            && let ToolSource::Provider(owner_id) = def.source()
            && let Some(entry) = providers
                .iter()
                .find(|p| p.provider.id().trim() == owner_id.as_str())
        {
            let policy_ctx = ToolPolicyContext {
                session_id: &session_id,
                cwd: &cwd,
                origin: &origin,
            };
            let current_binding =
                mounted_provenance.get(tool_name).map(|prov| {
                    ProviderApprovalBinding {
                        provider_id: prov.provider_id.clone(),
                        registration_id: prov.registration_id,
                        tool_name: tool_name.to_owned(),
                        tool_fingerprint: prov.tool_fingerprint.clone(),
                        cwd_key: cwd_key.clone(),
                    }
                });
            let decision =
                entry
                    .provider
                    .evaluate_pre_tool(tool_name, args, &policy_ctx);
            match decision {
                Some(flowy_agent_core::PreToolDecision::Ask {
                    target_summary,
                    risk_reason,
                    command_preview,
                    ..
                }) => {
                    return consume_or_request_approval(
                        &state,
                        &session_id,
                        tool_name,
                        args,
                        &process_nonce,
                        current_binding,
                        target_summary,
                        risk_reason,
                        command_preview,
                    );
                }
                Some(
                    other @ (flowy_agent_core::PreToolDecision::Deny { .. }
                    | flowy_agent_core::PreToolDecision::SuspendWithCard {
                        ..
                    }),
                ) => return other,
                Some(flowy_agent_core::PreToolDecision::Allow) | None => {
                    if force_reconfirm_tools.contains(tool_name) {
                        let target = tool_target(args);
                        let preview = if target.is_empty() {
                            tool_name.to_owned()
                        } else {
                            flowy_agent_core::log_preview(
                                &format!("{tool_name}({target})"),
                                160,
                            )
                        };
                        return consume_or_request_approval(
                            &state,
                            &session_id,
                            tool_name,
                            args,
                            &process_nonce,
                            current_binding,
                            format!("{tool_name} ({owner_id})"),
                            "previous provider approval expired; user confirmation required"
                                .to_owned(),
                            preview,
                        );
                    }
                }
            }
        }
        flowy_agent_core::PreToolDecision::Allow
    })
}

pub(super) fn ensure_tool_cwd(root: &Path) -> PathBuf {
    let dir = root.join("sessions").join("cwd");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log_storage_io_error("create tool working directory", &dir, &error);
    }
    dir
}

pub(super) fn session_working_directory(
    root: &Path,
    workspace: Option<&AgentWorkspace>,
) -> PathBuf {
    workspace.map_or_else(|| ensure_tool_cwd(root), |item| item.root.clone())
}

pub(super) fn tool_registry(
    cwd: &Path,
    skills: &[InstalledSkill],
    active_skills: Vec<SkillId>,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry);
    registry.register(crate::pptx::definition());
    registry.register(pi_shell_definition(cwd));
    // The app has no todo surface yet; expose only tools whose results the
    // host can satisfy instead of advertising one that always fails.
    registry.unregister("todo_write");
    let root = cwd.to_path_buf();
    let active = Arc::new(Mutex::new(skill_scope(skills, active_skills)));
    let skills_for_paths = skills.to_vec();
    let path_active = Arc::clone(&active);
    let file_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        path_guard(&root, &skills_for_paths, &path_active, args)
    });
    registry.add_guard(file_guard);
    let skills_for_shell = skills.to_vec();
    let skill_shell_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        shell_guard(&skills_for_shell, &active, args)
    });
    registry.add_guard(skill_shell_guard);
    let shell_root = cwd.to_path_buf();
    let policy_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        shell_policy_guard(&shell_root, args)
    });
    registry.add_guard(policy_guard);
    registry
}

/// Strictly read-only tool registry for `WorkAccessMode::ReadOnly` Workers:
/// exposes ONLY the 3 built-in inspection tools (`read_file`, `glob`, `grep`)
/// confined by `path_guard`, with zero shell, write, deck, or publish tools.
pub(super) fn read_only_tool_registry(
    cwd: &Path,
    skills: &[InstalledSkill],
    active_skills: Vec<SkillId>,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry);
    registry.unregister("write_file");
    registry.unregister("edit_file");
    registry.unregister("todo_write");
    let root = cwd.to_path_buf();
    let active = Arc::new(Mutex::new(skill_scope(skills, active_skills)));
    let skills_for_paths = skills.to_vec();
    let file_guard: ToolGuard = Arc::new(move |args: &ToolCallArgs| {
        path_guard(&root, &skills_for_paths, &active, args)
    });
    registry.add_guard(file_guard);
    registry
}

/// Static cross-platform safety guard for shell commands (`powershell` / `bash`).
///
/// Complements `path_guard` by blocking host-wide destructive operations,
/// credential store exfiltration, OS privilege escalation, and macOS privacy
/// dialog tampering before `pi_shell` spawns any subprocess.
pub(super) fn shell_policy_guard(
    root: &Path,
    args: &ToolCallArgs,
) -> Option<String> {
    if !matches!(args.name.as_str(), "powershell" | "bash") {
        return None;
    }
    let command = args
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)?;
    let decision = evaluate_shell_command(command, root);
    if let Some(reason) = decision.denial_message() {
        tracing::warn!(
            target: "flowy.sandbox",
            tool = %args.name,
            reason = %reason,
            "blocked shell command by static safety policy"
        );
        return Some(reason);
    }
    None
}

/// Host policy for file tools. The platform shell is handled by dedicated
/// guards because it executes commands rather than host file paths.
pub(super) fn path_guard(
    root: &Path,
    skills: &[InstalledSkill],
    active: &Arc<Mutex<HashSet<SkillId>>>,
    args: &ToolCallArgs,
) -> Option<String> {
    let key = match args.name.as_str() {
        "read_file" | "write_file" | "edit_file" | "glob" | "grep"
        | "create_deck" | "verify_deck" | "verify_artifact" => "path",
        _ => return None,
    };
    let raw = args.arguments.get(key).and_then(|value| value.as_str())?;
    if raw.trim().is_empty() {
        return None;
    }
    if confined_to(root, raw) {
        return None;
    }
    let writable = matches!(
        args.name.as_str(),
        "write_file" | "edit_file" | "create_deck"
    );
    if writable {
        return Some(format!("path escapes the working directory: {raw}"));
    }
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        root.join(raw)
    };
    let Some(skill) = skills
        .iter()
        .find(|skill| confined_to(&skill.root, &path.to_string_lossy()))
    else {
        return Some(format!("path escapes the working directory: {raw}"));
    };
    if matches!(args.name.as_str(), "read_file" | "grep") {
        lock(active).insert(skill.id.clone());
    }
    None
}

pub(super) fn skill_scope(
    skills: &[InstalledSkill],
    explicit: Vec<SkillId>,
) -> HashSet<SkillId> {
    let mut active = explicit.into_iter().collect::<HashSet<_>>();
    active.extend(
        skills
            .iter()
            .filter(|skill| skill.auto_invoke)
            .map(|skill| skill.id.clone()),
    );
    active
}

pub(super) fn shell_guard(
    skills: &[InstalledSkill],
    active: &Arc<Mutex<HashSet<SkillId>>>,
    args: &ToolCallArgs,
) -> Option<String> {
    if !matches!(args.name.as_str(), "bash" | "powershell") {
        return None;
    }
    let active = lock(active);
    let blocked = skills
        .iter()
        .find(|skill| active.contains(&skill.id) && !skill.scripts_enabled);
    blocked.map(|skill| {
        format!(
            "skill \"{}\" does not have script permission; enable it in My Skills before using a shell",
            skill.name
        )
    })
}

pub use flowy_agent_core::tools::{
    has_reserved_device_component, is_windows_reparse_point,
};

pub fn confined_to(root: &Path, raw: &str) -> bool {
    if raw.trim().is_empty() || raw.contains('\0') {
        return false;
    }
    let path = Path::new(raw);
    if has_reserved_device_component(path) {
        return false;
    }
    // Reject Windows drive-relative (`C:foo`) or rooted-without-drive (`\foo`)
    // paths on Windows, and walk component-by-component canonicalizing existing
    // symlinks/junctions BEFORE folding `..` so `link/../outside` can never
    // lexically collapse inside `root` while the OS follows `link` outside.
    let resolved_root = resolve_deepest_ancestor(root);
    let mut current = if path.is_absolute() {
        PathBuf::new()
    } else {
        let has_prefix_or_root = path.components().any(|c| {
            matches!(
                c,
                std::path::Component::Prefix(_) | std::path::Component::RootDir
            )
        });
        if has_prefix_or_root {
            return false;
        }
        resolved_root.clone()
    };
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
            }
            std::path::Component::RootDir => {
                current.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !current.pop() {
                    return false;
                }
                if let Ok(canon) = std::fs::canonicalize(&current) {
                    current = canon;
                }
            }
            std::path::Component::Normal(part) => {
                current.push(part);
                if let Ok(meta) = std::fs::symlink_metadata(&current) {
                    if let Ok(canon) = std::fs::canonicalize(&current) {
                        current = canon;
                    } else if meta.file_type().is_symlink()
                        || is_windows_reparse_point(&meta)
                    {
                        // Dangling symlink or NTFS junction: fail closed
                        // rather than trusting a lexical target.
                        return false;
                    }
                }
            }
        }
    }
    let candidate = resolve_deepest_ancestor(&current);
    within(&resolved_root, &candidate)
}

pub(super) fn resolve_deepest_ancestor(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    // Walk top-down component by component, canonicalizing every existing prefix
    // BEFORE folding `.` / `..`. Once a non-existent segment begins
    // (`missing_depth > 0`), track suffix depth explicitly so trailing or
    // intermediate `..` never abort ancestor canonicalization (unlike
    // `Path::file_name()`, which returns `None` when ending in `..`) and still
    // re-enable canonicalization if `..` climbs back to the existing ancestor.
    let mut current = PathBuf::new();
    let mut missing_depth: usize = 0;
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
            }
            std::path::Component::RootDir => {
                current.push(component.as_os_str());
                if missing_depth == 0
                    && let Ok(canon) = std::fs::canonicalize(&current)
                {
                    current = canon;
                }
            }
            std::path::Component::CurDir => {
                if current.as_os_str().is_empty()
                    && missing_depth == 0
                    && let Ok(canon) = std::fs::canonicalize(".")
                {
                    current = canon;
                }
            }
            std::path::Component::Normal(part) => {
                current.push(part);
                if missing_depth == 0 {
                    if let Ok(canon) = std::fs::canonicalize(&current) {
                        current = canon;
                    } else {
                        missing_depth = 1;
                    }
                } else {
                    missing_depth += 1;
                }
            }
            std::path::Component::ParentDir => {
                if missing_depth > 0 {
                    current.pop();
                    missing_depth -= 1;
                } else if matches!(
                    current.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) {
                    current.pop();
                    if let Ok(canon) = std::fs::canonicalize(&current) {
                        current = canon;
                    }
                } else if !matches!(
                    current.components().next_back(),
                    Some(
                        std::path::Component::Prefix(_)
                            | std::path::Component::RootDir
                    )
                ) {
                    current.push(component.as_os_str());
                }
            }
        }
    }
    if current.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        current
    }
}

pub(super) fn within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        let root = root.to_string_lossy().replace('/', "\\").to_lowercase();
        let candidate = candidate
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();
        candidate == root || candidate.starts_with(&format!("{root}\\"))
    }
    #[cfg(not(windows))]
    {
        candidate.starts_with(root)
    }
}

pub(super) fn tool_target(args: &serde_json::Value) -> String {
    for key in [
        "description",
        "path",
        "file_path",
        "command",
        "query",
        "pattern",
        "target",
    ] {
        if let Some(value) = args.get(key).and_then(serde_json::Value::as_str) {
            return value.to_owned();
        }
    }
    String::new()
}
