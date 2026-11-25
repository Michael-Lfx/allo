use super::FlowyApp;
use super::skills::{InstallPhase, InstallUi, unix_now};
use gpui::Context;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

enum InstallNote {
    Progress(flowy_skills::InstallProgress),
    Done(Result<Vec<flowy_agent::InstalledSkill>, String>),
}

impl FlowyApp {
    fn run_skill_job<T, W, A>(
        &mut self,
        work: W,
        mut apply: A,
        interval: Duration,
        cx: &mut Context<'_, Self>,
    ) where
        T: Send + 'static,
        W: FnOnce(PathBuf, Arc<Mutex<VecDeque<T>>>) + Send + 'static,
        A: for<'a> FnMut(&mut Self, Vec<T>, &mut Context<'a, Self>) -> bool
            + 'static,
    {
        let root = self.store.root().to_path_buf();
        let notes = Arc::new(Mutex::new(VecDeque::new()));
        let writer = Arc::clone(&notes);
        std::thread::spawn(move || work(root, writer));
        self.skills.install_task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let Ok(mut notes) = notes.lock() else {
                    break;
                };
                let batch = notes.drain(..).collect::<Vec<_>>();
                drop(notes);
                if batch.is_empty() {
                    continue;
                }
                let mut finished = false;
                let updated = weak
                    .update(cx, |app, cx| {
                        finished = apply(app, batch, cx);
                    })
                    .is_ok();
                if finished || !updated {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn run_skill_catalog_job(
        &mut self,
        work: impl FnOnce(&Path) -> Result<Vec<flowy_agent::InstalledSkill>, String>
        + Send
        + 'static,
        cx: &mut Context<'_, Self>,
    ) {
        self.run_skill_job(
            move |root, notes| {
                let value = work(&root);
                if let Ok(mut notes) = notes.lock() {
                    notes.push_back(value);
                }
            },
            |app, mut batch, cx| {
                if let Some(value) = batch.pop() {
                    match value {
                        Ok(installed) => {
                            app.skills.installed.clone_from(&installed);
                            app.studio.update(cx, |panel, cx| {
                                panel.set_skill_catalog(installed, cx);
                            });
                        }
                        Err(error) => tracing::warn!(
                            target: "flowy.skills",
                            error = %error,
                            "skill management operation failed"
                        ),
                    }
                    cx.notify();
                }
                true
            },
            Duration::from_millis(50),
            cx,
        );
    }

    pub(super) fn set_auto_invoke(
        &mut self,
        id: flowy_skills::SkillId,
        value: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(skill) = self
            .skills
            .installed
            .iter()
            .find(|skill| skill.id == id)
            .cloned()
        else {
            return;
        };
        self.run_skill_catalog_job(
            move |root| {
                let store = flowy_skills::Store::open(
                    &root.join("skills").join("market.sqlite3"),
                )
                .map_err(|error| error.to_string())?;
                store
                    .set_auto_invoke(&skill.id, value)
                    .map_err(|error| error.to_string())?;
                flowy_skills::load_installed(root)
                    .map(|catalog| catalog.skills)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    pub(super) fn set_scripts_enabled(
        &mut self,
        id: flowy_skills::SkillId,
        value: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(skill) = self
            .skills
            .installed
            .iter()
            .find(|skill| skill.id == id)
            .cloned()
        else {
            return;
        };
        self.run_skill_catalog_job(
            move |root| {
                let store = flowy_skills::Store::open(
                    &root.join("skills").join("market.sqlite3"),
                )
                .map_err(|error| error.to_string())?;
                store
                    .set_scripts_enabled(&skill.id, value)
                    .map_err(|error| error.to_string())?;
                flowy_skills::load_installed(root)
                    .map(|catalog| catalog.skills)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    pub(super) fn uninstall_skill(
        &mut self,
        id: flowy_skills::SkillId,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(skill) = self
            .skills
            .installed
            .iter()
            .find(|skill| skill.id == id)
            .cloned()
        else {
            return;
        };
        if skill.origin == flowy_skills::InstallOrigin::Official {
            return;
        }
        self.run_skill_catalog_job(
            move |root| {
                let store = flowy_skills::Store::open(
                    &root.join("skills").join("market.sqlite3"),
                )
                .map_err(|error| error.to_string())?;
                let record = store
                    .installed()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|record| record.id == skill.id)
                    .ok_or_else(|| {
                        "installed skill record is missing".to_owned()
                    })?;
                let installer = flowy_skills::SkillInstaller::network(root)
                    .map_err(|error| error.to_string())?;
                installer
                    .remove(&record)
                    .map_err(|error| error.to_string())?;
                flowy_skills::load_installed(root)
                    .map(|catalog| catalog.skills)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    pub(super) fn check_skill_updates(&mut self, cx: &mut Context<'_, Self>) {
        self.run_skill_catalog_job(
            move |root| {
                let store = flowy_skills::Store::open(
                    &root.join("skills").join("market.sqlite3"),
                )
                .map_err(|error| error.to_string())?;
                let installer = flowy_skills::SkillInstaller::network(root)
                    .map_err(|error| error.to_string())?;
                for record in
                    store.installed().map_err(|error| error.to_string())?
                {
                    if record.origin == flowy_skills::InstallOrigin::Official {
                        continue;
                    }
                    if let Ok(check) = installer.check_update(&record) {
                        let latest = check
                            .available
                            .then(|| check.latest.version.clone());
                        let _ = store.set_latest_version(
                            &record.id,
                            latest.as_deref(),
                            unix_now(),
                        );
                    }
                }
                flowy_skills::load_installed(root)
                    .map(|catalog| catalog.skills)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    pub(super) fn install_shelf_skill(
        &mut self,
        owner: &str,
        slug: &str,
        version: &str,
        saved_key: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if self.skills.install.as_ref().is_some_and(|install| {
            !matches!(install.phase, InstallPhase::Failed(_))
        }) {
            return;
        }
        let Ok(id) = flowy_skills::parse_skill_id(owner, slug, version) else {
            return;
        };
        let key = saved_key.to_owned();
        self.skills.install = Some(InstallUi {
            key: key.clone(),
            phase: InstallPhase::Verifying,
        });
        self.run_skill_job(
            move |root, notes| {
                let result = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| {
                        let previous = {
                            let store = flowy_skills::Store::open(
                                &root.join("skills").join("market.sqlite3"),
                            )
                            .map_err(|error| error.to_string())?;
                            store
                                .installed()
                                .map_err(|error| error.to_string())?
                                .into_iter()
                                .find(|record| {
                                    record.id.owner_handle == id.owner_handle
                                        && record.id.slug == id.slug
                                })
                        };
                        let installer =
                            flowy_skills::SkillInstaller::network(root.clone())
                                .map_err(|error| error.to_string())?;
                        let mut progress = |stage| {
                            if let Ok(mut notes) = notes.lock() {
                                notes.push_back(InstallNote::Progress(stage));
                            }
                        };
                        let outcome = if let Some(previous) = previous.as_ref()
                        {
                            installer.install_update(
                                &id,
                                previous,
                                &mut progress,
                            )
                        } else {
                            installer.install(&id, &mut progress)
                        }
                        .map_err(|error| error.to_string())?;
                        if let Some(previous) = previous
                            && previous.id.version
                                != outcome.record.id.version
                        {
                            let old_root = flowy_skills::install_dir(
                                &root,
                                &previous.id,
                            );
                            if old_root.exists()
                                && let Err(error) =
                                    std::fs::remove_dir_all(&old_root)
                            {
                                tracing::warn!(
                                    target: "flowy.skills",
                                    path = %old_root.display(),
                                    error = %error,
                                    "old skill directory cleanup failed"
                                );
                            }
                        }
                        flowy_skills::load_installed(&root)
                            .map(|catalog| catalog.skills)
                            .map_err(|error| error.to_string())
                    }),
                )
                .unwrap_or_else(|_| Err("skill installer panicked".to_owned()));
                if let Ok(mut notes) = notes.lock() {
                    notes.push_back(InstallNote::Done(result));
                }
            },
            move |app, batch, cx| {
                let mut finished = false;
                for note in batch {
                    match note {
                        InstallNote::Progress(stage) => {
                            if let Some(install) = &mut app.skills.install {
                                install.phase = match stage {
                                    flowy_skills::InstallProgress::Verifying => {
                                        InstallPhase::Verifying
                                    }
                                    flowy_skills::InstallProgress::Downloading => {
                                        InstallPhase::Downloading
                                    }
                                    flowy_skills::InstallProgress::Validating
                                    | flowy_skills::InstallProgress::Scanning
                                    | flowy_skills::InstallProgress::Committing => {
                                        InstallPhase::Installing
                                    }
                                };
                            }
                        }
                        InstallNote::Done(result) => {
                            finished = true;
                            match result {
                                Ok(installed) => {
                                    app.skills.installed.clone_from(&installed);
                                    app.skills.install = None;
                                    app.prefs.skills.retain(|item| item != &key);
                                    let _ =
                                        app.prefs.save_checked(app.store.root());
                                    app.studio.update(cx, |panel, cx| {
                                        panel.set_skill_catalog(installed, cx);
                                    });
                                }
                                Err(error) => {
                                    app.skills.install = Some(InstallUi {
                                        key: key.clone(),
                                        phase: InstallPhase::Failed(error),
                                    });
                                }
                            }
                        }
                    }
                }
                cx.notify();
                finished
            },
            Duration::from_millis(40),
            cx,
        );
        cx.notify();
    }
}
