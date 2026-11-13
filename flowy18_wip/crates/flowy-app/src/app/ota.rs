use super::FlowyApp;
use flowy_ui as theme;
use flowy_ota::{
    AvailableUpdate, BeginUpdateDownload, DownloadProgress, ReadyPackage,
    TakeReadyError, UpdateError, UpdatePackageCache, Updater,
};
use gpui::{Context, Task, div, prelude::*};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub(super) enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
        notes: Option<String>,
    },
    Downloading {
        version: String,
        received: u64,
        total: Option<u64>,
    },
    Ready {
        version: String,
    },
    Installing,
    Failed {
        key: &'static str,
    },
}

pub struct OtaController {
    pub status: UpdateStatus,
    pub cache: Arc<UpdatePackageCache>,
    pub error_detail: Option<String>,
    pub task: Option<Task<()>>,
    pub checked: bool,
}

impl OtaController {
    pub(super) fn new() -> Self {
        Self {
            status: UpdateStatus::Idle,
            cache: Arc::new(UpdatePackageCache::default()),
            error_detail: None,
            task: None,
            checked: false,
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}

impl FlowyApp {
    pub(super) fn start_update_check(&mut self, cx: &mut Context<'_, Self>) {
        if matches!(
            self.ota.status,
            UpdateStatus::Checking
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Installing
        ) {
            return;
        }
        self.ota.status = UpdateStatus::Checking;
        self.ota.error_detail = None;
        cx.notify();
        self.spawn_ota(cx, OtaJob::Check);
    }

    pub(super) fn start_update_download(&mut self, cx: &mut Context<'_, Self>) {
        let UpdateStatus::Available { version, .. } = &self.ota.status else {
            return;
        };
        let version = version.clone();
        match self
            .ota
            .cache
            .begin_download(&version, ReadyPackage::retained_len)
        {
            Ok(BeginUpdateDownload::AlreadyReady { retained_len }) => {
                self.ota.status = UpdateStatus::Ready {
                    version: version.clone(),
                };
                self.ota.error_detail = None;
                capture_update("update_already_ready", &version, |props| {
                    props.insert(
                        "already_ready".into(),
                        serde_json::Value::Bool(true),
                    );
                    props.insert(
                        "bytes_total".into(),
                        serde_json::Value::from(retained_len),
                    );
                });
                cx.notify();
            }
            Ok(
                BeginUpdateDownload::Start
                | BeginUpdateDownload::StartReplacingReady { .. },
            ) => {
                self.ota.status = UpdateStatus::Downloading {
                    version: version.clone(),
                    received: 0,
                    total: None,
                };
                self.ota.error_detail = None;
                capture_update("update_download_start", &version, |_| {});
                cx.notify();
                self.spawn_ota(cx, OtaJob::Download { version });
            }
            Err(message) => {
                self.ota.error_detail = Some(message);
                self.ota.status = UpdateStatus::Failed {
                    key: "ota.err_busy",
                };
                cx.notify();
            }
        }
    }

    pub(super) fn start_update_install(&mut self, cx: &mut Context<'_, Self>) {
        let UpdateStatus::Ready { version } = &self.ota.status else {
            self.ota.status = UpdateStatus::Failed {
                key: "ota.err_not_ready",
            };
            cx.notify();
            return;
        };
        let version = version.clone();
        match self.ota.cache.take_ready(&version) {
            Ok(package) => {
                self.ota.status = UpdateStatus::Installing;
                cx.notify();
                self.spawn_ota(cx, OtaJob::Install { package });
            }
            Err(TakeReadyError::NotRetained { .. }) => {
                self.ota.status = UpdateStatus::Failed {
                    key: "ota.err_not_ready",
                };
                cx.notify();
            }
            Err(error) => {
                self.ota.error_detail = Some(error.message());
                self.ota.status = UpdateStatus::Failed {
                    key: "ota.err_busy",
                };
                cx.notify();
            }
        }
    }

    fn spawn_ota(&mut self, cx: &Context<'_, Self>, job: OtaJob) {
        let locale = self.i18n.locale().code().to_owned();
        let data_dir = self.store.root().to_path_buf();
        let cache = Arc::clone(&self.ota.cache);
        let outcome_slot: Arc<Mutex<Option<OtaOutcome>>> =
            Arc::new(Mutex::new(None));
        let progress_slot: Arc<Mutex<Option<DownloadProgress>>> =
            Arc::new(Mutex::new(None));
        let outcome_writer = Arc::clone(&outcome_slot);
        let progress_writer = Arc::clone(&progress_slot);
        let progress_counters = cache.progress_counters();
        std::thread::spawn(move || {
            let outcome = run_job(
                job,
                data_dir,
                locale,
                cache,
                progress_writer,
                progress_counters,
            );
            if let Ok(mut guard) = outcome_writer.lock() {
                *guard = Some(outcome);
            }
        });
        self.ota.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                if let Some(progress) =
                    progress_slot.lock().ok().and_then(|mut guard| guard.take())
                {
                    let _ = weak.update(cx, |app, cx| {
                        if let UpdateStatus::Downloading {
                            received,
                            total,
                            ..
                        } = &mut app.ota.status
                        {
                            *received = progress.received;
                            *total = progress.total;
                        }
                        cx.notify();
                    });
                }
                let outcome =
                    outcome_slot.lock().ok().and_then(|mut guard| guard.take());
                let Some(outcome) = outcome else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    app.apply_ota_outcome(outcome, cx);
                    cx.notify();
                });
                break;
            }
        }));
    }

    fn apply_ota_outcome(
        &mut self,
        outcome: OtaOutcome,
        cx: &Context<'_, Self>,
    ) {
        match outcome {
            OtaOutcome::Checked(Ok(None)) => {
                self.ota.status = UpdateStatus::UpToDate;
            }
            OtaOutcome::Checked(Ok(Some(update))) => {
                let version = update.version.clone();
                self.ota.status = UpdateStatus::Available {
                    version: update.version,
                    notes: update.notes,
                };
                self.notify_update(
                    flowy_shell::NotifyKind::Available,
                    &version,
                    cx,
                );
            }
            OtaOutcome::Checked(Err(error))
            | OtaOutcome::Downloaded(Err(error)) => {
                self.ota.error_detail = Some(error.to_string());
                self.ota.status = UpdateStatus::Failed { key: error.key() };
            }
            OtaOutcome::Downloaded(Ok(version)) => {
                capture_update("update_download_ready", &version, |props| {
                    props.insert(
                        "phase".into(),
                        serde_json::Value::String("ready".into()),
                    );
                });
                self.notify_update(
                    flowy_shell::NotifyKind::Ready,
                    &version,
                    cx,
                );
                self.ota.status = UpdateStatus::Ready { version };
            }
            OtaOutcome::Installed(Ok(())) => {
                self.ota.status = UpdateStatus::Idle;
            }
            OtaOutcome::Installed(Err(error)) => {
                self.ota.error_detail = Some(error.to_string());
                let key = match &error {
                    UpdateError::InstallBlocked(key) => *key,
                    other => other.key(),
                };
                let status = self.ota.cache.status();
                self.ota.status = if status.state == "ready" {
                    UpdateStatus::Ready {
                        version: status.version.unwrap_or_default(),
                    }
                } else {
                    UpdateStatus::Failed { key }
                };
            }
        }
    }

    fn notify_update(
        &self,
        kind: flowy_shell::NotifyKind,
        version: &str,
        cx: &Context<'_, Self>,
    ) {
        let (title_key, body_key) = match kind {
            flowy_shell::NotifyKind::Available => {
                ("shell.notify_update_title", "shell.notify_update_body")
            }
            flowy_shell::NotifyKind::Ready => {
                ("shell.notify_ready_title", "shell.notify_ready_body")
            }
        };
        let title = self.t(title_key);
        let body = self.tf(body_key, &[("version", version)]);
        let open = self.t("shell.notify_open");
        flowy_shell::show_update_notification(
            cx, kind, &title, &body, &open,
        );
    }

    pub(super) fn about_body(&self, cx: &Context<'_, Self>) -> gpui::Div {
        let version = env!("CARGO_PKG_VERSION");
        let mut column = div()
            .flex_1()
            .min_h_0()
            .px_8()
            .pt_4()
            .pb_8()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .child(self.tf("ota.current", &[("version", version)])),
            )
            .child(self.update_status_line())
            .child(self.update_actions(cx));
        if let Some(detail) = &self.ota.error_detail {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(detail.clone()),
            );
        }
        if let UpdateStatus::Available {
            notes: Some(notes), ..
        } = &self.ota.status
        {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(theme::muted())
                    .whitespace_normal()
                    .child(notes.clone()),
            );
        }
        column
    }

    fn update_status_line(&self) -> impl IntoElement {
        let text = match &self.ota.status {
            UpdateStatus::Idle => self.t("ota.idle"),
            UpdateStatus::Checking => self.t("ota.checking"),
            UpdateStatus::UpToDate => self.t("ota.up_to_date"),
            UpdateStatus::Available { version, .. } => {
                self.tf("ota.available", &[("version", version)])
            }
            UpdateStatus::Downloading {
                version,
                received,
                total,
            } => {
                let progress = match total {
                    Some(total) if *total > 0 => {
                        format!("{} / {}", grouped(*received), grouped(*total))
                    }
                    _ => grouped(*received),
                };
                self.tf(
                    "ota.downloading",
                    &[("version", version), ("progress", &progress)],
                )
            }
            UpdateStatus::Ready { version } => {
                self.tf("ota.ready", &[("version", version)])
            }
            UpdateStatus::Installing => self.t("ota.installing"),
            UpdateStatus::Failed { key } => self.t(key),
        };
        div().text_sm().text_color(theme::muted()).child(text)
    }

    fn update_actions(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let can_check = matches!(
            self.ota.status,
            UpdateStatus::Idle
                | UpdateStatus::UpToDate
                | UpdateStatus::Failed { .. }
                | UpdateStatus::Available { .. }
                | UpdateStatus::Ready { .. }
        );
        let mut row = div().flex().gap_2().flex_wrap().child(
            div()
                .id("ota-check")
                .px_3()
                .py_2()
                .rounded_lg()
                .text_sm()
                .bg(if can_check {
                    theme::fill()
                } else {
                    theme::soft()
                })
                .text_color(if can_check {
                    theme::on_fill()
                } else {
                    theme::muted()
                })
                .when(can_check, |button| {
                    button.cursor_pointer().on_click(cx.listener(
                        |app, _event, _window, cx| {
                            app.start_update_check(cx);
                        },
                    ))
                })
                .child(self.t("ota.check")),
        );
        if matches!(self.ota.status, UpdateStatus::Available { .. }) {
            row = row.child(
                div()
                    .id("ota-download")
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .text_sm()
                    .bg(theme::fill())
                    .text_color(theme::on_fill())
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.start_update_download(cx);
                    }))
                    .child(self.t("ota.download")),
            );
        }
        if matches!(self.ota.status, UpdateStatus::Ready { .. }) {
            row = row.child(
                div()
                    .id("ota-install")
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .text_sm()
                    .bg(theme::fill())
                    .text_color(theme::on_fill())
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.start_update_install(cx);
                    }))
                    .child(self.t("ota.install")),
            );
        }
        row
    }
}

