use crate::panel::{StudioEvent, StudioPanel};
use flowy_ui::{Button, ControlVariant};
use gpui::{Context, IntoElement, Styled, div, prelude::*, px};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationApprovalCard {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub details: String,
    pub busy: bool,
}

impl StudioPanel {
    pub fn set_automation_approvals(
        &mut self,
        cards: Vec<AutomationApprovalCard>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.automation_approvals != cards {
            self.automation_approvals = cards;
            cx.notify();
        }
    }

    pub(crate) fn automation_approval_slot(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let active = self.agent.active_session_id();
        let cards = self
            .automation_approvals
            .iter()
            .filter(|card| active.as_deref() == Some(card.session_id.as_str()))
            .collect();
        self.automation_approval_panel(cards, cx)
    }

    pub(crate) fn automation_page_approvals(
        &self,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let cards = self
            .automation_approvals
            .iter()
            .filter(|card| {
                card.session_id == flowy_automation::UI_APPROVAL_SESSION
            })
            .collect();
        self.automation_approval_panel(cards, cx)
    }

    fn automation_approval_panel(
        &self,
        cards: Vec<&AutomationApprovalCard>,
        cx: &Context<'_, Self>,
    ) -> gpui::AnyElement {
        let theme = &self.theme;
        div()
            .flex()
            .justify_center()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(crate::STAGE_MAX_W))
                    .px(theme.space(32.))
                    .children(cards.into_iter().take(1).map(|card| {
                        let id = card.id.clone();
                        let button = |approve: bool, label, variant| {
                            let id = id.clone();
                            Button::new(
                                (
                                    gpui::SharedString::from(id.clone()),
                                    usize::from(approve),
                                ),
                                self.t(label),
                            )
                            .variant(variant)
                            .loading(approve && card.busy)
                            .disabled(!approve && card.busy)
                            .size(flowy_ui::ControlSize::Small)
                            .render(
                                theme,
                                cx.listener(move |_, _, _, cx| {
                                    cx.emit(StudioEvent::AutomationApproval {
                                        id: id.clone(),
                                        approve,
                                    });
                                }),
                            )
                        };
                        div()
                            .id(gpui::SharedString::from(id.clone()))
                            .debug_selector(|| {
                                "automation-approval-card".to_owned()
                            })
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.line())
                            .bg(theme.card())
                            .min_w_0()
                            .p(theme.space(12.))
                            .flex()
                            .flex_col()
                            .gap(theme.space(8.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted())
                                    .child(self.t("auto.approval_title")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(card.title.clone()),
                            )
                            .child(
                                div()
                                    .id("automation-approval-scope")
                                    .max_h(px(96.))
                                    .overflow_y_scroll()
                                    .text_xs()
                                    .text_color(theme.muted())
                                    .child(card.details.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(theme.space(6.))
                                    .child(button(
                                        false,
                                        "card.tool_deny",
                                        ControlVariant::Secondary,
                                    ))
                                    .child(button(
                                        true,
                                        "card.tool_approve",
                                        ControlVariant::Primary,
                                    )),
                            )
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AutomationPage, StudioConfig, StudioSurface};
    use flowy_agent::{LlmConfig, Model, Plan};
    use flowy_i18n::{I18n, Locale};
    use flowy_ui::{ThemeMode, ThemeOptions, UiContext};
    use gpui::TestAppContext;

    #[gpui::test]
    fn approval_card_renders_in_both_themes_only_in_its_source_conversation(
        cx: &mut TestAppContext,
    ) {
        for (mode, locale) in [
            (ThemeMode::Light, Locale::En),
            (ThemeMode::Dark, Locale::ZhCn),
        ] {
            let root = std::env::temp_dir().join(format!(
                "flowy-approval-card-{}-{mode:?}",
                std::process::id()
            ));
            let (panel, visual) = cx.add_window_view(|window, cx| {
                let ui = UiContext::new(
                    ThemeOptions {
                        mode,
                        ..ThemeOptions::default()
                    },
                    false,
                    cx,
                );
                let mut i18n = I18n::embedded("Flowy");
                i18n.set_locale(locale);
                let mut panel = StudioPanel::new(
                    StudioConfig {
                        product: "Flowy",
                        i18n,
                        model: Model::Lite,
                        plan: Plan::Free,
                        model_info_version: String::new(),
                        model_info_open: false,
                        store_root: root.clone(),
                        llm: LlmConfig::default(),
                    },
                    ui,
                    window,
                    cx,
                );
                let id = panel.agent.create_home_session().unwrap();
                panel.surface = StudioSurface::Chat;
                panel.set_automation_approvals(
                    vec![AutomationApprovalCard {
                        id: "approval-card-fixture".into(),
                        session_id: id,
                        title: "Fixture task".into(),
                        details: "Read notes from the approved workspace."
                            .into(),
                        busy: false,
                    }],
                    cx,
                );
                panel
            });
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
            });
            assert!(visual.debug_bounds("automation-approval-card").is_some());
            visual.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.agent.create_home_session().unwrap();
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
            assert!(visual.debug_bounds("automation-approval-card").is_none());
            visual.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.surface = StudioSurface::Automation;
                    panel.automation_page = Some(AutomationPage::default());
                    panel.set_automation_approvals(
                        vec![AutomationApprovalCard {
                            id: "management-approval-card".into(),
                            session_id: flowy_automation::UI_APPROVAL_SESSION
                                .into(),
                            title: "Management task".into(),
                            details: "Approve from the automation page.".into(),
                            busy: false,
                        }],
                        cx,
                    );
                });
                window.draw(cx).clear(cx);
            });
            assert!(visual.debug_bounds("automation-approval-card").is_some());
            drop(panel);
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
