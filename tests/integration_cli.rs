mod common;

use common::{fixture_to_tempdir, run_kbmeta, stderr_text, stdout_text};
use rusqlite::Connection;
use std::fs::OpenOptions;
use std::io::Write;

#[test]
fn init_creates_kb_state_files() {
    let ws = fixture_to_tempdir("kb_minimal");

    let out = run_kbmeta(ws.path(), &["init"]);
    assert!(out.status.success(), "init failed: {}", stderr_text(&out));

    assert!(ws.path().join(".kb/config.toml").exists());
    assert!(ws.path().join(".kb/index.sqlite").exists());
    assert!(ws.path().join(".kb/prompts/cook-default.md").exists());

    let cfg = std::fs::read_to_string(ws.path().join(".kb/config.toml"))
        .expect("config should be readable");
    assert!(cfg.contains("output_dir = \"knowledge/staging\""));
}

#[test]
fn rebuild_and_scan_report_expected_counts() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let rebuild_out = run_kbmeta(ws.path(), &["rebuild"]);
    assert!(
        rebuild_out.status.success(),
        "rebuild failed: {}",
        stderr_text(&rebuild_out)
    );

    let scan_out = run_kbmeta(ws.path(), &["scan"]);
    assert!(scan_out.status.success(), "scan failed: {}", stderr_text(&scan_out));

    let scan_stdout = stdout_text(&scan_out);
    assert!(scan_stdout.contains("Scanned 2 documents"));
    assert!(scan_stdout.contains("Missing metadata block: 1"));
    assert!(scan_stdout.contains("Stale index entries: 0"));
}

#[test]
fn scan_fails_on_invalid_metadata_by_default() {
    let ws = fixture_to_tempdir("kb_invalid_metadata");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let scan_out = run_kbmeta(ws.path(), &["scan"]);
    assert!(!scan_out.status.success());

    let scan_stderr = stderr_text(&scan_out);
    assert!(scan_stderr.contains("failed to parse"));
}

#[test]
fn scan_skip_invalid_succeeds_with_warning() {
    let ws = fixture_to_tempdir("kb_invalid_metadata");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let scan_out = run_kbmeta(ws.path(), &["scan", "--skip-invalid"]);
    assert!(
        scan_out.status.success(),
        "scan --skip-invalid failed: {}",
        stderr_text(&scan_out)
    );

    let scan_stderr = stderr_text(&scan_out);
    let scan_stdout = stdout_text(&scan_out);

    assert!(scan_stderr.contains("warning [code=KBMETA_DISCOVERY_WARNING]: skipped"));
    assert!(scan_stdout.contains("Scanned 1 documents"));
}

#[test]
fn scan_fails_on_missing_required_metadata_fields() {
    let ws = fixture_to_tempdir("kb_missing_required");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let scan_out = run_kbmeta(ws.path(), &["scan"]);
    assert!(!scan_out.status.success());

    let scan_stderr = stderr_text(&scan_out);
    assert!(scan_stderr.contains("metadata validation failed"));
    assert!(scan_stderr.contains("code=KBMETA_MISSING_REQUIRED_FIELDS"));
    assert!(scan_stderr.contains("missing required fields:"));
    assert!(scan_stderr.contains("type"));
}

#[test]
fn scan_skip_invalid_skips_missing_required_metadata_fields() {
    let ws = fixture_to_tempdir("kb_missing_required");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let scan_out = run_kbmeta(ws.path(), &["scan", "--skip-invalid"]);
    assert!(
        scan_out.status.success(),
        "scan --skip-invalid failed: {}",
        stderr_text(&scan_out)
    );

    let scan_stderr = stderr_text(&scan_out);
    let scan_stdout = stdout_text(&scan_out);

    assert!(scan_stderr.contains("warning [code=KBMETA_MISSING_REQUIRED_FIELDS]: skipped"));
    assert!(scan_stderr.contains("missing required fields:"));
    assert!(scan_stderr.contains("type"));
    assert!(scan_stdout.contains("Scanned 1 documents"));
}

