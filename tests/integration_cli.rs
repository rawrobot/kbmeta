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
fn index_and_scan_report_expected_counts() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(
        index_out.status.success(),
        "index failed: {}",
        stderr_text(&index_out)
    );

    let scan_out = run_kbmeta(ws.path(), &["scan"]);
    assert!(scan_out.status.success(), "scan failed: {}", stderr_text(&scan_out));

    let scan_stdout = stdout_text(&scan_out);
    assert!(scan_stdout.contains("Scanned 3 documents"));
    assert!(scan_stdout.contains("Missing metadata block: 2"));
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
fn index_strict_fails_on_invalid_metadata() {
    let ws = fixture_to_tempdir("kb_invalid_metadata");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_out = run_kbmeta(ws.path(), &["index", "--strict"]);
    assert!(!index_out.status.success());

    let index_stderr = stderr_text(&index_out);
    assert!(index_stderr.contains("failed to parse"));
}

#[test]
fn index_normalize_tags_controls_index_values() {
    let ws = fixture_to_tempdir("kb_tags_raw");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let index_raw = run_kbmeta(ws.path(), &["index"]);
    assert!(
        index_raw.status.success(),
        "default index failed: {}",
        stderr_text(&index_raw)
    );

    let tags_raw = read_index_tags(ws.path());
    assert!(tags_raw.contains(&"Rust".to_string()));
    assert!(tags_raw.contains(&"embedded systems".to_string()));
    assert!(tags_raw.contains(&"GPU_ACCEL".to_string()));

    let index_normalized = run_kbmeta(ws.path(), &["index", "--normalize-tags"]);
    assert!(
        index_normalized.status.success(),
        "normalized index failed: {}",
        stderr_text(&index_normalized)
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

    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(
        index_out.status.success(),
        "index failed: {}",
        stderr_text(&index_out)
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
    assert!(scan_stdout.contains("Scanned 3 documents"));
    assert!(scan_stdout.contains("Stale index entries: 1"));
}

#[test]
fn cook_writes_kb_ready_copy_without_changing_input() {
    let ws = fixture_to_tempdir("kb_minimal");

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
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let cfg = ws.path().join(".kb/config.toml");
    let mut cfg_content = std::fs::read_to_string(&cfg).expect("config should be readable");
    cfg_content = cfg_content.replace("keep_intermediate = false", "keep_intermediate = true");
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
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md"]);
    assert!(!out.status.success());
    let stderr = stderr_text(&out);
    assert!(stderr.contains("cook requires interactive TTY unless --accept is set for batch mode"));
}

#[test]
fn cook_short_accept_flag_works_in_batch() {
    let ws = fixture_to_tempdir("kb_minimal");

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
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let first = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "-a"]);
    assert!(first.status.success(), "first cook failed: {}", stderr_text(&first));

    let second = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "-a"]);
    assert!(!second.status.success());
    let stderr = stderr_text(&second);
    assert!(stderr.contains("output collision:"));
    assert!(stderr.contains("--rewrite"));
}

#[test]
fn cook_rewrite_overwrites_existing_output() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let first = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "--accept"]);
    assert!(first.status.success(), "first cook failed: {}", stderr_text(&first));

    let second = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "--accept", "--rewrite"]);
    assert!(second.status.success(), "cook --rewrite failed: {}", stderr_text(&second));

    let output = ws.path().join("knowledge/staging/New_Note.md");
    let cooked = std::fs::read_to_string(&output).expect("cooked output should be readable");
    assert!(cooked.contains("title: New Note"));
}

#[test]
fn cook_rewrite_requires_accept() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["cook", "knowledge/documents/new-note.md", "--rewrite"]);
    assert!(!out.status.success());
    let stderr = stderr_text(&out);
    assert!(stderr.contains("--rewrite requires --accept"));
}

#[test]
fn index_folders_creates_index_md_in_documents_root() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["index"]);
    assert!(
        out.status.success(),
        "index failed: {}",
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

    let first = run_kbmeta(ws.path(), &["index"]);
    assert!(first.status.success(), "first index failed: {}", stderr_text(&first));

    let index_path = ws.path().join("knowledge/documents/index.md");
    std::fs::write(&index_path, "SENTINEL\n").expect("sentinel write should succeed");

    let second = run_kbmeta(ws.path(), &["index"]);
    assert!(second.status.success(), "second index failed: {}", stderr_text(&second));

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
    assert_eq!(read_documents_count(ws.path()), 3);
}

#[test]
fn index_creates_subdir_sections_and_nested_indexes() {
    let ws = fixture_to_tempdir("kb_minimal");
    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let docs_root = ws.path().join("knowledge/documents");
    let sub = docs_root.join("crates");
    std::fs::create_dir_all(&sub).expect("create subdir");
    std::fs::write(
        sub.join("tokio.md"),
        "---\ntype: Note\ntitle: Tokio vs Smol\ndescription: Async runtimes.\n---\n",
    )
    .expect("write subdir doc");

    let out = run_kbmeta(ws.path(), &["index"]);
    assert!(out.status.success(), "index failed: {}", stderr_text(&out));

    let sub_index = sub.join("index.md");
    assert!(sub_index.exists(), "subdir index should be generated");

    let parent_index = docs_root.join("index.md");
    let content = std::fs::read_to_string(&parent_index).expect("parent index readable");
    assert!(content.contains("## Subdirectories"));
    assert!(content.contains("[crates](crates/)"));
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
fn global_dry_run_prevents_index_writes() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["--dry-run", "index"]);
    assert!(out.status.success(), "dry-run index failed: {}", stderr_text(&out));

    let stdout = stdout_text(&out);
    assert!(stdout.contains("dry-run: full rebuild would scan"));

    let count = read_documents_count(ws.path());
    assert_eq!(count, 0, "dry-run should not write index rows");
}

