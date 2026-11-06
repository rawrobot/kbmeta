//! Debug-only dump of raw LLM answers to a dedicated file.
//!
//! The logging system truncates provider answers for readability; when a
//! request fails that truncation hides the reason. This module appends every
//! raw answer, in full, to `.kb/logs/llm-answers.log`, separate from the log
//! stream. It is inert unless debug mode is on (`RUST_LOG=debug` or higher).

use log::LevelFilter;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// File name for the raw-answer dump inside `.kb/logs`.
const ANSWERS_FILE_NAME: &str = "llm-answers.log";

/// Workspace root, set once at startup by [`init`].
static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Lazily opened append-only answer dump.
static SINK: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Records the workspace root so [`dump`] can resolve the answers file.
/// Safe to call unconditionally from startup; only the first root wins.
pub(crate) fn init(root: &Path) {
    let _ = ROOT.set(root.to_path_buf());
}

/// Appends the raw answer to `.kb/logs/llm-answers.log`, unless debug mode is
/// off or the file cannot be opened (warn once, then give up silently).
pub(crate) fn dump(content: &str) {
    if log::max_level() < LevelFilter::Debug {
        return;
    }
    let mut sink = SINK.lock().unwrap();
    if sink.is_none()
        && let Some(root) = ROOT.get()
    {
        *sink = open_answers_file(&root.join(".kb").join("logs"));
    }
    if let Some(file) = sink.as_mut() {
        let _ = dump_answer_to(file, content);
    }
}

/// Opens the append-only answer dump, creating parent directories. `None`
/// (after a warning) when the file cannot be opened.
fn open_answers_file(logs_dir: &Path) -> Option<std::fs::File> {
    if let Err(err) = std::fs::create_dir_all(logs_dir) {
        eprintln!(
            "warning: cannot create answers log directory {}: {err}",
            logs_dir.display()
        );
        return None;
    }
    let path = logs_dir.join(ANSWERS_FILE_NAME);
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => Some(file),
        Err(err) => {
            eprintln!("warning: cannot open answers log file {}: {err}", path.display());
            None
        }
    }
}

/// Writes one delimiter-anchored, complete answer entry. Kept separate from
/// [`dump`] so tests can exercise the exact write without touching statics.
fn dump_answer_to(file: &mut std::fs::File, content: &str) -> std::io::Result<()> {
    writeln!(file, "==== llm answer (t={}) ====", unix_seconds())?;
    write!(file, "{content}")?;
    writeln!(file)?; // separator newline for the next entry
    file.flush()
}

/// Seconds since the Unix epoch, for correlating entries with the log file.
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek};

    #[test]
    fn writes_full_answer_with_delimiter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ANSWERS_FILE_NAME);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();

        let answer = "Here's a thinking process:\n\n1. step one\n2. step two\n{\"type\":\"Note\"}";
        dump_answer_to(&mut file, answer).unwrap();

        file.seek(std::io::SeekFrom::Start(0)).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();

        assert!(content.starts_with("==== llm answer (t="));
        assert!(content.contains(answer));
        assert!(content.trim_end().ends_with(answer));
    }

    #[test]
    fn answers_file_appends_across_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ANSWERS_FILE_NAME);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();

        dump_answer_to(&mut file, "first {a:1}").unwrap();
        dump_answer_to(&mut file, "second {b:2}").unwrap();
        drop(file);

        let content = std::fs::read_to_string(&path).unwrap();
        let first = content.find("first {a:1}").unwrap();
        let second = content.find("second {b:2}").unwrap();
        assert!(first < second, "entries must be ordered, not overwritten");
    }

    #[test]
    fn open_answers_file_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("nested").join("logs");
        let file = open_answers_file(&logs_dir).expect("file opens");
        drop(file);
        let path = logs_dir.join(ANSWERS_FILE_NAME);
        assert!(path.exists());
        assert!(logs_dir.is_dir());
    }
}