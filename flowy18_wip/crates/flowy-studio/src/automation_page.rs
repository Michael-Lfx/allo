use crate::{StudioEvent, StudioPanel};
use flowy_automation::{
    ActionSpec, AutomationCommand, AutomationId, AutomationSnapshot,
    DefinitionState, PortError, PrivateText, RunId, RunOutcome, RunPhase,
    RunSnapshot,
};
use flowy_ui::{self as ui, ControlVariant};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window, div, px};
use gpui_kit::base::input::{InputState, TextareaState};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AutomationView {
    #[default]
    Definitions,
    Inbox,
    Definition(AutomationId),
    Run(RunId),
}

#[derive(Clone, Debug)]
pub enum AutomationUiAction {
    Navigate(AutomationView),
    Command(AutomationCommand),
    OpenResult(RunId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationPage {
    pub view: AutomationView,
    pub definitions: Vec<AutomationSnapshot>,
    pub runs: Vec<RunSnapshot>,
    pub busy: bool,
    pub loading: bool,
    pub error: Option<PortError>,
    pub feedback: Option<&'static str>,
}

pub struct DefinitionEditor {
    original: AutomationSnapshot,
    title: Entity<InputState>,
    text: Entity<TextareaState>,
}

#[derive(Clone, Copy)]
pub enum RunConfirmation {
    Retry,
    Abandon,
}

impl RunConfirmation {
    const fn label(self) -> &'static str {
        match self {
            Self::Retry => "auto.retry_run",
            Self::Abandon => "auto.abandon_artifacts",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Retry => "auto.retry_confirm",
            Self::Abandon => "auto.abandon_confirm",
        }
    }

    const fn command(self, run_id: RunId) -> AutomationCommand {
        match self {
            Self::Retry => AutomationCommand::RetryInterrupted { run_id },
            Self::Abandon => {
                AutomationCommand::AbandonPendingArtifacts { run_id }
            }
        }
    }
}

impl StudioPanel {
    pub fn set_automation_page(
        &mut self,
        page: Option<AutomationPage>,
        cx: &mut Context<'_, Self>,
    ) {
        if page == self.automation_page {
            return;
        }
        if page.as_ref().map(|page| &page.view)
            != self.automation_page.as_ref().map(|page| &page.view)
        {
            self.automation_editor = None;
            self.automation_confirm = None;
        }
        self.automation_page = page;
        cx.notify();
    }

