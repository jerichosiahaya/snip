# snip

Lightweight terminal note-taking app. Rust + SQLite (FTS5 full-text search).

## Install

```bash
cargo build --release
cp target/release/snip ~/.local/bin/   # or anywhere on your PATH
```

Release binary: **~1.5 MB**.

## Usage

```bash
snip                     # open the interactive TUI
snip add "note text" --tag work    # quick capture, first line = title
snip search "meeting"    # print matching notes
snip search "" --tag work
```

Notes are stored in `$XDG_DATA_HOME/snip.db` (normally `~/.local/share/snip.db`) by default. Override with `snip --db /path/to.db ...`.

## Interactive keys

```
↑↓         move through results
Enter      edit selected note (opens $EDITOR, falls back to nano)
Ctrl-N     new note
Ctrl-T     cycle tag filter
/          search (title + body)
Delete     delete selected note (asks y/N)
Ctrl-Q     quit
```

Note format: first line = title, the rest = body. Works in your normal editor.

## Data & backups

- Source of truth: the SQLite DB (WAL mode). Use SQLite's `.backup` command for a consistent backup, including while the app is open:
  ```bash
  sqlite3 "${XDG_DATA_HOME:-$HOME/.local/share}/snip.db" ".backup 'snip-backup.db'"
  ```
- Markdown export is not yet implemented.

## Tests

```bash
cargo test   # regression: FTS stays consistent across create/update/delete
```