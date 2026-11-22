//! Час для журналу, звітів і розкладу фонової перевірки: секунди від епохи
//! (настінний час, тож після сну комп'ютера розклад не запізнюється) і
//! місцевий час для людини.

use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Розкладений місцевий час (None — якщо libc не змогла його обчислити).
fn local_tm(secs: u64) -> Option<libc::tm> {
    static TZ: Once = Once::new();
    unsafe extern "C" {
        fn tzset();
    }
    // SAFETY: tzset лише читає TZ і /etc/localtime; виклик один раз до localtime_r.
    TZ.call_once(|| unsafe { tzset() });
    let t = secs as libc::time_t;
    // SAFETY: localtime_r заповнює передану структуру й не зберігає вказівників.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() { None } else { Some(tm) }
}

/// Місцевий час «2026-10-29 14:05».
pub fn local_time(secs: u64) -> String {
    match local_tm(secs) {
        Some(tm) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min
        ),
        None => secs.to_string(),
    }
}

/// Сьогоднішня дата як «YYYYmmdd» за місцевим часом (порожньо, якщо невідома).
pub fn today() -> String {
    local_tm(now())
        .map(|tm| format!("{:04}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_format() {
        let s = local_time(1_761_740_000);
        assert_eq!(s.len(), 16);
        assert!(s.starts_with("2025-"));
        let d = today();
        assert_eq!(d.len(), 8);
        assert!(d.bytes().all(|b| b.is_ascii_digit()));
    }
}
