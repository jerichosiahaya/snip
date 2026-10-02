use snip::ui::{self, Mode};
use snip::{db, editor};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::PrintStyledContent,
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
    tags: Vec<String>, // all distinct tags for Ctrl-T cycling
    mode: Mode,
    status: String, // one-off message, shown in the footer until the next key
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
        "mcp" => {
            let stdin = std::io::stdin();
            if let Err(e) = snip::mcp::serve(&conn, stdin.lock(), std::io::stdout().lock()) {
                eprintln!("snip mcp: {e}");
                std::process::exit(1);
            }
            return Ok(());
        }
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
        mode: Mode::Browse,
        status: String::new(),
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
        if let Ok(notes) = db::list(&self.conn, &self.query, &self.tag, 5000) {
            self.notes = notes;
            if self.selected >= self.notes.len() {
                self.selected = self.notes.len().saturating_sub(1);
            }
        }
    }

    /// Re-run the query and the tag list after a write, keeping `id` selected
    /// when it is still in the list.
    fn reload(&mut self, id: Option<i64>) {
        self.refresh_list();
        self.tags = db::distinct_tags(&self.conn);
        if let Some(pos) = id.and_then(|id| self.notes.iter().position(|n| n.id == id)) {
            self.selected = pos;
        }
    }

    fn set_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;
        self.refresh_list();
    }

    fn view(&self) -> ui::View<'_> {
        ui::View {
            notes: &self.notes,
            selected: self.selected,
            offset: self.offset,
            query: &self.query,
            tag: &self.tag,
            mode: self.mode,
            status: &self.status,
            now: unix_now(),
        }
    }
}

enum Flow {
    Continue,
    Quit,
}

fn run_ui(app: &mut App) -> rusqlite::Result<()> {
    terminal::enable_raw_mode().ok();
    let mut out = stdout();
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(ClearType::All)).ok();

    loop {
        draw(app, &mut out);
        let key = match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => k,
            Ok(Event::Resize(_, _)) => {
                execute!(out, terminal::Clear(ClearType::All)).ok();
                continue;
            }
            Ok(_) => continue,
            Err(_) => break, // terminal gone: nothing left to read keys from
        };
        app.status.clear();
        if let Flow::Quit = handle_key(app, key) {
            break;
        }
    }

    terminal::disable_raw_mode().ok();
    execute!(out, terminal::LeaveAlternateScreen, cursor::Show).ok();
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Flow {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('c')) {
        return Flow::Quit;
    }

    match app.mode {
        Mode::ConfirmDelete => {
            app.mode = Mode::Browse;
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                delete_selected(app);
            } else {
                app.status = String::from("delete cancelled");
            }
            return Flow::Continue;
        }
        Mode::Search => match key.code {
            KeyCode::Char(c) if !ctrl => {
                let mut q = std::mem::take(&mut app.query);
                q.push(c);
                app.set_query(q);
                return Flow::Continue;
            }
            KeyCode::Backspace => {
                let mut q = std::mem::take(&mut app.query);
                q.pop();
                app.set_query(q);
                return Flow::Continue;
            }
            KeyCode::Enter => {
                app.mode = Mode::Browse;
                return Flow::Continue;
            }
            KeyCode::Esc => {
                app.mode = Mode::Browse;
                app.set_query(String::new());
                return Flow::Continue;
            }
            _ => {} // navigation and Ctrl keys work while searching
        },
        Mode::Browse => {}
    }

    let rows = ui::list_rows(screen_size().1);
    let last = app.notes.len().saturating_sub(1);
    match key.code {
        KeyCode::Up => app.selected = app.selected.saturating_sub(1),
        KeyCode::Down => app.selected = (app.selected + 1).min(last),
        KeyCode::PageUp => app.selected = app.selected.saturating_sub(rows),
        KeyCode::PageDown => app.selected = (app.selected + rows).min(last),
        KeyCode::Home => app.selected = 0,
        KeyCode::End => app.selected = last,
        KeyCode::Enter => edit_selected(app),
        KeyCode::Char('n') if ctrl => new_note(app),
        KeyCode::Char('t') if ctrl => cycle_tag(app),
        KeyCode::Char('/') => app.mode = Mode::Search,
        KeyCode::Delete if !app.notes.is_empty() => app.mode = Mode::ConfirmDelete,
        KeyCode::Esc if !app.query.is_empty() || !app.tag.is_empty() => {
            app.tag.clear();
            app.set_query(String::new());
            app.status = String::from("filters cleared");
        }
        _ => {}
    }
    Flow::Continue
}