#[test]
fn rebuild_strict_fails_on_invalid_metadata() {
    let ws = fixture_to_tempdir("kb_invalid_metadata");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let rebuild_out = run_kbmeta(ws.path(), &["rebuild", "--strict"]);
    assert!(!rebuild_out.status.success());

    let rebuild_stderr = stderr_text(&rebuild_out);
    assert!(rebuild_stderr.contains("failed to parse"));
}

#[test]
fn rebuild_normalize_tags_controls_index_values() {
    let ws = fixture_to_tempdir("kb_tags_raw");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let rebuild_raw = run_kbmeta(ws.path(), &["rebuild"]);
    assert!(
        rebuild_raw.status.success(),
        "default rebuild failed: {}",
        stderr_text(&rebuild_raw)
    );

    let tags_raw = read_index_tags(ws.path());
    assert!(tags_raw.contains(&"Rust".to_string()));
    assert!(tags_raw.contains(&"embedded systems".to_string()));
    assert!(tags_raw.contains(&"GPU_ACCEL".to_string()));

    let rebuild_normalized = run_kbmeta(ws.path(), &["rebuild", "--normalize-tags"]);
    assert!(
        rebuild_normalized.status.success(),
        "normalized rebuild failed: {}",
        stderr_text(&rebuild_normalized)
    );

    let tags_normalized = read_index_tags(ws.path());
    assert!(tags_normalized.contains(&"rust".to_string()));
    assert!(tags_normalized.contains(&"embedded-systems".to_string()));
    assert!(tags_normalized.contains(&"gpu-accel".to_string()));
}

#[test]
fn scan_reports_stale_index_after_document_change() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let rebuild_out = run_kbmeta(ws.path(), &["rebuild"]);
    assert!(
        rebuild_out.status.success(),
        "rebuild failed: {}",
        stderr_text(&rebuild_out)
    );

    let changed_doc = ws
        .path()
        .join("knowledge/documents/rust-tokio.md");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&changed_doc)
        .expect("changed document should open");
    writeln!(file, "\nNew line that changes content hash.")
        .expect("document mutation should succeed");

    let scan_out = run_kbmeta(ws.path(), &["scan"]);
    assert!(scan_out.status.success(), "scan failed: {}", stderr_text(&scan_out));

    let scan_stdout = stdout_text(&scan_out);
    assert!(scan_stdout.contains("Scanned 2 documents"));
    assert!(scan_stdout.contains("Stale index entries: 1"));
}

#[test]
fn suggest_reject_keeps_document_unchanged() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let doc = ws.path().join("knowledge/documents/new-note.md");
    let before = std::fs::read_to_string(&doc).expect("fixture doc should be readable");

    let out = run_kbmeta(
        ws.path(),
        &["suggest", "knowledge/documents/new-note.md", "--action", "reject"],
    );
    assert!(out.status.success(), "suggest reject failed: {}", stderr_text(&out));

    let after = std::fs::read_to_string(&doc).expect("fixture doc should be readable");
    assert_eq!(before, after);
}

#[test]
fn suggest_accept_writes_metadata_and_updates_index() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(
        ws.path(),
        &["suggest", "knowledge/documents/new-note.md", "--action", "accept"],
    );
    assert!(out.status.success(), "suggest accept failed: {}", stderr_text(&out));

    let doc = ws.path().join("knowledge/documents/new-note.md");
    let content = std::fs::read_to_string(&doc).expect("updated doc should be readable");
    assert!(content.starts_with("---\n"));
    assert!(content.contains("title: New Note"));

    let db_path = ws.path().join(".kb/index.sqlite");
    let conn = Connection::open(db_path).expect("index database should open");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("count query should run");
    assert_eq!(count, 2);
}