#[test]
fn index_scoped_updates_only_selected_folder_and_uses_cache() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let docs_root = ws.path().join("knowledge/documents");
    std::fs::create_dir_all(docs_root.join("alpha")).expect("alpha dir should be created");
    std::fs::create_dir_all(docs_root.join("beta")).expect("beta dir should be created");
    std::fs::write(docs_root.join("alpha/a.md"), "# alpha\n")
        .expect("alpha doc should be created");
    std::fs::write(docs_root.join("beta/b.md"), "# beta\n").expect("beta doc should be created");

    let first = run_kbmeta(ws.path(), &["index"]);
    assert!(first.status.success(), "first index failed: {}", stderr_text(&first));
    assert_eq!(read_documents_count(ws.path()), 5);

    std::fs::write(docs_root.join("alpha/a.md"), "# alpha\nchanged\n")
        .expect("alpha doc should be updated");

    let scoped_update = run_kbmeta(ws.path(), &["index", "--path", "alpha"]);
    assert!(
        scoped_update.status.success(),
        "scoped index failed: {}",
        stderr_text(&scoped_update)
    );
    let update_stdout = stdout_text(&scoped_update);
    assert!(update_stdout.contains("updated=1"));
    assert_eq!(read_documents_count(ws.path()), 5);

    let scoped_cached = run_kbmeta(ws.path(), &["index", "--path", "alpha"]);
    assert!(
        scoped_cached.status.success(),
        "cached scoped index failed: {}",
        stderr_text(&scoped_cached)
    );
    let cached_stdout = stdout_text(&scoped_cached);
    assert!(cached_stdout.contains("skipped_unchanged=1"));
    assert_eq!(read_documents_count(ws.path()), 5);

    std::fs::remove_file(docs_root.join("alpha/a.md")).expect("alpha doc should be deleted");
    let scoped_remove = run_kbmeta(ws.path(), &["index", "--path", "alpha"]);
    assert!(
        scoped_remove.status.success(),
        "scoped removal index failed: {}",
        stderr_text(&scoped_remove)
    );
    let remove_stdout = stdout_text(&scoped_remove);
    assert!(remove_stdout.contains("removed=1"));
    assert_eq!(read_documents_count(ws.path()), 4);
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
fn index_dry_run_shows_scoped_paths() {
    let ws = fixture_to_tempdir("kb_minimal");

    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));

    let out = run_kbmeta(ws.path(), &["--dry-run", "index", "--path", "knowledge/documents"]);
    assert!(out.status.success(), "dry-run scoped index failed: {}", stderr_text(&out));
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
    assert!(stdout.contains("Missing metadata block: 2"));
}

fn indexed_minimal_workspace() -> tempfile::TempDir {
    let ws = fixture_to_tempdir("kb_minimal");
    let init_out = run_kbmeta(ws.path(), &["init"]);
    assert!(init_out.status.success(), "init failed: {}", stderr_text(&init_out));
    let index_out = run_kbmeta(ws.path(), &["index"]);
    assert!(index_out.status.success(), "index failed: {}", stderr_text(&index_out));
    ws
}

#[test]
fn search_json_returns_ranked_identity_fields() {
    let ws = indexed_minimal_workspace();

    let out = run_kbmeta(ws.path(), &["search", "async rust runtime", "--json"]);
    assert!(out.status.success(), "search failed: {}", stderr_text(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout_text(&out)).expect("valid json");
    let first = &json.as_array().expect("array")[0];
    assert!(first["path"].as_str().unwrap().ends_with("knowledge/documents/rust-tokio.md"));
    assert_eq!(first["title"], "Rust Tokio");
    assert!(first["id"].is_i64());
    assert!(first["score"].as_f64().unwrap() > 0.0);
    assert_eq!(first.as_object().unwrap().len(), 4);
}

#[test]
fn search_filters_constrain_text_matches() {
    let ws = indexed_minimal_workspace();

    let out = run_kbmeta(ws.path(), &["search", "rust", "--tag", "-Rust", "--json"]);
    assert!(out.status.success(), "search failed: {}", stderr_text(&out));
    assert!(!stdout_text(&out).contains("rust-tokio.md"));
}

#[test]
fn search_empty_json_and_fail_empty() {
    let ws = indexed_minimal_workspace();

    let out = run_kbmeta(ws.path(), &["search", "kubernetes", "--json"]);
    assert!(out.status.success());
    assert_eq!(stdout_text(&out).trim(), "[]");

    let out = run_kbmeta(ws.path(), &["search", "kubernetes", "--fail-empty"]);
    assert!(!out.status.success());
    assert!(stderr_text(&out).contains("No matching documents found."));
}

#[test]
fn search_rejects_invalid_filter_expression() {
    let ws = indexed_minimal_workspace();

    let out = run_kbmeta(ws.path(), &["search", "git", "--tag", "version control"]);
    assert!(!out.status.success());
    assert!(stderr_text(&out).contains("invalid filter expression"));
}
