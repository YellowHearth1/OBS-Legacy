//! Фонові відомості: система, звіти, пакети, яких бракує, каталог плагінів і нові версії. Кожне
//! збирається в окремому потоці й приходить повідомленням `Msg`, тож інтерфейс не завмирає.

use std::process::{Command, Stdio};
use std::thread;

use super::{App, Screen};
use crate::catalog::{self, PluginInfo};
use crate::reports::{self, ReportInfo};
use crate::service;
use crate::settings;
use crate::system::{self, SysInfo};
use crate::updates;
use crate::versions::{self, Updates};

pub(super) enum Msg {
    Sys(Box<SysInfo>),
    Updates(Updates),
    Catalog(Result<Vec<PluginInfo>, String>),
    Reports(Vec<ReportInfo>),
    /// Пакети для збірки, яких бракує: (є в репозиторіях, немає в репозиторіях).
    Deps(Option<(Vec<String>, Vec<String>)>),
    /// Сповіщення про оновлення: Some(служба) — увімкнено.
    Notifier(Option<service::Backend>),
}

impl App {
    pub(super) fn spawn_sys(&mut self) {
        if self.sys_loading {
            return;
        }
        self.sys_loading = true;
        let tx = self.tx.clone();
        let exe = self.exe.clone();
        let tag = self.settings.get("NV_HEADERS_TAG");
        thread::spawn(move || {
            let info = system::gather(&exe, &tag);
            let _ = tx.send(Msg::Sys(Box::new(info)));
            let _ = tx.send(Msg::Notifier(service::installed().first().map(|p| p.backend)));
        });
    }

    pub(super) fn spawn_reports(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Msg::Reports(reports::list()));
        });
    }

    /// Яких пакетів для збірки бракує (`deps --missing`, без sudo і швидко).
    pub(super) fn spawn_deps(&mut self) {
        let tx = self.tx.clone();
        let script = self.script.clone();
        thread::spawn(move || {
            let out = Command::new("bash")
                .arg(&script)
                .args(["deps", "--missing"])
                .env("OBS_LEGACY_SETTINGS", settings::settings_path())
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            let parsed = out.ok().filter(|o| o.status.success()).map(|o| {
                let (mut repo, mut norepo) = (Vec::new(), Vec::new());
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    match line.split_once(' ') {
                        Some(("repo", p)) => repo.push(p.trim().to_string()),
                        Some(("norepo", p)) => norepo.push(p.trim().to_string()),
                        _ => {}
                    }
                }
                (repo, norepo)
            });
            let _ = tx.send(Msg::Deps(parsed));
        });
    }

    pub(super) fn spawn_catalog(&mut self) {
        let tx = self.tx.clone();
        let script = self.script.clone();
        thread::spawn(move || {
            let r = catalog::load(&script).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Catalog(r));
        });
    }

    fn spawn_updates(&mut self) {
        let (Some(sys), Some(cat)) = (&self.sys, &self.catalog) else { return };
        self.updates_requested = true;
        self.updates_loading = true;
        let mut ids: Vec<String> =
            sys.active_build().map(|b| b.plugins().into_iter().map(|(id, _)| id).collect()).unwrap_or_default();
        for id in self.settings.plugins() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let repos: Vec<(String, String)> =
            ids.into_iter().filter_map(|id| cat.iter().find(|p| p.id == id).map(|p| (id, p.repo.clone()))).collect();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let self_repo = updates::self_repo();
            let _ = tx.send(Msg::Updates(versions::check_updates(&repos, Some(&self_repo))));
        });
    }

    pub(super) fn refresh(&mut self) {
        self.spawn_sys();
        self.spawn_reports();
        self.spawn_deps();
        self.updates = None;
        self.updates_requested = false;
    }

    pub(super) fn process_messages(&mut self) {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Sys(info) => {
                    self.sys = Some(*info);
                    self.sys_loading = false;
                    changed = true;
                }
                Msg::Updates(u) => {
                    self.updates = Some(u);
                    self.updates_loading = false;
                    changed = true;
                }
                Msg::Catalog(Ok(list)) => {
                    self.catalog = Some(list);
                    self.catalog_error = None;
                }
                Msg::Catalog(Err(e)) => self.catalog_error = Some(e),
                Msg::Reports(list) => {
                    self.reports = list;
                    self.reports_loaded = true;
                    self.reports_sel = self.reports_sel.min(self.reports.len().saturating_sub(1));
                    changed = true;
                }
                Msg::Deps(Some((repo, norepo))) => {
                    self.missing_packages = Some(repo);
                    self.missing_norepo = norepo;
                    changed = true;
                }
                Msg::Deps(None) => {}
                Msg::Notifier(n) => {
                    self.notifier = Some(n);
                    changed = true;
                }
            }
        }
        if changed {
            self.preselect();
        }
        if !self.updates_requested && self.screen != Screen::Intro && self.sys.is_some() && self.catalog.is_some() {
            self.spawn_updates();
        }
    }
}
