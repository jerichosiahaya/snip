# Changelog

## [Unreleased]

### New

- `snip export [DIR] [--tag NAME]` writes every note as a Markdown file with
  YAML front matter (title, tags, created, updated), readable by Obsidian and
  other Markdown tools. Files are named after the note's title and carry its
  last-edit time.

## [0.2.0] - 2026-10-02

### New

- **Use snip from Claude.** `snip mcp` runs an MCP server, so Claude Code,
  Claude Desktop or any MCP client can search, read, create, append to and
  update your notes. There is no delete tool: deleting stays in the app.
  See the README for setup.
- **Redesigned screen.** Search field always visible in the header, a
  preview pane for the selected note on wide terminals, relative edit ages,
  live search as you type, and key hints, messages and the delete
  confirmation in the footer. Redraws no longer flicker.
- PgUp/PgDn/Home/End navigation; Esc clears the search and tag filter.
- Also published on crates.io as `snip-notes` (`cargo install snip-notes`).
- MIT license.

### Fixed

- Editing a note no longer adds a blank line to the start of its body
  each time.
- Saving a note without changes no longer moves it to the top of the list.
- Search matches word prefixes in any order ("meet" finds "Meeting notes").
- Ctrl-T now cycles back to showing all tags.
- The list scrolls instead of running off the screen; long lines no longer
  wrap and break the layout. Chinese, Japanese and other wide characters
  line up.
- The editor now gets a normal terminal while it runs.
- Temp files for editing get unique names with private permissions and are
  removed afterwards, even when the editor fails.
- `$EDITOR` values with arguments, like `code -w`, now work.

## [0.1.0] - 2026-10-01

- Initial release for Arch Linux x86-64: terminal note capture, full-text
  search, tag filtering, and external-editor integration.
