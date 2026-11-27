use super::{FlowyApp, state::ActiveDialog};
use flowy_studio::library::{
    ArtifactAvailability, Category, LibraryEvent, LibraryItem,
    MAX_LOCAL_PREVIEW_BYTES, PreviewEvent, PreviewPanel,
};
use gpui::{
    Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement,
    Subscription, Window, div, prelude::*,
};
use std::io::Read;
use std::path::{Path, PathBuf};

type PreviewReadJob = gpui::Task<
    Result<(PreviewSource, Result<Vec<u8>, &'static str>), &'static str>,
>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreviewSource {
    Artifact(String),
    LocalFile(PathBuf),
}

#[derive(Default)]
pub(super) struct ArtifactPreviewState {
    view: Option<Entity<PreviewPanel>>,
    source: Option<PreviewSource>,
    request_id: u64,
    return_focus: Option<FocusHandle>,
    focus_pending: bool,
    retired: Vec<Entity<PreviewPanel>>,
    subscriptions: Vec<Subscription>,
}

impl ArtifactPreviewState {
    const fn artifact_id(&self) -> Option<&str> {
        match &self.source {
            Some(PreviewSource::Artifact(id)) => Some(id.as_str()),
            _ => None,
        }
    }
}

pub(super) struct ArtifactFeedback {
    artifact_id: String,
    request_id: Option<u64>,
}

fn category_for_local_ext(ext: &str) -> Category {
    match ext {
        "pdf" | "md" | "markdown" | "txt" | "log" | "json" | "yaml" | "yml"
        | "toml" | "xml" | "html" | "htm" => Category::Document,
        "pptx" => Category::Slides,
        "csv" | "tsv" => Category::Table,
        "png" | "jpg" | "jpeg" | "webp" => Category::Image,
        _ => Category::Other,
    }
}

fn read_local_preview_bytes(path: &Path) -> Result<Vec<u8>, &'static str> {
    let (_safe_path, file, meta) =
        flowy_studio::ports::open_safe_local_regular_file(path).map_err(
            |error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    "library.error.missing"
                } else {
                    "library.error.read"
                }
            },
        )?;
    if meta.len() > MAX_LOCAL_PREVIEW_BYTES {
        return Err("library.error.large");
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(meta.len().min(MAX_LOCAL_PREVIEW_BYTES)).unwrap_or(0),
    );
    file.take(MAX_LOCAL_PREVIEW_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "library.error.read")?;
    if bytes.len() as u64 > MAX_LOCAL_PREVIEW_BYTES {
        return Err("library.error.large");
    }
    Ok(bytes)
}

