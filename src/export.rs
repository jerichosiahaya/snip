//! `snip export`: one Markdown file per note, so notes are never locked in the
//! database. Files carry YAML front matter (title, tags, times) that Obsidian
//! and other Markdown tools read, and their modification time is the note's
//! last edit.
use crate::db::{self, Note};
use rusqlite::Connection;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const MAX_SLUG_CHARS: usize = 60;

/// Write every note (or only those tagged `tag`) into `dir` as `<slug>.md`.
/// Existing files with the same names are overwritten; other files are left
/// alone. Returns the paths written, in note-id order.
pub fn export(conn: &Connection, dir: &Path, tag: &str) -> io::Result<Vec<PathBuf>> {
    let notes = db::all(conn, tag).map_err(io::Error::other)?;
    fs::create_dir_all(dir)?;
    let mut taken = HashSet::new();
    let mut written = Vec::with_capacity(notes.len());
    for note in &notes {
        let path = dir.join(file_name(note, &mut taken));
        fs::write(&path, to_markdown(note))?;
        // keep "sort by modified" meaningful in file managers and editors
        if let Ok(secs) = u64::try_from(note.updated) {
            File::options()
                .write(true)
                .open(&path)?
                .set_modified(UNIX_EPOCH + Duration::from_secs(secs))?;
        }
        written.push(path);
    }
    Ok(written)
}

/// `<slug>.md`, or `<slug>-<id>.md` when an earlier note already has the slug.
/// Notes are exported oldest first, so the oldest keeps the plain name.
fn file_name(note: &Note, taken: &mut HashSet<String>) -> String {
    let slug = slug(&note.title);
    let mut name = slug.clone();
    let mut n = 1;
    // `<slug>-<id>` can itself be another note's slug (a note titled "x 7"), so keep going
    while taken.contains(&name) {
        name = if n == 1 { format!("{slug}-{}", note.id) } else { format!("{slug}-{}-{n}", note.id) };
        n += 1;
    }
    taken.insert(name.clone());
    format!("{name}.md")
}

/// A file-name-safe version of a title: lowercase letters and digits (any
/// script) joined by single dashes, at most 60 characters. `untitled` if empty.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c);
        } else {
            dash = true;
        }
    }
    let mut out: String = out.chars().take(MAX_SLUG_CHARS).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "untitled".into() } else { out }
}

/// The note as Markdown with YAML front matter. Strings are written as JSON
/// strings, which YAML reads as double-quoted scalars, so any title is safe.
pub fn to_markdown(note: &Note) -> String {
    let quote = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let tags: Vec<String> = note.tags.split_whitespace().map(quote).collect();
    let title = if note.title.trim().is_empty() { "Untitled" } else { note.title.as_str() };
    let mut md = format!(
        "---\ntitle: {}\ntags: [{}]\ncreated: {}\nupdated: {}\nsnip_id: {}\n---\n\n# {}\n",
        quote(&note.title),
        tags.join(", "),
        db::iso_time(note.created),
        db::iso_time(note.updated),
        note.id,
        title,
    );
    let body = note.body.trim_end();
    if !body.is_empty() {
        md.push('\n');
        md.push_str(body);
        md.push('\n');
    }
    md
}