#[test]
fn suggest_edit_uses_metadata_from_yaml_file() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let edited_path = ws.path().join("edited-metadata.yaml");
    std::fs::write(
        &edited_path,
        "type: Reference\ntitle: Edited Title\ndescription: Edited desc.\ntags:\n  - edited tag\ntopics:\n  - docs\ntechnologies:\n  - Rust\nlanguages:\n  - rust\n",
    )
    .expect("edited metadata file should be written");

    let out = run_kbmeta(
        ws.path(),
        &[
            "suggest",
            "knowledge/documents/new-note.md",
            "--action",
            "edit",
            "--edit-file",
            "edited-metadata.yaml",
        ],
    );
    assert!(out.status.success(), "suggest edit failed: {}", stderr_text(&out));

    let doc = ws.path().join("knowledge/documents/new-note.md");
    let content = std::fs::read_to_string(&doc).expect("updated doc should be readable");
    assert!(content.contains("title: Edited Title"));
    assert!(content.contains("description: Edited desc."));
    assert!(content.contains("- edited-tag"));
}

#[test]
fn suggest_edit_without_edit_file_fails_without_tty() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(
        ws.path(),
        &["suggest", "knowledge/documents/new-note.md", "--action", "edit"],
    );
    assert!(!out.status.success());

    let stderr = stderr_text(&out);
    assert!(stderr.contains("interactive edit requires a TTY"));
    assert!(stderr.contains("--edit-file"));
}

#[test]
fn suggest_uses_existing_kb_vocabulary_rules() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(
        ws.path(),
        &["suggest", "knowledge/documents/new-note.md", "--action", "accept"],
    );
    assert!(out.status.success(), "suggest accept failed: {}", stderr_text(&out));

    let doc = ws.path().join("knowledge/documents/new-note.md");
    let content = std::fs::read_to_string(&doc).expect("updated doc should be readable");

    // Existing style uses Reference type and Rust/tokio casing.
    assert!(content.contains("type: Reference"));
    assert!(content.contains("- Rust"));
    assert!(content.contains("- tokio"));
}

#[test]
fn cook_writes_kb_ready_copy_without_changing_input() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let input = ws.path().join("knowledge/documents/new-note.md");
    let before = std::fs::read_to_string(&input).expect("input should be readable");

    let out = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "--accept"]);
    assert!(out.status.success(), "cook failed: {}", stderr_text(&out));

    let after = std::fs::read_to_string(&input).expect("input should be readable");
    assert_eq!(before, after);

    let output = ws.path().join("knowledge/staging/New_Note.md");
    let cooked = std::fs::read_to_string(&output).expect("cooked output should be readable");
    assert!(cooked.starts_with("---\n"));
    assert!(cooked.contains("title: New Note"));
}

#[test]
fn cook_writes_intermediates_when_enabled_in_config() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let cfg = ws.path().join(".kb/config.toml");
    let mut cfg_content = std::fs::read_to_string(&cfg).expect("config should be readable");
    cfg_content = cfg_content.replacen("[provider]", "keep_intermediate = true\n\n[provider]", 1);
    std::fs::write(&cfg, cfg_content).expect("config should be writable");

    let out = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "--accept"]);
    assert!(out.status.success(), "cook failed: {}", stderr_text(&out));

    let interm = ws.path().join(".kb/intermediate");
    assert!(interm.join("New_Note.clean.md").exists());
    assert!(interm.join("New_Note.context.md").exists());
    assert!(interm.join("New_Note.suggested.yaml").exists());
}

#[test]
fn cook_requires_accept_without_tty() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md"]);
    assert!(!out.status.success());
    let stderr = stderr_text(&out);
    assert!(stderr.contains("cook requires interactive TTY unless --accept is set for batch mode"));
}

#[test]
fn cook_short_accept_flag_works_in_batch() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "-a"]);
    assert!(out.status.success(), "cook -a failed: {}", stderr_text(&out));

    let output = ws.path().join("knowledge/staging/New_Note.md");
    let cooked = std::fs::read_to_string(&output).expect("cooked output should be readable");
    assert!(cooked.contains("title: New Note"));
}

#[test]
fn cook_accept_fails_on_output_collision() {
    let ws = fixture_to_tempdir("kb_suggest");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let first = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "-a"]);
    assert!(first.status.success(), "first cook failed: {}", stderr_text(&first));

    let second = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "-a"]);
    assert!(!second.status.success());
    let stderr = stderr_text(&second);
    assert!(stderr.contains("output collision:"));
}

