use snip::{db, editor};

use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyModifiers},
    execute, style::Stylize,
    terminal::{self, ClearType},
};
use db::Note;
use std::io::{stdout, Write};

struct App {
    conn: rusqlite::Connection,
    notes: Vec<Note>,
    selected: usize,
    offset: usize, // index of the first note shown on screen
    query: String, // current search terms
    tag: String,   // active tag filter
    tags: Vec<String>, // all distinct tags for Tab cycling
    status: String,
}

fn main() -> rusqlite::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut db_flag: Option<String> = None;
    let mut cmd = String::new();
    let mut cmd_args: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--db" {
            i += 1;
            db_flag = args.get(i).cloned();
        } else if cmd.is_empty() {
            cmd = args[i].clone();
        } else {
            cmd_args.push(args[i].clone());
        }
        i += 1;
    }

    let db_path = match db_flag {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            // DB lives directly under the data dir: ~/.local/share/snip.db
            let p = db::default_db_path();
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            p
        }
    };
    let conn = db::open(&db_path)?;

    match cmd.as_str() {
        "add" => return quick_add(&conn, &cmd_args),
        "search" => {
            let q = cmd_args.first().cloned().unwrap_or_default();
            let tag = find_tag_flag(&cmd_args);
            let notes = db::list(&conn, &q, &tag, 50)?;
            for n in notes {
                println!("{}  [{}]  {}", n.title, short_tags(&n.tags), n.updated);
            }
            return Ok(());
        }
        _ => {} // interactive TUI
    }

    let mut app = App {
        conn,
        notes: Vec::new(),
        selected: 0,
        offset: 0,
        query: String::new(),
        tag: String::new(),
        tags: Vec::new(),
        status: String::from("↑↓ nav · Enter edit · C-n new · C-t tag · / search · C-q quit"),
    };
    app.tags = db::distinct_tags(&app.conn);
    app.refresh_list();
    run_ui(&mut app)
}

fn quick_add(conn: &rusqlite::Connection, args: &[String]) -> rusqlite::Result<()> {
    if args.is_empty() {
        eprintln!("usage: snip add \"note text\" [--tag x]");
        std::process::exit(1);
    }
    // text is first non-flag arg; rest are --tag values
    let mut text = String::new();
    let mut tags = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--tag" {
            if let Some(t) = it.next() {
                tags.push_str(t);
                tags.push(' ');
            }
        } else {
            text.push_str(a);
            text.push(' ');
        }
    }
    let trimmed = text.trim().to_string();
    let (title, body) = split_title_body(&trimmed);
    db::create(conn, &title, &body, tags.trim())?;
    println!("snip: note saved");
    Ok(())
}

fn split_title_body(trimmed: &str) -> (String, String) {
    // first line = title, rest = body; blank lines between them are only a separator
    let mut lines = trimmed.lines();
    let title = lines.next().unwrap_or("").to_string();
    let body = lines
        .skip_while(|l| l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (title, body)
}

fn find_tag_flag(args: &[String]) -> String {
    let mut tag = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--tag" {
            if let Some(t) = it.next() {
                tag = t.clone();
            }
        }
    }
    tag
}

fn short_tags(tags: &str) -> String {
    tags.split_whitespace().collect::<Vec<_>>().join(",")
}

impl App {
    fn refresh_list(&mut self) {
        if let Ok(notes) = db::list(&self.conn, &self.query, &self.tag, 100) {
            self.notes = notes;
            if self.selected >= self.notes.len() {
                self.selected = self.notes.len().saturating_sub(1);
            }
        }
    }
}

