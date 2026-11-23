use super::{FlowyApp, state::ActiveDialog};
use flowy_studio::library::{
    LibraryEvent, LibraryItem, PreviewEvent, PreviewPanel,
};
use gpui::{
    Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement,
    Subscription, Window, div, prelude::*,
};

#[derive(Default)]
pub(super) struct ArtifactPreviewState {
    view: Option<Entity<PreviewPanel>>,
    artifact_id: Option<String>,
    request_id: u64,
    return_focus: Option<FocusHandle>,
    focus_pending: bool,
    retired: Vec<Entity<PreviewPanel>>,
    subscriptions: Vec<Subscription>,
}

pub(super) struct ArtifactFeedback {
    artifact_id: String,
    request_id: Option<u64>,
}

impl FlowyApp {
    pub(super) fn artifact_feedback(
        &self,
        artifact_id: &str,
    ) -> ArtifactFeedback {
        ArtifactFeedback {
            artifact_id: artifact_id.to_owned(),
            request_id: (self.active_dialog == ActiveDialog::ArtifactPreview
                && self.artifact_preview.artifact_id.as_deref()
                    == Some(artifact_id))
            .then_some(self.artifact_preview.request_id),
        }
    }

    pub(super) fn report_artifact_feedback(
        &mut self,
        feedback: &ArtifactFeedback,
        status: Option<&'static str>,
        cx: &mut Context<'_, Self>,
    ) {
        self.library
            .update(cx, |library, cx| library.set_status(status, cx));
        if feedback.request_id == Some(self.artifact_preview.request_id)
            && self.artifact_preview.artifact_id.as_deref()
                == Some(feedback.artifact_id.as_str())
            && self.active_dialog == ActiveDialog::ArtifactPreview
        {
            if let Some(view) = &self.artifact_preview.view {
                view.update(cx, |preview, cx| preview.set_status(status, cx));
            }
        } else if self.active_dialog == ActiveDialog::None
            && let Some(status) = status
        {
            self.note(status, cx);
        }
    }
    pub(super) fn sync_artifact_preview(
        &self,
        items: &[LibraryItem],
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(view) = &self.artifact_preview.view
            && let Some(item) = items.iter().find(|item| {
                Some(item.id.as_str())
                    == self.artifact_preview.artifact_id.as_deref()
            })
        {
            view.update(cx, |preview, cx| {
                preview.set_item(item.clone(), cx);
            });
        }
    }

    pub(super) fn sync_artifact_preview_locale(
        &self,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(view) = &self.artifact_preview.view {
            view.update(cx, |preview, cx| {
                preview.set_i18n(self.i18n.clone(), cx);
            });
        }
    }

    pub(super) fn open_artifact_preview(
        &mut self,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(item) = self
            .library
            .read(cx)
            .model
            .items
            .iter()
            .find(|item| item.id == artifact_id)
            .cloned()
        else {
            self.note("library.preview.unavailable", cx);
            return;
        };
        let return_focus = if self.section == super::Section::Library {
            self.library.read(cx).artifact_focus(artifact_id)
        } else {
            self.studio.read(cx).artifact_focus(artifact_id, cx)
        };
        if let Some(previous) = self.artifact_preview.view.take() {
            self.artifact_preview.retired.push(previous);
        }
        self.artifact_preview.request_id =
            self.artifact_preview.request_id.wrapping_add(1);
        let request_id = self.artifact_preview.request_id;
        let view = cx.new(|cx| {
            PreviewPanel::new(
                item,
                request_id,
                self.i18n.clone(),
                self.ui.clone(),
                cx,
            )
        });
        let subscription =
            cx.subscribe(&view, |app, _, event: &PreviewEvent, cx| {
                app.on_artifact_preview_event(event, cx);
            });
        let observer = cx.observe(&view, |_, _, cx| cx.notify());
        self.artifact_preview.view = Some(view.clone());
        self.artifact_preview.artifact_id = Some(artifact_id.to_owned());
        self.artifact_preview.return_focus = return_focus;
        self.artifact_preview.focus_pending = true;
        self.artifact_preview.subscriptions = vec![subscription, observer];
        self.activate_dialog(ActiveDialog::ArtifactPreview, cx);
        view.update(cx, |preview, cx| preview.request_bytes(request_id, cx));
    }

