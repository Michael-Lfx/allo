//! Profile-aware system-prompt graph: constitutions, advertised-tool routing,
//! and language-last placement.
//!
//! Golden rule: a tool name must not appear in the system prompt unless it will
//! be advertised this session. Tool HOW-TO lives in schemas; this module only
//! emits identity, constitution, and a short routing table.

use std::collections::HashSet;

use nomi_coding::{TaskProfile, advertise_tool};
use nomi_skills::types::{SkillMetadata, SkillSource};

/// Markers for the output-language section. The coding overlay must be inserted
/// *before* this section so the language directive actually wins.
pub const LANGUAGE_ZH_MARKER: &str = "【输出语言】";
pub const LANGUAGE_EN_MARKER: &str = "[Output language]";

/// Tools the model is allowed to see this session (lowercase names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvertisedToolSet {
    names: HashSet<String>,
}

impl AdvertisedToolSet {
    pub fn from_names(names: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Self {
            names: names
                .into_iter()
                .map(|n| n.as_ref().to_ascii_lowercase())
                .collect(),
        }
    }

    /// Office unit-test default: the common desktop file/shell/web surface.
    pub fn office_default() -> Self {
        Self::from_names([
            "Read",
            "ReadContentRef",
            "Write",
            "Edit",
            "Bash",
            "Grep",
            "Glob",
            "DirTree",
            "ApplyPatch",
            "exec_command",
            "write_stdin",
            "update_plan",
            "web_search",
            "web_extract",
            "Computer",
            "Browser",
            "ToolSearch",
            "Skill",
            "Lsp",
            "explore_code",
            "verify_change",
            "research",
        ])
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(&name.to_ascii_lowercase())
    }

