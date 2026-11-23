use super::gallery;
use super::invite;
use super::more;
use super::settings;
use super::skills;
use crate::prefs::FlexProvider;
use crate::session::Session;
use gpui::{Entity, Task, WindowAppearance};
use gpui_ce_elements::editable_text::EditableTextState;
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
    pub(super) email: Entity<EditableTextState>,
    pub(super) code: Entity<EditableTextState>,
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

pub(super) struct SkillsState {
    pub(super) filter: skills::Category,
    pub(super) query: Entity<EditableTextState>,
}

pub struct SettingsState {
    pub(super) page: settings::Page,
    pub(super) picker: settings::Picker,
    pub(super) query: Entity<EditableTextState>,
}

pub struct ProjectCreateState {
    pub(super) name: Entity<EditableTextState>,
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
    pub(super) email: Entity<EditableTextState>,
}

pub struct PersonaState {
    pub(super) page: more::Page,
    pub(super) tab: more::Tab,
    pub(super) status: Option<&'static str>,
    pub(super) knowledge: Vec<crate::persona::Knowledge>,
    pub(super) nickname: Entity<EditableTextState>,
    pub(super) occupation: Entity<EditableTextState>,
    pub(super) about: Entity<EditableTextState>,
    pub(super) instructions: Entity<EditableTextState>,
    pub(super) import_paste: Entity<EditableTextState>,
    pub(super) knowledge_name: Entity<EditableTextState>,
    pub(super) knowledge_usage: Entity<EditableTextState>,
    pub(super) knowledge_body: Entity<EditableTextState>,
}

pub struct AppearanceChrome {
    pub(super) known: bool,
    pub(super) forced: Option<WindowAppearance>,
    pub(super) font_size_field: Entity<EditableTextState>,
}

pub struct FlexState {
    pub(super) provider_menu: bool,
    pub(super) draft_provider: Option<FlexProvider>,
    /// Field contents (plaintext when visible, `*` mask when hidden).
    pub(super) api_key: Entity<EditableTextState>,
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
    Invite,
    ProjectCreate,
    ProjectDelete,
    ArtifactPreview,
    ConversationDelete,
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
            Self::Invite => Some("invite"),
            Self::ProjectCreate => Some("project-create"),
            Self::ProjectDelete => Some("project-delete"),
            Self::ArtifactPreview => Some("artifact-preview"),
            Self::ConversationDelete => Some("conversation-delete"),
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
