use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub struct Note {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub tags: String,
    #[allow(dead_code)] // created at DB holds it; not surfaced in MVP UI
    pub created: i64,
    pub updated: i64,
}

/// Open (and on first run, create) the database at `path`.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
            id      INTEGER PRIMARY KEY,
            title   TEXT NOT NULL,
            body    TEXT NOT NULL DEFAULT '',
            tags    TEXT NOT NULL DEFAULT '',
            created INTEGER NOT NULL,
            updated INTEGER NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
            title, body, content='notes', content_rowid='id'
        );
        CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
            INSERT INTO notes_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
        END;
        CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, title, body)
                VALUES ('delete', old.id, old.title, old.body);
        END;
        CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, title, body)
                VALUES ('delete', old.id, old.title, old.body);
            INSERT INTO notes_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
        END;",
    )?;
    Ok(conn)
}

/// Save a new note. Returns its id.
pub fn create(conn: &Connection, title: &str, body: &str, tags: &str) -> rusqlite::Result<i64> {
    let now = chrono_now();
    conn.execute(
        "INSERT INTO notes (title, body, tags, created, updated) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![title, body, tags, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Rebuild an existing note entirely.
pub fn update(conn: &Connection, id: i64, title: &str, body: &str, tags: &str) -> rusqlite::Result<()> {
    let now = chrono_now();
    conn.execute(
        "UPDATE notes SET title=?1, body=?2, tags=?3, updated=?4 WHERE id=?5",
        params![title, body, tags, now, id],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<Note>> {
    conn.query_row(
        "SELECT id, title, body, tags, created, updated FROM notes WHERE id=?1",
        params![id],
        row_to_note,
    )
    .optional()
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM notes WHERE id=?1", params![id])?;
    Ok(())
}

/// List notes. `query` matches title OR body via FTS; `tag` filters by exact tag.
/// Returns newest-updated first.
pub fn list(conn: &Connection, query: &str, tag: &str, limit: usize) -> rusqlite::Result<Vec<Note>> {
    let mut sql = String::from(
        "SELECT n.id, n.title, n.body, n.tags, n.created, n.updated
         FROM notes n ",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if !query.trim().is_empty() {
        // ponytail: FTS query string built by sanitising user input into a quoted blob
        sql.push_str("JOIN notes_fts f ON f.rowid = n.id WHERE notes_fts MATCH ?1 ");
        args.push(Box::new(fts_blob(query)));
    } else {
        sql.push_str("WHERE 1=1 ");
    }
    if !tag.is_empty() {
        // exact tag match; tags stored as space-separated list
        let idx = if query.trim().is_empty() { 1 } else { 2 };
        sql.push_str(&format!("AND ( ' ' || n.tags || ' ' ) LIKE ?{} ", idx));
        args.push(Box::new(format!("% {tag} %")));
    }
    sql.push_str("ORDER BY n.updated DESC LIMIT ?");
    args.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), row_to_note)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// List all distinct tags across notes, sorted.
pub fn distinct_tags(conn: &Connection) -> Vec<String> {
    let mut stmt = match conn.prepare("SELECT DISTINCT tags FROM notes") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    let mut set = std::collections::BTreeSet::new();
    if let Ok(rows) = rows {
        for r in rows.flatten() {
            for t in r.split_whitespace() {
                set.insert(t.to_string());
            }
        }
    }
    set.into_iter().collect()
}

/// Escape a user query into a safe FTS5 quoted string treating input literally.
fn fts_blob(input: &str) -> String {
    // quote each token as a phrase-preserving blob; keep it simple: wrap whole query
    // ponytail: naive, treats quotes as literal; fine for MVP
    format!("\"{}\"", input.replace('"', "\"\""))
}

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        tags: row.get(3)?,
        created: row.get(4)?,
        updated: row.get(5)?,
    })
}

// ponytail: no chrono dependency; seconds since unix epoch
fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve the default db path: $XDG_DATA_HOME/snip.db  (../snip.db = data dir + filename)
pub fn default_db_path() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("snip.db")
}