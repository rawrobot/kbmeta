use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;
use walkdir::WalkDir;

pub fn fixture_to_tempdir(name: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    copy_tree(&src, temp.path());
    temp
}

pub fn run_kbmeta(root: &Path, args: &[&str]) -> Output {
    let exe = std::env::var("CARGO_BIN_EXE_kbmeta").expect("test binary path should be available");
    Command::new(exe)
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("kbmeta command should run")
}

pub fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn copy_tree(src: &Path, dst: &Path) {
    for entry in WalkDir::new(src) {
        let entry = entry.expect("walkdir entry should be readable");
        let rel = entry
            .path()
            .strip_prefix(src)
            .expect("fixture path should be under source");
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).expect("target dir should be created");
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("parent dir should be created");
            }
            fs::copy(entry.path(), &target).expect("fixture file should be copied");
        }
    }
}
