//! 日志实现：同时输出到**日志文件**与标准输出。
//!
//! 之前的实现只用 `env_logger` 写 stderr，而正式版带
//! `windows_subsystem = "windows"`（没有控制台，stderr 被丢弃），结果就是
//! `[logging]` 里的配置项全都无效、日志文件从不生成 —— 机器人悄无声息地跑一晚上
//! 出问题后完全无从排查。
//!
//! 这里实现一个极简的 `log::Log`：
//!
//! * 按 `[logging].level` 过滤（支持运行时热更新，见 [`set_level`]）
//! * 追加写入 `[logging].file`（相对路径挂在用户数据目录下）
//! * 文件超过 [`MAX_LOG_BYTES`] 自动轮转为 `*.1`，避免长期运行撑爆磁盘
//! * `[logging].console` 控制是否同时写 stderr

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::config::LoggingConfig;

/// 单个日志文件的大小上限，超过后轮转为 `*.1`
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// 解析日志级别字符串。兼容 `WARNING` 写法，非法值回落到 `INFO`。
pub fn level_filter_from_str(raw: &str) -> LevelFilter {
    match raw.trim().to_ascii_uppercase().as_str() {
        "OFF" | "NONE" => LevelFilter::Off,
        "ERROR" | "ERR" => LevelFilter::Error,
        "WARN" | "WARNING" => LevelFilter::Warn,
        "INFO" => LevelFilter::Info,
        "DEBUG" => LevelFilter::Debug,
        "TRACE" | "ALL" => LevelFilter::Trace,
        "" => LevelFilter::Info,
        other => {
            // 未知级别：不静默吞掉，回落到 INFO 并提示一次
            eprintln!("[logger] 未知日志级别 {:?}，已回落到 INFO", other);
            LevelFilter::Info
        }
    }
}

fn filter_to_usize(f: LevelFilter) -> usize {
    match f {
        LevelFilter::Off => 0,
        LevelFilter::Error => 1,
        LevelFilter::Warn => 2,
        LevelFilter::Info => 3,
        LevelFilter::Debug => 4,
        LevelFilter::Trace => 5,
    }
}

fn level_matches(filter: usize, level: Level) -> bool {
    let l = match level {
        Level::Error => 1,
        Level::Warn => 2,
        Level::Info => 3,
        Level::Debug => 4,
        Level::Trace => 5,
    };
    filter >= l && filter > 0
}

fn level_tag(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

struct FileSink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    warned: bool,
}

impl FileSink {
    fn open(path: PathBuf) -> Self {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[logger] 创建日志目录 {:?} 失败: {}", parent, e);
            }
        }
        let (file, written) = Self::open_file(&path);
        Self {
            path,
            file,
            written,
            warned: false,
        }
    }

    fn open_file(path: &PathBuf) -> (Option<File>, u64) {
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => {
                let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                (Some(f), len)
            }
            Err(e) => {
                eprintln!("[logger] 打开日志文件 {:?} 失败: {}", path, e);
                (None, 0)
            }
        }
    }

    fn write_line(&mut self, line: &str) {
        let bytes = line.len() as u64 + 1;
        if self.written + bytes > MAX_LOG_BYTES {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if let Err(e) = writeln!(file, "{}", line).and_then(|_| file.flush()) {
            if !self.warned {
                eprintln!("[logger] 写入日志文件失败: {}", e);
                self.warned = true;
            }
            return;
        }
        self.written += bytes;
    }

    /// 轮转：`bot.log` → `bot.log.1`（覆盖旧的 `.1`）
    fn rotate(&mut self) {
        self.file = None;
        let backup = match self.path.extension().and_then(|e| e.to_str()) {
            Some(ext) => self.path.with_extension(format!("{}.1", ext)),
            None => self.path.with_extension("1"),
        };
        let _ = std::fs::remove_file(&backup);
        if let Err(e) = std::fs::rename(&self.path, &backup) {
            eprintln!("[logger] 日志轮转失败 {:?}: {}", self.path, e);
        }
        let (file, written) = Self::open_file(&self.path);
        self.file = file;
        self.written = written;
    }

    /// 热更新日志文件路径
    fn set_path(&mut self, path: PathBuf) {
        if self.path == path {
            return;
        }
        self.file = None;
        *self = Self::open(path);
    }
}