fn edit_selected(app: &mut App) {
    let Some(id) = app.notes.get(app.selected).map(|n| n.id) else {
        return;
    };
    let Some(note) = db::get(&app.conn, id).ok().flatten() else {
        return;
    };
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
                app.status = format!("saved “{title}”");
            } else {
                app.status = String::from("not saved: the first line (title) was empty");
            }
        }
        Err(e) => app.status = format!("edit failed: {e}"),
    }
    app.reload(Some(id));
}

fn new_note(app: &mut App) {
    let mut created = None;
    match edit_in_terminal("", "snip-new") {
        Ok((txt, changed)) if changed && !txt.trim().is_empty() => {
            let (title, body) = split_title_body(txt.trim());
            created = db::create(&app.conn, &title, &body, "").ok();
            if created.is_some() {
                app.status = format!("created “{title}”");
            }
        }
        Ok(_) => app.status = String::from("empty note discarded"),
        Err(e) => app.status = format!("edit failed: {e}"),
    }
    app.reload(created);
}

fn delete_selected(app: &mut App) {
    let Some(note) = app.notes.get(app.selected) else {
        return;
    };
    let title = note.title.clone();
    if db::delete(&app.conn, note.id).is_ok() {
        app.status = format!("deleted “{title}”");
    }
    app.reload(None);
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

fn cycle_tag(app: &mut App) {
    if app.tags.is_empty() && app.tag.is_empty() {
        app.status = String::from("no tags yet: add some with snip add \"…\" --tag name");
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

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Terminal (columns, rows), with a sane fallback when it can't be queried.
fn screen_size() -> (usize, usize) {
    terminal::size().map_or((80, 24), |(w, h)| (w as usize, h as usize))
}

fn draw(app: &mut App, out: &mut std::io::Stdout) {
    let (width, height) = screen_size();
    app.offset = ui::scroll_offset(app.selected, app.offset, ui::list_rows(height));
    let lines = ui::render(&app.view(), width, height);
    let _ = paint(out, &lines, width);
}

/// Write each row in place instead of clearing the whole screen, so redraws
/// don't flicker. Rows are positioned with cursor moves, never newlines,
/// which raw mode would not return to the left edge.
fn paint(out: &mut impl Write, lines: &[ui::Line], width: usize) -> std::io::Result<()> {
    queue!(out, terminal::BeginSynchronizedUpdate)?;
    for (row, line) in lines.iter().enumerate() {
        queue!(out, cursor::MoveTo(0, row as u16))?;
        for s in line {
            queue!(out, PrintStyledContent(s.style.apply(&s.text)))?;
        }
        // a full-width row leaves the cursor past the edge, where a clear
        // would erase the last cell
        if ui::line_width(line) < width {
            queue!(out, terminal::Clear(ClearType::UntilNewLine))?;
        }
    }
    queue!(out, terminal::EndSynchronizedUpdate)?;
    out.flush()
}

#[test]
fn paint_positions_rows_without_newlines() {
    let lines = vec![
        vec![ui::Span { text: "first".into(), style: ui::Style::Plain }],
        vec![ui::Span { text: "second".into(), style: ui::Style::Dim }],
    ];
    let mut out = Vec::new();
    paint(&mut out, &lines, 10).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains('\n'));
    assert!(out.contains("\x1b[1;1Hfirst\x1b[K"));
    assert!(out.contains("\x1b[2;1H"));
    assert!(out.contains("second"));
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
