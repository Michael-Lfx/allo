//! Automation manager: list, detail with run history, and a single run.

use crate::{StudioEvent, StudioPanel};
use flowy_automation::schedule::{CalendarFrequency, TriggerSpec};
use flowy_automation::{
    Access, Action, Automation, AutomationId, AutomationInput, Run, RunId,
    RunStatus,
};
use flowy_ui::{self as ui, ControlVariant};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window, div, px};
use gpui_kit::base::input::{InputState, TextareaState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutomationView {
    #[default]
    List,
    Detail(AutomationId),
    Run(RunId),
}

#[derive(Clone, Debug)]
pub enum AutomationUiAction {
    Navigate(AutomationView),
    /// Edits an automation. Only the agent creates them, in a conversation.
    Save(AutomationId, AutomationInput),
    Pause(AutomationId),
    Resume(AutomationId),
    Delete(AutomationId),
    Cancel(RunId),
    /// Opens the conversation that owns the automation and its results.
    OpenConversation(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationPage {
    pub view: AutomationView,
    pub automations: Vec<Automation>,
    /// History of the automation in `Detail`, or the single run in `Run`.
    pub runs: Vec<Run>,
    pub busy: bool,
    pub loading: bool,
    pub error: Option<&'static str>,
    pub feedback: Option<&'static str>,
}

pub struct DefinitionEditor {
    original: Automation,
    title: Entity<InputState>,
    text: Entity<TextareaState>,
    access: Access,
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
        if page.as_ref().map(|page| page.view)
            != self.automation_page.as_ref().map(|page| page.view)
        {
            self.automation_editor = None;
            self.automation_confirm = None;
        }
        self.automation_page = page;
        cx.notify();
    }

    pub fn close_automation_editor(&mut self, cx: &mut Context<'_, Self>) {
        if self.automation_editor.take().is_some() {
            cx.notify();
        }
    }

    fn automation_button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: &'static str,
        action: AutomationUiAction,
        busy: bool,
        variant: ControlVariant,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        ui::Button::new(id, self.t(label))
            .variant(variant)
            .disabled(busy)
            .render(
                &self.theme,
                cx.listener(move |_, _, _, cx| {
                    cx.emit(StudioEvent::Automation(Box::new(action.clone())));
                }),
            )
    }

    fn automation_back(
        &self,
        view: AutomationView,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        div()
            .child(self.automation_button(
                "auto-back",
                "auto.back",
                AutomationUiAction::Navigate(view),
                false,
                ControlVariant::Secondary,
                cx,
            ))
            .into_any_element()
    }

    pub(crate) fn automation_manager(
        &self,
        page: &AutomationPage,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let unavailable =
            || div().child(self.t("auto.unavailable")).into_any_element();
        let content = if page.loading {
            div().child(self.t("library.loading")).into_any_element()
        } else {
            match page.view {
                AutomationView::List => self.automation_list(page, cx),
                AutomationView::Detail(id) => page
                    .automations
                    .iter()
                    .find(|item| item.id == id)
                    .map_or_else(unavailable, |item| {
                        self.automation_detail(item, page, cx)
                    }),
                AutomationView::Run(_) => {
                    page.runs.first().map_or_else(unavailable, |run| {
                        self.automation_run_view(run, page, cx)
                    })
                }
            }
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
                ui::scroll_area("automation-manager-scroll", &self.theme)
                    .a11y_label(self.t("a11y.scroll_automation_manager"))
                    .debug_selector(|| "automation-manager-scroll".to_owned())
                    .flex_1()
                    .min_h_0()
                    .viewport_style({
                        let pad_x = self.theme.space(24.);
                        move |s| s.px(pad_x)
                    })
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
                            .when_some(page.error, |body, key| {
                                body.child(
                                    div()
                                        .text_color(self.theme.ink())
                                        .child(self.t(key)),
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

    fn automation_list(
        &self,
        page: &AutomationPage,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        div()
            .when(
                page.automations.is_empty() && page.error.is_none(),
                |body| body.child(self.t("auto.definitions_empty")),
            )
            .children(page.automations.iter().map(|item| {
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
                            .child(item.title.clone())
                            .child(div().text_color(self.theme.muted()).child(
                                format!(
                                    "{} · {}",
                                    self.t(state_key(item)),
                                    self.trigger_text(&item.trigger)
                                ),
                            )),
                    )
                    .child(self.automation_button(
                        gpui::SharedString::from(format!(
                            "auto-definition-{}",
                            item.id
                        )),
                        "auto.details",
                        AutomationUiAction::Navigate(AutomationView::Detail(
                            item.id,
                        )),
                        false,
                        ControlVariant::Secondary,
                        cx,
                    ))
            }))
            .into_any_element()
    }

    fn automation_detail(
        &self,
        item: &Automation,
        page: &AutomationPage,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let id = item.id;
        let button = |label, action, variant| {
            self.automation_button(label, label, action, page.busy, variant, cx)
        };
        let toggle = if item.enabled {
            button(
                "auto.pause",
                AutomationUiAction::Pause(id),
                ControlVariant::Secondary,
            )
        } else {
            button(
                "auto.resume",
                AutomationUiAction::Resume(id),
                ControlVariant::Secondary,
            )
        };
        let edit = item.clone();
        let confirming = self.automation_confirm == Some(id);
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.automation_back(AutomationView::List, cx))
            .child(div().text_size(px(20.)).child(item.title.clone()))
            .child(div().text_color(self.theme.muted()).child(format!(
                "{} · {} · {}",
                self.t(state_key(item)),
                self.trigger_text(&item.trigger),
                self.t(access_key(item.access))
            )))
            .when_some(item.next_run_at_ms, |body, time| {
                body.child(
                    self.i18n.tf(
                        "auto.next_run",
                        &[("time", &automation_time(time))],
                    ),
                )
            })
            .child(div().child(action_text(&item.action).to_owned()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(button(
                        "auto.open_conversation",
                        AutomationUiAction::OpenConversation(
                            item.conversation_id.clone(),
                        ),
                        ControlVariant::Primary,
                    ))
                    .child(toggle)
                    .child(
                        ui::Button::new("auto-edit", self.t("auto.edit"))
                            .disabled(page.busy)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, window, cx| {
                                    panel.open_automation_editor(
                                        edit.clone(),
                                        window,
                                        cx,
                                    );
                                }),
                            ),
                    )
                    .when(!confirming, |row| {
                        row.child(
                            ui::Button::new(
                                "auto-delete",
                                self.t("auto.delete"),
                            )
                            .disabled(page.busy)
                            .render(
                                &self.theme,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.automation_confirm = Some(id);
                                    cx.notify();
                                }),
                            ),
                        )
                    }),
            )
            .when(confirming, |body| {
                body.child(self.automation_delete_confirm(id, page.busy, cx))
            })
            .when_some(self.automation_editor.as_ref(), |body, editor| {
                body.child(self.automation_edit_form(editor, page.busy, cx))
            })
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(16.))
                    .child(self.t("auto.history")),
            )
            .child(self.automation_runs(&page.runs, false, page.busy, cx))
            .into_any_element()
    }

    fn automation_delete_confirm(
        &self,
        id: AutomationId,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .child(div().w_full().child(self.t("auto.delete_confirm")))
            .child(
                ui::Button::new("auto-delete-cancel", self.t("auto.back"))
                    .render(
                        &self.theme,
                        cx.listener(|panel, _, _, cx| {
                            panel.automation_confirm = None;
                            cx.notify();
                        }),
                    ),
            )
            .child(
                ui::Button::new("auto-delete-confirm", self.t("auto.delete"))
                    .variant(ControlVariant::Danger)
                    .disabled(busy)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            panel.automation_confirm = None;
                            cx.emit(StudioEvent::Automation(Box::new(
                                AutomationUiAction::Delete(id),
                            )));
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }

    fn open_automation_editor(
        &mut self,
        original: Automation,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let title = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(&original.title, window, cx);
            state
        });
        let text = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx);
            state.set_value(action_text(&original.action), window, cx);
            state
        });
        self.automation_editor = Some(DefinitionEditor {
            access: original.access,
            original,
            title,
            text,
        });
        cx.notify();
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
        let access = editor.access;
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
                        ui::Button::new(
                            "auto-edit-access",
                            self.t(access_key(access)),
                        )
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                if let Some(editor) =
                                    panel.automation_editor.as_mut()
                                {
                                    editor.access = match editor.access {
                                        Access::Read => Access::Write,
                                        Access::Write => Access::Read,
                                    };
                                    cx.notify();
                                }
                            }),
                        ),
                    )
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
                                    let content =
                                        text.read(cx).value().trim().to_owned();
                                    let action = match original.action {
                                        Action::AgentTask { .. } => {
                                            Action::AgentTask {
                                                prompt: content,
                                            }
                                        }
                                        Action::Reminder { .. } => {
                                            Action::Reminder { text: content }
                                        }
                                    };
                                    let input = AutomationInput {
                                        title: title
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .to_owned(),
                                        action,
                                        access,
                                        ..original.input()
                                    };
                                    cx.emit(StudioEvent::Automation(Box::new(
                                        AutomationUiAction::Save(
                                            original.id,
                                            input,
                                        ),
                                    )));
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn automation_run_view(
        &self,
        run: &Run,
        page: &AutomationPage,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let owner = page
            .automations
            .iter()
            .find(|item| item.id == run.automation_id);
        let title = owner.map(|item| item.title.clone());
        let conversation = owner.map(|item| item.conversation_id.clone());
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                self.automation_back(
                    AutomationView::Detail(run.automation_id),
                    cx,
                ),
            )
            .when_some(title, |body, title| {
                body.child(div().text_size(px(20.)).child(title))
            })
            .child(self.automation_runs(
                std::slice::from_ref(run),
                true,
                page.busy,
                cx,
            ))
            .when_some(conversation, |body, conversation| {
                body.child(div().child(self.automation_button(
                    "auto-open-conversation",
                    "auto.open_conversation",
                    AutomationUiAction::OpenConversation(conversation),
                    page.busy,
                    ControlVariant::Primary,
                    cx,
                )))
            })
            .into_any_element()
    }

    fn automation_runs(
        &self,
        runs: &[Run],
        detail: bool,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(runs.is_empty(), |body| {
                body.child(self.t("auto.runs_empty"))
            })
            .children(runs.iter().map(|run| {
                let row = div()
                    .border_b_1()
                    .border_color(self.theme.line())
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .min_w_0()
                    .child(automation_time(
                        run.started_at_ms.unwrap_or(run.created_at_ms),
                    ))
                    .child(
                        div()
                            .text_color(self.theme.muted())
                            .child(self.t(status_key(run.status))),
                    );
                if detail {
                    row.when_some(run.error.clone(), |row, error| {
                        row.child(error)
                    })
                    .child(self.automation_run_actions(run, busy, cx))
                } else {
                    row.child(div().child(self.automation_button(
                        gpui::SharedString::from(format!(
                            "auto-run-{}",
                            run.id
                        )),
                        "auto.view_run",
                        AutomationUiAction::Navigate(AutomationView::Run(
                            run.id,
                        )),
                        false,
                        ControlVariant::Secondary,
                        cx,
                    )))
                }
            }))
            .into_any_element()
    }

    fn automation_run_actions(
        &self,
        run: &Run,
        busy: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .when(!run.status.is_final(), |row| {
                row.child(self.automation_button(
                    "auto-cancel-run",
                    "auto.cancel_run",
                    AutomationUiAction::Cancel(run.id),
                    busy,
                    ControlVariant::Secondary,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn trigger_text(&self, trigger: &TriggerSpec) -> String {
        match trigger {
            TriggerSpec::Once { scheduled_at_ms } => {
                automation_time(*scheduled_at_ms)
            }
            TriggerSpec::Calendar(rule) => {
                let time = format!(
                    "{} {}",
                    rule.local_time.format("%H:%M"),
                    rule.time_zone
                );
                match rule.frequency {
                    CalendarFrequency::Daily => {
                        self.i18n.tf("auto.daily", &[("time", &time)])
                    }
                    CalendarFrequency::Weekly { weekday } => {
                        let day = self.t(weekday_key(weekday));
                        self.i18n.tf(
                            "auto.weekly",
                            &[("day", day.as_str()), ("time", &time)],
                        )
                    }
                }
            }
        }
    }
}

fn action_text(action: &Action) -> &str {
    match action {
        Action::AgentTask { prompt } => prompt,
        Action::Reminder { text } => text,
    }
}

const fn state_key(item: &Automation) -> &'static str {
    if item.enabled {
        "auto.state_enabled"
    } else {
        "auto.state_paused"
    }
}

const fn access_key(access: Access) -> &'static str {
    match access {
        Access::Read => "auto.access_read",
        Access::Write => "auto.access_write",
    }
}

const fn status_key(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "auto.running",
        RunStatus::Succeeded => "auto.succeeded",
        RunStatus::Failed => "auto.failed",
        RunStatus::Interrupted => "auto.interrupted",

        RunStatus::Cancelled => "auto.cancelled",
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
