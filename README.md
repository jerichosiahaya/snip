# snip

Lightweight terminal note-taking app. Rust + SQLite (FTS5 full-text search),
with an [MCP server](#use-snip-from-claude-mcp) so Claude can read and write your notes.

- crates.io: [`snip-notes`](https://crates.io/crates/snip-notes)
- mcp-name: io.github.jerichosiahaya/snip
- License: MIT

## Install from the AUR (Arch Linux)

```bash
yay -S snip-notes-bin   # prebuilt binary from the GitHub release
yay -S snip-notes       # or build from source
```

Either installs the `snip` command (use `paru` or `makepkg` if you prefer).
The PKGBUILDs live in [`packaging/aur/`](packaging/aur/). Each release
updates them and pushes them to the AUR automatically (see
`.github/workflows/aur-publish.yml`; it needs the `AUR_SSH_PRIVATE_KEY`
repository secret).

## Install with Cargo (any platform)

Requires Rust/Cargo and a C compiler for the bundled SQLite:

```bash
cargo install snip-notes   # installs the `snip` command into ~/.cargo/bin
```

## Install on Arch Linux (x86-64)

Download the prebuilt executable—**no Rust or Cargo required**:

```bash
curl -fLO https://github.com/jerichosiahaya/snip/releases/download/v0.2.0/snip-arch-linux-x86_64.tar.gz
curl -fLO https://github.com/jerichosiahaya/snip/releases/download/v0.2.0/SHA256SUMS
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

Release executable: **~1.6 MB**, below the 5 MB budget (excluding system libraries and notes).

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

## Use snip from Claude (MCP)

`snip mcp` runs a [Model Context Protocol](https://modelcontextprotocol.io)
server on stdin/stdout, so Claude can save, find and update your notes. It
uses the same database as the interactive app, and both can be open at once.

**Claude Code:**

```bash
claude mcp add --scope user snip -- "$(command -v snip)" mcp
```

**Claude Desktop:** add this to `claude_desktop_config.json` (Settings →
Developer → Edit Config), using the absolute path to `snip` (`command -v snip`
prints it: `~/.cargo/bin/snip` after `cargo install`, `~/.local/bin/snip` for
the prebuilt download), then restart Claude Desktop:

```json
{
  "mcpServers": {
    "snip": { "command": "/home/YOU/.local/bin/snip", "args": ["mcp"] }
  }
}
```

To use a different database, put `--db /path/to.db` before `mcp` in either
command.

Claude gets these tools:

| Tool | Does |
|---|---|
| `search_notes` | find notes by words (prefixes ok) and/or tag; no query lists recent notes |
| `get_note` | read one note in full |
| `list_tags` | all tags in use |
| `create_note` | save a new note (title, body, tags) |
| `append_to_note` | add text to the end of a note |
| `update_note` | change a note's title, body or tags |

There is deliberately no delete tool: deleting notes stays something you do
in the app. `update_note` can still replace a note's body, so keep backups
(below) if Claude edits important notes.

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