    fn on_artifact_preview_event(
        &mut self,
        event: &PreviewEvent,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            PreviewEvent::Close => {
                self.active_dialog = ActiveDialog::None;
                cx.notify();
            }
            PreviewEvent::ReadRequested {
                artifact_id,
                request_id,
            } => {
                if self.artifact_preview.artifact_id.as_deref()
                    != Some(artifact_id)
                    || self.active_dialog != ActiveDialog::ArtifactPreview
                {
                    return;
                }
                let store = self.studio.read(cx).agent.artifact_store();
                let artifact_id = artifact_id.clone();
                let request_id = *request_id;
                let job = cx.background_executor().spawn(async move {
                    let result = store
                        .map_err(|_| "library.error.catalog")?
                        .read_payload(&artifact_id)
                        .map_err(|error| match error.kind() {
                            std::io::ErrorKind::NotFound => {
                                "library.error.missing"
                            }
                            std::io::ErrorKind::InvalidData
                            | std::io::ErrorKind::InvalidInput => {
                                "library.availability.corrupt"
                            }
                            _ => "library.error.read",
                        });
                    Ok::<_, &'static str>((artifact_id, result))
                });
                cx.spawn(async move |weak, cx| {
                    let result = job.await;
                    let _ = weak.update(cx, |app, cx| {
                        if app.artifact_preview.request_id != request_id
                            || app.active_dialog
                                != ActiveDialog::ArtifactPreview
                        {
                            return;
                        }
                        let Some(preview) = &app.artifact_preview.view else {
                            return;
                        };
                        let result = match result {
                            Ok((id, result))
                                if app
                                    .artifact_preview
                                    .artifact_id
                                    .as_deref()
                                    == Some(&id) =>
                            {
                                result
                            }
                            Ok(_) => return,
                            Err(error) => Err(error),
                        };
                        preview.update(cx, |preview, cx| {
                            preview.set_bytes(request_id, result, cx);
                        });
                    });
                })
                .detach();
            }
            PreviewEvent::Retry => {
                self.artifact_preview.request_id =
                    self.artifact_preview.request_id.wrapping_add(1);
                let request_id = self.artifact_preview.request_id;
                if let Some(preview) = &self.artifact_preview.view {
                    preview.update(cx, |preview, cx| {
                        preview.request_bytes(request_id, cx);
                    });
                }
            }
            PreviewEvent::SaveAs(id) => self.save_artifact_as(id, cx),
            PreviewEvent::OpenDefault => {
                if let Some(id) = self.artifact_preview.artifact_id.clone() {
                    self.library_event(
                        &LibraryEvent::OpenWithDefaultRequested(id),
                        cx,
                    );
                }
            }
            PreviewEvent::LocateSource {
                session_id,
                artifact_id,
            } => {
                self.library_event(
                    &LibraryEvent::LocateSourceRequested {
                        session_id: session_id.clone(),
                        artifact_id: artifact_id.clone(),
                    },
                    cx,
                );
            }
        }
    }

    pub(super) fn reconcile_artifact_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        for retired in self.artifact_preview.retired.drain(..) {
            retired.update(cx, |preview, cx| preview.release(window, cx));
        }
        if self.active_dialog != ActiveDialog::ArtifactPreview {
            if let Some(view) = self.artifact_preview.view.take() {
                view.update(cx, |preview, cx| preview.release(window, cx));
                self.artifact_preview.subscriptions.clear();
                self.artifact_preview.artifact_id = None;
                if let Some(focus) = self.artifact_preview.return_focus.take() {
                    focus.focus(window, cx);
                }
            }
        } else if self.artifact_preview.focus_pending {
            self.artifact_preview.focus_pending = false;
            if let Some(view) = &self.artifact_preview.view {
                view.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
    }

    pub(super) fn artifact_preview_layer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let content = div().children(self.artifact_preview.view.clone());
        flowy_ui::modal_layer(
            "artifact-preview-layer",
            &self.theme,
            &self.ui,
            content,
        )
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|app, _, _, cx| {
                app.active_dialog = ActiveDialog::None;
                cx.notify();
            }),
        )
    }
}
