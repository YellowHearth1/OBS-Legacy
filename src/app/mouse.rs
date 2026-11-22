//! Миша: клацання по пунктах і кнопках (їхні області запам'ятовуються під час малювання) і коліщатко.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::menu::Row;
use super::{App, Screen};
use crate::settings;

/// Елементи, на які можна клацнути мишею (заповнюються під час малювання).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Menu(usize),
    Plugin(usize),
    Build(usize),
    Setting(usize),
    Report(usize),
    DialogOk,
    DialogCancel,
    DialogAlt,
    Continue,
    Back,
    Quit,
    OpenReport,
    CopyPath,
    OpenExternal,
    AdviceAction,
    AdviceAlt,
    RunPrimary,
    ToggleDetails,
}

impl App {
    fn target_at(&self, x: u16, y: u16) -> Option<Target> {
        let pos = Position { x, y };
        self.hits.iter().rev().find(|(r, _)| r.contains(pos)).map(|(_, t)| *t)
    }

    pub(super) fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                self.on_wheel(up);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(t) = self.target_at(m.column, m.row) {
                    self.on_click(t);
                }
            }
            _ => {}
        }
    }

    fn on_wheel(&mut self, up: bool) {
        let step = |v: usize, n: usize| if up { (v + n - 1) % n } else { (v + 1) % n };
        match self.screen {
            Screen::Run => {
                if let Some(run) = self.run.as_mut() {
                    run.scroll = if up { run.scroll + 3 } else { run.scroll.saturating_sub(3) };
                }
            }
            Screen::Home if self.dialog.is_none() => {
                self.menu_sel = step(self.menu_sel, self.rows().len());
                self.menu_touched = true;
            }
            Screen::Plugins => {
                let n = self.catalog.as_ref().map_or(0, Vec::len);
                if n > 0 {
                    self.plugin_sel = step(self.plugin_sel, n);
                }
            }
            Screen::Rollback if !self.builds.is_empty() => self.build_sel = step(self.build_sel, self.builds.len()),
            Screen::Settings if self.editor.is_none() => {
                self.settings_sel = step(self.settings_sel, settings::FIELDS.len())
            }
            Screen::Reports if !self.reports.is_empty() => {
                self.reports_sel = step(self.reports_sel, self.reports.len())
            }
            Screen::Report => {
                if let Some(v) = self.report_view.as_mut() {
                    v.scroll_by(if up { -3 } else { 3 });
                }
            }
            Screen::Help => {
                self.help_scroll = if up {
                    self.help_scroll.saturating_sub(3)
                } else {
                    (self.help_scroll + 3).min(self.help_max_scroll)
                }
            }
            _ => {}
        }
    }

    fn on_click(&mut self, t: Target) {
        match t {
            Target::DialogOk => self.confirm_dialog(),
            Target::DialogCancel => self.dialog = None,
            Target::DialogAlt => self.alt_dialog(),
            Target::Quit => self.quit = true,
            _ if self.dialog.is_some() => {}
            Target::Continue => match self.screen {
                Screen::Intro => self.go_home(),
                Screen::Run if self.run.as_ref().is_some_and(|r| !r.job.running()) => self.go_home(),
                _ => {}
            },
            Target::Back => match self.screen {
                Screen::Run if self.run.as_ref().is_some_and(|r| r.job.running()) => {}
                Screen::Report => self.close_report(),
                _ => self.go_home(),
            },
            // Клац по вибраному пункту — запуск; по іншому — вибір.
            Target::Menu(i) => {
                let rows = self.rows();
                if let Some(row) = rows.get(i).copied() {
                    self.menu_touched = true;
                    if self.menu_sel == i || row == Row::More {
                        self.menu_sel = i;
                        self.activate_row(row);
                    } else {
                        self.menu_sel = i;
                    }
                }
            }
            Target::Plugin(i) => {
                self.plugin_sel = i;
                self.toggle_plugin(i);
            }
            Target::Build(i) => {
                if self.build_sel == i {
                    self.rollback_to(i);
                } else {
                    self.build_sel = i;
                }
            }
            Target::Setting(i) if self.editor.is_none() => {
                if self.settings_sel == i {
                    self.edit_setting(i);
                } else {
                    self.settings_sel = i;
                }
            }
            Target::Report(i) => {
                if self.reports_sel == i {
                    if let Some(r) = self.reports.get(i) {
                        let path = r.path.clone();
                        self.open_report(path, Screen::Reports);
                    }
                } else {
                    self.reports_sel = i;
                }
            }
            Target::OpenReport => self.view_current_report(),
            Target::CopyPath => self.copy_report_path(),
            Target::OpenExternal => self.open_report_externally(false),
            Target::AdviceAction | Target::AdviceAlt => {
                let advice = self.advice();
                let action = advice.and_then(|a| if t == Target::AdviceAlt { a.alt } else { a.action });
                if let Some(action) = action {
                    self.menu_touched = true;
                    self.select_action(action);
                    self.activate(action);
                }
            }
            Target::RunPrimary => self.run_primary_activate(),
            Target::ToggleDetails => self.details = !self.details,
            _ => {}
        }
    }
}
