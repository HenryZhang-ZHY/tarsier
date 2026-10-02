//! Minimal logger: warnings and errors (info in debug builds) to stderr and
//! `%APPDATA%\tarsier\tarsier.log`, so release builds without a console still
//! leave a trace.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use log::{Level, Log, Metadata, Record};

struct Logger {
    file: Option<Mutex<File>>,
    level: Level,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:5} {}: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.target(),
            record.args()
        );
        eprint!("{line}");
        if let Some(file) = &self.file
            && let Ok(mut f) = file.lock()
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let dir = crate::config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tarsier.log");
    // Keep the log small: start over once it passes 1 MB.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::remove_file(&path);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
        .map(Mutex::new);
    let level = if cfg!(debug_assertions) {
        Level::Info
    } else {
        Level::Warn
    };
    if log::set_boxed_logger(Box::new(Logger { file, level })).is_ok() {
        log::set_max_level(level.to_level_filter());
    }
}
