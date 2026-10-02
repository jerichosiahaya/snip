//! Editor handoff: shell-parsed $EDITOR, private unique temp files, cleanup.
#![cfg(unix)]
use snip::editor;
use std::path::PathBuf;

/// Write a fake editor script that records the path and mode it was given,
/// then appends a line. Returns (script path, record file path).
fn fake_editor(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("snip-editor-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("ed.sh");
    let record = dir.join("record");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\np=\"${{2:-$1}}\"\nprintf '%s %s' \"$p\" \"$(stat -c %a \"$p\")\" > '{}'\n[ \"$1\" = --flag ] || exit 3\necho added >> \"$p\"\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (script, record)
}

#[test]
fn editor_with_args_edits_private_temp_file_and_cleans_up() {
    let (script, record) = fake_editor("ok");
    let cmd = format!("{} --flag", script.display());
    let (txt, changed) = editor::edit_with(&cmd, "Title\n", "snip-test").unwrap();
    assert_eq!(txt, "Title\nadded\n");
    assert!(changed);

    let rec = std::fs::read_to_string(&record).unwrap();
    let (path, mode) = rec.rsplit_once(' ').unwrap();
    assert_eq!(mode, "600");
    assert!(!std::path::Path::new(path).exists(), "temp file left behind");
    std::fs::remove_dir_all(script.parent().unwrap()).ok();
}

#[test]
fn each_edit_gets_a_fresh_file_and_failures_clean_up() {
    let (script, record) = fake_editor("fresh");
    let cmd = format!("{} --flag", script.display());
    editor::edit_with(&cmd, "", "snip-test").unwrap();
    let first = std::fs::read_to_string(&record).unwrap();
    editor::edit_with(&cmd, "", "snip-test").unwrap();
    let second = std::fs::read_to_string(&record).unwrap();
    assert_ne!(first, second);

    // editor fails (script exits 3 without --flag): error, file still removed
    assert!(editor::edit_with(&script.display().to_string(), "x", "snip-test").is_err());
    let rec = std::fs::read_to_string(&record).unwrap();
    let (path, _) = rec.rsplit_once(' ').unwrap();
    assert!(!std::path::Path::new(path).exists(), "temp file left behind after failure");
    std::fs::remove_dir_all(script.parent().unwrap()).ok();
}
