use super::gallery;
use super::invite;
use super::more;
use super::settings;
use crate::prefs::FlexProvider;
use crate::session::Session;
use gpui::{Entity, Task, WindowAppearance};
use gpui_ce_elements::editable_text::EditableTextState;

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
    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Privacy => "login.legal_privacy",
            Self::Terms => "login.legal_terms",
        }
    }

    #[must_use]
    pub const fn body(self) -> &'static str {
        match self {
            Self::Privacy => include_str!("../../res/legal/privacy.txt"),
            Self::Terms => include_str!("../../res/legal/terms.txt"),
        }
    }
}

pub struct AuthState {
    pub session: Option<Session>,
    pub email: Entity<EditableTextState>,
    pub code: Entity<EditableTextState>,
    pub step: LoginStep,
    pub phase: LoginPhase,
    pub message: Option<&'static str>,
    pub pending_req_no: Option<String>,
    pub attempted_code: String,
    pub cooldown_left: u64,
    pub generation: u64,
    pub task: Option<Task<()>>,
    pub cooldown_task: Option<Task<()>>,
    pub legal: Option<LegalDoc>,
}

pub struct GalleryState {
    pub page: usize,
    pub ideas_open: bool,
    pub idea_filter: gallery::Filter,
    pub idea_id: Option<&'static str>,
    pub idea_note: bool,
}

pub struct SettingsState {
    pub open: bool,
    pub page: settings::Page,
    pub picker: settings::Picker,
    pub query: Entity<EditableTextState>,
}

pub struct InviteState {
    pub open: bool,
    pub panel: invite::Panel,
    pub status: Option<&'static str>,
    pub code: String,
    pub inbound_code: Option<String>,
    pub emails: Vec<String>,
    pub email: Entity<EditableTextState>,
}

pub struct PersonaState {
    pub menu: bool,
    pub page: more::Page,
    pub tab: more::Tab,
    pub dialog: more::Dialog,
    pub status: Option<&'static str>,
    pub knowledge: Vec<crate::persona::Knowledge>,
    pub nickname: Entity<EditableTextState>,
    pub occupation: Entity<EditableTextState>,
    pub about: Entity<EditableTextState>,
    pub instructions: Entity<EditableTextState>,
    pub import_paste: Entity<EditableTextState>,
    pub knowledge_name: Entity<EditableTextState>,
    pub knowledge_usage: Entity<EditableTextState>,
    pub knowledge_body: Entity<EditableTextState>,
}

pub struct AppearanceChrome {
    pub known: bool,
    pub forced: Option<WindowAppearance>,
    pub font_size_field: Entity<EditableTextState>,
}

pub struct FlexState {
    pub open: bool,
    pub provider_menu: bool,
    pub draft_provider: Option<FlexProvider>,
    /// Field contents (plaintext when visible, `*` mask when hidden).
    pub api_key: Entity<EditableTextState>,
    /// Real API key while the field shows a mask.
    pub secret: String,
    pub key_visible: bool,
}
