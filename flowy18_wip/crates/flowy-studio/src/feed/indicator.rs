//! Bottom elapsed-time working indicator and delivery retry prompt.

use flowy_ui::{self as theme, ResolvedTheme, UiContext};
use gpui::{Context, Render, Subscription, Window, div, prelude::*, px};

#[derive(Clone, Debug)]
pub enum WorkingIndicatorEvent {
    RetryDelivery,
}

impl gpui::EventEmitter<WorkingIndicatorEvent> for WorkingIndicator {}

pub struct WorkingIndicator {
    pub(super) label: String,
    pub(super) visible: bool,
    pub(super) error: bool,
    pub(super) retry: bool,
    pub(super) retry_label: String,
    theme: ResolvedTheme,
    _theme_sub: Subscription,
}

impl WorkingIndicator {
    pub(crate) fn new(ui: UiContext, cx: &mut Context<'_, Self>) -> Self {
        let theme = ui.theme.read(cx).resolved();
        let theme_sub = cx.observe(&ui.theme, |indicator, entity, cx| {
            indicator.theme = entity.read(cx).resolved();
            cx.notify();
        });
        Self {
            label: String::new(),
            visible: false,
            error: false,
            retry: false,
            retry_label: String::new(),
            theme,
            _theme_sub: theme_sub,
        }
    }

    #[cfg(test)]
    pub(crate) const fn is_visible(&self) -> bool {
        self.visible
    }

    #[cfg(test)]
    pub(crate) const fn is_error(&self) -> bool {
        self.error
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn set_label(
        &mut self,
        label: String,
        visible: bool,
        error: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if self.label != label || self.visible != visible || self.error != error
        {
            self.label = label;
            self.visible = visible;
            self.error = error;
            cx.notify();
        }
    }

    pub(crate) fn set_retry(
        &mut self,
        retry: bool,
        retry_label: String,
        cx: &mut Context<'_, Self>,
    ) {
        if self.retry != retry || self.retry_label != retry_label {
            self.retry = retry;
            self.retry_label = retry_label;
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn label_for_test(&self) -> &str {
        &self.label
    }

    #[cfg(test)]
    pub(crate) const fn retry_enabled(&self) -> bool {
        self.retry
    }
}

impl Render for WorkingIndicator {
    fn render(
        &mut self,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let retry = self.retry;
        let retry_label = self.retry_label.clone();
        div().id("chat-working").role(gpui::Role::Status).when(
            self.visible,
            |element| {
                element
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .text_xs()
                    .text_color(if self.error {
                        self.theme.danger()
                    } else {
                        self.theme.muted()
                    })
                    .when(self.error, |row| {
                        row.child(
                            theme::icons::circle_alert()
                                .size(px(14.))
                                .text_color(self.theme.danger()),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(self.label.clone()),
                    )
                    .when(retry, |row| {
                        row.child(
                            theme::Button::new(
                                "chat-retry-delivery",
                                retry_label,
                            )
                            .size(theme::ControlSize::Small)
                            .variant(theme::ControlVariant::Secondary)
                            .render(
                                &self.theme,
                                cx.listener(|_, _, _, cx| {
                                    cx.emit(
                                        WorkingIndicatorEvent::RetryDelivery,
                                    );
                                }),
                            ),
                        )
                    })
            },
        )
    }
}
