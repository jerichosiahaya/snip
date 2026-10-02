//! Screen layout, checked as plain text.
use snip::db::Note;
use snip::ui::{self, line_text, line_width, Mode, Style, View};
use unicode_width::UnicodeWidthStr;

const NOW: i64 = 1_000_000;

fn note(id: i64, title: &str, body: &str, tags: &str, age: i64) -> Note {
    Note { id, title: title.into(), body: body.into(), tags: tags.into(), created: 0, updated: NOW - age }
}

fn sample() -> Vec<Note> {
    vec![
        note(1, "Meeting notes", "Discuss the new feature\n\n  - ship it", "work", 2 * 3600),
        note(2, "Groceries", "milk\teggs", "home", 3 * 86400),
        note(3, "会議のメモ 長いタイトルです 長いタイトルです", "本文", "", 30),
        note(4, "", "", "", 400 * 86400),
    ]
}

fn view<'a>(notes: &'a [Note]) -> View<'a> {
    View { notes, selected: 0, offset: 0, query: "", tag: "", mode: Mode::Browse, status: "", now: NOW }
}

fn screen(v: &View, w: usize, h: usize) -> Vec<String> {
    ui::render(v, w, h).iter().map(line_text).collect()
}

#[test]
fn every_size_fills_the_screen_without_overflowing() {
    let notes = sample();
    for (w, h) in [(20, 5), (40, 8), (60, 12), (71, 12), (72, 12), (100, 30), (200, 50), (3, 3), (0, 0)] {
        for mode in [Mode::Browse, Mode::Search, Mode::ConfirmDelete] {
            let mut v = view(&notes);
            v.mode = mode;
            v.query = "a very long query that goes on and on and on";
            let lines = ui::render(&v, w, h);
            assert!(lines.len() <= h, "{w}x{h}: {} lines", lines.len());
            if h >= 5 {
                assert_eq!(lines.len(), h, "{w}x{h}");
            }
            for (i, l) in lines.iter().enumerate() {
                assert!(line_width(l) <= w, "{w}x{h} {mode:?} row {i} too wide: {:?}", line_text(l));
            }
            // footer never fills the bottom-right cell
            if let Some(last) = lines.last().filter(|_| h >= 5) {
                assert!(line_width(last) < w.max(1), "{w}x{h} footer fills last cell");
            }
        }
    }
}

#[test]
fn wide_screen_shows_a_preview_pane() {
    let notes = sample();
    let s = screen(&view(&notes), 80, 12);
    assert!(s[0].starts_with(" snip  / to search"));
    assert!(s[0].ends_with("4 notes "));
    assert!(s[1].contains('┬') && s[10].contains('┴'));
    // the divider lines up on every body row, in screen columns (wide chars count 2)
    let column = |row: &str, c: char| row.find(c).map(|i| row[..i].width());
    let col = column(&s[1], '┬').unwrap();
    for row in &s[2..10] {
        assert_eq!(column(row, '│'), Some(col), "{row:?}");
    }
    let preview: Vec<String> = s[2..10].iter().map(|r| r[r.find('│').unwrap() + '│'.len_utf8()..].trim_end().to_string()).collect();
    assert_eq!(preview[0], " Meeting notes");
    assert_eq!(preview[1], " #work · edited 2h ago");
    assert_eq!(preview[2], "");
    assert_eq!(preview[3], " Discuss the new feature");
    assert_eq!(preview[5], "   - ship it"); // indentation kept
    // list rows carry ages, right-aligned
    assert!(s[2].starts_with(" Meeting notes"));
    assert!(s[2][..s[2].find('│').unwrap()].trim_end().ends_with("2h"));
    assert!(s[5].contains("(untitled)") && s[5].contains("1y"));
}

#[test]
fn narrow_screen_is_list_only_with_tags() {
    let notes = sample();
    let s = screen(&view(&notes), 60, 12);
    assert!(!s.iter().any(|r| r.contains('│')));
    assert!(s[2].contains("Meeting notes  #work") && s[2].trim_end().ends_with("2h"));
    assert!(s[3].contains("#home") && s[3].trim_end().ends_with("3d"));
    assert!(s[4].trim_end().ends_with("now"));
}

#[test]
fn selected_row_is_highlighted_across_the_list() {
    let notes = sample();
    let mut v = view(&notes);
    v.selected = 1;
    let lines = ui::render(&v, 60, 12);
    assert!(lines[3].iter().all(|s| s.style == Style::Selected));
    assert_eq!(line_width(&lines[3]), 60);
    assert!(lines[2].iter().all(|s| s.style != Style::Selected));
}

