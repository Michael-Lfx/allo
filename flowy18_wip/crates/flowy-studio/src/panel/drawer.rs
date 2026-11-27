use super::{StudioEvent, StudioPanel, StudioSurface};
use crate::drawer_preview::{DrawerPreviewBody, PreviewMode};
use flowy_agent::FileEntry;
use gpui::{Context, Focusable, Window};
use std::time::Duration;

impl StudioPanel {
    pub(crate) fn clear_right_sheet(&mut self) {
        self.right_drawer_open = false;
        self.deliverables = None;
        self.inspected_work = None;
        self.drawer_preview.clear();
        self.drawer_meta_generation =
            self.drawer_meta_generation.wrapping_add(1);
        self.drawer_meta_task = None;
        self.deliverable_sizes.clear();
        self.promote_project_candidate = None;
        self.refresh_trace_cache();
    }

    pub(crate) fn open_deliverables(
        &mut self,
        files: Vec<FileEntry>,
        cx: &mut Context<'_, Self>,
    ) {
        let had_input = !files.is_empty();
        let filtered = self.feed.read(cx).filter_settled_deliverables(files);
        if had_input && filtered.is_empty() {
            if self
                .drawer_preview
                .file
                .as_deref()
                .is_some_and(|path| self.feed.read(cx).is_path_failed(path))
            {
                self.drawer_preview.clear();
            }
            self.emit_notice(Some("verify.card.failed"), cx);
            return;
        }
        let quick_single = (filtered.len() == 1
            && self.feed.read(cx).failed_verifications().is_empty()
            && self.feed.read(cx).session_artifacts().is_empty())
        .then(|| filtered[0].path.clone());
        self.clear_right_sheet();
        self.right_drawer_open = true;
        self.deliverables = Some(
            crate::deliverables::DeliverableScope::TurnSnapshot(filtered),
        );
        self.deliverables_view_scope =
            crate::deliverables::DeliverablesViewScope::Latest;
        self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        self.close_all_popovers();
        self.refresh_drawer_metadata(cx);
        if let Some(single_path) = quick_single {
            self.set_preview_file(Some(single_path), cx);
        } else {
            cx.notify();
        }
    }

    /// Opens the right-hand inspector sheet for one running session or queued
    /// receipt without interrupting the user's current conversation.
    pub(crate) fn open_work_inspector(
        &mut self,
        work: crate::deliverables::InspectedWork,
        cx: &mut Context<'_, Self>,
    ) {
        self.clear_right_sheet();
        self.right_drawer_open = true;
        self.inspected_work = Some(work);
        self.drawer_tab = crate::deliverables::RightDrawerTab::Steps;
        self.close_all_popovers();
        cx.notify();
    }

    /// Toggles the unified right-hand 3-mode drawer (Deliverables / Steps / Trace),
    /// even before the turn has produced files so Steps and Trace stay reachable.
    pub(crate) fn toggle_deliverables(&mut self, cx: &mut Context<'_, Self>) {
        if self.right_drawer_open {
            self.close_right_sheet(cx);
            return;
        }
        let (latest_files, has_all, has_failures, has_artifacts) = {
            let feed = self.feed.read(cx);
            (
                feed.latest_deliverables(),
                feed.all_deliverable_items().is_some_and(|v| !v.is_empty()),
                !feed.failed_verifications().is_empty(),
                !feed.session_artifacts().is_empty(),
            )
        };
        self.clear_right_sheet();
        self.right_drawer_open = true;
        if let Some(files) = latest_files {
            self.deliverables =
                Some(crate::deliverables::DeliverableScope::Latest(files));
            self.deliverables_view_scope =
                crate::deliverables::DeliverablesViewScope::Latest;
            self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        } else if has_all || has_failures || has_artifacts {
            self.deliverables_view_scope = if has_all {
                crate::deliverables::DeliverablesViewScope::All
            } else {
                crate::deliverables::DeliverablesViewScope::Latest
            };
            self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        } else {
            self.drawer_tab = crate::deliverables::RightDrawerTab::Steps;
        }
        self.refresh_drawer_metadata(cx);
        self.close_all_popovers();
        cx.notify();
    }

    /// Closes whichever right-hand sheet is open (deliverables or single-work
    /// step inspector).
    pub(crate) fn close_right_sheet(&mut self, cx: &mut Context<'_, Self>) {
        self.clear_right_sheet();
        cx.notify();
    }

