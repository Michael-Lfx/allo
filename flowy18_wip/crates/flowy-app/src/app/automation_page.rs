use super::automation_approval::join_automation_ui_task;
use super::automation_host::current_caller;
use super::{AppHost, FlowyApp, Screen, Section};
use flowy_automation::{
    AutomationCommand, AutomationHandle, AutomationQuery, CallerContext,
    CommandEnvelope, CommandOutcome, PortError, QuerySnapshot, RequestKey,
    RunId, RunSnapshot,
};
use flowy_studio::{AutomationPage, AutomationUiAction, AutomationView};
use gpui::{Context, Task};
use std::time::Duration;

#[derive(Default)]
pub(super) struct AutomationPageState {
    owner: Option<CallerContext>,
    view: AutomationView,
    watch: Option<Task<()>>,
    command: Option<Task<()>>,
    command_busy: bool,
    route: Option<Task<()>>,
    route_busy: bool,
    pending_run: Option<RunId>,
    feedback: Option<Result<&'static str, PortError>>,
}

async fn query_page(
    handle: AutomationHandle,
    caller: CallerContext,
    view: AutomationView,
) -> Result<AutomationPage, PortError> {
    let mut page = AutomationPage {
        view: view.clone(),
        ..AutomationPage::default()
    };
    match view {
        AutomationView::Definitions => {
            let QuerySnapshot::Automations(items) =
                handle.query(&caller, AutomationQuery::List).await?
            else {
                return Err(PortError::Storage);
            };
            page.definitions = items;
        }
        AutomationView::Definition(automation_id) => {
            let QuerySnapshot::Automation(item) = handle
                .query(
                    &caller,
                    AutomationQuery::Automation {
                        automation_id: automation_id.clone(),
                    },
                )
                .await?
            else {
                return Err(PortError::Storage);
            };
            let QuerySnapshot::History(runs) = handle
                .query(
                    &caller,
                    AutomationQuery::RecentHistory { automation_id },
                )
                .await?
            else {
                return Err(PortError::Storage);
            };
            page.definitions.push(*item);
            page.runs = runs;
        }
        AutomationView::Inbox => {
            let QuerySnapshot::History(runs) =
                handle.query(&caller, AutomationQuery::InboxRuns).await?
            else {
                return Err(PortError::Storage);
            };
            page.runs = runs;
        }
        AutomationView::Run(run_id) => {
            page.runs.push(query_run(&handle, &caller, run_id).await?);
        }
    }
    Ok(page)
}

async fn query_run(
    handle: &AutomationHandle,
    caller: &CallerContext,
    run_id: RunId,
) -> Result<RunSnapshot, PortError> {
    let QuerySnapshot::Run(run) = handle
        .query(caller, AutomationQuery::Run { run_id })
        .await?
    else {
        return Err(PortError::Storage);
    };
    Ok(run)
}

fn caller(host: &AppHost) -> Option<CallerContext> {
    current_caller(host.automation_authority())
}

