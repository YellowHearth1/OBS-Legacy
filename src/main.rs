//! OBS-Legacy — TUI для збирання OBS Studio з NVENC під драйвери NVIDIA 580xx
//! на Artix/Arch Linux.
//!
//! Створено за допомогою Claude Code (Anthropic) на замовлення і під керівництвом
//! автора репозиторію. Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy

mod advice;
mod analyzer;
mod app;
mod catalog;
mod cli;
mod desktop;
mod launcher;
mod logo;
mod notify;
mod nvenc;
mod reports;
mod runner;
mod script;
mod service;
mod settings;
mod system;
mod term_view;
mod theme;
mod ui;
mod updates;

use std::env;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

use ratatui::crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::SetTitle;

fn print_help() {
    println!(
        "OBS-Legacy {} — OBS Studio з NVENC для NVIDIA 580xx (Artix/Arch)

Використання: obs-legacy [параметр]

Без параметрів відкриває меню. Якщо запустити без термінала (клацанням з меню
програм чи файлового менеджера), сам відкриє вікно термінала.

Параметри:
  --script-path   показати шлях до скрипта збирання (його можна запускати й без TUI)
  --check-updates перевірити нові версії OBS, плагінів і OBS-Legacy зараз
  --notifier status          сповіщення про оновлення: чи ввімкнено, остання перевірка
  --notifier enable [служба] увімкнути фонову перевірку (раз на 12 годин): служба
                             systemd, dinit, openrc, runit, s6 чи autostart; без
                             назви програма вибере сама
  --notifier disable         вимкнути й прибрати службу
  --notifier restart         перезапустити, якщо ввімкнено (після оновлення програми)
  --notifier test            пробне сповіщення
  --watch         фонова перевірка (так її запускає служба; вручну не потрібно)
  -V, --version   версія
  -h, --help      ця довідка

Створено за допомогою Claude Code (Anthropic).
Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy",
        env!("CARGO_PKG_VERSION")
    );
}

fn main() -> ExitCode {
    let mut no_relaunch = env::var_os(launcher::NO_RELAUNCH_ENV).is_some_and(|v| v == "1");
    let args: Vec<String> = env::args().skip(1).collect();
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("obs-legacy {} (скрипт {})", env!("CARGO_PKG_VERSION"), script::script_version());
                return ExitCode::SUCCESS;
            }
            "--probe-nvenc" => return ExitCode::from(nvenc::probe_main() as u8),
            "--check-updates" => return updates::check_main(),
            "--watch" => return updates::watch_main(&service::exe_path()),
            "--notifier" => return service::main(&args[i + 1..]),
            "--script-path" => {
                return match script::ensure_extracted() {
                    Ok(p) => {
                        println!("{}", p.display());
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("obs-legacy: {e:#}");
                        ExitCode::FAILURE
                    }
                };
            }
            "--no-relaunch" => no_relaunch = true,
            other => {
                eprintln!("obs-legacy: невідомий параметр «{other}»\n");
                print_help();
                return ExitCode::from(2);
            }
        }
    }

    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        if no_relaunch {
            eprintln!("obs-legacy: потрібен термінал.");
            return ExitCode::FAILURE;
        }
        return match launcher::relaunch_in_terminal() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("obs-legacy: {e:#}");
                ExitCode::FAILURE
            }
        };
    }

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("obs-legacy: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let script = script::ensure_extracted()?;
    let exe = env::current_exe()?;

    // Спершу власний хук (вимкнути мишу), потім ratatui додасть свій
    // (відновити термінал) — під час паніки вони спрацюють у зворотному порядку.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
        previous(info);
    }));
    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste, SetTitle("OBS-Legacy"));
    let result = app::App::new(exe, script).run(&mut terminal);
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}