    /// Syncs whether the left dock sidebar is visible so the right drawer can
    /// clamp its width to keep the central chat canvas legible.
    pub fn set_dock_visible(
        &mut self,
        visible: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if self.dock_visible != visible {
            self.dock_visible = visible;
            cx.notify();
        }
    }

    /// Number of deliverables available in the current session's latest turn.
    pub(crate) fn deliverable_count(&self, cx: &Context<'_, Self>) -> usize {
        self.feed.read(cx).latest_deliverable_count()
    }

    fn is_preview_path_failed(
        &self,
        path: &str,
        cx: &Context<'_, Self>,
    ) -> bool {
        if self.surface == StudioSurface::Board {
            let target_key = crate::ports::canonical_path_key(path);
            return self.resolve_board_inspector().is_some_and(|group| {
                group.branches.iter().any(|branch| {
                    matches!(
                        &branch.phase,
                        crate::ports::BranchPhase::NeedsApproval {
                            artifact_path,
                            ..
                        } if crate::ports::canonical_path_key(artifact_path)
                            == target_key
                            || branch
                                .deliverable_path
                                .as_deref()
                                .is_some_and(|p| {
                                    crate::ports::canonical_path_key(p)
                                        == target_key
                                })
                    )
                })
            });
        }
        self.feed.read(cx).is_path_failed(path)
    }

    /// Picks a file to preview asynchronously inside the drawer; `None` returns
    /// to the deliverables list.
    pub(crate) fn set_preview_file(
        &mut self,
        path: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(target) = path.as_deref()
            && self.is_preview_path_failed(target, cx)
        {
            self.drawer_preview.clear();
            self.emit_notice(Some("verify.card.failed"), cx);
            return;
        }
        self.drawer_preview.clear();
        let Some(target) = path else {
            cx.notify();
            return;
        };
        let generation = self.drawer_preview.generation;
        self.drawer_preview.file = Some(target.clone());
        self.drawer_preview.mode = PreviewMode::Rendered;
        self.drawer_preview.body = Some(DrawerPreviewBody::Loading);
        let load_path = target;
        let job = cx.background_executor().spawn(async move {
            crate::drawer_preview::load_drawer_preview(&load_path)
        });
        self.drawer_preview.task =
            Some(cx.spawn(async move |weak, cx| {
                let loaded = job.await;
                let _ =
                    weak.update(cx, |panel, cx| {
                        if panel.drawer_preview.generation != generation {
                            return;
                        }
                        if panel.drawer_preview.file.as_deref().is_some_and(
                            |p| panel.is_preview_path_failed(p, cx),
                        ) {
                            panel.drawer_preview.clear();
                            panel.emit_notice(Some("verify.card.failed"), cx);
                            return;
                        }
                        panel.drawer_preview.body = Some(loaded);
                        panel.drawer_preview.task = None;
                        cx.notify();
                    });
            }));
        cx.notify();
    }

    pub(crate) fn toggle_drawer_preview_mode(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        self.drawer_preview.mode = match self.drawer_preview.mode {
            PreviewMode::Rendered => PreviewMode::Source,
            PreviewMode::Source => PreviewMode::Rendered,
        };
        cx.notify();
    }

