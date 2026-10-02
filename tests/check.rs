//! Regression check: DB CRUD + FTS search consistency, on a temp db.
use snip::db;

fn tmp_conn(name: &str) -> rusqlite::Connection {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("snip-cargo-test-{}-{name}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    db::open(&path).unwrap()
}

#[test]
fn fts_stays_consistent() {
    let conn = tmp_conn("fts");
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
#[test]
fn search_matches_word_prefixes_in_any_order() {
    let conn = tmp_conn("prefix");
    let id = db::create(&conn, "Meeting notes", "quarterly planning", "").unwrap();
    db::create(&conn, "Groceries", "milk", "").unwrap();

    for q in ["meet", "notes meeting", "MEET plan", "  quarter  "] {
        let r = db::list(&conn, q, "", 10).unwrap();
        assert_eq!(r.len(), 1, "query {q:?}");
        assert_eq!(r[0].id, id, "query {q:?}");
    }
    // every word must match
    assert!(db::list(&conn, "meeting milk", "", 10).unwrap().is_empty());
}

#[test]
fn search_treats_fts_syntax_literally() {
    let conn = tmp_conn("syntax");
    db::create(&conn, "Meeting notes", "", "").unwrap();
    for q in ["\"", "*", "AND", "(", "meet)", "NEAR(a b)", "a\"b", "title:x", "-x", "^"] {
        assert!(db::list(&conn, q, "", 10).is_ok(), "query {q:?} errored");
    }
}
