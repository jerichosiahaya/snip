# snip

Lightweight terminal note-taking app. Rust + SQLite (FTS5 full-text search).

## Install on Arch Linux (x86-64)

Download the prebuilt executable—**no Rust or Cargo required**:

```bash
curl -fLO https://github.com/jerichosiahaya/snip/releases/download/v0.1.0/snip-arch-linux-x86_64.tar.gz
curl -fLO https://github.com/jerichosiahaya/snip/releases/download/v0.1.0/SHA256SUMS
sha256sum -c SHA256SUMS && \
  tar -xzf snip-arch-linux-x86_64.tar.gz && \
  install -Dm755 snip "$HOME/.local/bin/snip"
```

Run `~/.local/bin/snip`, or `snip` if `~/.local/bin` is on your PATH.
Requires an up-to-date Arch Linux x86-64 system (`glibc`, `gcc-libs`) and
`$EDITOR` or `nano` for editing. SQLite is bundled. This build is not for
ARM or guaranteed compatible with older Linux distributions.

### Build from source

Requires Rust/Cargo and a C compiler for bundled SQLite:

```bash
cargo build --release --locked
install -Dm755 target/release/snip "$HOME/.local/bin/snip"
```

Release executable: **~1.5 MB**, below the 5 MB budget (excluding system libraries and notes).

Early release: Markdown export and trash/restore are not implemented; deletion
is permanent after confirmation. Keep backups of important notes.

## Usage

```bash
snip                     # open the interactive TUI
snip add "note text" --tag work    # quick capture, first line = title
snip search "meeting"    # print matching notes
snip search "" --tag work
```

Notes are stored in `$XDG_DATA_HOME/snip.db` (normally `~/.local/share/snip.db`) by default. Override with `snip --db /path/to.db ...`.

## Interactive screen

```
 snip  / to search                                                 17 notes
──────────────────────────────┬────────────────────────────────────────────
 Meeting notes             2h │ Meeting notes
 Groceries                 3d │ #work · edited 2h ago
 Book list                 1w │
                              │ Discuss the new feature with the team…
──────────────────────────────┴────────────────────────────────────────────
 ↑↓ move  ⏎ edit  ^N new  / search  ^Q quit  ^T tag  Del delete
```

The list shows the most recently edited notes first. Terminals 72 columns
or wider also show a preview of the selected note; narrower ones show its
tags in the list instead.

```
↑↓ PgUp PgDn Home End   move through notes
Enter      edit selected note (opens $EDITOR, falls back to nano)
Ctrl-N     new note
/          search as you type (title + body; every word must match,
           prefixes ok: "meet" finds "meeting"); Enter keeps it, Esc clears it
Ctrl-T     cycle tag filter (all → each tag → all)
Esc        clear search and tag filter
Delete     delete selected note (asks y/N in the footer)
Ctrl-Q     quit
```

Note format: first line = title, the rest = body. Works in your normal editor;
`$EDITOR` may include arguments, e.g. `EDITOR="code -w"`.

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