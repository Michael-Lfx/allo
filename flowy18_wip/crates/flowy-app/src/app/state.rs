use super::gallery;
use super::invite;
use super::more;
use super::settings;
use super::skills;
use crate::prefs::FlexProvider;
use crate::session::Session;
use gpui::{Entity, ScrollHandle, Task, WindowAppearance};
use gpui_kit::base::input::{InputState, TextareaState};
use std::cell::Cell;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginStep {
    Email,
    Otp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPhase {
    Idle,
    Sending,
    Verifying,
    Success,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegalDoc {
    Privacy,
    Terms,
}

impl LegalDoc {
    #[must_use]
    pub(super) const fn title_key(self) -> &'static str {
        match self {
            Self::Privacy => "login.legal_privacy",
            Self::Terms => "login.legal_terms",
        }
    }

    #[must_use]
    pub(super) const fn body(self) -> &'static str {
        match self {
            Self::Privacy => include_str!("../../res/legal/privacy.txt"),
            Self::Terms => include_str!("../../res/legal/terms.txt"),
        }
    }
}

pub(super) struct AuthState {
    pub(super) session: Option<Session>,
    pub(super) email: Entity<InputState>,
    pub(super) code: Entity<InputState>,
    pub(super) step: LoginStep,
    pub(super) phase: LoginPhase,
    pub(super) message: Option<&'static str>,
    pub(super) pending_req_no: Option<String>,
    pub(super) attempted_code: String,
    pub(super) cooldown_left: u64,
    pub(super) generation: u64,
    pub(super) task: Option<Task<()>>,
    pub(super) cooldown_task: Option<Task<()>>,
}

pub(super) struct GalleryState {
    pub(super) page: usize,
    pub(super) idea_filter: gallery::Filter,
    pub(super) idea_note: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum MySkillsTab {
    #[default]
    Installed,
    Saved,
}

pub(super) struct SkillsState {
    /// Selected market category slug. `None` is the trending shelf.
    pub(super) category: Option<String>,
    pub(super) query: Entity<InputState>,
    pub(super) cards: Vec<skills::ShelfCard>,
    pub(super) cursor: Option<String>,
    pub(super) loading: bool,
    pub(super) unavailable: bool,
    pub(super) exhausted: bool,
    pub(super) generation: u64,
    pub(super) task: Option<Task<()>>,
    pub(super) search_task: Option<Task<()>>,
    pub(super) opened: Option<skills::ShelfCard>,
    pub(super) scroll: ScrollHandle,
    pub(super) auto_pages: u8,
    pub(super) follow_armed: Cell<bool>,
    pub(super) empty_follows: u8,
    pub(super) follow_mark: usize,
    pub(super) installed: Vec<flowy_agent::InstalledSkill>,
    pub(super) mine_query: Entity<InputState>,
    pub(super) mine_tab: MySkillsTab,
    pub(super) mine_selected: Option<(String, String)>,
    pub(super) install: Option<skills::InstallUi>,
    pub(super) install_task: Option<Task<()>>,
    pub(super) script_confirm: Option<flowy_skills::SkillId>,
}

pub struct SettingsState {
    pub(super) page: settings::Page,
    pub(super) picker: settings::Picker,
    pub(super) query: Entity<InputState>,
}

pub struct ProjectCreateState {
    pub(super) name: Entity<InputState>,
    pub(super) folder: Option<PathBuf>,
    pub(super) status: Option<&'static str>,
    pub(super) title_key: &'static str,
    pub(super) picker_generation: u64,
    pub(super) editing_project_id: Option<String>,
    pub(super) deleting_project_id: Option<String>,
}

pub struct InviteState {
    pub(super) panel: invite::Panel,
    pub(super) status: Option<&'static str>,
    pub(super) code: String,
    pub(super) inbound_code: Option<String>,
    pub(super) emails: Vec<String>,
    pub(super) email: Entity<InputState>,
}

pub struct PersonaState {
    pub(super) tab: more::Tab,
    pub(super) status: Option<&'static str>,
    pub(super) knowledge: Vec<crate::persona::Knowledge>,
    pub(super) nickname: Entity<InputState>,
    pub(super) occupation: Entity<InputState>,
    pub(super) about: Entity<TextareaState>,
    pub(super) instructions: Entity<TextareaState>,
    pub(super) import_paste: Entity<TextareaState>,
    pub(super) knowledge_name: Entity<InputState>,
    pub(super) knowledge_usage: Entity<InputState>,
    pub(super) knowledge_body: Entity<TextareaState>,
}

pub struct AppearanceChrome {
    pub(super) known: bool,
    pub(super) forced: Option<WindowAppearance>,
    pub(super) font_size_field: Entity<InputState>,
}

pub struct FlexState {
    pub(super) provider_menu: bool,
    pub(super) draft_provider: Option<FlexProvider>,
    /// Field contents (plaintext when visible, `*` mask when hidden).
    pub(super) api_key: Entity<InputState>,
    /// Real API key while the field shows a mask.
    pub(super) secret: String,
    pub(super) key_visible: bool,
}

/// The sole business owner of the window's main dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ActiveDialog {
    #[default]
    None,
    Settings,
    Ideas,
    Idea(&'static str),
    Skill(&'static str),
    SkillUnlock(&'static str),
    SkillDocs(&'static str),
    SkillScriptConfirm,
    RemoteSkill,
    Invite,
    ProjectCreate,
    ProjectDelete,
    ArtifactPreview,
    ConversationDelete,
    ConversationRename,
    QuickSearch,
    ArtifactCleanup,
    Flex,
    Import,
    Knowledge,
    Legal(LegalDoc),
}

impl ActiveDialog {
    pub(super) const fn scope(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Settings => Some("settings"),
            Self::Ideas => Some("ideas"),
            Self::Idea(_) => Some("idea"),
            Self::Skill(_) => Some("skill"),
            Self::SkillUnlock(_) => Some("skill-unlock"),
            Self::SkillDocs(_) => Some("skill-docs"),
            Self::SkillScriptConfirm => Some("skill-script-confirm"),
            Self::RemoteSkill => Some("skill-shelf"),
            Self::Invite => Some("invite"),
            Self::ProjectCreate => Some("project-create"),
            Self::ProjectDelete => Some("project-delete"),
            Self::ArtifactPreview => Some("artifact-preview"),
            Self::ConversationDelete => Some("conversation-delete"),
            Self::ConversationRename => Some("conversation-rename"),
            Self::QuickSearch => Some("quick-search"),
            Self::ArtifactCleanup => Some("artifact-cleanup"),
            Self::Flex => Some("flex"),
            Self::Import => Some("import"),
            Self::Knowledge => Some("knowledge"),
            Self::Legal(_) => Some("legal"),
        }
    }
    pub(super) const fn idea(self) -> Option<&'static str> {
        if let Self::Idea(id) = self {
            Some(id)
        } else {
            None
        }
    }
    pub(super) const fn skill(self) -> Option<&'static str> {
        match self {
            Self::Skill(id) | Self::SkillUnlock(id) | Self::SkillDocs(id) => {
                Some(id)
            }
            _ => None,
        }
    }
}