enum OtaJob {
    Check,
    Download { version: String },
    Install { package: ReadyPackage },
}

enum OtaOutcome {
    Checked(Result<Option<AvailableUpdate>, UpdateError>),
    Downloaded(Result<String, UpdateError>),
    Installed(Result<(), UpdateError>),
}

fn run_job(
    job: OtaJob,
    data_dir: std::path::PathBuf,
    locale: String,
    cache: Arc<UpdatePackageCache>,
    progress: Arc<Mutex<Option<DownloadProgress>>>,
    counters: Arc<flowy_ota::DownloadProgressCounters>,
) -> OtaOutcome {
    let updater = match Updater::new(
        crate::brand::channel_config(),
        data_dir,
        Some(locale),
    ) {
        Ok(updater) => updater,
        Err(error) => {
            return match job {
                OtaJob::Check => OtaOutcome::Checked(Err(error)),
                OtaJob::Download { version } => {
                    cache.cancel_download(&version);
                    OtaOutcome::Downloaded(Err(error))
                }
                OtaJob::Install { package } => {
                    cache.restore_ready(package.version.clone(), package);
                    OtaOutcome::Installed(Err(error))
                }
            };
        }
    };
    match job {
        OtaJob::Check => OtaOutcome::Checked(updater.check()),
        OtaJob::Download { version } => {
            let result = updater.download(&version, &mut |current| {
                counters.set(current.received, current.total);
                if let Ok(mut guard) = progress.lock() {
                    *guard = Some(current);
                }
            });
            match result {
                Ok(package) => match cache.finish_download(&version, package) {
                    Ok(()) => OtaOutcome::Downloaded(Ok(version)),
                    Err(message) => {
                        cache.cancel_download(&version);
                        OtaOutcome::Downloaded(Err(UpdateError::Busy(message)))
                    }
                },
                Err(error) => {
                    cache.cancel_download(&version);
                    OtaOutcome::Downloaded(Err(error))
                }
            }
        }
        OtaJob::Install { package } => {
            let version = package.version.clone();
            match updater.install(&package) {
                Ok(()) => {
                    cache.finish_install(&version);
                    OtaOutcome::Installed(Ok(()))
                }
                Err(error) => {
                    cache.restore_ready(version, package);
                    OtaOutcome::Installed(Err(error))
                }
            }
        }
    }
}

fn capture_update(
    name: &str,
    version: &str,
    enrich: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) {
    let Some(telemetry) = crate::telemetry() else {
        return;
    };
    let mut props = serde_json::Map::new();
    props.insert(
        "to_version".into(),
        serde_json::Value::String(version.to_owned()),
    );
    props.insert(
        "from_version".into(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_owned()),
    );
    enrich(&mut props);
    telemetry.capture(name, props);
}

fn grouped(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::new();
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}
