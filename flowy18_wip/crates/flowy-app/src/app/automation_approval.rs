use super::{AppHost, FlowyApp};
use flowy_automation::store::ApprovalProposal;
use flowy_automation::{
    ActionSpec, AutomationCommand, CallerContext, CallerSource,
    CommandEnvelope, PortError,
};
use flowy_studio::AutomationApprovalCard;
use gpui::{Context, Task};
use std::time::Duration;

#[derive(Default)]
pub(super) struct AutomationApprovals {
    task: Option<Task<()>>,
    owner: Option<CallerContext>,
    cards: Vec<ApprovalProposal>,
    resolving: Option<String>,
}

fn caller(host: &AppHost) -> Option<CallerContext> {
    let (principal, account_generation) = host.automation_account_binding()?;
    Some(CallerContext {
        principal,
        account_generation,
        source: CallerSource::UserInterface,
        grant_refs: vec![],
    })
}

fn card_id(caller: &CallerContext, proposal: &ApprovalProposal) -> String {
    let generation = &caller.account_generation;
    format!(
        "{}:{}:{}",
        generation.epoch.as_str(),
        generation.sequence,
        proposal.envelope.request_key.as_str()
    )
}

// Poll completion with the GPUI scheduler so a Tokio worker never wakes a
// GPUI foreground task directly (including under its deterministic test executor).
pub(super) async fn join_automation_ui_task<T>(
    task: tokio::task::JoinHandle<T>,
    background: &gpui::BackgroundExecutor,
) -> Result<T, tokio::task::JoinError> {
    while !task.is_finished() {
        background.timer(Duration::from_millis(20)).await;
    }
    task.await
}

const fn weekday_key(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "auto.weekday_mon",
        chrono::Weekday::Tue => "auto.weekday_tue",
        chrono::Weekday::Wed => "auto.weekday_wed",
        chrono::Weekday::Thu => "auto.weekday_thu",
        chrono::Weekday::Fri => "auto.weekday_fri",
        chrono::Weekday::Sat => "auto.weekday_sat",
        chrono::Weekday::Sun => "auto.weekday_sun",
    }
}