fn run_ui(app: &mut App) -> rusqlite::Result<()> {
    terminal::enable_raw_mode().ok();
    let mut out = stdout();
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide).ok();
    // purge any residual content from a prior run before drawing
    execute!(out, terminal::Clear(ClearType::All)).ok();

    loop {
        draw(app, &mut out);
        match event::read() {
            Ok(Event::Key(key)) => match key.code {
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Enter => {
                    if !app.notes.is_empty() {
                        let id = app.notes[app.selected].id;
                        let current = db::get(&app.conn, id).ok().flatten();
                        if let Some(note) = current {
                            let seed = format!("{}\n\n{}", note.title, note.body);
                            match edit_in_terminal(&seed, "snip-edit") {
                                Ok((txt, _)) => {
                                    // compare parsed content, not raw text: editors often
                                    // add a trailing newline to an untouched file
                                    let (title, body) = split_title_body(txt.trim());
                                    if title == note.title && body == note.body {
                                        app.status = String::from("no changes");
                                    } else if !title.is_empty() {
                                        db::update(&app.conn, id, &title, &body, &note.tags).ok();
                                        app.status = format!("updated: {} (tags: {})", title, note.tags);
                                    } else {
                                        app.status = String::from("note unchanged (title empty, skipped)");
                                    }
                                }
                                Err(e) => app.status = format!("edit failed: {e}"),
                            }
                        }
                        app.refresh_list();
                        app.tags = db::distinct_tags(&app.conn);
                    }
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    new_note(app);
                }
                KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => cycle_tag(app),
                KeyCode::Char('/') => {
                    app.query.clear();
                    input_loop(app, "/");
                }
                KeyCode::Up => {
                    if app.selected > 0 {
                        app.selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if app.selected + 1 < app.notes.len() {
                        app.selected += 1;
                    }
                }
                KeyCode::Delete if !app.notes.is_empty() => {
                    let id = app.notes[app.selected].id;
                    if confirm_delete(&app.conn, id) {
                        db::delete(&app.conn, id).ok();
                        app.refresh_list();
                        app.tags = db::distinct_tags(&app.conn);
                        app.status = String::from("note deleted");
                    }
                }
                _ => {}
            },
            Ok(Event::Resize(_, _)) => {}
            Err(e) => {
                app.status = format!("event error: {e}");
            }
            _ => {}
        }
    }

    terminal::disable_raw_mode().ok();
    execute!(out, terminal::LeaveAlternateScreen, cursor::Show).ok();
    Ok(())
}

fn new_note(app: &mut App) {
    let seed = "";
    match edit_in_terminal(seed, "snip-new") {
        Ok((txt, changed)) if changed && !txt.trim().is_empty() => {
            let (title, body) = split_title_body(txt.trim());
            let id = db::create(&app.conn, &title, &body, "").ok();
            if id.is_some() {
                app.status = format!("created: {}", title);
            }
        }
        _ => app.status = String::from("note not saved"),
    }
    app.refresh_list();
    app.tags = db::distinct_tags(&app.conn);
}

/// Hand the terminal to the editor: leave raw mode and the alternate screen while it runs.
fn edit_in_terminal(seed: &str, prefix: &str) -> std::io::Result<(String, bool)> {
    let mut out = stdout();
    terminal::disable_raw_mode().ok();
    execute!(out, terminal::LeaveAlternateScreen, cursor::Show).ok();
    let result = editor::edit(seed, prefix);
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(ClearType::All)).ok();
    terminal::enable_raw_mode().ok();
    result
}

/// minimal inline search/input prompt: reads chars into `buf` until Enter/Esc
fn input_loop(app: &mut App, prefix: &str) {
    loop {
        draw_prompt(app, prefix, &app.query);
        match event::read() {
            Ok(Event::Key(key)) => match key.code {
                KeyCode::Enter => break,
                KeyCode::Esc => {
                    app.query.clear();
                    break;
                }
                KeyCode::Char(c) => app.query.push(c),
                KeyCode::Backspace => {
                    app.query.pop();
                }
                _ => {}
            },
            _ => break,
        }
    }
    app.selected = 0;
    app.refresh_list();
}

fn cycle_tag(app: &mut App) {
    if app.tags.is_empty() && app.tag.is_empty() {
        app.status = String::from("no tags yet");
        return;
    }
    app.tag = next_tag(&app.tags, &app.tag);
    app.selected = 0;
    app.refresh_list();
}

