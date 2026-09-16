use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl LogLevel {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => LogLevel::Debug,
            1 => LogLevel::Info,
            2 => LogLevel::Warn,
            _ => LogLevel::Error,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "[DEBUG]",
            LogLevel::Info => "[INFO ]",
            LogLevel::Warn => "[WARN ]",
            LogLevel::Error => "[ERROR]",
        }
    }
}

struct LoggerInner {
    log_file: Option<File>,
    syslog_ident: Option<CString>,
}

static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);
static LOGGER_INNER: Mutex<Option<LoggerInner>> = Mutex::new(None);

pub struct Logger;

impl Logger {
    pub fn set_level(level: LogLevel) {
        CURRENT_LEVEL.store(level as u8, Ordering::Relaxed);
    }

    pub fn get_level() -> LogLevel {
        LogLevel::from_u8(CURRENT_LEVEL.load(Ordering::Relaxed))
    }

    pub fn set_log_file(path: &str) {
        let mut guard = LOGGER_INNER.lock().unwrap();
        let inner = guard.get_or_insert_with(|| LoggerInner {
            log_file: None,
            syslog_ident: None,
        });

        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => inner.log_file = Some(file),
            Err(e) => eprintln!("[Logger] Failed to open log file {path}: {e}"),
        }
    }

    pub fn enable_syslog(ident: &str) {
        let mut guard = LOGGER_INNER.lock().unwrap();
        let inner = guard.get_or_insert_with(|| LoggerInner {
            log_file: None,
            syslog_ident: None,
        });

        if let Ok(c_ident) = CString::new(ident) {
            unsafe {
                libc::openlog(
                    c_ident.as_ptr(),
                    libc::LOG_PID | libc::LOG_NDELAY,
                    libc::LOG_DAEMON,
                );
            }
            inner.syslog_ident = Some(c_ident);
        }
    }

    pub fn log(level: LogLevel, msg: &str) {
        if (level as u8) < CURRENT_LEVEL.load(Ordering::Relaxed) {
            return;
        }

        let mut guard = LOGGER_INNER.lock().unwrap();

        if let Some(inner) = guard.as_mut() {
            if inner.syslog_ident.is_some() {
                let priority = match level {
                    LogLevel::Debug => libc::LOG_DEBUG,
                    LogLevel::Info => libc::LOG_INFO,
                    LogLevel::Warn => libc::LOG_WARNING,
                    LogLevel::Error => libc::LOG_ERR,
                };
                if let Ok(c_msg) = CString::new(msg) {
                    let fmt = b"%s\0";
                    unsafe {
                        libc::syslog(priority, fmt.as_ptr() as *const libc::c_char, c_msg.as_ptr());
                    }
                }
            }
        }

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe {
            libc::localtime_r(&now, &mut tm);
        }

        let time_str = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        );

        let full_msg = format!("{} {} {}", time_str, level.as_str(), msg);

        if level >= LogLevel::Error {
            eprintln!("{full_msg}");
        } else {
            println!("{full_msg}");
        }

        if let Some(inner) = guard.as_mut() {
            if let Some(file) = inner.log_file.as_mut() {
                let _ = writeln!(file, "{full_msg}");
                let _ = file.flush();
            }
        }
    }
}

pub fn log_debug(msg: impl AsRef<str>) {
    Logger::log(LogLevel::Debug, msg.as_ref());
}

pub fn log_info(msg: impl AsRef<str>) {
    Logger::log(LogLevel::Info, msg.as_ref());
}

pub fn log_warn(msg: impl AsRef<str>) {
    Logger::log(LogLevel::Warn, msg.as_ref());
}

pub fn log_error(msg: impl AsRef<str>) {
    Logger::log(LogLevel::Error, msg.as_ref());
}
