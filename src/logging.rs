use env_logger::WriteStyle;
use log::{LevelFilter, Log, Metadata, Record};
use std::env;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// File name for the persisted log inside `.kb/logs`.
const DEFAULT_LOG_FILE_NAME: &str = "kbmeta.log";

/// Environment variable overriding the log file path.
const LOG_FILE_ENV: &str = "KBMETA_LOG_FILE";

/// Installs a logger that mirrors every record to the terminal (stderr, as
/// before) and appends the same line to the log file.
///
/// The file is opened lazily on the first record so that commands like `init`
/// run against a fresh workspace do not pre-create `.kb` (which would make
/// `init` treat the workspace as already initialized).
pub fn init(root: &Path) {
    let mut terminal = env_logger::Builder::from_default_env();
    terminal.target(env_logger::Target::Stderr);
    let terminal_logger = terminal.build();
    let max_level = terminal_logger.filter();

    let logger = DualLogger {
        terminal: terminal_logger,
        file: Mutex::new(FileSink::Unopened {
            path: resolve_log_path(root),
            level: None,
        }),
    };

    match log::set_boxed_logger(Box::new(logger)) {
        Ok(()) => log::set_max_level(max_level),
        Err(err) => eprintln!("warning: failed to install logger: {err}"),
    }
}

/// Where the persisted log goes: `$KBMETA_LOG_FILE` when set, otherwise
/// `<root>/.kb/logs/kbmeta.log`.
fn resolve_log_path(root: &Path) -> PathBuf {
    env::var_os(LOG_FILE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(root))
}

/// Default log location relative to the workspace root.
fn default_log_path(root: &Path) -> PathBuf {
    root.join(".kb").join("logs").join(DEFAULT_LOG_FILE_NAME)
}

/// Opens the append-only log file, creating parent directories. Returns
/// `None` (after a warning) when the file cannot be opened.
fn open_log_file(path: &Path) -> Option<std::fs::File> {
    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!("warning: cannot create log directory {}: {err}", parent.display());
        return None;
    }
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => Some(file),
        Err(err) => {
            eprintln!("warning: cannot open log file {}: {err}", path.display());
            None
        }
    }
}

/// A `log::Log` that fans each record out to the terminal and the log file.
struct DualLogger {
    terminal: env_logger::Logger,
    file: Mutex<FileSink>,
}

impl Log for DualLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.terminal.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        self.terminal.log(record);
        if let Ok(mut sink) = self.file.lock() {
            sink.log(record);
        }
    }

    fn flush(&self) {
        self.terminal.flush();
        if let Ok(mut sink) = self.file.lock() {
            sink.flush();
        }
    }
}

/// Lazily opened append-only log sink: opens the file on first record, then
/// delegates to an env_logger instance writing through it.
enum FileSink {
    Unopened {
        path: PathBuf,
        /// Optional baseline filter override. `None` in production: the file
        /// mirrors whatever `RUST_LOG` gives env_logger. Tests set this so
        /// their expectations don't depend on the ambient environment.
        level: Option<LevelFilter>,
    },
    Open(env_logger::Logger),
    Failed,
}

impl FileSink {
    fn log(&mut self, record: &Record<'_>) {
        match self {
            FileSink::Open(logger) => logger.log(record),
            FileSink::Failed => {}
            FileSink::Unopened { path, level } => match open_log_file(path) {
                Some(file) => {
                    let mut builder = env_logger::Builder::from_default_env();
                    if let Some(level) = level {
                        builder.filter_level(*level);
                    }
                    builder.target(env_logger::Target::Pipe(Box::new(file)));
                    builder.write_style(WriteStyle::Never);
                    let logger = builder.build();
                    logger.log(record);
                    *self = FileSink::Open(logger);
                }
                None => {
                    eprintln!("warning: log file disabled; logging to terminal only");
                    *self = FileSink::Failed;
                }
            },
        }
    }

    fn flush(&mut self) {
        if let FileSink::Open(logger) = self {
            logger.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{Level, Record};

    /// Logs `message` through `sink`. The message is owned locally so the
    /// built `Record` (which borrows it) never outlives its data.
    fn log_message(sink: &mut FileSink, level: Level, message: &str) {
        let owned = message.to_string();
        let args = format_args!("{owned}");
        let record = Record::builder()
            .args(args)
            .level(level)
            .module_path(Some("logging_test"))
            .target("logging_test")
            .build();
        sink.log(&record);
    }

    #[test]
    fn default_log_path_lives_under_kb_logs() {
        assert_eq!(
            default_log_path(Path::new("/tmp/work")),
            PathBuf::from("/tmp/work/.kb/logs/kbmeta.log")
        );
    }

    #[test]
    fn file_sink_opens_lazily_and_appends_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs/test.log");
        assert!(!path.exists());

        let mut sink = FileSink::Unopened {
            path: path.clone(),
            level: Some(LevelFilter::Trace),
        };
        log_message(&mut sink, Level::Info, "first line");
        log_message(&mut sink, Level::Info, "second line");
        sink.flush();

        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("first line"));
        assert!(content.contains("second line"));
        assert!(content.contains("logging_test"));
    }

    #[test]
    fn file_sink_filters_below_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");

        let mut sink = FileSink::Unopened {
            path: path.clone(),
            level: Some(LevelFilter::Warn),
        };
        log_message(&mut sink, Level::Debug, "debug noise");
        log_message(&mut sink, Level::Warn, "warning alert");
        log_message(&mut sink, Level::Error, "fatal failure");
        sink.flush();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("debug noise"));
        assert!(content.contains("warning alert"));
        assert!(content.contains("fatal failure"));
    }

    #[test]
    fn file_sink_marks_failed_without_panicking() {
        // A path whose parent is a regular file cannot be created as a dir.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("nested/logs/test.log");

        let mut sink = FileSink::Unopened {
            path,
            level: Some(LevelFilter::Trace),
        };
        log_message(&mut sink, Level::Info, "dropped silently");
        sink.flush();
        assert!(matches!(sink, FileSink::Failed));
    }
}