/// Cycle all → tag1 → … → tagN → all. A tag that no longer exists resets to all.
fn next_tag(tags: &[String], current: &str) -> String {
    if current.is_empty() {
        return tags.first().cloned().unwrap_or_default();
    }
    match tags.iter().position(|t| t == current) {
        Some(i) => tags.get(i + 1).cloned().unwrap_or_default(),
        None => String::new(),
    }
}

fn confirm_delete(conn: &rusqlite::Connection, id: i64) -> bool {
    // inline y/n confirm; "y" deletes, anything else aborts. Esc aborts.
    let title = db::get(conn, id)
        .ok()
        .flatten()
        .map(|n| n.title)
        .unwrap_or_default();
    let mut out = stdout();
    let _ = write!(
        out,
        "\r\n{} Delete '{title}'? [yN] ",
        " SNIP ".black().on_red()
    );
    let _ = out.flush();
    loop {
        match event::read() {
            Ok(Event::Key(k)) => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => return true,
                KeyCode::Esc => return false,
                _ => return false,
            },
            _ => return false,
        }
    }
}

// Raw mode disables newline translation: each LF needs a carriage return.
fn write_frame(out: &mut impl Write, frame: &str) -> std::io::Result<()> {
    execute!(out, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All))?;
    write!(out, "{}\r\n", frame.replace("\r\n", "\n").replace('\n', "\r\n"))?;
    out.flush()
}

/// Terminal (columns, rows), with a sane fallback when it can't be queried.
fn screen_size() -> (usize, usize) {
    terminal::size().map_or((80, 24), |(w, h)| (w as usize, h as usize))
}

/// Shorten `s` to at most `width` columns (one per char), marking cuts with `…`.
/// Control chars (tabs etc.) become spaces so they can't break the layout.
fn fit(s: &str, width: usize) -> String {
    let clean = s.chars().map(|c| if c.is_control() { ' ' } else { c });
    if s.chars().count() <= width {
        return clean.collect();
    }
    let mut out: String = clean.take(width.saturating_sub(1)).collect();
    if width > 0 {
        out.push('…');
    }
    out
}

/// Scroll so `selected` stays inside a window of `rows` lines starting at `offset`.
fn scroll_offset(selected: usize, offset: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    if selected < offset {
        selected
    } else if selected >= offset + rows {
        selected + 1 - rows
    } else {
        offset
    }
}

fn draw(app: &mut App, out: &mut std::io::Stdout) {
    let (width, height) = screen_size();
    // header + list + blank/help + blank/status, and write_frame's final newline
    // must not scroll the screen: list rows = height - 6
    let rows = height.saturating_sub(6).max(1);
    app.offset = scroll_offset(app.selected, app.offset, rows);

    let mut frame = String::new();

    // header line: query + tag
    frame.push_str(&format!(
        "{}{}\n",
        "SNIP".black().on_magenta(),
        fit(&format!(" search: `{}`  tag=({})", app.query,
            if app.tag.is_empty() { "all" } else { &app.tag }), width.saturating_sub(4)),
    ));

    // results: list + selected line shows preview
    for (i, note) in app.notes.iter().enumerate().skip(app.offset).take(rows) {
        let selected = i == app.selected;
        let mut text = note.title.clone();
        if selected && !note.body.is_empty() {
            text.push_str("  —  ");
            text.push_str(&note.body.replace('\n', " "));
        }
        if text.is_empty() {
            text.push_str("(untitled)");
        }
        let mut render = String::from(if selected { "> " } else { "  " });
        render.push_str(&text);
        if !note.tags.is_empty() {
            render.push_str(&format!("  [{}]", short_tags(&note.tags)));
        }
        let render = fit(&render, width);
        if selected {
            frame.push_str(&format!("{}\n", render.bold().yellow()));
        } else {
            frame.push_str(&format!("{render}\n"));
        }
    }

    // help + status lines
    frame.push_str(&format!(
        "\n{}\n",
        fit(&format!("{} notes · ↑↓ move · Enter edit · C-n new · C-t tag · C-q quit",
            app.notes.len()), width)
    ));
    frame.push_str(&format!("\n{}", fit(&app.status, width)));

    let _ = write_frame(out, &frame);
}