impl FlowyApp {
    pub(super) fn watch_automation_approvals(
        &mut self,
        cx: &Context<'_, Self>,
    ) {
        let host = self.host.clone();
        let Some(runtime) = host.automation_runtime() else {
            return;
        };
        self.automation_approvals.task =
            Some(cx.spawn(async move |weak, cx| {
                loop {
                    let owner = caller(&host);
                    let cards = if let Some(owner) = &owner
                        && host.automation_is_active()
                    {
                        let host = host.clone();
                        let owner = owner.clone();
                        join_automation_ui_task(
                            runtime.spawn(async move {
                                host.approval_cards(&owner).await
                            }),
                            cx.background_executor(),
                        )
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or_default()
                    } else {
                        vec![]
                    };
                    if weak
                        .update(cx, |app, cx| {
                            if owner != app.automation_approvals.owner {
                                app.automation_approvals.resolving = None;
                            }
                            if owner == caller(&app.host) {
                                app.automation_approvals.owner = owner;
                                app.automation_approvals.cards = cards;
                            } else {
                                app.automation_approvals.owner = None;
                                app.automation_approvals.cards.clear();
                            }
                            app.sync_automation_approvals(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(600))
                        .await;
                }
            }));
    }

    pub(super) fn sync_automation_approvals(&self, cx: &mut Context<'_, Self>) {
        let cards = self
            .automation_approvals
            .owner
            .as_ref()
            .filter(|owner| Some(*owner) == caller(&self.host).as_ref())
            .into_iter()
            .flat_map(|owner| {
                self.automation_approvals
                    .cards
                    .iter()
                    .map(move |item| (owner, item))
            })
            .map(|(owner, item)| {
                let id = card_id(owner, item);
                AutomationApprovalCard {
                    busy: self.automation_approvals.resolving.as_ref()
                        == Some(&id),
                    id,
                    session_id: item.session_id.as_str().into(),
                    title: item.title.expose().into(),
                    details: self.approval_details(&item.envelope, item),
                }
            })
            .collect();
        self.studio
            .update(cx, |panel, cx| panel.set_automation_approvals(cards, cx));
    }

    fn approval_details(
        &self,
        envelope: &CommandEnvelope,
        proposal: &ApprovalProposal,
    ) -> String {
        let mut details = match &envelope.command {
            AutomationCommand::CreateManual { definition }
            | AutomationCommand::UpdateManual { definition, .. } => {
                let action = match &definition.action {
                    ActionSpec::AgentTask { prompt } => prompt,
                    ActionSpec::Reminder { text } => text,
                };
                vec![action.expose().to_owned()]
            }
            AutomationCommand::RunNow { .. } => vec![self.t("auto.run_now")],
            _ => return String::new(),
        };
        details.extend(proposal.scope.tools.iter().cloned());
        details.extend(proposal.scope.resources.iter().cloned());
        let (AutomationCommand::CreateManual { definition }
        | AutomationCommand::UpdateManual { definition, .. }) =
            &envelope.command
        else {
            return details.join("\n");
        };
        if let Some(budget) = &definition.budget {
            details.push(
                self.i18n.tf(
                    "auto.approval_budget",
                    &[
                        (
                            "seconds",
                            &(budget.max_execution_ms.get() as f64 / 1_000.)
                                .to_string(),
                        ),
                        ("model", &budget.max_model_calls.to_string()),
                        ("tools", &budget.max_tool_calls.to_string()),
                    ],
                ),
            );
        }
        if let Some(schedule) = definition.schedule {
            use flowy_automation::schedule::{CalendarFrequency, TriggerSpec};
            let text = match schedule.trigger {
                TriggerSpec::Manual => String::new(),
                TriggerSpec::Once { scheduled_at_ms } => {
                    chrono::DateTime::from_timestamp_millis(scheduled_at_ms)
                        .map_or_else(String::new, |t| t.to_rfc3339())
                }
                TriggerSpec::Calendar(rule) => {
                    let time =
                        format!("{} {}", rule.local_time, rule.time_zone);
                    match rule.frequency {
                        CalendarFrequency::Daily => self
                            .i18n
                            .tf("auto.approval_daily", &[("time", &time)]),
                        CalendarFrequency::Weekly { weekday } => {
                            let day = self.t(weekday_key(weekday));
                            self.i18n.tf(
                                "auto.approval_weekly",
                                &[("day", day.as_str()), ("time", &time)],
                            )
                        }
                    }
                }
            };
            if !text.is_empty() {
                details.push(text);
            }
        }
        details.join("\n")
    }

    pub(super) fn decide_automation_approval(
        &mut self,
        id: &str,
        approve: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if self.automation_approvals.resolving.is_some() {
            return;
        }
        let Some(owner) = self
            .automation_approvals
            .owner
            .clone()
            .filter(|owner| Some(owner) == caller(&self.host).as_ref())
        else {
            return;
        };
        let Some(proposal) = self
            .automation_approvals
            .cards
            .iter()
            .find(|p| card_id(&owner, p) == id)
            .cloned()
        else {
            return;
        };
        let Some(runtime) = self.host.automation_runtime() else {
            return;
        };
        self.automation_approvals.resolving = Some(id.to_owned());
        self.sync_automation_approvals(cx);
        let host = self.host.clone();
        let id = id.to_owned();
        cx.spawn(async move |weak, cx| {
            let guard_owner = owner.clone();
            let result = join_automation_ui_task(
                runtime.spawn(async move {
                    host.resolve_approval(owner, proposal, approve).await
                }),
                cx.background_executor(),
            )
            .await
            .unwrap_or(Err(PortError::Storage));
            let _ = weak.update(cx, |app, cx| {
                if app.automation_approvals.resolving.as_ref() == Some(&id) {
                    app.automation_approvals.resolving = None;
                }
                if caller(&app.host).as_ref() == Some(&guard_owner) {
                    match result {
                        Ok(()) => app
                            .automation_approvals
                            .cards
                            .retain(|p| card_id(&guard_owner, p) != id),
                        Err(PortError::RevisionConflict) => {
                            app.notice = Some("notice.approval_hash_mismatch");
                        }
                        Err(
                            PortError::RequestExpired | PortError::NotFound,
                        ) => app.notice = Some("notice.card_not_found"),
                        Err(
                            PortError::Forbidden | PortError::InvalidRequest,
                        ) => app.notice = Some("notice.card_selection_invalid"),
                        Err(_) => {
                            app.notice = Some("notice.session_storage_failed");
                        }
                    }
                }
                app.sync_automation_approvals(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
