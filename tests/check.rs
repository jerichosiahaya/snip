//! Regression check: DB CRUD + FTS search consistency, on a temp db.
use snip::db;

fn tmp_conn() -> rusqlite::Connection {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("snip-cargo-test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    db::open(&path).unwrap()
}

#[test]
fn fts_stays_consistent() {
    let conn = tmp_conn();
    let id1 = db::create(&conn, "Meeting notes", "Discuss the new feature", "work").unwrap();
    db::create(&conn, "Buy groceries", "milk and eggs", "personal").unwrap();

    // title search
    let r = db::list(&conn, "meeting", "", 10).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].id, id1);

    // body search
    let r = db::list(&conn, "milk", "", 10).unwrap();
    assert_eq!(r.len(), 1);

    // tag filter
    assert_eq!(db::list(&conn, "", "work", 10).unwrap().len(), 1);

    // update rebuilds FTS
    db::update(&conn, id1, "Renamed", "totally different body", "work").unwrap();
    assert!(db::list(&conn, "meeting", "", 10).unwrap().is_empty());
    assert_eq!(db::list(&conn, "renamed", "", 10).unwrap().len(), 1);

    // delete removes from FTS
    db::delete(&conn, id1).unwrap();
    let remaining = db::list(&conn, "renamed", "", 10).unwrap();
    assert!(remaining.is_empty());
}