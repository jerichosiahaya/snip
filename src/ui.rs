//! Layout for the interactive screen. Pure: state in, styled rows out, so the
//! whole screen can be checked as text without a terminal.
//!
//! ```text
//!  snip  / meet▏                                  #work  3 matches
//! ──────────────────────────────┬──────────────────────────────────
//!  Meeting notes             2h │ Meeting notes
//!  Team meeting agenda       3d │ #work · edited 2h ago
//!                               │
//!                               │ Discuss the new feature…
//! ──────────────────────────────┴──────────────────────────────────
//!  ↑↓ move  ⏎ edit  ^N new  / search  ^Q quit  ^T tag  Del delete
//! ```
use crate::db::Note;
use crossterm::style::{StyledContent, Stylize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Narrower screens show the list alone, without the preview pane.
const TWO_PANE_MIN_WIDTH: usize = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    Dim,
    Bold,
    Accent,
    Key,
    Brand,
    Selected,
    Danger,
}

impl Style {
    pub fn apply(self, text: &str) -> StyledContent<&str> {
        match self {
            Style::Plain => text.stylize(),
            Style::Dim => text.dark_grey(),
            Style::Bold => text.bold(),
            Style::Accent => text.magenta(),
            Style::Key => text.magenta().bold(),
            Style::Brand => text.black().on_magenta().bold(),
            Style::Selected => text.black().on_magenta(),
            Style::Danger => text.white().on_red().bold(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

pub type Line = Vec<Span>;

fn span(text: impl Into<String>, style: Style) -> Span {
    Span { text: text.into(), style }
}

/// The line's text without styling.
pub fn line_text(line: &Line) -> String {
    line.iter().map(|s| s.text.as_str()).collect()
}

/// Columns the line takes up on screen.
pub fn line_width(line: &Line) -> usize {
    line.iter().map(|s| s.text.width()).sum()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Search,
    ConfirmDelete,
}

/// Everything the screen shows.
pub struct View<'a> {
    pub notes: &'a [Note],
    pub selected: usize,
    pub offset: usize,
    pub query: &'a str,
    pub tag: &'a str,
    pub mode: Mode,
    pub status: &'a str,
    /// Seconds since the unix epoch, for "edited 2h ago".
    pub now: i64,
}

impl View<'_> {
    fn filtered(&self) -> bool {
        !self.query.trim().is_empty() || !self.tag.is_empty()
    }
}

/// Rows left for the note list on a screen `height` rows tall
/// (header, two rules and the footer take the rest).
pub fn list_rows(height: usize) -> usize {
    height.saturating_sub(4).max(1)
}

/// Width of the list column, and of the preview pane if the screen has room.
fn columns(width: usize) -> (usize, Option<usize>) {
    if width >= TWO_PANE_MIN_WIDTH {
        let list = (width * 2 / 5).clamp(30, 48);
        (list, Some(width - list - 1))
    } else {
        (width, None)
    }
}

/// The whole screen: at most `height` lines, none wider than `width`.
pub fn render(v: &View, width: usize, height: usize) -> Vec<Line> {
    let rows = list_rows(height);
    let (list_w, preview_w) = columns(width);
    let list = list_lines(v, list_w, rows, preview_w.is_none());
    let preview = preview_w.map(|w| preview_lines(v, w, rows));

    let mut out = Vec::with_capacity(height);
    out.push(header(v, width));
    out.push(rule(list_w, preview_w, '┬', None));
    for (i, mut line) in list.into_iter().enumerate() {
        if let Some(p) = &preview {
            line.push(span("│", Style::Dim));
            line.extend(p[i].iter().cloned());
        }
        out.push(line);
    }
    let position = (v.notes.len() > rows).then(|| format!(" {}/{} ", v.selected + 1, v.notes.len()));
    out.push(rule(list_w, preview_w, '┴', position));
    out.push(footer(v, width));
    out.truncate(height);
    out
}

fn header(v: &View, width: usize) -> Line {
    let n = v.notes.len();
    let noun = match (v.filtered(), n) {
        (true, 1) => "match",
        (true, _) => "matches",
        (false, 1) => "note",
        (false, _) => "notes",
    };
    let mut right = Vec::new();
    if !v.tag.is_empty() {
        right.push(span(format!("#{}", v.tag), Style::Accent));
        right.push(span("  ", Style::Plain));
    }
    right.push(span(format!("{n} {noun} "), Style::Dim));

    // brand + space + "/" + query + cursor + space before the right side
    let fixed = " snip ".len() + 3 + 1;
    let mut room = width.saturating_sub(fixed + line_width(&right));
    if room < 10 {
        right.clear();
        room = width.saturating_sub(fixed);
    }
    let mut left = vec![span(" snip ", Style::Brand), span(" ", Style::Plain)];
    match v.mode {
        Mode::Search => {
            left.push(span("/", Style::Key));
            // keep the end of a long query, where the cursor is, in view
            left.push(span(fit_tail(v.query, room), Style::Plain));
            left.push(span("▏", Style::Accent));
        }
        _ if !v.query.is_empty() => {
            left.push(span("/", Style::Dim));
            left.push(span(fit(v.query, room), Style::Plain));
        }
        _ => left.push(span("/ to search", Style::Dim)),
    }
    spread(left, right, width)
}

/// A horizontal rule, joined to the pane divider; `label` sits near the
/// right end of the list column.
fn rule(list_w: usize, preview_w: Option<usize>, joint: char, label: Option<String>) -> Line {
    let mut cells: Vec<String> = vec!["─".to_string(); list_w];
    if let Some(label) = label {
        let label: Vec<char> = label.chars().collect();
        if list_w >= label.len() + 2 {
            let start = list_w - label.len() - 1;
            for (i, c) in label.into_iter().enumerate() {
                cells[start + i] = c.to_string();
            }
        }
    }
    let mut s = cells.concat();
    if let Some(p) = preview_w {
        s.push(joint);
        s.push_str(&"─".repeat(p));
    }
    vec![span(s, Style::Dim)]
}

fn list_lines(v: &View, w: usize, rows: usize, single_pane: bool) -> Vec<Line> {
    let mut lines = Vec::with_capacity(rows);
    if v.notes.is_empty() {
        let (title, hint) = if v.filtered() {
            ("No matching notes.", "Esc clears the search and tag filter.")
        } else {
            ("No notes yet.", "Press Ctrl-N to write one.")
        };
        let inner = w.saturating_sub(2);
        lines.push(Vec::new());
        lines.push(vec![span("  ", Style::Plain), span(fit(title, inner), Style::Bold)]);
        lines.push(vec![span("  ", Style::Plain), span(fit(hint, inner), Style::Dim)]);
    }
    for (i, note) in v.notes.iter().enumerate().skip(v.offset).take(rows) {
        lines.push(note_row(note, i == v.selected, w, single_pane, v.now));
    }
    lines.resize_with(rows, Vec::new);
    lines.into_iter().map(|l| pad_to(truncate_line(l, w), w, Style::Plain)).collect()
}

/// ` title  #tags            2h `, tags only in single-pane mode.
fn note_row(note: &Note, selected: bool, w: usize, single_pane: bool, now: i64) -> Line {
    let age = relative_time(now, note.updated);
    let text_w = w.saturating_sub(2 + age.width() + 1);
    let tags = if single_pane { hashtags(&note.tags) } else { String::new() };
    let show_tags = !tags.is_empty() && text_w >= tags.width() + 2 + 12;
    let title_w = if show_tags { text_w - tags.width() - 2 } else { text_w };

    let mut line = vec![span(" ", Style::Plain)];
    if note.title.trim().is_empty() {
        line.push(span(fit("(untitled)", title_w), Style::Dim));
    } else {
        line.push(span(fit(&note.title, title_w), Style::Plain));
    }
    if show_tags {
        line.push(span("  ", Style::Plain));
        line.push(span(tags, Style::Dim));
    }
    let gap = w.saturating_sub(line_width(&line) + age.width() + 1);
    line.push(span(" ".repeat(gap), Style::Plain));
    line.push(span(age, Style::Dim));
    line.push(span(" ", Style::Plain));

    let line = pad_to(truncate_line(line, w), w, Style::Plain);
    if selected {
        line.into_iter().map(|s| span(s.text, Style::Selected)).collect()
    } else {
        line
    }
}

fn preview_lines(v: &View, w: usize, rows: usize) -> Vec<Line> {
    let inner = w.saturating_sub(2);
    let mut lines: Vec<Line> = Vec::new();
    if let Some(note) = v.notes.get(v.selected) {
        let title = if note.title.trim().is_empty() { "(untitled)" } else { &note.title };
        for t in wrap(title, inner) {
            lines.push(vec![span(t, Style::Bold)]);
        }
        let mut meta = hashtags(&note.tags);
        if !meta.is_empty() {
            meta.push_str(" · ");
        }
        match relative_time(v.now, note.updated).as_str() {
            "now" => meta.push_str("edited just now"),
            age => meta.push_str(&format!("edited {age} ago")),
        }
        for m in wrap(&meta, inner) {
            lines.push(vec![span(m, Style::Dim)]);
        }
        lines.push(Vec::new());
        if note.body.trim().is_empty() {
            lines.push(vec![span(fit("No body yet — Enter to edit.", inner), Style::Dim)]);
        } else {
            for b in wrap(&note.body, inner) {
                lines.push(vec![span(b, Style::Plain)]);
            }
        }
    }
    if lines.len() > rows {
        lines.truncate(rows);
        lines[rows - 1] = vec![span("…", Style::Dim)];
    }
    lines.resize_with(rows, Vec::new);
    lines
        .into_iter()
        .map(|l| {
            let mut padded = vec![span(" ", Style::Plain)];
            padded.extend(l);
            pad_to(truncate_line(padded, w), w, Style::Plain)
        })
        .collect()
}

fn footer(v: &View, width: usize) -> Line {
    let line = match v.mode {
        Mode::ConfirmDelete => {
            let title = v.notes.get(v.selected).map_or("", |n| n.title.as_str());
            vec![
                span(" delete ", Style::Danger),
                span(" ", Style::Plain),
                span(format!("“{}”?", fit(title, 30)), Style::Bold),
                span("  ", Style::Plain),
                span("y", Style::Key),
                span(" delete  ", Style::Dim),
                span("any key", Style::Key),
                span(" cancel", Style::Dim),
            ]
        }
        _ if !v.status.is_empty() => vec![span(" ", Style::Plain), span(v.status, Style::Plain)],
        Mode::Search => keys(&[("type", "filter"), ("↑↓", "move"), ("⏎", "done"), ("Esc", "clear")]),
        Mode::Browse => keys(&[
            ("↑↓", "move"),
            ("⏎", "edit"),
            ("^N", "new"),
            ("/", "search"),
            ("^Q", "quit"),
            ("^T", "tag"),
            ("Del", "delete"),
        ]),
    };
    // never write the bottom-right cell: some terminals scroll when it is filled
    let width = width.saturating_sub(1);
    if v.mode == Mode::Browse && v.status.is_empty() {
        drop_whole_hints(line, width)
    } else {
        truncate_line(line, width)
    }
}

fn keys(pairs: &[(&str, &str)]) -> Line {
    let mut line = vec![span(" ", Style::Plain)];
    for (key, label) in pairs {
        line.push(span(*key, Style::Key));
        line.push(span(format!(" {label}  "), Style::Dim));
    }
    line
}

/// Drop trailing key hints that don't fit, rather than cutting one in half.
fn drop_whole_hints(mut line: Line, width: usize) -> Line {
    while line_width(&line) > width && line.len() > 1 {
        line.truncate(line.len().saturating_sub(2).max(1));
    }
    line
}

/// `left` and `right` at the two ends of a `width`-wide line; `right` is
/// dropped when both don't fit.
fn spread(left: Line, right: Line, width: usize) -> Line {
    let (lw, rw) = (line_width(&left), line_width(&right));
    if lw + rw > width {
        return truncate_line(left, width);
    }
    let mut line = left;
    line.push(span(" ".repeat(width - lw - rw), Style::Plain));
    line.extend(right);
    line
}

/// Cut a styled line to `width` columns, marking the cut with `…`.
fn truncate_line(line: Line, width: usize) -> Line {
    if line_width(&line) <= width {
        return line;
    }
    let mut out = Vec::new();
    let mut used = 0;
    for s in line {
        let w = s.text.width();
        if used + w < width {
            used += w;
            out.push(s);
        } else {
            // this span crosses the edge (or exactly fills it, with more to come)
            out.push(span(fit_cut(&s.text, width - used), s.style));
            break;
        }
    }
    out
}

/// Like `fit`, but always ends in `…`: used where text follows the cut.
fn fit_cut(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = take_width(&clean(s), width - 1);
    out.push('…');
    out
}

fn pad_to(mut line: Line, width: usize, style: Style) -> Line {
    let w = line_width(&line);
    if w < width {
        line.push(span(" ".repeat(width - w), style));
    }
    line
}

/// Control characters (tabs etc.) become spaces so they can't break the layout.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// The longest prefix of `s` that fits in `width` columns.
fn take_width(s: &str, width: usize) -> String {
    let mut used = 0;
    s.chars()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= width
        })
        .collect()
}