    fn automation_tab(
        &self,
        id: impl Into<gpui::ElementId>,
        label: &'static str,
        view: AutomationView,
        selected: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        ui::filter_chip(id, self.t(label), selected, &self.theme)
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(StudioEvent::Automation(Box::new(
                    AutomationUiAction::Navigate(view.clone()),
                )));
            }))
            .into_any_element()
    }

    fn automation_button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: &'static str,
        action: AutomationUiAction,
        busy: bool,
        primary: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        ui::Button::new(id, self.t(label))
            .variant(if primary {
                ControlVariant::Primary
            } else {
                ControlVariant::Secondary
            })
            .disabled(busy)
            .render(
                &self.theme,
                cx.listener(move |_, _, _, cx| {
                    cx.emit(StudioEvent::Automation(Box::new(action.clone())));
                }),
            )
    }

    pub(crate) fn automation_manager(
        &self,
        page: &AutomationPage,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        use AutomationUiAction::Navigate;
        let tabs = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .items_center()
            .child(self.automation_tab(
                "auto-definitions",
                "auto.definitions",
                AutomationView::Definitions,
                matches!(
                    page.view,
                    AutomationView::Definitions | AutomationView::Definition(_)
                ),
                cx,
            ))
            .child(self.automation_tab(
                "auto-inbox",
                "auto.inbox",
                AutomationView::Inbox,
                matches!(
                    page.view,
                    AutomationView::Inbox | AutomationView::Run(_)
                ),
                cx,
            ));
        let content = if page.loading {
            div().child(self.t("library.loading")).into_any_element()
        } else if let AutomationView::Definition(id) = &page.view {
            page.definitions
                .iter()
                .find(|item| &item.automation_id == id)
                .map_or_else(
                    || {
                        div()
                            .child(self.t("auto.unavailable"))
                            .into_any_element()
                    },
                    |item| self.automation_definition(item, page, cx),
                )
        } else if matches!(page.view, AutomationView::Definitions) {
            div()
                .when(
                    page.definitions.is_empty() && page.error.is_none(),
                    |body| body.child(self.t("auto.definitions_empty")),
                )
                .children(page.definitions.iter().map(|item| {
                    div()
                        .border_b_1()
                        .border_color(self.theme.line())
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .items_center()
                        .py(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(item.title.expose().to_owned())
                                .child(
                                    div()
                                        .text_color(self.theme.muted())
                                        .child(self.t(definition_state(item))),
                                ),
                        )
                        .child(self.automation_button(
                            gpui::SharedString::from(format!(
                                "auto-definition-{}",
                                item.automation_id.as_str()
                            )),
                            "auto.details",
                            Navigate(AutomationView::Definition(
                                item.automation_id.clone(),
                            )),
                            false,
                            false,
                            cx,
                        ))
                }))
                .into_any_element()
        } else {
            self.automation_runs(page, cx)
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                ui::PageHeader::new(self.t("dock.automation"))
                    .render(&self.theme, window),
            )
            .child(
                div()
                    .id("automation-manager-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(self.theme.space(24.))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .max_w(px(760.))
                            .mx_auto()
                            .py(self.theme.space(24.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(tabs)
                            .child(self.automation_page_approvals(cx))
                            .when_some(page.error, |body, error| {
                                body.child(
                                    div().text_color(self.theme.ink()).child(
                                        self.t(match error {
                                            PortError::RevisionConflict => {
                                                "auto.conflict"
                                            }
                                            PortError::Forbidden
                                            | PortError::NotFound => {
                                                "auto.unavailable"
                                            }
                                            PortError::InvalidRequest => {
                                                "auto.invalid"
                                            }
                                            _ => "auto.load_error",
                                        }),
                                    ),
                                )
                            })
                            .when_some(page.feedback, |body, key| {
                                body.child(self.t(key))
                            })
                            .child(content),
                    ),
            )
            .into_any_element()
    }

    fn automation_definition(
        &self,
        item: &AutomationSnapshot,
        page: &AutomationPage,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let id = item.automation_id.clone();
        let revision = item.revision;
        let archived = item.state == DefinitionState::Archived;
        let enabled = item.state == DefinitionState::Enabled;
        let command = |label, command, primary| {
            self.automation_button(
                label,
                label,
                AutomationUiAction::Command(command),
                page.busy || archived,
                primary,
                cx,
            )
        };
        let (label, toggle) = if enabled {
            (
                "auto.pause",
                AutomationCommand::Pause {
                    automation_id: id.clone(),
                    expected_revision: revision,
                },
            )
        } else {
            (
                "auto.resume",
                AutomationCommand::Resume {
                    automation_id: id.clone(),
                    expected_revision: revision,
                },
            )
        };
        let edit = item.clone();
        let schedule = item.definition.schedule.map_or_else(|| self.t("auto.manual"), |schedule| match schedule.trigger {
            flowy_automation::schedule::TriggerSpec::Manual => self.t("auto.manual"),
            flowy_automation::schedule::TriggerSpec::Once { scheduled_at_ms } => automation_time(scheduled_at_ms),
            flowy_automation::schedule::TriggerSpec::Calendar(rule) => {
                let time = format!("{} {}", rule.local_time, rule.time_zone);
                match rule.frequency {
                    flowy_automation::schedule::CalendarFrequency::Daily => self.i18n.tf("auto.approval_daily", &[("time", &time)]),
                    flowy_automation::schedule::CalendarFrequency::Weekly { weekday } => {
                        let day = self.t(weekday_key(weekday));
                        self.i18n.tf(
                            "auto.approval_weekly",
                            &[("day", day.as_str()), ("time", &time)],
                        )
                    }
                }
            }
        });
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(20.))
                    .child(item.title.expose().to_owned()),
            )
            .child(
                div()
                    .text_color(self.theme.muted())
                    .child(self.t(definition_state(item))),
            )
            .child(div().text_color(self.theme.muted()).child(schedule))
            .when_some(item.next_run_at_ms, |body, time| {
                body.child(
                    self.i18n.tf(
                        "auto.next_run",
                        &[("time", &automation_time(time))],
                    ),
                )
            })
            .child(div().child(
                action_text(&item.definition.action).expose().to_owned(),
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(command(
                        "auto.run_now",
                        AutomationCommand::RunNow {
                            automation_id: id.clone(),
                            expected_revision: revision,
                        },
                        true,
                    ))
                    .child(command(label, toggle, false))
                    .child(command(
                        "auto.archive",
                        AutomationCommand::Archive {
                            automation_id: id,
                            expected_revision: revision,
                        },
                        false,
                    ))
                    .child(
                        ui::Button::new("auto-edit", self.t("auto.edit"))
                            .disabled(page.busy || archived)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, window, cx| {
                                    let title = cx.new(|cx| {
                                        let mut state =
                                            InputState::new(window, cx);
                                        state.set_value(
                                            edit.title.expose(),
                                            window,
                                            cx,
                                        );
                                        state
                                    });
                                    let text = cx.new(|cx| {
                                        let mut state =
                                            TextareaState::new(window, cx);
                                        state.set_value(
                                            action_text(
                                                &edit.definition.action,
                                            )
                                            .expose(),
                                            window,
                                            cx,
                                        );
                                        state
                                    });
                                    panel.automation_editor =
                                        Some(DefinitionEditor {
                                            original: edit.clone(),
                                            title,
                                            text,
                                        });
                                    cx.notify();
                                }),
                            ),
                    ),
            )
            .when_some(self.automation_editor.as_ref(), |body, editor| {
                body.child(self.automation_edit_form(editor, page.busy, cx))
            })
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(16.))
                    .child(self.t("auto.history")),
            )
            .child(self.automation_runs(page, cx))
            .into_any_element()
    }

    fn automation_edit_form(
        &self,
        editor: &DefinitionEditor,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let title = editor.title.clone();
        let text = editor.text.clone();
        let original = editor.original.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_1()
            .border_color(self.theme.line())
            .rounded_lg()
            .p(px(12.))
            .child(
                ui::TextField::new(
                    "auto-edit-title",
                    self.t("auto.title"),
                    editor.title.clone(),
                )
                .render(&self.theme, cx),
            )
            .child(
                ui::TextAreaField::new(
                    "auto-edit-text",
                    self.t("auto.instructions"),
                    editor.text.clone(),
                )
                .height(px(112.))
                .render(&self.theme, cx),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::Button::new("auto-edit-close", self.t("auto.back"))
                            .render(
                                &self.theme,
                                cx.listener(|panel, _, _, cx| {
                                    panel.automation_editor = None;
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        ui::Button::new("auto-edit-save", self.t("auto.save"))
                            .variant(ControlVariant::Primary)
                            .loading(busy)
                            .render(
                                &self.theme,
                                cx.listener(move |_, _, _, cx| {
                                    let mut definition =
                                        original.definition.clone();
                                    definition.title = PrivateText::new(
                                        title.read(cx).value().trim(),
                                    );
                                    let content = PrivateText::new(
                                        text.read(cx).value().trim(),
                                    );
                                    definition.action = match definition.action
                                    {
                                        ActionSpec::AgentTask { .. } => {
                                            ActionSpec::AgentTask {
                                                prompt: content,
                                            }
                                        }
                                        ActionSpec::Reminder { .. } => {
                                            ActionSpec::Reminder {
                                                text: content,
                                            }
                                        }
                                    };
                                    cx.emit(StudioEvent::Automation(Box::new(
                                        AutomationUiAction::Command(
                                            AutomationCommand::UpdateManual {
                                                automation_id: original
                                                    .automation_id
                                                    .clone(),
                                                expected_revision: original
                                                    .revision,
                                                definition: Box::new(
                                                    definition,
                                                ),
                                            },
                                        ),
                                    )));
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn automation_runs(
        &self,
        page: &AutomationPage,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let detail = matches!(page.view, AutomationView::Run(_));
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(page.runs.is_empty() && page.error.is_none(), |body| {
                body.child(self.t("auto.runs_empty"))
            })
            .children(page.runs.iter().map(|run| {
                let status = run_status(run);
                let row = div()
                    .border_b_1()
                    .border_color(self.theme.line())
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .min_w_0()
                    .child(run.title.expose().to_owned())
                    .child(
                        div()
                            .text_color(self.theme.muted())
                            .child(self.t(status)),
                    );
                if detail {
                    row.when_some(run.reminder.as_ref(), |row, text| {
                        row.child(text.expose().to_owned())
                    })
                    .when_some(run.blocked_reason, |row, reason| {
                        row.child(self.t(blocked_reason(reason)))
                    })
                    .when_some(run.scheduled_at_ms, |row, time| {
                        row.child(automation_time(time))
                    })
                    .child(self.automation_run_actions(run, page.busy, cx))
                } else {
                    row.child(div().child(self.automation_button(
                        gpui::SharedString::from(format!(
                            "auto-run-{}",
                            run.run_id.as_str()
                        )),
                        "auto.view_run",
                        AutomationUiAction::Navigate(AutomationView::Run(
                            run.run_id.clone(),
                        )),
                        false,
                        false,
                        cx,
                    )))
                }
            }))
            .into_any_element()
    }

    fn automation_run_actions(
        &self,
        run: &RunSnapshot,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let id = &run.run_id;
        let button = |label, command| {
            self.automation_button(
                label,
                label,
                AutomationUiAction::Command(command),
                busy,
                false,
                cx,
            )
        };
        let available = match run.phase {
            RunPhase::Interrupted => Some(RunConfirmation::Retry),
            RunPhase::Finalizing if !run.abandon_requested => {
                Some(RunConfirmation::Abandon)
            }
            _ => None,
        };
        let confirm = self
            .automation_confirm
            .as_ref()
            .filter(|(selected, _)| selected == id && available.is_some())
            .map(|(_, action)| *action);
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .when(run.result_session_id.is_some(), |row| {
                row.child(self.automation_button(
                    "auto-open-result",
                    "auto.open_result",
                    AutomationUiAction::OpenResult(id.clone()),
                    busy,
                    true,
                    cx,
                ))
            })
            .when(
                run.phase == RunPhase::Settled && !run.source_removed,
                |row| {
                    row.child(button(
                        if run.kept { "auto.unkeep" } else { "auto.keep" },
                        AutomationCommand::SetResultKept {
                            run_id: id.clone(),
                            kept: !run.kept,
                        },
                    ))
                },
            )
            .when(
                matches!(
                    run.phase,
                    RunPhase::Queued
                        | RunPhase::Blocked
                        | RunPhase::WaitingWorkspace
                        | RunPhase::Running
                ),
                |row| {
                    row.child(button(
                        "auto.cancel_run",
                        AutomationCommand::CancelRun { run_id: id.clone() },
                    ))
                },
            )
            .when(
                (run.phase == RunPhase::Finalizing && !run.abandon_requested)
                    || run.delivery.iter().any(|item| {
                        item.state == flowy_automation::OutboxState::RetryWait
                    }),
                |row| {
                    row.child(button(
                        "auto.retry_delivery",
                        AutomationCommand::RetryDelivery { run_id: id.clone() },
                    ))
                },
            )
            .when(
                run.phase == RunPhase::Finalizing && run.abandon_requested,
                |row| row.child(self.t("auto.abandon_pending")),
            )
            .when_some(
                available.filter(|_| confirm.is_none()),
                |row, action| {
                    let run_id = id.clone();
                    row.child(
                        ui::Button::new(
                            "auto-prepare-confirm",
                            self.t(action.label()),
                        )
                        .disabled(busy)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                panel.automation_confirm =
                                    Some((run_id.clone(), action));
                                cx.notify();
                            }),
                        ),
                    )
                },
            )
            .when_some(confirm, |row, action| {
                row.child(div().w_full().child(self.t(action.description())))
                    .child(
                        ui::Button::new(
                            "auto-cancel-confirm",
                            self.t("auto.back"),
                        )
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.automation_confirm = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        ui::Button::new(
                            "auto-confirm-run-action",
                            self.t(action.label()),
                        )
                        .variant(
                            if matches!(action, RunConfirmation::Abandon) {
                                ControlVariant::Danger
                            } else {
                                ControlVariant::Primary
                            },
                        )
                        .disabled(busy)
                        .render(
                            &self.theme,
                            Self::confirm_run_action(
                                action.command(id.clone()),
                                cx,
                            ),
                        ),
                    )
            })
            .into_any_element()
    }

    fn confirm_run_action(
        command: AutomationCommand,
        cx: &Context<'_, Self>,
    ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
        cx.listener(move |panel, _, _, cx| {
            panel.automation_confirm = None;
            cx.emit(StudioEvent::Automation(Box::new(
                AutomationUiAction::Command(command.clone()),
            )));
            cx.notify();
        })
    }
}

const fn action_text(action: &ActionSpec) -> &PrivateText {
    match action {
        ActionSpec::AgentTask { prompt } => prompt,
        ActionSpec::Reminder { text } => text,
    }
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

fn automation_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_default(
        |time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M %Z")
                .to_string()
        },
    )
}

const fn definition_state(item: &AutomationSnapshot) -> &'static str {
    match item.state {
        DefinitionState::Draft => "auto.state_draft",
        DefinitionState::Enabled if item.storage_suspended => {
            "auto.state_storage"
        }
        DefinitionState::Enabled => "auto.state_enabled",
        DefinitionState::Paused => "auto.state_paused",
        DefinitionState::Archived => "auto.state_archived",
        DefinitionState::SuspendedStorage => "auto.state_storage",
    }
}

const fn blocked_reason(
    reason: flowy_automation::BlockedReason,
) -> &'static str {
    use flowy_automation::BlockedReason;
    match reason {
        BlockedReason::AccountUnavailable => "auto.blocked_account",
        BlockedReason::CredentialUnavailable => "auto.blocked_credential",
        BlockedReason::ModelUnavailable => "auto.blocked_model",
        BlockedReason::ApprovalRequired => "auto.blocked_approval",
        BlockedReason::WorkspaceUnavailable => "auto.blocked_workspace",
        BlockedReason::StorageUnavailable => "auto.state_storage",
    }
}

const fn run_status(run: &RunSnapshot) -> &'static str {
    if run.source_removed {
        return "auto.source_removed";
    }
    if run.artifacts_abandoned {
        return "auto.artifacts_abandoned";
    }
    match run.phase {
        RunPhase::Queued => "auto.queued",
        RunPhase::Blocked => "auto.blocked",
        RunPhase::WaitingWorkspace => "auto.waiting_workspace",
        RunPhase::Running => "auto.running",
        RunPhase::Cancelling => "auto.cancelling",
        RunPhase::Finalizing => "auto.finalizing",
        RunPhase::Interrupted => "auto.interrupted",
        RunPhase::Settled => match run.outcome {
            Some(RunOutcome::Succeeded) => "auto.succeeded",
            Some(RunOutcome::Cancelled) => "auto.cancelled",
            Some(RunOutcome::Expired) => "auto.expired",
            Some(RunOutcome::Skipped) => "auto.skipped",
            Some(RunOutcome::TimedOut) => "auto.timed_out",
            Some(RunOutcome::BudgetExceeded) => "auto.budget_exceeded",
            Some(RunOutcome::Interrupted) => "auto.interrupted",
            Some(RunOutcome::Failed) | None => "auto.failed",
        },
    }
}