    pub fn fingerprint(&self) -> String {
        let mut names: Vec<&str> = self.names.iter().map(String::as_str).collect();
        names.sort_unstable();
        names.join(",")
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

/// Capability flags known at bootstrap (before manager post-wiring).
#[derive(Debug, Clone)]
pub struct SessionToolSurface {
    pub computer: bool,
    pub browser: bool,
    pub web: bool,
    pub memory: bool,
    pub ssh: bool,
    pub lsp: bool,
    pub has_deferred: bool,
    pub allowlist: Vec<String>,
}

/// Build the advertised set from profile + session capabilities, then apply
/// [`advertise_tool`] so coding cannot mention hidden office surfaces.
pub fn advertised_tools_for_session(
    profile: TaskProfile,
    surface: &SessionToolSurface,
) -> AdvertisedToolSet {
    let mut names: Vec<&str> = if surface.ssh {
        vec![
            "Read",
            "ReadContentRef",
            "Write",
            "Edit",
            "Bash",
            "Grep",
            "Glob",
        ]
    } else {
        vec![
            "Read",
            "ReadContentRef",
            "Write",
            "Edit",
            "Bash",
            "Grep",
            "Glob",
            "DirTree",
            "ApplyPatch",
            "exec_command",
            "write_stdin",
            "update_plan",
            "Skill",
            "explore_code",
            "verify_change",
            "research",
        ]
    };
    if surface.web {
        names.extend(["web_search", "web_extract"]);
    }
    if surface.computer {
        names.push("Computer");
    }
    if surface.browser {
        names.push("Browser");
    }
    if surface.memory {
        names.push("remember");
    }
    if surface.lsp {
        names.push("Lsp");
    }
    if surface.has_deferred {
        names.push("ToolSearch");
    }

    let filtered: Vec<&str> = names
        .into_iter()
        .filter(|name| advertise_tool(profile, name))
        .collect();

    let set = if surface.allowlist.is_empty() {
        AdvertisedToolSet::from_names(filtered)
    } else {
        let allowed: HashSet<String> = surface
            .allowlist
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();
        AdvertisedToolSet::from_names(
            filtered
                .into_iter()
                .filter(|name| allowed.contains(&name.to_ascii_lowercase())),
        )
    };
    set
}

/// Inputs for one cache-stable system prompt build.
pub struct SystemPromptInput<'a> {
    pub custom_prompt: Option<&'a str>,
    pub cwd: &'a str,
    pub skills: &'a [SkillMetadata],
    pub context_window_tokens: Option<usize>,
    pub memory_dir: Option<&'a std::path::Path>,
    pub toon_enabled: bool,
    pub browser_enabled: bool,
    pub profile: TaskProfile,
    pub advertised: AdvertisedToolSet,
    pub language_directive: Option<&'a str>,
    pub has_deferred_tools: bool,
}

impl<'a> SystemPromptInput<'a> {
    pub fn office_defaults(
        custom_prompt: Option<&'a str>,
        cwd: &'a str,
        skills: &'a [SkillMetadata],
        context_window_tokens: Option<usize>,
        memory_dir: Option<&'a std::path::Path>,
        toon_enabled: bool,
        browser_enabled: bool,
    ) -> Self {
        Self {
            custom_prompt,
            cwd,
            skills,
            context_window_tokens,
            memory_dir,
            toon_enabled,
            browser_enabled,
            profile: TaskProfile::Office,
            advertised: AdvertisedToolSet::office_default(),
            language_directive: None,
            has_deferred_tools: false,
        }
    }
}

pub fn skills_for_profile(profile: TaskProfile, skills: &[SkillMetadata]) -> Vec<SkillMetadata> {
    skills
        .iter()
        .filter(|skill| {
            if skill.disable_model_invocation {
                return false;
            }
            if !profile.is_coding() {
                return true;
            }
            matches!(
                skill.source,
                SkillSource::Project | SkillSource::Managed | SkillSource::Legacy
            )
        })
        .cloned()
        .collect()
}

pub fn language_section_start(prompt: &str) -> Option<usize> {
    prompt
        .find(LANGUAGE_ZH_MARKER)
        .or_else(|| prompt.find(LANGUAGE_EN_MARKER))
}

/// Insert `block` before the language section (or append if none). Used when
/// the coding overlay is installed after bootstrap.
pub fn insert_before_language_section(prompt: &mut String, block: &str) {
    let block = block.trim();
    if block.is_empty() {
        return;
    }
    match language_section_start(prompt) {
        Some(idx) => {
            let prefix = if idx == 0 {
                String::new()
            } else if prompt[..idx].ends_with("\n\n") {
                String::new()
            } else if prompt[..idx].ends_with('\n') {
                "\n".to_string()
            } else {
                "\n\n".to_string()
            };
            prompt.insert_str(idx, &format!("{prefix}{block}\n\n"));
        }
        None => {
            if !prompt.is_empty() && !prompt.ends_with("\n\n") {
                if prompt.ends_with('\n') {
                    prompt.push('\n');
                } else {
                    prompt.push_str("\n\n");
                }
            }
            prompt.push_str(block);
        }
    }
}

/// Short routing table. Per-tool contracts live in schemas.
pub fn tool_usage_guidance(advertised: &AdvertisedToolSet, has_deferred_tools: bool) -> String {
    let mut s = String::from("# Using your tools\n");
    let has_file = advertised.contains("Glob")
        || advertised.contains("Grep")
        || advertised.contains("Read")
        || advertised.contains("Edit")
        || advertised.contains("Write")
        || advertised.contains("DirTree");
    if has_file {
        s.push_str(
            " - Do NOT use Bash when a dedicated tool is available. Using dedicated tools \
allows the user to better understand and review your work:\n",
        );
        if advertised.contains("Glob") {
            s.push_str(
                "   - File listing/search: Glob on every operating system (not shell-specific listing commands such as ls, dir, Get-ChildItem, or find). When asked what files are in the current directory or workspace, use Glob with \"*\" for top-level files or \"**/*\" recursively before saying there are no files.\n",
            );
        }
        if advertised.contains("Grep") {
            s.push_str("   - Content search: Grep (not grep or rg)\n");
        }
        if advertised.contains("Read") {
            s.push_str("   - Read files: Read (not cat, head, or tail)\n");
        }
        if advertised.contains("Edit") {
            s.push_str("   - Edit files: Edit (not sed or awk)\n");
        }
        if advertised.contains("Write") {
            s.push_str(
                "   - Write files: Write (not echo redirection or cat with heredoc)\n",
            );
        }
    }
    if advertised.contains("web_extract") {
        s.push_str(
            " - Public URL reading: when `web_extract` is advertised and the user gives a public HTTP(S) URL \
and asks for its content, use `web_extract` directly, including direct PDF and JavaScript-shell \
URLs. Do not use Browser or Bash/Python/exec_command merely to read or parse that public URL. \
Use Browser for interactive or rendered actions, and shell/Python for local files or after \
`web_extract` genuinely fails. For an explicit request to download or save the original file, \
follow the appropriate file or artifact workflow instead of using `web_extract`.\n",
        );
    }
    s.push_str(
        " - You can call multiple tools in a single response. If there are no \
dependencies between them, make all independent, concurrency-safe calls in parallel. \
This reduces model round trips and latency, but it does not reduce the number of tool calls. \
If one call depends on a previous result or changes shared state, run them sequentially. \
Do not repeat an unchanged file read, identical search query, or state check when the result \
already in context is sufficient.\n",
    );
    if advertised.contains("Read") {
        s.push_str(
            " - When several already-known files need the same slice, use one Read call with file_paths \
instead of separate Read calls or a shell reader.\n",
        );
    }
    if advertised.contains("Edit") && advertised.contains("Write") {
        s.push_str(
            " - Prefer Edit over Write for modifying existing files — Edit sends only \
the diff, which is easier to review.\n",
        );
    }
    if advertised.contains("Edit") && advertised.contains("Read") {
        s.push_str(
            " - Read each unread range once before the first Edit of that range. Prefer Edit \
anchor mode: copy `line:hash` prefixes from Read/Grep output. After a successful Edit, reuse the returned \
anchors — do not re-Read covered ranges or the whole file.\n",
        );
    }
    if advertised.contains("ToolSearch") && has_deferred_tools {
        s.push_str(
            " - Some tools are deferred — only their names are visible. Before calling \
a deferred tool, call ToolSearch, wait for its result, then invoke the tool in a subsequent \
model turn after its full schema has been activated.\n",
        );
    }
    s.push_str(
        " - Treat every tool or command error as a hard checkpoint. Do not run \
dependent follow-up steps after a failure until you inspect the result and decide whether to \
retry, increase the timeout, change strategy, or verify the required state another way.",
    );
    if advertised.contains("exec_command") || advertised.contains("write_stdin") {
        s.push_str(
            " For installs, dependency downloads, builds, migrations, servers, and other long-running \
commands, choose a generous explicit timeout or, when available, use exec_command/write_stdin so you can poll \
without killing the process.",
        );
    }
    #[cfg(target_os = "windows")]
    {
        if advertised.contains("Bash") || advertised.contains("exec_command") {
            s.push_str(
                "\n - On Windows, the Bash and exec_command tools run commands through Windows PowerShell 5.1 \
when shell-only work is necessary. They do not use cmd.exe, Unix bash, or pwsh. Do not prefix the command \
with powershell.exe or pwsh. Write Windows PowerShell 5.1 syntax: `Get-ChildItem`, `Get-Content`, `Set-Location`, \
`$env:NAME`, and `;` for sequential commands. Do not assume PowerShell 7-only syntax such as `??`, `??=`, `?:`, `&&`, or `||`. \
If cmd.exe syntax is truly required, wrap it explicitly as `cmd /C \"...\"`.",
            );
        }
        if advertised.contains("Computer") {
            s.push_str(
                "\n - To open an application, URL, file, or folder on Windows, use the Computer \
tool's `launch` action when it is available — do NOT run `cmd /c start`, `Start-Process`, or \
`explorer` in Bash to launch GUI apps or URLs. `cmd /c start` mis-parses the target as a window \
title and pops a blocking \"Windows cannot find\" dialog that hangs the command.",
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_advertised_set_omits_office_surfaces() {
        let set = advertised_tools_for_session(
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
        assert!(set.contains("Read"));
        assert!(set.contains("Edit"));
        assert!(set.contains("web_extract"));
        assert!(!set.contains("Computer"));
        assert!(!set.contains("Browser"));
        assert!(!set.contains("ApplyPatch"));
        assert!(!set.contains("remember"));
        assert!(!set.contains("ToolSearch"));
    }

    #[test]
    fn guidance_omits_unadvertised_computer_launch() {
        let set = AdvertisedToolSet::from_names(["Read", "Edit", "Bash"]);
        let text = tool_usage_guidance(&set, false);
        assert!(!text.contains("Computer"));
        assert!(!text.contains("ApplyPatch"));
        assert!(!text.contains("update_plan"));
        assert!(!text.contains("exec_command script"));
        assert!(text.contains("Read"));
        assert!(text.contains("parallel"));
        assert!(text.contains("hard checkpoint"));
    }

    #[test]
    fn windows_guidance_states_powershell_51_contract() {
        let set = AdvertisedToolSet::from_names(["Bash", "exec_command"]);
        let text = tool_usage_guidance(&set, false);
        if cfg!(windows) {
            assert!(text.contains("Windows PowerShell 5.1"));
            assert!(text.contains("Do not prefix the command"));
            assert!(text.contains("PowerShell 7-only syntax"));
        } else {
            assert!(!text.contains("Windows PowerShell 5.1"));
        }
    }

    #[test]
    fn insert_before_language_keeps_directive_last() {
        let mut prompt = "intro\n\n【输出语言】请用中文".to_string();
        insert_before_language_section(&mut prompt, "# Coding mode\nbody");
        let coding = prompt.find("# Coding mode").unwrap();
        let lang = prompt.find("【输出语言】").unwrap();
        assert!(coding < lang);
    }
}
