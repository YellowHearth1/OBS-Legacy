//! Стан фонової перевірки (~/.local/state/obs-legacy/updates.env): коли перевіряли, що знайшли й про
//! що вже сповістили, — і розклад наступної перевірки.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::PathBuf;

use super::Facts;
use crate::paths;

/// Як часто перевіряти нові версії (секунди).
pub const PERIOD: u64 = 12 * 3600;

/// Повторна спроба, якщо мережі не було.
const RETRY: u64 = 30 * 60;

/// Перша перевірка після запуску: дати сесії й мережі піднятися.
const FIRST_DELAY: u64 = 2 * 60;

/// ~/.local/state/obs-legacy/updates.env: KEY=VALUE.
#[derive(Debug, Default, Clone)]
pub struct State {
    pub(super) map: BTreeMap<String, String>,
}

impl State {
    pub fn path() -> PathBuf {
        paths::state_dir().join("updates.env")
    }

    pub fn load() -> Self {
        Self::parse(&fs::read_to_string(Self::path()).unwrap_or_default())
    }

    pub(super) fn parse(text: &str) -> Self {
        let map = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        Self { map }
    }

    pub fn save(&self) -> io::Result<()> {
        let mut body = String::from(tr!(
            "# OBS-Legacy: стан фонової перевірки оновлень. Якщо файл видалити, про наявні\n\
             # оновлення буде сповіщено ще раз.\n",
            "# OBS-Legacy: state of the background update check. Delete this file to be notified\n\
             # about the available updates again.\n"
        ));
        for (k, v) in &self.map {
            let _ = writeln!(body, "{k}={v}");
        }
        crate::atomic::write(&Self::path(), body.as_bytes(), 0o644)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub(super) fn num(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }

    pub(super) fn set(&mut self, key: &str, value: impl ToString) {
        self.map.insert(key.to_string(), value.to_string());
    }

    pub(super) fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    /// Записати підсумок перевірки нових версій: коли, чи відповів GitHub, що знайшли.
    pub(super) fn record_check(&mut self, at: u64, facts: &Facts) {
        self.set("checked", at);
        if facts.online() {
            self.set("ok", at);
        }
        self.set("summary", facts.summary());
    }

    /// Коли востаннє перевіряли нові версії (успішно чи ні).
    pub fn last_check(&self) -> Option<u64> {
        self.num("checked")
    }

    /// Що знайшла остання перевірка — коротко.
    pub fn summary(&self) -> Option<&str> {
        self.get("summary")
    }

    /// Коли перевіряти наступного разу: через 12 год після вдалої перевірки,
    /// через 30 хв після невдалої, але не раніше ніж за 2 хв після запуску.
    pub fn next_check(&self, now: u64) -> u64 {
        let due = match (self.last_check(), self.num("ok")) {
            (Some(t), Some(ok)) if ok == t => t + PERIOD,
            (Some(t), _) => t + RETRY,
            (None, _) => 0,
        };
        due.max(now + FIRST_DELAY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule() {
        let mut st = State::default();
        assert_eq!(st.next_check(1000), 1000 + FIRST_DELAY);
        st.set("checked", 5000);
        st.set("ok", 5000);
        assert_eq!(st.next_check(6000), 5000 + PERIOD);
        st.set("checked", 9000);
        assert_eq!(st.next_check(9000), 9000 + RETRY);
        // Давно не перевіряли — за 2 хв після запуску.
        assert_eq!(st.next_check(9000 + PERIOD * 3), 9000 + PERIOD * 3 + FIRST_DELAY);
    }
}
