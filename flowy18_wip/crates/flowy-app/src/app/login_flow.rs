//! Email OTP authentication controller, invite deep-link handoff, and sign-out cleanup.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flowy_auth::{
    AuthClient, AuthError, EMAIL_OTP_COOLDOWN_SECS, EMAIL_OTP_LENGTH,
    normalize_otp,
};
use gpui::Context;

use super::state::{self, LegalDoc, LoginPhase, LoginStep};
use super::{FlowyApp, Screen, Section};

impl FlowyApp {
    pub fn receive_invite_code(
        &mut self,
        code: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.invite.inbound_code = Some(code);
        if self.screen == Screen::Main {
            self.open_invite_with_status(Some("invite.deep_link_received"), cx);
        } else {
            self.invite.pending_open = true;
            self.invite.status = Some("invite.deep_link_received");
            cx.notify();
        }
    }

    pub(super) fn flush_pending_invite(&mut self, cx: &mut Context<'_, Self>) {
        if self.screen == Screen::Main && self.invite.pending_open {
            self.open_invite_with_status(Some("invite.deep_link_received"), cx);
        }
    }

    pub(super) fn submit_login(&mut self, cx: &mut Context<'_, Self>) {
        match self.auth.step {
            LoginStep::Email => self.send_login_code(cx),
            LoginStep::Otp => self.verify_login_code(cx),
        }
    }

    pub(super) fn send_login_code(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying | LoginPhase::Success
        ) || self.auth.cooldown_left > 0
        {
            return;
        }
        let email = self.auth.email.read(cx).value().to_string();
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.phase = LoginPhase::Sending;
        self.auth.message = None;
        self.auth.attempted_code.clear();
        self.set_input_text(&self.auth.code, "", cx);
        cx.notify();

