use super::{FlowyApp, state::ActiveDialog};
use flowy_agent::ArtifactStore;
use flowy_ui as ui;
use gpui::{Context, IntoElement, ParentElement, Window, div, prelude::*, px};
use std::collections::BTreeSet;
use std::io;

/// View copies belong to external applications until the user explicitly
/// confirms that those applications have closed. No timer or shutdown hook
/// guesses that it is safe to remove them.
#[derive(Default)]
pub(super) struct ArtifactCleanupState {
    pub(super) busy: bool,
    error: Option<&'static str>,
    generation: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CleanupOutcome {
    removed: usize,
    failed: usize,
    remaining: usize,
}

impl CleanupOutcome {
    const fn complete(&self) -> bool {
        self.failed == 0 && self.remaining == 0
    }
}

/// Take a bounded inventory, then remove only those controlled view request
/// IDs. A copy created concurrently after the inventory is retained for the
/// next explicit confirmation. The store owns path validation and locking.
fn cleanup_view_copies(store: &ArtifactStore) -> io::Result<CleanupOutcome> {
    let requests = store
        .pending_views()?
        .into_iter()
        .map(|copy| copy.request_id)
        .collect::<BTreeSet<_>>();
    let mut outcome = CleanupOutcome::default();
    for request_id in requests {
        match store.remove_view_copy(&request_id) {
            Ok(()) => outcome.removed += 1,
            // Windows can retain an open handle even after an application
            // window closes. Leave those bytes allocated and offer retry.
            Err(_) => outcome.failed += 1,
        }
    }
    outcome.remaining = store
        .pending_views()?
        .into_iter()
        .map(|copy| copy.request_id)
        .collect::<BTreeSet<_>>()
        .len();
    Ok(outcome)
}

impl FlowyApp {
    pub(super) fn request_artifact_cleanup(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        if self.artifact_cleanup.busy {
            return;
        }
        self.artifact_cleanup.generation =
            self.artifact_cleanup.generation.wrapping_add(1);
        self.artifact_cleanup.error = None;
        self.activate_dialog(ActiveDialog::ArtifactCleanup, cx);
    }

    pub(super) fn artifact_cleanup_layer(
        &self,
        window: &Window,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let frame = self.theme.modal_size(window.viewport_size());
        let dialog = ui::dialog(
            "artifact-cleanup-card",
            self.t("library.storage.clean_views.title"),
            &self.theme,
            &self.ui,
            cx,
        )
        .w(px(440.))
        .max_w(frame.width)
        .max_h(frame.height)
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .overflow_hidden()
        .child(
            ui::Title::new(
                ui::TitleLevel::Card,
                self.t("library.storage.clean_views.title"),
            )
            .render(&self.theme),
        )
        .child(
            ui::Text::new(
                ui::TextRole::Body,
                self.t("library.storage.clean_views.body"),
            )
            .render(&self.theme)
            .whitespace_normal(),
        )
        .when_some(self.artifact_cleanup.error, |dialog, error| {
            dialog.child(
                ui::Text::new(ui::TextRole::Body, self.t(error))
                    .render(&self.theme)
                    .text_color(self.theme.danger())
                    .whitespace_normal(),
            )
        })
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .child(
                    ui::Button::new(
                        "cancel-artifact-cleanup",
                        self.t("library.cancel"),
                    )
                    .disabled(self.artifact_cleanup.busy)
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            if app.artifact_cleanup.busy {
                                return;
                            }
                            app.active_dialog = ActiveDialog::None;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    ui::Button::new(
                        "confirm-artifact-cleanup",
                        self.t("library.storage.clean_views.confirm"),
                    )
                    .variant(ui::ControlVariant::Danger)
                    .disabled(self.artifact_cleanup.busy)
                    .render(
                        &self.theme,
                        cx.listener(|app, _, _, cx| {
                            app.confirm_artifact_cleanup(cx);
                        }),
                    ),
                ),
        );
        ui::modal_layer("artifact-cleanup-layer", &self.theme, &self.ui, dialog)
    }

    fn confirm_artifact_cleanup(&mut self, cx: &mut Context<'_, Self>) {
        if self.artifact_cleanup.busy
            || self.active_dialog != ActiveDialog::ArtifactCleanup
        {
            return;
        }
        self.artifact_cleanup.busy = true;
        self.artifact_cleanup.error = None;
        self.artifact_cleanup.generation =
            self.artifact_cleanup.generation.wrapping_add(1);
        let generation = self.artifact_cleanup.generation;
        let store = self.studio.read(cx).agent.artifact_store();
        let job = cx.background_executor().spawn(async move {
            cleanup_view_copies(&store.map_err(io::Error::other)?)
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |app, cx| {
                if app.artifact_cleanup.generation != generation {
                    return;
                }
                app.artifact_cleanup.busy = false;
                let complete =
                    result.as_ref().is_ok_and(CleanupOutcome::complete);
                app.artifact_cleanup.error =
                    (!complete).then_some("library.error.trash_failed");
                // Recompute actual usage after every attempt. A failed remove
                // never subtracts promised bytes from the quota projection.
                app.load_library(cx);
                if complete
                    && app.active_dialog == ActiveDialog::ArtifactCleanup
                {
                    app.active_dialog = ActiveDialog::None;
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_view_copies;
    use flowy_agent::ArtifactStore;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("build-test")
                .join(format!(
                    "artifact-cleanup-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn store_with_copies(&self) -> ArtifactStore {
            let store = ArtifactStore::open(self.0.join("artifacts")).unwrap();
            for id in ["view-one", "view-two"] {
                let dir = store.root().join("views").join(id);
                fs::create_dir(&dir).unwrap();
                fs::write(dir.join("artifact.txt"), b"view bytes").unwrap();
            }
            // Unrelated retained snapshot and user-selected export are outside
            // the only subtree this cleanup controller is authorized to touch.
            let item = store.root().join("items/retained");
            fs::create_dir(&item).unwrap();
            fs::write(item.join("payload"), b"primary snapshot").unwrap();
            fs::write(self.0.join("user-export.txt"), b"user export").unwrap();
            store
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn explicit_cleanup_removes_only_view_copies_and_reports_actual_usage() {
        let scratch = Scratch::new();
        let store = scratch.store_with_copies();
        assert_eq!(store.usage().unwrap().view_bytes, 20);
        let outcome = cleanup_view_copies(&store).unwrap();
        assert!(outcome.complete());
        assert_eq!(outcome.removed, 2);
        assert_eq!(store.usage().unwrap().view_bytes, 0);
        assert_eq!(
            fs::read(store.root().join("items/retained/payload")).unwrap(),
            b"primary snapshot"
        );
        assert_eq!(
            fs::read(scratch.0.join("user-export.txt")).unwrap(),
            b"user export"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_occupied_view_is_retained_until_a_later_confirmed_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let scratch = Scratch::new();
        let store = scratch.store_with_copies();
        let path = store.root().join("views/view-one/artifact.txt");
        let occupied = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let first = cleanup_view_copies(&store).unwrap();
        assert!(!first.complete());
        assert_eq!(first.failed, 1);
        assert_eq!(first.remaining, 1);
        assert!(path.exists());
        assert_eq!(store.usage().unwrap().view_bytes, 10);
        drop(occupied);
        let retry = cleanup_view_copies(&store).unwrap();
        assert!(retry.complete());
        assert_eq!(store.usage().unwrap().view_bytes, 0);
    }
}