/// Shorten `s` to at most `width` columns, marking cuts with `…`.
pub fn fit(s: &str, width: usize) -> String {
    let s = clean(s);
    if s.width() <= width {
        return s;
    }
    fit_cut(&s, width)
}

/// Like [`fit`], but keeps the end of `s` and marks the cut at the start.
pub fn fit_tail(s: &str, width: usize) -> String {
    let s = clean(s);
    if s.width() <= width {
        return s;
    }
    if width == 0 {
        return String::new();
    }
    let rev: String = s.chars().rev().collect();
    let tail: String = take_width(&rev, width - 1).chars().rev().collect();
    format!("…{tail}")
}

/// Word-wrap `text` to `width` columns; words longer than a line are split.
/// Paragraph indentation is kept.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let para = clean(para.trim_end_matches('\r'));
        let indent = (para.len() - para.trim_start_matches(' ').len()).min(width / 2);
        let mut cur = " ".repeat(indent);
        let mut cur_w = indent;
        for (n, word) in para.split(' ').filter(|w| !w.is_empty()).enumerate() {
            if n > 0 {
                if cur_w + 1 + word.width() <= width {
                    cur.push(' ');
                    cur_w += 1;
                } else {
                    out.push(std::mem::take(&mut cur));
                    cur_w = 0;
                }
            }
            for c in word.chars() {
                let cw = c.width().unwrap_or(0);
                if cur_w + cw > width && cur_w > 0 {
                    out.push(std::mem::take(&mut cur));
                    cur_w = 0;
                }
                cur.push(c);
                cur_w += cw;
            }
        }
        out.push(cur);
    }
    out
}

/// `work home` → `#work #home`
fn hashtags(tags: &str) -> String {
    tags.split_whitespace().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")
}

/// Compact age: `now`, `5m`, `3h`, `2d`, `3w`, `4mo`, `2y`.
pub fn relative_time(now: i64, then: i64) -> String {
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    let s = (now - then).max(0);
    if s < MIN {
        "now".into()
    } else if s < HOUR {
        format!("{}m", s / MIN)
    } else if s < DAY {
        format!("{}h", s / HOUR)
    } else if s < 7 * DAY {
        format!("{}d", s / DAY)
    } else if s < 30 * DAY {
        format!("{}w", s / (7 * DAY))
    } else if s < 365 * DAY {
        format!("{}mo", s / (30 * DAY))
    } else {
        format!("{}y", s / (365 * DAY))
    }
}

/// Scroll so `selected` stays inside a window of `rows` lines starting at `offset`.
pub fn scroll_offset(selected: usize, offset: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    if selected < offset {
        selected
    } else if selected >= offset + rows {
        selected + 1 - rows
    } else {
        offset
    }
}