#[test]
fn index_folders_creates_index_md_in_documents_root() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["index-folders"]);
    assert!(
        out.status.success(),
        "index-folders failed: {}",
        stderr_text(&out)
    );

    let index_path = ws.path().join("knowledge/documents/index.md");
    let content = std::fs::read_to_string(&index_path).expect("index.md should be readable");
    assert!(content.contains("# Index"));
    assert!(content.contains("untagged-note.md"));
    assert!(content.contains("rust-tokio.md"));
}

#[test]
fn index_folders_does_not_rewrite_without_flag() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let first = run_kbmeta(ws.path(), &["index-folders"]);
    assert!(first.status.success(), "first index-folders failed: {}", stderr_text(&first));

    let index_path = ws.path().join("knowledge/documents/index.md");
    std::fs::write(&index_path, "SENTINEL\n").expect("sentinel write should succeed");

    let second = run_kbmeta(ws.path(), &["index-folders"]);
    assert!(second.status.success(), "second index-folders failed: {}", stderr_text(&second));

    let content = std::fs::read_to_string(&index_path).expect("index.md should be readable");
    assert_eq!(content, "SENTINEL\n");
}

#[test]
fn index_runs_rebuild_and_folder_generation() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["index"]);
    assert!(out.status.success(), "index failed: {}", stderr_text(&out));

    let index_path = ws.path().join("knowledge/documents/index.md");
    assert!(index_path.exists(), "folder index should be generated");
    assert_eq!(read_documents_count(ws.path()), 2);
}

#[test]
fn query_prints_matching_filenames_by_default() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(index_out.status.success(), "index failed: {}", stderr_text(&index_out));

    let out = run_kbmeta(ws.path(), &["query", "--tag", "Rust"]);
    assert!(out.status.success(), "query failed: {}", stderr_text(&out));
    let stdout = stdout_text(&out);
    assert!(stdout.contains("knowledge/documents/rust-tokio.md"));
    assert!(!stdout.contains("knowledge/documents/untagged-note.md"));
}

#[test]
fn query_json_outputs_selected_fields() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(index_out.status.success(), "index failed: {}", stderr_text(&index_out));

    let out = run_kbmeta(
        ws.path(),
        &["query", "--lang", "rust", "--json", "--fields", "path,title,tags"],
    );
    assert!(out.status.success(), "query json failed: {}", stderr_text(&out));
    let stdout = stdout_text(&out);
    assert!(stdout.contains("\"path\""));
    assert!(stdout.contains("\"title\""));
    assert!(stdout.contains("\"tags\""));
    assert!(!stdout.contains("\"technologies\""));
}

#[test]
fn query_csv_outputs_header_and_rows() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(index_out.status.success(), "index failed: {}", stderr_text(&index_out));

    let out = run_kbmeta(
        ws.path(),
        &["query", "--lang", "rust", "--format", "csv", "--fields", "path,title,type"],
    );
    assert!(out.status.success(), "query csv failed: {}", stderr_text(&out));
    let stdout = stdout_text(&out);
    assert!(stdout.lines().next().unwrap_or_default().contains("path,title,type"));
    assert!(stdout.contains("knowledge/documents/rust-tokio.md"));
}

#[test]
fn global_dry_run_prevents_rebuild_writes() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["--dry-run", "rebuild"]);
    assert!(out.status.success(), "dry-run rebuild failed: {}", stderr_text(&out));

    let stdout = stdout_text(&out);
    assert!(stdout.contains("dry-run: full rebuild would scan"));

    let count = read_documents_count(ws.path());
    assert_eq!(count, 0, "dry-run should not write index rows");
}