impl FlowyApp {
    pub(super) fn artifact_feedback(
        &self,
        artifact_id: &str,
    ) -> ArtifactFeedback {
        ArtifactFeedback {
            artifact_id: artifact_id.to_owned(),
            request_id: (self.active_dialog == ActiveDialog::ArtifactPreview
                && self.artifact_preview.artifact_id() == Some(artifact_id))
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
            && self.artifact_preview.artifact_id()
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
            && let Some(active_id) = self.artifact_preview.artifact_id()
            && let Some(item) = items.iter().find(|item| item.id == active_id)
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

    fn open_preview_panel(
        &mut self,
        item: LibraryItem,
        source: PreviewSource,
        return_focus: Option<FocusHandle>,
        cx: &mut Context<'_, Self>,
    ) {
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
        self.artifact_preview.source = Some(source);
        self.artifact_preview.return_focus = return_focus;
        self.artifact_preview.focus_pending = true;
        self.artifact_preview.subscriptions = vec![subscription, observer];
        self.activate_dialog(ActiveDialog::ArtifactPreview, cx);
        view.update(cx, |preview, cx| preview.request_bytes(request_id, cx));
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
        self.open_preview_panel(
            item,
            PreviewSource::Artifact(artifact_id.to_owned()),
            return_focus,
            cx,
        );
    }

    pub(super) fn open_local_file_preview(
        &mut self,
        raw_path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(safe_path) =
            flowy_studio::ports::resolve_safe_local_file(raw_path)
        else {
            self.note("library.error.missing", cx);
            return;
        };
        let file_name = safe_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_owned();
        let extension = safe_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map_or_default(str::to_ascii_lowercase);
        let size_bytes = std::fs::symlink_metadata(&safe_path)
            .ok()
            .filter(|meta| {
                meta.is_file()
                    && !flowy_studio::ports::is_link_or_unsafe_reparse(
                        &safe_path, meta,
                    )
            })
            .map(|meta| meta.len());
        let item = LibraryItem {
            id: String::new(),
            title: file_name.clone(),
            original_name: file_name,
            description: String::new(),
            category: category_for_local_ext(&extension),
            date: String::new(),
            time: String::new(),
            pending: false,
            preview: None,
            size_bytes,
            extension,
            media_type: None,
            delivered_at_ms: 0,
            delivered_date: String::new(),
            delivered_time: String::new(),
            source_session_id: String::new(),
            source_title: flowy_studio::ports::display_path(&safe_path),
            source_deleted: false,
            source_unavailable: false,
            favorite: false,
            availability: ArtifactAvailability::Available,
            override_used: false,
        };
        self.open_preview_panel(
            item,
            PreviewSource::LocalFile(safe_path),
            None,
            cx,
        );
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
                if self.active_dialog != ActiveDialog::ArtifactPreview {
                    return;
                }
                let request_id = *request_id;
                match self.artifact_preview.source.clone() {
                    Some(PreviewSource::Artifact(expected_id))
                        if expected_id == *artifact_id =>
                    {
                        let store = self.studio.read(cx).agent.artifact_store();
                        let expected = PreviewSource::Artifact(expected_id);
                        let id = artifact_id.clone();
                        let job = cx.background_executor().spawn(async move {
                            let result = store
                                .map_err(|_| "library.error.catalog")?
                                .read_payload(&id)
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
                            Ok::<_, &'static str>((expected, result))
                        });
                        Self::finish_preview_read(request_id, job, cx);
                    }
                    Some(PreviewSource::LocalFile(path))
                        if artifact_id.is_empty() =>
                    {
                        let expected = PreviewSource::LocalFile(path.clone());
                        let job = cx.background_executor().spawn(async move {
                            let result = read_local_preview_bytes(&path);
                            Ok::<_, &'static str>((expected, result))
                        });
                        Self::finish_preview_read(request_id, job, cx);
                    }
                    _ => {}
                }
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
            PreviewEvent::SaveAs(id) => {
                if !id.is_empty() {
                    self.save_artifact_as(id, cx);
                }
            }
            PreviewEvent::OpenDefault => match &self.artifact_preview.source {
                Some(PreviewSource::Artifact(id)) => {
                    let id = id.clone();
                    self.library_event(
                        &LibraryEvent::OpenWithDefaultRequested(id),
                        cx,
                    );
                }
                Some(PreviewSource::LocalFile(path)) => {
                    let result = path
                        .to_str()
                        .ok_or("library.error.open")
                        .and_then(flowy_studio::open_safe_local_file);
                    if let Err(notice) = result {
                        if let Some(view) = &self.artifact_preview.view {
                            view.update(cx, |preview, cx| {
                                preview.set_status(Some(notice), cx);
                            });
                        } else {
                            self.note(notice, cx);
                        }
                    }
                }
                None => {}
            },
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

    fn finish_preview_read(
        request_id: u64,
        job: PreviewReadJob,
        cx: &Context<'_, Self>,
    ) {
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| {
                if app.artifact_preview.request_id != request_id
                    || app.active_dialog != ActiveDialog::ArtifactPreview
                {
                    return;
                }
                let Some(preview) = &app.artifact_preview.view else {
                    return;
                };
                let result = match result {
                    Ok((source, result))
                        if app.artifact_preview.source.as_ref()
                            == Some(&source) =>
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
                self.artifact_preview.source = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::session::SessionStore;
    use gpui::TestAppContext;

    #[gpui::test]
    fn local_file_fullscreen_preview_loads_md_and_pdf_and_releases_cleanly(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir()
            .join(format!("flowy-app-local-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let md_path = root.join("notes.md");
        let pdf_path = root.join("sample.pdf");
        std::fs::write(&md_path, "# Release Notes\n\nAll green.").unwrap();
        std::fs::write(
            &pdf_path,
            include_bytes!(
                "../../../flowy-studio/examples/fixtures/preview/sample.pdf"
            ),
        )
        .unwrap();

        let (app, cx) = cx.add_window_view(|window, cx| {
            FlowyApp::new(SessionStore::new(root.clone()), false, window, cx)
        });
        cx.run_until_parked();

        // Missing file reports an error notice without opening modal preview.
        app.update(cx, |app, cx| {
            app.screen = Screen::Main;
            app.open_local_file_preview(
                root.join("missing.md").to_str().unwrap(),
                cx,
            );
            assert_eq!(app.active_dialog, ActiveDialog::None);
            assert_eq!(app.notice, Some("library.error.missing"));

            app.open_local_file_preview(md_path.to_str().unwrap(), cx);
            assert_eq!(app.active_dialog, ActiveDialog::ArtifactPreview);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        app.update(cx, |app, cx| {
            let view = app.artifact_preview.view.as_ref().expect("md view");
            assert!(view.read(cx).is_loaded());
            assert_eq!(view.read(cx).load_error(), None);
        });
        assert!(
            cx.debug_bounds("preview-open-default").is_some(),
            "local preview must paint default-open control"
        );
        assert!(
            cx.debug_bounds("preview-save-as").is_none(),
            "local preview must hide artifact-only Save As button"
        );

        // Open local PDF full-screen, ensure document loads cleanly, then close.
        app.update(cx, |app, cx| {
            app.open_local_file_preview(pdf_path.to_str().unwrap(), cx);
            assert_eq!(app.active_dialog, ActiveDialog::ArtifactPreview);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        app.update(cx, |app, cx| {
            let view = app.artifact_preview.view.as_ref().expect("pdf view");
            assert!(view.read(cx).is_loaded());
            assert_eq!(view.read(cx).load_error(), None);
            app.on_artifact_preview_event(&PreviewEvent::Close, cx);
            assert_eq!(app.active_dialog, ActiveDialog::None);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        app.update(cx, |app, _| {
            assert!(app.artifact_preview.view.is_none());
            assert!(app.artifact_preview.source.is_none());
        });

        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_local_preview_bytes_revalidates_paths_and_rejects_swaps() {
        let root = std::env::temp_dir()
            .join(format!("flowy-app-retry-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file_path = root.join("draft.md");
        std::fs::write(&file_path, b"# OK").unwrap();

        assert_eq!(read_local_preview_bytes(&file_path).unwrap(), b"# OK");
        assert_eq!(
            read_local_preview_bytes(Path::new("relative/draft.md")),
            Err("library.error.missing")
        );
        assert_eq!(
            read_local_preview_bytes(Path::new(r"\\attacker\share\draft.md")),
            Err("library.error.missing")
        );

        // Simulate path swapped into a non-regular entry before Retry.
        let target_dir = root.join("target_dir");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::remove_file(&file_path).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&file_path)
                .arg(&target_dir)
                .output()
                .expect("invoke mklink /J");
            assert!(status.status.success());
            assert_eq!(
                read_local_preview_bytes(&file_path),
                Err("library.error.missing")
            );
            let _ = std::fs::remove_dir(&file_path);
        }
        #[cfg(unix)]
        {
            let real_other = root.join("real_other.md");
            std::fs::write(&real_other, b"swapped").unwrap();
            std::os::unix::fs::symlink(&real_other, &file_path).unwrap();
            assert_eq!(
                read_local_preview_bytes(&file_path),
                Err("library.error.missing")
            );
            let _ = std::fs::remove_file(&file_path);
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
