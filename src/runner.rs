//! Запуск скрипта в псевдотерміналі (PTY). Так усе інтерактивне працює як у
//! звичайному терміналі: пароль sudo/doas, запитання [y/N], підтвердження
//! pacman. Вивід одночасно малюється емулятором vt100 і розбирається
//! аналізатором.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::analyzer::Analyzer;

/// Скільки рядків історії виводу тримає емулятор (гортання PgUp).
const SCROLLBACK: usize = 10_000;
/// Найменший розмір псевдотермінала: менший ламає вивід програм, що рахують ширину.
const MIN_ROWS: u16 = 2;
const MIN_COLS: u16 = 10;
/// Скільки чекати на решту виводу після завершення процесу (PTY віддає його із запізненням).
const EXIT_DRAIN: Duration = Duration::from_millis(1500);
const DRAIN_POLL: Duration = Duration::from_millis(50);
/// Буфер читання з PTY.
const READ_CHUNK: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub code: u32,
    pub success: bool,
    pub at: Instant,
}

pub struct Job {
    pub parser: vt100::Parser,
    pub analyzer: Analyzer,
    pub started: Instant,
    pub exit: Option<Exit>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    rx: Receiver<Vec<u8>>,
    eof: bool,
    size: (u16, u16),
}

fn nz(v: u16) -> NonZeroU16 {
    NonZeroU16::new(v.max(1)).unwrap_or(NonZeroU16::MIN)
}

impl Job {
    /// `program` + `args` у PTY розміром rows×cols.
    pub fn spawn(
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &[(&str, String)],
        rows: u16,
        cols: u16,
    ) -> Result<Self> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: rows.max(MIN_ROWS), cols: cols.max(MIN_COLS), pixel_width: 0, pixel_height: 0 })
            .context(tr!("не вдалося відкрити псевдотермінал", "couldn't open a pseudo-terminal"))?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| trf!("не вдалося запустити {program}", "couldn't start {program}"))?;
        // Без цього читач не отримає EOF, коли процес завершиться.
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, rx) = mpsc::channel();
        thread::Builder::new().name("pty-reader".into()).spawn(move || {
            let mut buf = [0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        })?;
        Ok(Self {
            parser: vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK),
            analyzer: Analyzer::new(),
            started: Instant::now(),
            exit: None,
            master: pair.master,
            writer,
            child,
            rx,
            eof: false,
            size: (rows, cols),
        })
    }

    /// Забирає новий вивід і перевіряє, чи процес завершився.
    /// Повертає true, якщо щось змінилося.
    pub fn pump(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    self.consume(&chunk, now);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.eof = true;
                    break;
                }
            }
        }
        if self.exit.is_none()
            && let Ok(Some(status)) = self.child.try_wait()
        {
            // Дочекатися решти виводу (EOF) або хоча б короткої паузи.
            let deadline = now + EXIT_DRAIN;
            while !self.eof && Instant::now() < deadline {
                match self.rx.recv_timeout(DRAIN_POLL) {
                    Ok(chunk) => self.consume(&chunk, Instant::now()),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => self.eof = true,
                }
            }
            let exit = Exit { code: status.exit_code(), success: status.success(), at: Instant::now() };
            self.analyzer.finish(exit.success, exit.at);
            self.exit = Some(exit);
            changed = true;
        }
        changed
    }

    /// Новий шматок виводу: на екран емулятора й в аналізатор.
    fn consume(&mut self, chunk: &[u8], now: Instant) {
        self.parser.process(chunk);
        self.analyzer.feed(chunk, now);
    }

    pub fn running(&self) -> bool {
        self.exit.is_none()
    }

    pub fn elapsed(&self) -> Duration {
        self.exit.map_or_else(Instant::now, |e| e.at).saturating_duration_since(self.started)
    }

    /// Введення від людини в скрипт. Помилка запису означає, що процес щойно завершився, — тоді
    /// вводити вже нікуди, і про це скаже картка результату.
    pub fn write_input(&mut self, bytes: &[u8]) {
        if self.running() {
            let _ = self.writer.write_all(bytes);
            let _ = self.writer.flush();
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(MIN_ROWS), cols.max(MIN_COLS));
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        self.parser.set_size(nz(rows), nz(cols));
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    /// Ctrl+C у термінал — як у звичайному вікні.
    pub fn interrupt(&mut self) {
        self.write_input(&[0x03]);
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
        }
    }
}