#[test]
fn long_lists_scroll_and_show_position() {
    let notes: Vec<Note> = (0..30).map(|i| note(i, &format!("Note {i}"), "", "", 60)).collect();
    let mut v = view(&notes);
    v.selected = 20;
    v.offset = ui::scroll_offset(20, 0, ui::list_rows(12));
    let s = screen(&v, 60, 12);
    assert!(s[2..10].iter().any(|r| r.starts_with(" Note 20 ")));
    assert!(!s.iter().any(|r| r.starts_with(" Note 0 ")));
    assert!(s[10].contains(" 21/30 "));
    assert!(s[0].ends_with("30 notes "));
}

#[test]
fn empty_states_explain_what_to_do() {
    let s = screen(&view(&[]), 60, 12);
    assert!(s[3].contains("No notes yet.") && s[4].contains("Ctrl-N"));

    let mut v = view(&[]);
    v.query = "zzz";
    let s = screen(&v, 60, 12);
    assert!(s[3].contains("No matching notes.") && s[4].contains("Esc"));
    assert!(s[0].ends_with("0 matches "));
}

#[test]
fn search_mode_shows_query_cursor_and_hints() {
    let notes = sample();
    let mut v = view(&notes[..1]);
    v.mode = Mode::Search;
    v.query = "meet";
    v.tag = "work";
    let s = screen(&v, 60, 12);
    assert!(s[0].starts_with(" snip  /meet▏"));
    assert!(s[0].ends_with("#work  1 match "));
    assert!(s[11].contains("type filter") && s[11].contains("Esc clear"));

    // a long query keeps its end (where the cursor is) visible
    v.query = "the quick brown fox jumps over the lazy dog again and again";
    let s = screen(&v, 40, 12);
    assert!(s[0].starts_with(" snip  /…") && s[0].contains("and again▏"), "{:?}", s[0]);
}

#[test]
fn footer_hints_drop_whole_entries_and_status_wins() {
    let notes = sample();
    let s = screen(&view(&notes), 40, 12);
    let footer = s[11].trim_end();
    assert!(footer.starts_with(" ↑↓ move  ⏎ edit"));
    assert!(footer.ends_with("search") || footer.ends_with("new") || footer.ends_with("quit"), "{footer:?}");

    let mut v = view(&notes);
    v.status = "saved “Meeting notes”";
    assert_eq!(screen(&v, 60, 12)[11], " saved “Meeting notes”");
}

#[test]
fn delete_asks_for_confirmation_in_the_footer() {
    let notes = sample();
    let mut v = view(&notes);
    v.mode = Mode::ConfirmDelete;
    let lines = ui::render(&v, 80, 12);
    assert!(line_text(&lines[11]).starts_with(" delete  “Meeting notes”?  y delete  any key cancel"));
    assert_eq!(lines[11][0].style, Style::Danger);
}

#[test]
fn text_helpers_respect_display_width() {
    assert_eq!(ui::fit("hello world", 6), "hello…");
    assert_eq!(ui::fit("a\tb", 10), "a b");
    assert_eq!(ui::fit("会議のメモ", 6), "会議…"); // 2 columns per character
    assert_eq!(ui::fit("abc", 0), "");
    assert_eq!(ui::fit_tail("long query_", 6), "…uery_");
    assert_eq!(ui::fit_tail("会議のメモ", 5), "…メモ");

    assert_eq!(ui::wrap("the quick brown fox", 9), ["the quick", "brown fox"]);
    assert_eq!(ui::wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(ui::wrap("a\n\nb", 10), ["a", "", "b"]);
    assert_eq!(ui::wrap("  - item one two", 10), ["  - item", "one two"]);
    assert_eq!(ui::wrap("会議のメモです", 6), ["会議の", "メモで", "す"]);

    assert_eq!(ui::relative_time(NOW, NOW - 5), "now");
    assert_eq!(ui::relative_time(NOW, NOW - 300), "5m");
    assert_eq!(ui::relative_time(NOW, NOW - 3 * 3600), "3h");
    assert_eq!(ui::relative_time(NOW, NOW - 2 * 86400), "2d");
    assert_eq!(ui::relative_time(NOW, NOW - 15 * 86400), "2w");
    assert_eq!(ui::relative_time(NOW, NOW - 100 * 86400), "3mo");
    assert_eq!(ui::relative_time(NOW, NOW - 800 * 86400), "2y");
    assert_eq!(ui::relative_time(NOW, NOW + 50), "now"); // clock skew

    assert_eq!(ui::scroll_offset(5, 0, 5), 1);
    assert_eq!(ui::scroll_offset(2, 3, 5), 2);
    assert_eq!(ui::scroll_offset(7, 3, 5), 3);
    assert_eq!(ui::scroll_offset(3, 0, 0), 3);
}