/// Like [`fit`], but keeps the end of `s` and marks the cut at the start.
fn fit_tail(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n <= width {
        return fit(s, width);
    }
    let tail: String = s.chars().skip(n + 1 - width.max(1)).collect();
    fit(&format!("…{tail}"), width)
}

fn draw_prompt(app: &App, prefix: &str, query: &str) {
    let mut out = stdout();
    let (width, height) = screen_size();
    // header + hint + blank + results, each ending in a newline, then write_frame
    // adds one more: the cursor must stay on screen, so results = height - 5
    let rows = height.saturating_sub(5).max(1);

    let mut frame = String::new();
    frame.push_str(&format!(
        "{}{}\n",
        "SNIP".black().on_magenta(),
        // keep the end of a long query (where the cursor is) visible
        fit_tail(&format!(" {prefix}{query}_"), width.saturating_sub(4)),
    ));
    frame.push_str(&format!("{}\n\n", fit("(type to search · Enter apply · Esc cancel)", width)));
    if let Ok(notes) = db::list(&app.conn, &app.query, &app.tag, rows) {
        for n in notes {
            frame.push_str(&format!("{}\n", fit(&n.title, width)));
        }
    }
    let _ = write_frame(&mut out, &frame);
}

#[test]
fn raw_mode_lines_return_to_left_edge() {
    let mut output = Vec::new();
    write_frame(&mut output, "SNIP search: ``  tag=(all)\n> test\n\n1 notes\r\nhelp").unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.starts_with("\x1b[1;1H\x1b[2J"));
    assert!(output.ends_with("SNIP search: ``  tag=(all)\r\n> test\r\n\r\n1 notes\r\nhelp\r\n"));
}

#[test]
fn editing_round_trip_keeps_body_stable() {
    let (mut title, mut body) = (String::from("Title"), String::from("line one\n\n  indented"));
    for _ in 0..3 {
        let seed = format!("{title}\n\n{body}");
        (title, body) = split_title_body(seed.trim());
    }
    assert_eq!(title, "Title");
    assert_eq!(body, "line one\n\n  indented");
    // a trailing newline added by the editor parses to the same note
    assert_eq!(split_title_body("Title\n\nline one\n".trim()), ("Title".into(), "line one".into()));
}

#[test]
fn tag_cycle_returns_to_all() {
    let tags = vec!["home".to_string(), "work".to_string()];
    assert_eq!(next_tag(&tags, ""), "home");
    assert_eq!(next_tag(&tags, "home"), "work");
    assert_eq!(next_tag(&tags, "work"), "");
    assert_eq!(next_tag(&tags, "deleted"), "");
    assert_eq!(next_tag(&[], "deleted"), "");
    assert_eq!(next_tag(&[], ""), "");
}

#[test]
fn scroll_keeps_selection_visible() {
    assert_eq!(scroll_offset(0, 0, 5), 0);
    assert_eq!(scroll_offset(4, 0, 5), 0);
    assert_eq!(scroll_offset(5, 0, 5), 1); // moved past the bottom
    assert_eq!(scroll_offset(7, 3, 5), 3); // still inside the window
    assert_eq!(scroll_offset(2, 3, 5), 2); // moved above the top
    assert_eq!(scroll_offset(3, 0, 0), 3); // tiny terminal: one row
}

#[test]
fn fit_truncates_to_width() {
    assert_eq!(fit("hello", 10), "hello");
    assert_eq!(fit("hello", 5), "hello");
    assert_eq!(fit("hello world", 6), "hello…");
    assert_eq!(fit("a\tb", 10), "a b");
    assert_eq!(fit("héllo wörld", 4), "hél…");
    assert_eq!(fit("abc", 0), "");
    assert_eq!(fit_tail(" /long query_", 6), "…uery_");
    assert_eq!(fit_tail("abc", 6), "abc");
}