    pub(crate) fn copy_drawer_preview(
        &mut self,
        generation: u64,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        if self.drawer_preview.generation != generation {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.drawer_preview.copied_generation = Some(generation);
        self.drawer_preview.copy_task =
            Some(cx.spawn(async move |weak, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(1500))
                    .await;
                let _ = weak.update(cx, |panel, cx| {
                    if panel.drawer_preview.copied_generation
                        == Some(generation)
                    {
                        panel.drawer_preview.copied_generation = None;
                        panel.drawer_preview.copy_task = None;
                        cx.notify();
                    }
                });
            }));
        cx.notify();
    }

    pub(crate) fn toggle_drawer_width(&mut self, cx: &mut Context<'_, Self>) {
        self.drawer_wide = !self.drawer_wide;
        cx.notify();
    }

    pub(crate) fn set_deliverables_view_scope(
        &mut self,
        scope: crate::deliverables::DeliverablesViewScope,
        cx: &mut Context<'_, Self>,
    ) {
        self.deliverables_view_scope = scope;
        if matches!(
            self.deliverables,
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(_))
        ) {
            self.deliverables = self
                .feed
                .read(cx)
                .latest_deliverables()
                .map(crate::deliverables::DeliverableScope::Latest);
        }
        self.refresh_drawer_metadata(cx);
        cx.notify();
    }

    pub(crate) fn toggle_step_turn(
        &mut self,
        session_id: &str,
        turn_ix: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let entry = self
            .step_turn_toggled
            .entry(session_id.to_owned())
            .or_default();
        if !entry.remove(&turn_ix) {
            entry.insert(turn_ix);
        }
        cx.notify();
    }

    pub(crate) fn toggle_step_item(
        &mut self,
        item_id: String,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.expanded_step_items.remove(&item_id) {
            self.expanded_step_items.insert(item_id);
        }
        cx.notify();
    }

    pub(crate) fn copy_drawer_step_text(
        &mut self,
        item_id: String,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.drawer_step_copy_epoch =
            self.drawer_step_copy_epoch.wrapping_add(1);
        let epoch = self.drawer_step_copy_epoch;
        self.drawer_step_copied = Some((item_id.clone(), epoch));
        self.drawer_step_copy_task = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = weak.update(cx, |panel, cx| {
                if panel.drawer_step_copied == Some((item_id, epoch)) {
                    panel.drawer_step_copied = None;
                    panel.drawer_step_copy_task = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(crate) fn open_step_file_in_deliverables(
        &mut self,
        path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.drawer_tab = crate::deliverables::RightDrawerTab::Deliverables;
        self.inspected_work = None;
        if self.deliverables.is_none() {
            self.deliverables = self
                .feed
                .read(cx)
                .latest_deliverables()
                .map(crate::deliverables::DeliverableScope::Latest);
        }
        if self.feed.read(cx).is_path_failed(path) {
            self.drawer_preview.clear();
            self.emit_notice(Some("verify.card.failed"), cx);
            return;
        }
        let target_key = crate::ports::canonical_path_key(path);
        let in_latest =
            self.feed.read(cx).latest_deliverable_items().is_some_and(
                |items| {
                    items.iter().any(|item| {
                        crate::ports::canonical_path_key(&item.entry.path)
                            == target_key
                    })
                },
            );
        if !in_latest {
            self.deliverables_view_scope =
                crate::deliverables::DeliverablesViewScope::All;
        }
        self.refresh_drawer_metadata(cx);
        self.set_preview_file(Some(path.to_owned()), cx);
    }

    fn ensure_composer_surface_from_board(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        if self.surface != StudioSurface::Board {
            return;
        }
        self.board_inspector.close_drawer();
        self.drawer_preview.clear();
        let board_sid = self
            .board_inspector
            .session
            .clone()
            .filter(|id| id != crate::ports::UNASSIGNED_QUEUE_SESSION_ID);
        if let Some(sid) = board_sid {
            self.open_session(&sid, cx);
        } else {
            self.surface = StudioSurface::Home;
            self.emit_notice(None, cx);
            cx.emit(StudioEvent::SessionsChanged);
            cx.notify();
        }
    }

    pub(crate) fn quote_file_to_composer(
        &mut self,
        path: &str,
        window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        self.ensure_composer_surface_from_board(cx);
        let name = crate::ports::basename(path);
        let display = crate::ports::display_path_str(path);
        let quote_text = self.tf(
            "drawer.quote_file_text",
            &[("name", &name), ("path", &display)],
        );
        self.set_active_quote(
            crate::composer::QuoteRole::Assistant,
            &quote_text,
            cx,
        );
        if let Some(window) = window {
            self.prompt.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    pub(crate) fn request_fix_in_chat(
        &mut self,
        summary: String,
        path: Option<String>,
        window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        self.ensure_composer_surface_from_board(cx);
        let fix_text = if let Some(path) = path.as_deref() {
            let name = crate::ports::basename(path);
            self.tf(
                "drawer.fix_prompt",
                &[("name", &name), ("issue", &summary)],
            )
        } else {
            self.tf("drawer.fix_prompt_generic", &[("issue", &summary)])
        };
        if self.prompt_text(cx).trim().is_empty() {
            self.fill_prompt(&fix_text, cx);
        } else {
            // Preserve the user's in-progress draft and attach fix context as a quote.
            self.set_active_quote(
                crate::composer::QuoteRole::Assistant,
                &fix_text,
                cx,
            );
        }
        if let Some(window) = window {
            self.prompt.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    pub(crate) fn locate_verification_card(
        &self,
        report_id: &str,
        artifact_path: &str,
        window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        let found = self.feed.update(cx, |feed, cx| {
            feed.focus_verification_row(report_id, artifact_path, cx)
        });
        if found && let Some(window) = window {
            window.refresh();
        }
        cx.notify();
    }

    pub(crate) fn visible_workspace_deliverables(
        &self,
        cx: &Context<'_, Self>,
    ) -> Vec<crate::feed::DeliverableItem> {
        let feed = self.feed.read(cx);
        match &self.deliverables {
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(
                files,
            )) => feed.filter_settled_deliverable_items(files.clone()),
            _ => match self.deliverables_view_scope {
                crate::deliverables::DeliverablesViewScope::Latest => feed
                    .latest_deliverable_items()
                    .or_else(|| feed.all_deliverable_items())
                    .unwrap_or_default(),
                crate::deliverables::DeliverablesViewScope::All => {
                    feed.all_deliverable_items().unwrap_or_default()
                }
            },
        }
    }

    pub(crate) fn refresh_drawer_metadata(&mut self, cx: &Context<'_, Self>) {
        self.drawer_meta_generation =
            self.drawer_meta_generation.wrapping_add(1);
        let generation = self.drawer_meta_generation;
        let items = self.visible_workspace_deliverables(cx);
        let paths: Vec<String> =
            items.into_iter().map(|item| item.entry.path).collect();
        if paths.is_empty() {
            self.deliverable_sizes.clear();
            self.promote_project_candidate = None;
            self.drawer_meta_task = None;
            return;
        }
        let check_promote = self.active_project_id.is_none();
        let store_root = self.store_root.clone();
        let job = cx.background_executor().spawn(async move {
            let mut sizes =
                std::collections::HashMap::with_capacity(paths.len());
            for path in &paths {
                let key = crate::ports::canonical_path_key(path);
                let size = crate::ports::resolve_safe_local_file(path)
                    .and_then(|safe| {
                        std::fs::symlink_metadata(&safe).ok().map(|m| (safe, m))
                    })
                    .filter(|(safe, meta)| {
                        meta.is_file()
                            && !crate::ports::is_link_or_unsafe_reparse(
                                safe, meta,
                            )
                    })
                    .map(|(_, meta)| meta.len());
                sizes.insert(key, size);
            }
            let promote_candidate = if check_promote {
                paths.first().and_then(|first| {
                    let parent_dir =
                        crate::ports::resolve_safe_parent_dir(first)?;
                    if parent_dir.parent().is_some()
                        && !crate::ports::is_same_or_overlapping_dir(
                            &parent_dir,
                            &store_root,
                        )
                    {
                        Some(first.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };
            (sizes, promote_candidate)
        });
        self.drawer_meta_task = Some(cx.spawn(async move |weak, cx| {
            let (sizes, promote_candidate) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                if panel.drawer_meta_generation != generation {
                    return;
                }
                panel.deliverable_sizes = sizes;
                panel.promote_project_candidate = promote_candidate;
                panel.drawer_meta_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn sync_right_drawer_deliverables(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.right_drawer_open {
            return;
        }
        let mut changed = false;
        let next = match &self.deliverables {
            Some(crate::deliverables::DeliverableScope::TurnSnapshot(
                existing,
            )) => {
                let filtered = self
                    .feed
                    .read(cx)
                    .filter_settled_deliverables(existing.clone());
                (!filtered.is_empty()).then_some(
                    crate::deliverables::DeliverableScope::TurnSnapshot(
                        filtered,
                    ),
                )
            }
            Some(crate::deliverables::DeliverableScope::Latest(_)) | None => {
                self.feed
                    .read(cx)
                    .latest_deliverables()
                    .map(crate::deliverables::DeliverableScope::Latest)
            }
        };
        if next != self.deliverables {
            self.deliverables = next;
            self.refresh_drawer_metadata(cx);
            changed = true;
        }
        if self
            .drawer_preview
            .file
            .as_deref()
            .is_some_and(|path| self.feed.read(cx).is_path_failed(path))
        {
            self.drawer_preview.clear();
            self.emit_notice(Some("verify.card.failed"), cx);
            changed = false;
        }
        if changed {
            cx.notify();
        }
    }
}