struct AppLogger {
    filter: AtomicUsize,
    console: AtomicBool,
    sink: Mutex<FileSink>,
}

impl Log for AppLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        level_matches(self.filter.load(Ordering::Relaxed), metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} [{}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            level_tag(record.level()),
            record.args()
        );

        if self.console.load(Ordering::Relaxed) {
            eprintln!("{}", line);
        }

        if let Ok(mut sink) = self.sink.lock() {
            sink.write_line(&line);
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock() {
            if let Some(file) = sink.file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

static LOGGER: std::sync::OnceLock<&'static AppLogger> = std::sync::OnceLock::new();

/// 安装全局 logger（重复调用只生效一次）。返回是否由本次调用完成安装。
pub fn init(logging: &LoggingConfig) -> bool {
    let logger: &'static AppLogger = Box::leak(Box::new(AppLogger {
        filter: AtomicUsize::new(filter_to_usize(level_filter_from_str(&logging.level))),
        console: AtomicBool::new(logging.console),
        sink: Mutex::new(FileSink::open(crate::paths::log_file(&logging.file))),
    }));

    match log::set_logger(logger) {
        Ok(()) => {
            log::set_max_level(level_filter_from_str(&logging.level));
            let _ = LOGGER.set(logger);
            true
        }
        Err(_) => false,
    }
}

/// 运行时调整日志级别（配置热更新时调用）
pub fn set_level(raw: &str) {
    let filter = level_filter_from_str(raw);
    log::set_max_level(filter);
    if let Some(logger) = LOGGER.get() {
        logger.filter.store(filter_to_usize(filter), Ordering::Relaxed);
    }
}

/// 运行时调整是否输出到 stderr
pub fn set_console(enabled: bool) {
    if let Some(logger) = LOGGER.get() {
        logger.console.store(enabled, Ordering::Relaxed);
    }
}

/// 运行时切换日志文件
pub fn set_file(configured: &str) {
    if let Some(logger) = LOGGER.get() {
        if let Ok(mut sink) = logger.sink.lock() {
            sink.set_path(crate::paths::log_file(configured));
        }
    }
    log::info!("日志文件已切换为 {:?}", crate::paths::log_file(configured));
}

/// 按配置整体应用（级别 + 文件 + 控制台）
pub fn apply(logging: &LoggingConfig) {
    set_level(&logging.level);
    set_console(logging.console);
    set_file(&logging.file);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_parsing() {
        assert_eq!(level_filter_from_str("INFO"), LevelFilter::Info);
        assert_eq!(level_filter_from_str(" info "), LevelFilter::Info);
        assert_eq!(level_filter_from_str("DEBUG"), LevelFilter::Debug);
        assert_eq!(level_filter_from_str("WARN"), LevelFilter::Warn);
        // 前端历史上用的是 WARNING 写法，必须兼容
        assert_eq!(level_filter_from_str("WARNING"), LevelFilter::Warn);
        assert_eq!(level_filter_from_str("off"), LevelFilter::Off);
        assert_eq!(level_filter_from_str(""), LevelFilter::Info);
        // 未知值回落到 INFO，而不是静默变成 Off
        assert_eq!(level_filter_from_str("随便写的"), LevelFilter::Info);
    }

    #[test]
    fn test_level_matching() {
        let info = filter_to_usize(LevelFilter::Info);
        assert!(level_matches(info, Level::Error));
        assert!(level_matches(info, Level::Warn));
        assert!(level_matches(info, Level::Info));
        assert!(!level_matches(info, Level::Debug));

        let off = filter_to_usize(LevelFilter::Off);
        assert!(!level_matches(off, Level::Error));
    }
}
