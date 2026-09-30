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
        query: String::new(),
        tag: String::new(),
        tags: Vec::new(),
        status: String::from("↑↓ nav · Enter edit · C-n new · C-t tag · / search · q quit"),
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
    // first line = title, rest = body
    let mut lines = trimmed.lines();
    let title = lines.next().unwrap_or("").to_string();
    let body = lines.collect::<Vec<_>>().join("\n");
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
                            match editor::edit(&seed, "snip-edit.md") {
                                Ok((txt, _changed)) => {
                                    let (title, body) = split_title_body(txt.trim());
                                    if !title.is_empty() {
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
    match editor::edit(seed, "snip-new.md") {
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
    app.refresh_list();
}

fn cycle_tag(app: &mut App) {
    if app.tags.is_empty() {
        app.status = String::from("no tags yet");
        return;
    }
    app.tag = match app.tags.iter().position(|t| t == &app.tag) {
        Some(i) => app.tags[(i + 1) % app.tags.len()].clone(),
        None => app.tags[0].clone(),
    };
    app.refresh_list();
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

fn draw(app: &App, out: &mut std::io::Stdout) {

    let mut frame = String::new();

    // header line: query + tag
    frame.push_str(&format!(
        "{}{}\n",
        "SNIP".black().on_magenta(),
        format!(" search: `{}`  tag=({})", app.query,
            if app.tag.is_empty() { "all" } else { &app.tag }),
    ));

    // results: list + selected line shows preview
    for (i, note) in app.notes.iter().enumerate() {
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
        if selected {
            frame.push_str(&format!("{}\n", render.bold().yellow()));
        } else {
            frame.push_str(&format!("{render}\n"));
        }
    }

    // help + status lines
    frame.push_str(&format!(
        "\n{} notes · ↑↓ move · Enter edit · C-n new · C-t tag · C-q quit\n",
        app.notes.len()
    ));
    frame.push_str(&format!("\n{}", app.status));

    let _ = write_frame(out, &frame);
}

fn draw_prompt(app: &App, prefix: &str, query: &str) {
    let mut out = stdout();

    let mut frame = String::new();
    frame.push_str(&format!(
        "{}{}\n",
        "SNIP".black().on_magenta(),
        format!(" {prefix}{query}_"),
    ));
    frame.push_str("(type to search · Enter apply · Esc cancel)\n\n");
    if let Ok(notes) = db::list(&app.conn, &app.query, &app.tag, 20) {
        for n in notes {
            frame.push_str(&format!("{}\n", n.title));
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
