//! Minimal logger: warnings and errors (info in debug builds) to stderr and
//! `%APPDATA%\tarsier\tarsier.log`, so release builds without a console still
//! leave a trace.

use std::fs::{File, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write};
use std::sync::Mutex;

use log::{Level, Log, Metadata, Record};

/// The log starts over once it grows past this. Checked on every write, not
/// just at startup: developer mode logs every DDC/CI command, and the app runs
/// for weeks at a time.
const MAX_LOG_BYTES: u64 = 1 << 20;

struct Logger {
    file: Option<Mutex<LogFile>>,
}

struct LogFile {
    file: File,
    len: u64,
}

impl LogFile {
    fn append(&mut self, line: &str) {
        if self.len + line.len() as u64 > MAX_LOG_BYTES
            && self.file.set_len(0).is_ok()
            && self.file.seek(SeekFrom::Start(0)).is_ok()
        {
            self.len = 0;
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.len += line.len() as u64;
        }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
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
            f.append(&line);
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let dir = crate::config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tarsier.log");
    // Opened for writing and positioned at the end, not opened for appending:
    // an append-only handle on Windows lacks the access that truncating needs.
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .and_then(|mut file| {
            let len = file.seek(SeekFrom::End(0))?;
            Ok(LogFile { file, len })
        })
        .ok()
        .map(Mutex::new);
    if log::set_boxed_logger(Box::new(Logger { file })).is_ok() {
        set_verbose(false);
    }
}

/// Developer mode logs at debug level, including every DDC/CI command.
pub fn set_verbose(verbose: bool) {
    let level = if verbose {
        Level::Debug
    } else if cfg!(debug_assertions) {
        Level::Info
    } else {
        Level::Warn
    };
    log::set_max_level(level.to_level_filter());
}