#[test]
fn rebuild_scoped_updates_only_selected_folder_and_uses_cache() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let docs_root = ws.path().join("knowledge/documents");
    std::fs::create_dir_all(docs_root.join("alpha")).expect("alpha dir should be created");
    std::fs::create_dir_all(docs_root.join("beta")).expect("beta dir should be created");
    std::fs::write(docs_root.join("alpha/a.md"), "# alpha\n")
        .expect("alpha doc should be created");
    std::fs::write(docs_root.join("beta/b.md"), "# beta\n").expect("beta doc should be created");

    let first = run_kbmeta(ws.path(), &["rebuild"]);
    assert!(first.status.success(), "first rebuild failed: {}", stderr_text(&first));
    assert_eq!(read_documents_count(ws.path()), 4);

    std::fs::write(docs_root.join("alpha/a.md"), "# alpha\nchanged\n")
        .expect("alpha doc should be updated");

    let scoped_update = run_kbmeta(ws.path(), &["rebuild", "--path", "alpha"]);
    assert!(
        scoped_update.status.success(),
        "scoped rebuild failed: {}",
        stderr_text(&scoped_update)
    );
    let update_stdout = stdout_text(&scoped_update);
    assert!(update_stdout.contains("updated=1"));
    assert_eq!(read_documents_count(ws.path()), 4);

    let scoped_cached = run_kbmeta(ws.path(), &["rebuild", "--path", "alpha"]);
    assert!(
        scoped_cached.status.success(),
        "cached scoped rebuild failed: {}",
        stderr_text(&scoped_cached)
    );
    let cached_stdout = stdout_text(&scoped_cached);
    assert!(cached_stdout.contains("skipped_unchanged=1"));
    assert_eq!(read_documents_count(ws.path()), 4);

    std::fs::remove_file(docs_root.join("alpha/a.md")).expect("alpha doc should be deleted");
    let scoped_remove = run_kbmeta(ws.path(), &["rebuild", "--path", "alpha"]);
    assert!(
        scoped_remove.status.success(),
        "scoped removal rebuild failed: {}",
        stderr_text(&scoped_remove)
    );
    let remove_stdout = stdout_text(&scoped_remove);
    assert!(remove_stdout.contains("removed=1"));
    assert_eq!(read_documents_count(ws.path()), 3);
}

fn read_index_tags(root: &std::path::Path) -> Vec<String> {
    let db_path = root.join(".kb/index.sqlite");
    let conn = Connection::open(db_path).expect("index database should open");
    let mut stmt = conn
        .prepare("SELECT name FROM tags ORDER BY name")
        .expect("tags query should prepare");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("tags query should execute");

    rows.map(|row| row.expect("row should parse")).collect()
}

fn read_documents_count(root: &std::path::Path) -> i64 {
    let db_path = root.join(".kb/index.sqlite");
    let conn = Connection::open(db_path).expect("index database should open");
    conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("documents count query should run")
}

#[test]
fn query_fail_empty_returns_error_on_no_match() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(index_out.status.success(), "index failed: {}", stderr_text(&index_out));

    let out = run_kbmeta(ws.path(), &["query", "--tag", "nonexistent", "--fail-empty"]);
    assert!(!out.status.success());
    let stderr = stderr_text(&out);
    assert!(stderr.contains("no documents matched filters"));
}

#[test]
fn query_invalid_filter_fails() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(index_out.status.success(), "index failed: {}", stderr_text(&index_out));

    let out = run_kbmeta(ws.path(), &["query", "--tag", "rust!"]);
    assert!(!out.status.success());
    let stderr = stderr_text(&out);
    assert!(stderr.contains("invalid filter expression"));
}

#[test]
fn rebuild_dry_run_shows_scoped_paths() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["--dry-run", "rebuild", "--path", "knowledge/documents"]);
    assert!(out.status.success(), "dry-run scoped rebuild failed: {}", stderr_text(&out));
    let stdout = stdout_text(&out);
    assert!(stdout.contains("dry-run scope:"));
}

#[test]
fn scan_after_init_without_rebuild_reports_missing_metadata() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["scan"]);
    assert!(out.status.success(), "scan failed: {}", stderr_text(&out));
    let stdout = stdout_text(&out);
    assert!(stdout.contains("Missing metadata block: 1"));
}
