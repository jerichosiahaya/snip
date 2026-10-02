//! `snip mcp`: a Model Context Protocol server on stdin/stdout, so Claude
//! (Claude Code, Claude Desktop, any MCP client) can read and write notes.
//!
//! Transport is MCP stdio: one JSON-RPC 2.0 message per line. Only protocol
//! messages go to stdout; diagnostics go to stderr.
//!
//! Notes can be created, read, searched and changed, but not deleted: deletion
//! stays a human action in the interactive app.
use crate::db::{self, Note};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Write};

/// Newest first; the first one is offered when a client asks for something else.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const SNIPPET_CHARS: usize = 200;

const INSTRUCTIONS: &str = "snip is the user's personal note store. A note has a one-line \
title, a free-form body (Markdown is fine) and tags (single words). Search before \
creating to avoid duplicates, and prefer append_to_note for adding to an existing note. \
Notes cannot be deleted from here.";

/// Serve requests from `input` until it closes.
pub fn serve(conn: &Connection, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(conn, &msg),
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// Answer one JSON-RPC message. Notifications (no `id`) get no reply.
pub fn handle(conn: &Connection, msg: &Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(rpc_error(Value::Null, -32600, "expected a JSON-RPC object"));
    };
    let id = obj.get("id").cloned();
    let method = obj.get("method").and_then(Value::as_str);
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    let (Some(id), Some(method)) = (id, method) else {
        // a notification (initialized, cancelled, …) or a response to us: nothing to say
        return None;
    };
    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(conn, &params),
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => rpc_error(id, code, &message),
    })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "snip", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tool_definitions() -> Value {
    let tags = json!({
        "type": "array",
        "items": { "type": "string" },
        "description": "Single-word tags, e.g. [\"work\", \"ideas\"]. A leading # is ignored."
    });
    let id = json!({ "type": "integer", "description": "Note id, from search_notes or create_note." });
    json!([
        {
            "name": "search_notes",
            "title": "Search notes",
            "description": "Find notes by words in the title or body (every word must match; \
                word prefixes match, so \"meet\" finds \"meeting\") and/or by tag. With no query \
                and no tag, lists the most recently edited notes. Returns ids, titles, tags, \
                last edit time and a short snippet of each body.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for. Omit to list by recency." },
                    "tag": { "type": "string", "description": "Only notes with this tag." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "get_note",
            "title": "Read a note",
            "description": "Read one note in full: title, body, tags, created and last edited times.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": id },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "list_tags",
            "title": "List tags",
            "description": "All tags in use, alphabetically.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "create_note",
            "title": "Create a note",
            "description": "Save a new note. Returns its id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "One line, shown in the note list." },
                    "body": { "type": "string", "description": "The note's content. Markdown is fine." },
                    "tags": tags
                },
                "required": ["title"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }
        },
        {
            "name": "append_to_note",
            "title": "Append to a note",
            "description": "Add text to the end of a note's body, after a blank line. \
                Use this to add to a note without rewriting it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id,
                    "text": { "type": "string", "description": "Text to add." }
                },
                "required": ["id", "text"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }
        },
        {
            "name": "update_note",
            "title": "Update a note",
            "description": "Replace a note's title, body and/or tags. Fields you leave out \
                keep their current value; tags, when given, replace all existing tags. \
                Read the note with get_note first so no content is lost.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id,
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "tags": tags
                },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }
        }
    ])
}

/// A tool failure the model should see and can recover from.
struct ToolError(String);

impl From<rusqlite::Error> for ToolError {
    fn from(e: rusqlite::Error) -> Self {
        ToolError(format!("database error: {e}"))
    }
}

fn call_tool(conn: &Connection, params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let empty = Map::new();
    let args = params.get("arguments").and_then(Value::as_object).unwrap_or(&empty);
    let outcome = match name {
        "search_notes" => search_notes(conn, args),
        "get_note" => get_note(conn, args),
        "list_tags" => Ok(json!({ "tags": db::distinct_tags(conn) })),
        "create_note" => create_note(conn, args),
        "append_to_note" => append_to_note(conn, args),
        "update_note" => update_note(conn, args),
        _ => return Err((-32602, format!("unknown tool: {name}"))),
    };
    Ok(match outcome {
        Ok(value) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
            "structuredContent": value,
            "isError": false,
        }),
        Err(ToolError(message)) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true,
        }),
    })
}

