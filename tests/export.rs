//! `snip export`: Markdown files with front matter, one per note.
use snip::db::{self, Note};
use snip::export;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

fn tmp(name: &str) -> (rusqlite::Connection, PathBuf) {
    let base = std::env::temp_dir().join(format!("snip-export-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    (db::open(&base.join("notes.db")).unwrap(), base.join("out"))
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn markdown_has_front_matter_heading_and_body() {
    let note = Note {
        id: 12,
        title: "Meeting: \"Q4\" plan".into(),
        body: "- ship it\n\n## Details\nmore\n\n".into(),
        tags: "work q4".into(),
        created: 1_790_959_740,
        updated: 1_790_963_340,
    };
    assert_eq!(
        export::to_markdown(&note),
        "---\n\
         title: \"Meeting: \\\"Q4\\\" plan\"\n\
         tags: [\"work\", \"q4\"]\n\
         created: 2026-10-02T16:49:00Z\n\
         updated: 2026-10-02T17:49:00Z\n\
         snip_id: 12\n\
         ---\n\
         \n\
         # Meeting: \"Q4\" plan\n\
         \n\
         - ship it\n\
         \n\
         ## Details\n\
         more\n"
    );

    let bare = Note { id: 1, title: String::new(), body: String::new(), tags: String::new(), created: 0, updated: 0 };
    let md = export::to_markdown(&bare);
    assert!(md.contains("title: \"\"\ntags: []\n"));
    assert!(md.ends_with("---\n\n# Untitled\n"));
}

#[test]
fn slugs_are_safe_file_names() {
    assert_eq!(export::slug("Meeting notes"), "meeting-notes");
    assert_eq!(export::slug("  Hello, World!!  "), "hello-world");
    assert_eq!(export::slug("Café Déjà Vu"), "café-déjà-vu");
    assert_eq!(export::slug("会議のメモ"), "会議のメモ");
    assert_eq!(export::slug("../../etc/passwd"), "etc-passwd");
    assert_eq!(export::slug("C:\\Windows\\x"), "c-windows-x");
    assert_eq!(export::slug(""), "untitled");
    assert_eq!(export::slug("!!! ???"), "untitled");
    let long = export::slug(&"word ".repeat(40));
    assert!(long.chars().count() <= 60 && !long.ends_with('-'), "{long}");
}

#[test]
fn exports_every_note_with_unique_names() {
    let (conn, out) = tmp("all");
    // ids 1..=7: two "x" notes, a note whose slug is "x-7", and two untitled ones
    for (title, tags) in [("x", "a"), ("x 7", ""), ("Groceries", "home"), ("", ""), ("", ""), ("Other", ""), ("x", "")] {
        db::create(&conn, title, "body", tags).unwrap();
    }
    let paths = export::export(&conn, &out, "").unwrap();
    assert_eq!(
        names(&paths),
        ["x.md", "x-7.md", "groceries.md", "untitled.md", "untitled-5.md", "other.md", "x-7-2.md"]
    );
    let mut on_disk: Vec<String> = std::fs::read_dir(&out).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    on_disk.sort();
    assert_eq!(on_disk.len(), 7, "no file overwrote another: {on_disk:?}");
    assert!(std::fs::read_to_string(out.join("x-7-2.md")).unwrap().contains("snip_id: 7\n"));
}

#[test]
fn tag_filter_mtime_and_re_export() {
    let (conn, out) = tmp("tag");
    let id = db::create(&conn, "Work note", "v1", "work").unwrap();
    db::create(&conn, "Home note", "x", "home").unwrap();

    let paths = export::export(&conn, &out, "work").unwrap();
    assert_eq!(names(&paths), ["work-note.md"]);

    let note = db::get(&conn, id).unwrap().unwrap();
    let mtime = std::fs::metadata(&paths[0]).unwrap().modified().unwrap();
    assert_eq!(mtime, UNIX_EPOCH + Duration::from_secs(note.updated as u64));

    // re-export overwrites its own files and leaves anything else alone
    std::fs::write(out.join("mine.txt"), "keep me").unwrap();
    db::update(&conn, id, "Work note", "v2", "work").unwrap();
    export::export(&conn, &out, "").unwrap();
    assert!(std::fs::read_to_string(out.join("work-note.md")).unwrap().ends_with("\nv2\n"));
    assert!(out.join("home-note.md").exists());
    assert_eq!(std::fs::read_to_string(out.join("mine.txt")).unwrap(), "keep me");
}

#[test]
fn empty_database_exports_nothing() {
    let (conn, out) = tmp("empty");
    assert!(export::export(&conn, &out, "").unwrap().is_empty());
    assert!(out.is_dir());
}
