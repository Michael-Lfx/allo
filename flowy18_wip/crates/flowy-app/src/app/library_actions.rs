use super::{FlowyApp, Section};
use flowy_studio::library::{LibraryEvent, LibraryUsage};
use gpui::Context;
use std::collections::HashSet;
use std::io;
use std::path::Path;

#[derive(Clone, Copy)]
enum ArtifactAction {
    Favorite(bool),
    Open,
}

impl FlowyApp {
    pub(super) fn load_library(&mut self, cx: &mut Context<'_, Self>) {
        self.library_generation = self.library_generation.wrapping_add(1);
        let generation = self.library_generation;
        let agent = self.studio.read(cx).agent.clone();
        self.library
            .update(cx, |panel, cx| panel.set_loading(true, cx));
        if agent.is_initializing() {
            return;
        }
        let store = agent.artifact_store();
        let job = cx.background_executor().spawn(async move {
            let store = store.map_err(|_| "library.error.catalog")?;
            let records = store.list().map_err(|_| "library.error.catalog")?;
            let sessions = agent.list_sessions();
            let source_ids = records
                .iter()
                .map(|record| record.manifest.receipt.session_id.clone())
                .collect::<HashSet<_>>();
            let deleted = source_ids
                .iter()
                .filter(|id| agent.source_deleted(id))
                .cloned()
                .collect::<HashSet<_>>();
            let unavailable = source_ids
                .iter()
                .filter(|id| {
                    !deleted.contains(*id) && !agent.source_available(id)
                })
                .cloned()
                .collect::<HashSet<_>>();
            let mut items = crate::library::items(records, &sessions, &deleted);
            for item in &mut items {
                item.source_unavailable |=
                    unavailable.contains(&item.source_session_id);
            }
            let usage = store.usage().ok().map(|usage| LibraryUsage {
                staging_bytes: usage.staging_bytes,
                view_bytes: usage.view_bytes,
            });
            let has_diagnostics = store
                .diagnostics()
                .map_or(true, |issues| !issues.is_empty());
            Ok::<_, &'static str>((items, usage, has_diagnostics))
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| {
                if generation != app.library_generation {
                    return;
                }
                if let Ok((items, _, _)) = &result {
                    app.studio.update(cx, |panel, cx| {
                        panel.set_artifacts(items.clone(), cx);
                    });
                    app.sync_artifact_preview(items, cx);
                }
                app.library.update(cx, |panel, cx| {
                    panel.set_loading(false, cx);
                    match result {
                        Ok((items, usage, issues)) => {
                            panel.set_items(items, cx);
                            if let Some(usage) = usage {
                                panel.set_usage(usage, cx);
                            }
                            panel.set_status(
                                issues.then_some("library.error.catalog"),
                                cx,
                            );
                        }
                        Err(error) => panel.set_status(Some(error), cx),
                    }
                });
            });
        })
        .detach();
    }

    pub(super) fn library_event(
        &mut self,
        event: &LibraryEvent,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            LibraryEvent::PreviewRequested(id) => {
                self.open_artifact_preview(id, cx);
            }
            LibraryEvent::ReadPreviewRequested {
                artifact_id,
                request_id,
            } => {
                let id = artifact_id.clone();
                let request_id = *request_id;
                let store = self.studio.read(cx).agent.artifact_store();
                let job = cx.background_executor().spawn(async move {
                    store
                        .map_err(|_| "library.error.catalog")?
                        .read_payload(&id)
                        .map_err(preview_error)
                });
                let id = artifact_id.clone();
                cx.spawn(async move |weak, cx| {
                    let result = job.await;
                    let _ = weak.update(cx, |app, cx| {
                        app.library.update(cx, |panel, cx| {
                            panel
                                .set_preview_bytes(&id, request_id, result, cx);
                        });
                    });
                })
                .detach();
            }
            LibraryEvent::SaveAsRequested(id) => self.save_artifact_as(id, cx),
            LibraryEvent::OpenWithDefaultRequested(id) => {
                self.artifact_action(id, ArtifactAction::Open, cx);
            }
            LibraryEvent::SetFavoriteRequested {
                artifact_id,
                favorite,
            } => self.artifact_action(
                artifact_id,
                ArtifactAction::Favorite(*favorite),
                cx,
            ),
            LibraryEvent::LocateSourceRequested {
                session_id,
                artifact_id,
            } => self.navigate_to_artifact(session_id, artifact_id, cx),
            LibraryEvent::StartConversationRequested => {
                self.start_library_conversation(cx);
            }
            LibraryEvent::ClearFiltersRequested => {}
            LibraryEvent::CleanViewCopiesRequested => {
                self.request_artifact_cleanup(cx);
            }
        }
    }

    fn artifact_action(
        &self,
        artifact_id: &str,
        action: ArtifactAction,
        cx: &Context<'_, Self>,
    ) {
        let feedback = self.artifact_feedback(artifact_id);
        let id = artifact_id.to_owned();
        let store = self.studio.read(cx).agent.artifact_store();
        let job_id = id;
        let job = cx.background_executor().spawn(async move {
            let store = store.map_err(io::Error::other)?;
            match action {
                ArtifactAction::Favorite(favorite) => {
                    store.set_favorite(&job_id, favorite)
                }
                ArtifactAction::Open => {
                    let copy = store.create_view_copy(&job_id)?;
                    // Launching an external app does not prove when it releases the
                    // file. Keep and account for its copy until release is confirmed.
                    open::that(copy.path).map_err(io::Error::other)
                }
            }
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| match result {
                Ok(()) => app.load_library(cx),
                Err(_) => app.report_artifact_feedback(
                    &feedback,
                    Some(match action {
                        ArtifactAction::Open => "library.error.open",
                        ArtifactAction::Favorite(_) => "library.error.save",
                    }),
                    cx,
                ),
            });
        })
        .detach();
    }

    fn navigate_to_artifact(
        &mut self,
        session_id: &str,
        artifact_id: &str,
        cx: &Context<'_, Self>,
    ) {
        self.artifact_navigation_generation =
            self.artifact_navigation_generation.wrapping_add(1);
        let generation = self.artifact_navigation_generation;
        let origin_section = self.section;
        let feedback = self.artifact_feedback(artifact_id);
        let agent = self.studio.read(cx).agent.clone();
        let previous = agent.active_session_id();
        let session_id = session_id.to_owned();
        let artifact_id = artifact_id.to_owned();
        let target = session_id.clone();
        let anchor = artifact_id.clone();
        let job = cx.background_executor().spawn(async move {
            if !agent.source_available(&target) { return Err("library.source.unavailable"); }
            agent.select_session_if_active(&target, previous.as_deref())?;
            if !agent.chat_history().rows.iter().any(|row| matches!(row, flowy_agent::ChatLine::Artifact { artifact_id } if artifact_id == &anchor)) {
                if let Some(previous) = previous { let _ = agent.select_session_if_active(&previous, Some(&target)); }
                else { let _ = agent.begin_home_conversation_if_active(Some(&target)); }
                return Err("library.source.unavailable");
            }
            Ok(())
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| {
                if generation != app.artifact_navigation_generation
                    || app.section != origin_section
                {
                    return;
                }
                if result.is_ok()
                    && app.studio.read(cx).agent.active_session_id().as_deref()
                        == Some(&session_id)
                {
                    let focused = app.studio.update(cx, |panel, cx| {
                        panel.show_selected_artifact(&artifact_id, cx)
                    });
                    if focused.is_ok() {
                        app.active_dialog = super::state::ActiveDialog::None;
                        app.push_section(Section::Studio, cx);
                        return;
                    }
                }
                app.report_artifact_feedback(
                    &feedback,
                    Some("library.source.unavailable"),
                    cx,
                );
            });
        })
        .detach();
    }

    pub(super) fn start_library_conversation(
        &mut self,
        cx: &Context<'_, Self>,
    ) {
        let origin_section = self.section;
        self.artifact_navigation_generation =
            self.artifact_navigation_generation.wrapping_add(1);
        let generation = self.artifact_navigation_generation;
        let agent = self.studio.read(cx).agent.clone();
        let previous = agent.active_session_id();
        let job = cx.background_executor().spawn(async move {
            agent.begin_home_conversation_if_active(previous.as_deref())
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| {
                if generation != app.artifact_navigation_generation
                    || app.section != origin_section
                {
                    return;
                }
                match result {
                    Ok(()) => {
                        app.studio.update(
                            cx,
                            flowy_studio::StudioPanel::show_new_home,
                        );
                        app.push_section(Section::Studio, cx);
                    }
                    Err(error) => app.note(error, cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn save_artifact_as(
        &self,
        artifact_id: &str,
        cx: &Context<'_, Self>,
    ) {
        let Some(name) = self.library.read(cx).model.items.iter().find(|item| item.id == artifact_id && item.availability == flowy_studio::library::ArtifactAvailability::Available).map(|item| item.title.clone()) else { return; };
        let suggested_name = Path::new(&name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_owned();
        let directory = directories::UserDirs::new()
            .and_then(|dirs| dirs.document_dir().map(Path::to_path_buf))
            .unwrap_or_default();
        let selected =
            cx.prompt_for_new_path(&directory, Some(&suggested_name));
        let store = self.studio.read(cx).agent.artifact_store();
        let id = artifact_id.to_owned();
        let feedback = self.artifact_feedback(artifact_id);
        cx.spawn(async move |weak, cx| {
            let destination = match selected.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                _ => {
                    let _ = weak.update(cx, |app, cx| {
                        app.report_artifact_feedback(
                            &feedback,
                            Some("library.error.export"),
                            cx,
                        );
                    });
                    return;
                }
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    store.map_err(io::Error::other)?.copy_to(&id, &destination)
                })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.report_artifact_feedback(
                    &feedback,
                    result.err().map(|_| "library.error.export"),
                    cx,
                );
            });
        })
        .detach();
    }
}

fn preview_error(error: io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "library.error.missing",
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            "library.availability.corrupt"
        }
        _ => "library.error.read",
    }
}