fn search_notes(conn: &Connection, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let query = opt_str(args, "query")?.unwrap_or_default();
    let tag = opt_str(args, "tag")?.map(|t| t.trim().trim_start_matches('#').to_string()).unwrap_or_default();
    let limit = match args.get("limit") {
        None | Some(Value::Null) => 20,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=100).contains(n))
            .ok_or_else(|| ToolError("limit must be an integer from 1 to 100".into()))? as usize,
    };
    let notes = db::list(conn, &query, &tag, limit)?;
    let notes: Vec<Value> = notes.iter().map(summary_json).collect();
    Ok(json!({ "count": notes.len(), "notes": notes }))
}

fn get_note(conn: &Connection, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let note = fetch(conn, args)?;
    Ok(full_json(&note))
}

fn create_note(conn: &Connection, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let title = title_arg(args)?.ok_or_else(|| ToolError("title is required".into()))?;
    let body = opt_str(args, "body")?.unwrap_or_default();
    let tags = tags_arg(args)?.unwrap_or_default();
    let id = db::create(conn, &title, body.trim_end(), &tags)?;
    Ok(json!({ "id": id, "title": title, "tags": tag_list(&tags), "created": true }))
}

fn append_to_note(conn: &Connection, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let note = fetch(conn, args)?;
    let text = opt_str(args, "text")?.ok_or_else(|| ToolError("text is required".into()))?;
    let text = text.trim();
    if text.is_empty() {
        return Err(ToolError("text is empty".into()));
    }
    let body = if note.body.trim().is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{text}", note.body.trim_end())
    };
    db::update(conn, note.id, &note.title, &body, &note.tags)?;
    Ok(json!({ "id": note.id, "title": note.title, "appended": true }))
}

fn update_note(conn: &Connection, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let note = fetch(conn, args)?;
    let title = title_arg(args)?.unwrap_or(note.title);
    let body = opt_str(args, "body")?.map(|b| b.trim_end().to_string()).unwrap_or(note.body);
    let tags = tags_arg(args)?.unwrap_or(note.tags);
    db::update(conn, note.id, &title, &body, &tags)?;
    Ok(json!({ "id": note.id, "title": title, "tags": tag_list(&tags), "updated": true }))
}

fn fetch(conn: &Connection, args: &Map<String, Value>) -> Result<Note, ToolError> {
    let id = args
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| ToolError("id is required and must be an integer".into()))?;
    db::get(conn, id)?.ok_or_else(|| ToolError(format!("no note with id {id}; use search_notes to find ids")))
}

fn opt_str(args: &Map<String, Value>, key: &str) -> Result<Option<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ToolError(format!("{key} must be a string"))),
    }
}

/// Titles are one line: the note list and the editor format both rely on it.
fn title_arg(args: &Map<String, Value>) -> Result<Option<String>, ToolError> {
    let Some(title) = opt_str(args, "title")? else {
        return Ok(None);
    };
    let title = title.trim();
    if title.is_empty() {
        return Err(ToolError("title is empty".into()));
    }
    if title.contains(['\n', '\r']) {
        return Err(ToolError("title must be a single line; put the rest in body".into()));
    }
    Ok(Some(title.to_string()))
}

/// Tags as stored: single words, space-separated, `#` stripped, no repeats.
fn tags_arg(args: &Map<String, Value>) -> Result<Option<String>, ToolError> {
    let Some(value) = args.get("tags").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let items = value
        .as_array()
        .ok_or_else(|| ToolError("tags must be an array of strings".into()))?;
    let mut tags: Vec<String> = Vec::new();
    for item in items {
        let s = item
            .as_str()
            .ok_or_else(|| ToolError("tags must be an array of strings".into()))?;
        for word in s.split_whitespace() {
            let word = word.trim_start_matches('#');
            if !word.is_empty() && !tags.iter().any(|t| t == word) {
                tags.push(word.to_string());
            }
        }
    }
    Ok(Some(tags.join(" ")))
}

fn tag_list(tags: &str) -> Vec<&str> {
    tags.split_whitespace().collect()
}

fn summary_json(note: &Note) -> Value {
    let flat: String = note.body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut snippet: String = flat.chars().take(SNIPPET_CHARS).collect();
    if flat.chars().count() > SNIPPET_CHARS {
        snippet.push('…');
    }
    json!({
        "id": note.id,
        "title": note.title,
        "tags": tag_list(&note.tags),
        "updated": iso_time(note.updated),
        "snippet": snippet,
    })
}

fn full_json(note: &Note) -> Value {
    json!({
        "id": note.id,
        "title": note.title,
        "body": note.body,
        "tags": tag_list(&note.tags),
        "created": iso_time(note.created),
        "updated": iso_time(note.updated),
    })
}

/// Unix seconds → `2026-10-02T16:49:00Z`.
pub fn iso_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // days since 1970-01-01 → civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
