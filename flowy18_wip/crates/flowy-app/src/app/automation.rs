//! Automation wiring: the store, worker and service behind the automation
//! page and the model tools, plus run notifications.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use flowy_automation::{
    Automation, AutomationService, AutomationStore, Run, RunId, RunListener,
    ServiceError, Worker, WorkerConfig, now_ms,
};
use flowy_studio::{AutomationPage, AutomationUiAction, AutomationView};
use gpui::{Context, Task};

use super::{FlowyApp, Section};

/// Finished runs kept per automation.
const KEEP_RUNS: usize = 20;
const REFRESH: Duration = Duration::from_millis(500);

/// Runs that ended on the worker, waiting to become system notifications.
type Finished = Arc<Mutex<Vec<(String, RunId)>>>;

#[derive(Default)]
pub(super) struct AutomationState {
    service: Option<Arc<AutomationService>>,
    finished: Finished,
    view: AutomationView,
    feedback: Option<&'static str>,
    watch: Option<Task<()>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
        match store.run_session_ids() {
            Ok(ids) => agent.mark_automation_sessions(ids),
            Err(error) => {
                tracing::warn!(%error, "could not read automation sessions");
                return;
            }
        }
        let finished = Arc::clone(&self.automations.finished);
        let on_finished: RunListener =
            Arc::new(move |automation: &Automation, run: &Run| {
                lock(&finished).push((automation.title.clone(), run.id));
            });
        let worker = Worker::start(
            Arc::clone(&store),
            agent.automation_executor(),
            on_finished,
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

    pub(super) fn open_automation_run(
        &mut self,
        run_id: RunId,
        cx: &mut Context<'_, Self>,
    ) {
        self.automations.view = AutomationView::Run(run_id);
        self.automations.feedback = None;
        if self.section != Section::Automation {
            self.push_section(Section::Automation, cx);
        }
        self.refresh_automation(cx);
    }

    pub(super) fn refresh_automation(&mut self, cx: &mut Context<'_, Self>) {
        self.notify_finished_runs(cx);
        if self.section != Section::Automation {
            return;
        }
        let page = self.automation_page();
        // An empty list shows the welcome canvas with the draft composer.
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

    fn notify_finished_runs(&self, cx: &Context<'_, Self>) {
        let finished = std::mem::take(&mut *lock(&self.automations.finished));
        if finished.is_empty() {
            return;
        }
        let title = self.t("auto.notification_title");
        let open = self.t("auto.view_run");
        for (name, run_id) in finished {
            flowy_shell::show_automation_notification(
                cx,
                &run_id.to_string(),
                &title,
                &name,
                &open,
            );
        }
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
            A::OpenSession(session_id) => {
                self.push_section(Section::Studio, cx);
                self.studio
                    .update(cx, |panel, cx| panel.open_session(session_id, cx));
                return;
            }
            _ => {}
        }
        let Some(service) = self.automations.service.clone() else {
            return;
        };
        let result = match action {
            A::Save(Some(id), input) => {
                service.update(*id, input.clone()).map(|_| "auto.saved")
            }
            A::Save(None, input) => service.create(input.clone()).map(|item| {
                self.automations.view = AutomationView::Detail(item.id);
                "auto.saved"
            }),
            A::Pause(id) => service.pause(*id).map(|_| "auto.saved"),
            A::Resume(id) => service.resume(*id).map(|_| "auto.saved"),
            A::Delete(id) => service.delete(*id).map(|()| {
                self.automations.view = AutomationView::List;
                "auto.saved"
            }),
            A::RunNow(id) => service.run_now(*id).map(|run| {
                self.automations.view = AutomationView::Run(run.id);
                "auto.queued"
            }),
            A::Cancel(run_id) => {
                service.cancel(*run_id).map(|_| "auto.cancelled")
            }
            A::Navigate(_) | A::OpenSession(_) => return,
        };
        let saved = matches!(action, A::Save(..)) && result.is_ok();
        self.automations.feedback = Some(match result {
            Ok(key) => key,
            Err(ServiceError::NotFound) => "auto.unavailable",
            Err(ServiceError::Invalid(_) | ServiceError::Schedule(_)) => {
                "auto.invalid"
            }
            Err(ServiceError::Store(_)) => "auto.load_error",
        });
        if saved {
            self.studio
                .update(cx, flowy_studio::StudioPanel::close_automation_editor);
        }
        self.refresh_automation(cx);
    }
}