impl FlowyApp {
    pub(super) fn watch_automation_page(&mut self, cx: &Context<'_, Self>) {
        let host = self.host.clone();
        let Some(runtime) = host.automation_runtime() else {
            return;
        };
        self.automation_page.watch = Some(cx.spawn(async move |weak, cx| {
            loop {
                let Ok(request) = weak.update(cx, |app, cx| {
                    app.sync_automation_page_owner(cx);
                    app.retry_pending_automation_run(cx);
                    if app.section == Section::Automation
                        && app.screen == Screen::Main
                    {
                        app.automation_page.owner.as_ref().map(|owner| {
                            (owner.clone(), app.automation_page.view.clone())
                        })
                    } else {
                        None
                    }
                }) else {
                    break;
                };
                if let Some((owner, view)) = request {
                    let host = host.clone();
                    let (query_owner, query_view) =
                        (owner.clone(), view.clone());
                    let result = join_automation_ui_task(
                        runtime.spawn(async move {
                            query_page(
                                host.automation_handle().await?,
                                query_owner,
                                query_view,
                            )
                            .await
                        }),
                        cx.background_executor(),
                    )
                    .await
                    .unwrap_or(Err(PortError::Unavailable));
                    if weak
                        .update(cx, |app, cx| {
                            if caller(&app.host).as_ref() != Some(&owner)
                                || app.automation_page.view != view
                                || app.automation_page.owner.as_ref()
                                    != Some(&owner)
                            {
                                return;
                            }
                            let mut page =
                                result.unwrap_or_else(|error| AutomationPage {
                                    view,
                                    error: Some(error),
                                    ..AutomationPage::default()
                                });
                            page.busy = app.automation_page.command_busy;
                            match app.automation_page.feedback {
                                Some(Ok(key)) => page.feedback = Some(key),
                                Some(Err(error)) => page.error = Some(error),
                                None => {}
                            }
                            app.studio.update(cx, |panel, cx| {
                                panel.set_automation_page(Some(page), cx);
                            });
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }
        }));
    }

    pub(super) fn sync_automation_page_owner(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        let owner = caller(&self.host);
        // A short lock conflict during an ordinary query does not invalidate
        // existing results. A different account generation always does.
        let owner = owner.filter(|value| {
            self.automation_page.owner.as_ref() == Some(value)
                || self.host.automation_is_active()
        });
        if self.automation_page.owner == owner {
            return;
        }
        self.automation_page.owner = owner;
        self.automation_page.view = AutomationView::Definitions;
        self.automation_page.feedback = None;
        self.automation_page.command = None;
        self.automation_page.command_busy = false;
        self.automation_page.route = None;
        self.automation_page.route_busy = false;
        let page =
            self.automation_page.owner.as_ref().map(|_| AutomationPage {
                loading: true,
                ..AutomationPage::default()
            });
        self.studio.update(cx, |panel, cx| {
            panel.set_automation_page(None, cx);
            panel.set_automation_page(page, cx);
        });
    }

    pub(super) fn open_automation_run(
        &mut self,
        run_id: RunId,
        cx: &Context<'_, Self>,
    ) {
        self.automation_page.pending_run = Some(run_id);
        self.retry_pending_automation_run(cx);
    }

    pub(super) fn retry_pending_automation_run(
        &mut self,
        cx: &Context<'_, Self>,
    ) {
        if self.automation_page.route_busy {
            return;
        }
        let Some(run_id) = self.automation_page.pending_run.clone() else {
            return;
        };
        if caller(&self.host).is_none()
            || !self.host.automation_is_active()
            || self.host.automation_runtime().is_none()
        {
            return;
        }
        self.route_automation_run(run_id, false, cx);
    }

    fn route_automation_run(
        &mut self,
        run_id: RunId,
        open_result: bool,
        cx: &Context<'_, Self>,
    ) {
        if self.automation_page.route_busy {
            return;
        }
        let host = self.host.clone();
        let (Some(owner), Some(runtime)) =
            (caller(&host), host.automation_runtime())
        else {
            return;
        };
        let query_owner = owner.clone();
        let target_run = run_id.clone();
        let task = runtime.spawn(async move {
            query_run(&host.automation_handle().await?, &query_owner, run_id)
                .await
        });
        self.automation_page.route_busy = true;
        self.automation_page.route = Some(cx.spawn(async move |weak, cx| {
            let result =
                join_automation_ui_task(task, cx.background_executor()).await;
            let _ = weak.update(cx, |app, cx| {
                app.automation_page.route_busy = false;
                if caller(&app.host).as_ref() != Some(&owner) {
                    return;
                }
                if open_result && app.screen != Screen::Main {
                    return;
                }
                match result {
                    Ok(Ok(run)) => {
                        if open_result {
                            if app.automation_page.view
                                != AutomationView::Run(run.run_id.clone())
                            {
                                return;
                            }
                            if let Some(session) = run.result_session_id
                                && app
                                    .host
                                    .agent()
                                    .source_available(session.as_str())
                            {
                                app.push_section(Section::Studio, cx);
                                app.studio.update(cx, |panel, cx| {
                                    panel.open_session(session.as_str(), cx);
                                });
                            }
                        } else {
                            if app.automation_page.pending_run.as_ref()
                                != Some(&run.run_id)
                            {
                                return;
                            }
                            app.automation_page.pending_run = None;
                            app.sync_automation_page_owner(cx);
                            app.select_navigation(
                                "automation",
                                Section::Automation,
                                cx,
                            );
                            app.navigate_automation(
                                AutomationView::Run(run.run_id.clone()),
                                Some(run),
                                cx,
                            );
                        }
                    }
                    Ok(Err(PortError::Forbidden | PortError::NotFound)) => {
                        if app.automation_page.pending_run.as_ref()
                            == Some(&target_run)
                        {
                            app.automation_page.pending_run = None;
                        }
                    }
                    // Startup and account recovery are retried by the page watch.
                    Ok(Err(_)) | Err(_) => {}
                }
            });
        }));
    }

    fn navigate_automation(
        &mut self,
        view: AutomationView,
        run: Option<RunSnapshot>,
        cx: &mut Context<'_, Self>,
    ) {
        self.automation_page.view = view.clone();
        self.automation_page.feedback = None;
        let page = AutomationPage {
            view,
            loading: run.is_none(),
            runs: run.into_iter().collect(),
            ..AutomationPage::default()
        };
        self.studio
            .update(cx, |panel, cx| panel.set_automation_page(Some(page), cx));
    }

    pub(super) fn handle_automation_action(
        &mut self,
        action: &AutomationUiAction,
        cx: &mut Context<'_, Self>,
    ) {
        self.sync_automation_page_owner(cx);
        let Some(owner) = self.automation_page.owner.clone() else {
            return;
        };
        match action {
            AutomationUiAction::Navigate(view) => {
                self.navigate_automation(view.clone(), None, cx);
            }
            AutomationUiAction::OpenResult(run) => {
                self.route_automation_run(run.clone(), true, cx);
            }
            AutomationUiAction::Command(command) => {
                self.run_automation_command(command.clone(), owner, cx);
            }
        }
    }

    fn run_automation_command(
        &mut self,
        command: AutomationCommand,
        mut owner: CallerContext,
        cx: &Context<'_, Self>,
    ) {
        if self.automation_page.command_busy {
            return;
        }
        let host = self.host.clone();
        let Some(runtime) = host.automation_runtime() else {
            return;
        };
        let view = self.automation_page.view.clone();
        let generation = owner.clone();
        // The user can edit fields on the stored revision. The service
        // verifies the capability scope and the original grant again.
        if let AutomationCommand::UpdateManual { definition, .. } = &command
            && let Some(grant) = definition.capabilities.grant_ref.clone()
        {
            owner.grant_refs.push(grant);
        }
        self.automation_page.feedback = None;
        self.automation_page.command_busy = true;
        let task = runtime.spawn(async move {
            let now = super::automation_access::current_time_ms();
            let key =
                RequestKey::parse(format!("ui-{}", RunId::generate().as_str()))
                    .map_err(|_| PortError::InvalidRequest)?;
            let envelope = CommandEnvelope::new(
                key,
                now,
                now.saturating_add(86_400_000),
                command,
            )?;
            host.automation_handle()
                .await?
                .command(&owner, envelope)
                .await
        });
        self.automation_page.command = Some(cx.spawn(async move |weak, cx| {
            let result =
                join_automation_ui_task(task, cx.background_executor())
                    .await
                    .unwrap_or(Err(PortError::Unavailable));
            let _ = weak.update(cx, |app, cx| {
                if caller(&app.host).as_ref() != Some(&generation) {
                    return;
                }
                app.automation_page.command_busy = false;
                if app.automation_page.view != view {
                    return;
                }
                match result {
                    Ok(receipt) => match receipt.outcome {
                        CommandOutcome::RunQueued { run_id } => {
                            app.navigate_automation(
                                AutomationView::Run(run_id),
                                None,
                                cx,
                            );
                        }
                        CommandOutcome::Updated { .. } => {
                            app.studio.update(cx, |panel, cx| {
                                panel.set_automation_page(None, cx);
                            });
                            app.navigate_automation(view, None, cx);
                        }
                        CommandOutcome::Rejected => {
                            app.automation_page.feedback =
                                Some(Err(PortError::Forbidden));
                        }
                        CommandOutcome::PendingApproval => {
                            app.automation_page.feedback =
                                Some(Ok("auto.blocked_approval"));
                        }
                        _ => {
                            app.automation_page.feedback =
                                Some(Ok("auto.saved"));
                        }
                    },
                    Err(error) => {
                        app.automation_page.feedback = Some(Err(error));
                    }
                }
                cx.notify();
            });
        }));
    }
}