        let client = AuthClient::from_env();
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.send_email_code(&email);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.auth.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(result) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.auth.generation != generation {
                        return;
                    }
                    match result {
                        Ok(req_no) => {
                            app.auth.pending_req_no = Some(req_no);
                            app.auth.step = LoginStep::Otp;
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some("login.code_sent");
                            app.start_login_cooldown(cx);
                        }
                        Err(error) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some(error.key());
                        }
                    }
                    cx.notify();
                });
                break;
            }
        }));
    }

    fn verify_login_code(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying | LoginPhase::Success
        ) {
            return;
        }
        let Some(req_no) = self.auth.pending_req_no.clone() else {
            self.auth.message = Some("login.send_first");
            cx.notify();
            return;
        };
        let email = self.auth.email.read(cx).value().to_string();
        let code = normalize_otp(self.auth.code.read(cx).value().as_ref());
        if code.len() != EMAIL_OTP_LENGTH {
            self.auth.message = Some(AuthError::OtpRequired.key());
            cx.notify();
            return;
        }
        self.auth.attempted_code.clone_from(&code);
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.phase = LoginPhase::Verifying;
        self.auth.message = Some("login.verifying");
        cx.notify();

        let client = AuthClient::from_env();
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.complete_login(&email, &code, &req_no);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.auth.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(result) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.auth.generation != generation {
                        return;
                    }
                    match result {
                        Ok(session) => {
                            if let Err(error) = app.store.save(&session) {
                                app.auth.phase = LoginPhase::Idle;
                                app.auth.message = Some(error.key());
                                cx.notify();
                                return;
                            }
                            app.auth.attempted_code.clear();
                            app.set_input_text(&app.auth.code, "", cx);
                            app.auth.session = Some(session);
                            app.set_automation_signed_in(true);
                            app.auth.phase = LoginPhase::Success;
                            app.auth.message = Some("login.success");
                            app.auth.pending_req_no = None;
                            app.auth.cooldown_left = 0;
                            app.notice = None;
                            app.screen = Screen::Main;
                            app.refresh_models(cx);
                            app.refresh_credits(cx);
                            app.studio.update(cx, |panel, cx| {
                                panel.dismiss_overlay("model-info", cx);
                            });
                            app.flush_pending_invite(cx);
                        }
                        Err(AuthError::InvalidCode) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.attempted_code.clear();
                            app.set_input_text(&app.auth.code, "", cx);
                            app.auth.message =
                                Some(AuthError::InvalidCode.key());
                        }
                        Err(AuthError::SessionExpired) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.pending_req_no = None;
                            app.auth.attempted_code.clear();
                            app.set_input_text(&app.auth.code, "", cx);
                            app.auth.cooldown_left = 0;
                            app.auth.message =
                                Some(AuthError::SessionExpired.key());
                        }
                        Err(error) => {
                            app.auth.phase = LoginPhase::Idle;
                            app.auth.message = Some(error.key());
                        }
                    }
                    cx.notify();
                });
                break;
            }
        }));
    }

    pub(super) fn on_otp_changed(&mut self, cx: &mut Context<'_, Self>) {
        if self.screen != Screen::Login || self.auth.step != LoginStep::Otp {
            cx.notify();
            return;
        }
        let raw = self.auth.code.read(cx).value().to_string();
        let normalized = normalize_otp(&raw);
        if normalized != raw {
            self.set_input_text(&self.auth.code, normalized.as_str(), cx);
        }
        cx.notify();
        if normalized.len() == EMAIL_OTP_LENGTH
            && normalized != self.auth.attempted_code
            && self.auth.phase == LoginPhase::Idle
        {
            self.verify_login_code(cx);
        }
    }

    pub(super) fn reset_login_to_email(&mut self, cx: &mut Context<'_, Self>) {
        self.auth.generation = self.auth.generation.saturating_add(1);
        self.auth.step = LoginStep::Email;
        self.auth.phase = LoginPhase::Idle;
        self.auth.message = None;
        self.auth.pending_req_no = None;
        self.auth.attempted_code.clear();
        self.auth.cooldown_left = 0;
        self.auth.task = None;
        self.auth.cooldown_task = None;
        self.set_input_text(&self.auth.code, "", cx);
        cx.notify();
    }

    pub(super) fn open_legal(
        &mut self,
        doc: LegalDoc,
        cx: &mut Context<'_, Self>,
    ) {
        self.activate_dialog(state::ActiveDialog::Legal(doc), cx);
        cx.notify();
    }

    pub(super) fn close_legal(&mut self, cx: &mut Context<'_, Self>) {
        self.active_dialog = state::ActiveDialog::None;
        cx.notify();
    }

    pub(super) fn run_login_diagnose(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.auth.phase,
            LoginPhase::Sending | LoginPhase::Verifying
        ) {
            return;
        }
        self.auth.generation = self.auth.generation.saturating_add(1);
        let generation = self.auth.generation;
        self.auth.message = Some("login.diagnose_running");
        cx.notify();
        let client = AuthClient::from_env();
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let ok = client.diagnose_network();
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(ok);
            }
        });
        self.auth.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(ok) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.auth.generation != generation {
                        return;
                    }
                    app.auth.message = Some(if ok {
                        "login.diagnose_ok"
                    } else {
                        "login.diagnose_failed"
                    });
                    cx.notify();
                });
                break;
            }
        }));
    }

    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI Context is forwarded to entity updates and task spawning"
    )]
    fn start_login_cooldown(&mut self, cx: &mut Context<'_, Self>) {
        self.auth.cooldown_left = EMAIL_OTP_COOLDOWN_SECS;
        self.auth.cooldown_task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let cont = weak
                    .update(cx, |app, cx| {
                        if app.auth.cooldown_left == 0 {
                            return false;
                        }
                        app.auth.cooldown_left =
                            app.auth.cooldown_left.saturating_sub(1);
                        cx.notify();
                        app.auth.cooldown_left > 0
                    })
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        }));
    }

    pub(super) fn sign_out(&mut self, cx: &mut Context<'_, Self>) {
        self.catalog_generation = self.catalog_generation.wrapping_add(1);
        self.catalog_task = None;
        self.catalog_loading = false;
        self.effective_tier = 0;
        self.applied_tier = None;
        self.credits_generation = self.credits_generation.wrapping_add(1);
        self.credits_task = None;
        self.studio.update(cx, |panel, cx| {
            panel.prepare_for_sign_out(Self::disconnected_config(), cx);
        });
        self.store.clear();
        self.set_input_text(&self.auth.email, "", cx);
        self.set_input_text(&self.auth.code, "", cx);
        self.auth.session = None;
        self.set_automation_signed_in(false);
        self.auth.step = LoginStep::Email;
        self.auth.phase = LoginPhase::Idle;
        self.auth.message = None;
        self.auth.pending_req_no = None;
        self.auth.attempted_code.clear();
        self.auth.cooldown_left = 0;
        self.auth.task = None;
        self.auth.cooldown_task = None;
        self.active_dialog = state::ActiveDialog::None;
        self.invite.status = None;
        self.invite.pending_open = false;
        self.invite.inbound_code = None;
        self.screen = Screen::Login;
        self.section = Section::Studio;
        self.dock_visible = true;
        self.nav_back.clear();
        self.nav_forward.clear();
        self.persona.status = None;
        self.user_menu = false;
        self.archive_undo.clear();
        self.notice = None;
        self.studio.update(cx, |panel, cx| {
            panel.reset_session(cx);
            panel.set_dock_visible(true, cx);
        });
        self.gallery.page = 0;
        self.gallery.idea_note = false;
        self.skills.category = None;
        self.skills.cards.clear();
        self.sync_studio_skills(cx);
        self.skills.loading = false;
        self.skills.unavailable = false;
        self.skills.exhausted = false;
        self.skills.generation = self.skills.generation.wrapping_add(1);
        self.skills.task = None;
        self.skills.search_task = None;
        self.skills.opened = None;
        self.ota.reset();
        cx.notify();
    }
}
