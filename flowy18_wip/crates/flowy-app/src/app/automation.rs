//! Automation wiring: the store, worker and service behind the automation
//! page and the Worker tools. Runs report inside their own conversation, so
//! there are no separate run notifications here.

use std::sync::Arc;
use std::time::Duration;

use flowy_automation::{
    AutomationService, AutomationStore, ServiceError, Worker, WorkerConfig,
    now_ms,
};
use flowy_studio::{AutomationPage, AutomationUiAction, AutomationView};
use gpui::{Context, Task};

use super::{FlowyApp, Section};

/// Finished runs kept per automation.
const KEEP_RUNS: usize = 20;
const REFRESH: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(super) struct AutomationState {
    service: Option<Arc<AutomationService>>,
    view: AutomationView,
    feedback: Option<&'static str>,
    watch: Option<Task<()>>,
}

impl FlowyApp {
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "GPUI Context is forwarded to entity updates"
    )]
    pub(super) fn start_automation(&mut self, cx: &mut Context<'_, Self>) {
        let agent = self.studio.read(cx).agent.clone();
        let path = self
            .store
            .root()
            .join("automation")
            .join("automations.sqlite3");
        let store = match AutomationStore::open(&path) {
            Ok(store) => Arc::new(store),
            Err(error) => {
                tracing::warn!(%error, "automation store unavailable");
                return;
            }
        };
        if let Err(error) = store
            .mark_running_interrupted(now_ms())
            .and_then(|_| store.prune(KEEP_RUNS))
        {
            tracing::warn!(%error, "could not tidy automation runs");
        }
        let worker = Worker::start(
            Arc::clone(&store),
            agent.automation_executor(),
            WorkerConfig::default(),
            agent.runtime_handle(),
        );
        let service = Arc::new(AutomationService::new(store, worker));
        service.set_signed_in(self.auth.session.is_some());
        agent.set_automation_service(Some(Arc::clone(&service)));
        self.automations.service = Some(service);
        self.automations.watch = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                if weak.update(cx, Self::refresh_automation).is_err() {
                    break;
                }
            }
        }));
    }

    /// Runs only happen while signed in; signing out cancels them.
    pub(super) fn set_automation_signed_in(&self, signed_in: bool) {
        if let Some(service) = &self.automations.service {
            service.set_signed_in(signed_in);
        }
    }

    /// A deleted conversation takes its automations with it.
    pub(super) fn delete_conversation_automations(&self, conversation: &str) {
        if let Some(service) = &self.automations.service
            && let Err(error) = service.delete_conversation(conversation)
        {
            tracing::warn!(%error, "could not delete conversation automations");
        }
    }

    pub(super) fn refresh_automation(&mut self, cx: &mut Context<'_, Self>) {
        if self.section != Section::Automation {
            return;
        }
        let page = self.automation_page();
        // An empty list shows the welcome canvas.
        let page = (page.view != AutomationView::List
            || !page.automations.is_empty()
            || page.error.is_some())
        .then_some(page);
        self.studio
            .update(cx, |panel, cx| panel.set_automation_page(page, cx));
    }

    fn automation_page(&self) -> AutomationPage {
        let view = self.automations.view;
        let mut page = AutomationPage {
            view,
            feedback: self.automations.feedback,
            ..AutomationPage::default()
        };
        let Some(service) = &self.automations.service else {
            page.error = Some("auto.load_error");
            return page;
        };
        let loaded = service.list().and_then(|automations| {
            page.automations = automations;
            page.runs = match view {
                AutomationView::List => Vec::new(),
                AutomationView::Detail(id) => service.history(id, KEEP_RUNS)?,
                AutomationView::Run(run_id) => {
                    service.run(run_id)?.into_iter().collect()
                }
            };
            Ok(())
        });
        if loaded.is_err() {
            page.error = Some("auto.load_error");
        }
        page
    }

    pub(super) fn handle_automation_action(
        &mut self,
        action: &AutomationUiAction,
        cx: &mut Context<'_, Self>,
    ) {
        use AutomationUiAction as A;
        match action {
            A::Navigate(view) => {
                self.automations.view = *view;
                self.automations.feedback = None;
                self.refresh_automation(cx);
                return;
            }
            A::OpenConversation(conversation) => {
                self.push_section(Section::Studio, cx);
                self.studio.update(cx, |panel, cx| {
                    panel.open_session(conversation, cx);
                });
                return;
            }
            _ => {}
        }
        let Some(service) = self.automations.service.clone() else {
            return;
        };
        let result = match action {
            A::Save(id, input) => {
                service.update(*id, input.clone()).map(|_| "auto.saved")
            }
            A::Pause(id) => service.pause(*id).map(|_| "auto.saved"),
            A::Resume(id) => service.resume(*id).map(|_| "auto.saved"),
            A::Delete(id) => service.delete(*id).map(|()| {
                self.automations.view = AutomationView::List;
                "auto.saved"
            }),
            A::Cancel(run_id) => Ok(if service.cancel(*run_id) {
                "auto.cancelled"
            } else {
                "auto.invalid"
            }),
            A::Navigate(_) | A::OpenConversation(_) => return,
        };
        let saved = matches!(action, A::Save(..)) && result.is_ok();
        self.automations.feedback = Some(match result {
            Ok(key) => key,
            Err(ServiceError::Invalid(_) | ServiceError::Schedule(_)) => {
                "auto.invalid"
            }
            Err(
                ServiceError::Store(_)
                | ServiceError::Start(flowy_automation::StartError::Store(_)),
            ) => "auto.load_error",
            Err(ServiceError::NotFound | ServiceError::Start(_)) => {
                "auto.unavailable"
            }
        });
        if saved {
            self.studio
                .update(cx, flowy_studio::StudioPanel::close_automation_editor);
        }
        self.refresh_automation(cx);
    }
}
