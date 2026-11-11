use super::gallery;
use super::invite;
use super::more;
use super::settings;
use crate::agent::AgentHandle;
use crate::session::Session;
use flowy_core::DocumentSession;
use gpui::{Entity, Task, WindowAppearance};
use gpui_ce_elements::editable_text::EditableTextState;

pub(super) struct AuthState {
    pub session: Option<Session>,
    pub account_exists: bool,
    pub email: Entity<EditableTextState>,
    pub password: Entity<EditableTextState>,
    pub password_visible: bool,
    pub login_error: Option<&'static str>,
}

pub(super) struct StudioState {
    pub prompt: Entity<EditableTextState>,
    pub projects: Vec<usize>,
    pub tasks: Vec<String>,
    pub thread: Option<crate::chat::Thread>,
    pub stream: Option<crate::chat::MockStream>,
    pub stream_task: Option<Task<()>>,
    pub confirm_rights: bool,
    pub confirm_terms: bool,
    pub document: DocumentSession,
    pub agent: AgentHandle,
    pub credits: u32,
    pub credits_hover: bool,
    pub credits_close: Option<Task<()>>,
    pub model_info_open: bool,
    pub model_menu_open: bool,
}

pub(super) struct GalleryState {
    pub page: usize,
    pub ideas_open: bool,
    pub idea_filter: gallery::Filter,
    pub idea_id: Option<&'static str>,
    pub idea_note: bool,
}

pub(super) struct SettingsState {
    pub open: bool,
    pub page: settings::Page,
    pub picker: settings::Picker,
    pub query: Entity<EditableTextState>,
}

pub(super) struct InviteState {
    pub open: bool,
    pub panel: invite::Panel,
    pub status: Option<&'static str>,
    pub code: String,
    pub inbound_code: Option<String>,
    pub emails: Vec<String>,
    pub email: Entity<EditableTextState>,
}

pub(super) struct PersonaState {
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

pub(super) struct AppearanceChrome {
    pub known: bool,
    pub forced: Option<WindowAppearance>,
    pub font_size_field: Entity<EditableTextState>,
}
