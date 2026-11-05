//! Вбудований переглядач звіту й дії зі звітом: переглянути, скопіювати шлях, відкрити в програмі.

use std::path::PathBuf;

use super::{App, Screen, ToastKind};
use crate::desktop;
use crate::paths::{expand_home, short_path};
use crate::reports;

/// Вбудований переглядач звіту.
pub struct ReportView {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub error: Option<String>,
    /// Перший видимий рядок (після перенесення).
    pub scroll: usize,
    /// Висота видимої частини (заповнюється під час малювання).
    pub page: usize,
    /// Перенесені рядки для поточної ширини: (ширина, [(номер рядка звіту, текст)]).
    pub wrapped: Option<(u16, Vec<(usize, String)>)>,
    /// Рядок звіту, до якого прокрутити під час наступного малювання.
    pub jump: Option<usize>,
    /// Куди повертатися (Esc).
    pub back: Screen,
}

impl ReportView {
    pub fn open(path: PathBuf, back: Screen) -> Self {
        let (lines, error) = match reports::load_lines(&path) {
            Ok(l) => (l, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        // Одразу показати останню помилку (а якщо її немає — останнє попередження).
        let kinds: Vec<_> = lines.iter().map(|l| reports::classify(l)).collect();
        let jump = kinds
            .iter()
            .rposition(|k| *k == reports::LineKind::Error)
            .or_else(|| kinds.iter().rposition(|k| k.is_problem()));
        Self { path, lines, error, scroll: 0, page: 1, wrapped: None, jump, back }
    }

    pub(super) fn rows(&self) -> usize {
        self.wrapped.as_ref().map_or(self.lines.len(), |(_, r)| r.len())
    }

    pub fn max_scroll(&self) -> usize {
        self.rows().saturating_sub(self.page)
    }

    pub(super) fn scroll_by(&mut self, delta: isize) {
        self.scroll = self.scroll.saturating_add_signed(delta).min(self.max_scroll());
    }

    /// Перейти до наступного (або попереднього) попередження чи помилки.
    pub(super) fn jump_problem(&mut self, forward: bool) -> bool {
        let Some((_, rows)) = &self.wrapped else { return false };
        let starts: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(i, (n, _))| (*i == 0 || rows[i - 1].0 != *n) && reports::classify(&self.lines[*n]).is_problem())
            .map(|(i, _)| i)
            .collect();
        // Позиція «фокуса» — третина екрана від верху.
        let focus = self.scroll + self.page / 3;
        let target = if forward {
            starts.into_iter().find(|&r| r > focus)
        } else {
            starts.into_iter().rev().find(|&r| r < focus)
        };
        match target {
            Some(r) => {
                self.scroll = r.saturating_sub(self.page / 3).min(self.max_scroll());
                true
            }
            None => false,
        }
    }
}

impl App {
    pub(super) fn open_report(&mut self, path: PathBuf, back: Screen) {
        self.report_view = Some(ReportView::open(path, back));
        self.screen = Screen::Report;
    }

    /// Шлях до звіту, з яким зараз працює екран.
    pub(super) fn current_report(&self) -> Option<PathBuf> {
        match self.screen {
            Screen::Report => self.report_view.as_ref().map(|v| v.path.clone()),
            Screen::Reports => self.reports.get(self.reports_sel).map(|r| r.path.clone()),
            Screen::Run => self.run.as_ref().and_then(|r| r.job.analyzer.report.as_deref()).map(expand_home),
            Screen::Home => {
                self.advice().and_then(|a| a.report).or_else(|| self.reports.first().map(|r| r.path.clone()))
            }
            _ => None,
        }
    }

    pub(super) fn copy_report_path(&mut self) {
        let Some(path) = self.current_report() else {
            self.toast(ToastKind::Info, tr!("Звіту ще немає.", "No report yet."));
            return;
        };
        let text = path.display().to_string();
        match desktop::copy(&text) {
            Ok(()) => self.toast(ToastKind::Ok, trf!("Шлях скопійовано: {}", "Path copied: {}", short_path(&path))),
            Err(e) => self.toast(ToastKind::Err, trf!("Не вдалося скопіювати: {e:#}", "Couldn't copy: {e:#}")),
        }
    }

    pub(super) fn open_report_externally(&mut self, folder: bool) {
        let Some(path) = self.current_report() else {
            self.toast(ToastKind::Info, tr!("Звіту ще немає.", "No report yet."));
            return;
        };
        let target = if folder { path.parent().map(PathBuf::from).unwrap_or(path) } else { path };
        match desktop::open(&target) {
            Ok(()) => self.toast(ToastKind::Ok, trf!("Відкриваю {}…", "Opening {}…", short_path(&target))),
            Err(e) => self.toast(ToastKind::Err, trf!("Не вдалося відкрити: {e:#}", "Couldn't open: {e:#}")),
        }
    }

    /// «o»: переглянути звіт у самій програмі.
    pub(super) fn view_current_report(&mut self) {
        match self.current_report() {
            Some(path) => self.open_report(path, self.screen),
            None => self.toast(ToastKind::Info, tr!("Звіту ще немає.", "No report yet.")),
        }
    }
}
