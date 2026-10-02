//! `snip mcp`: JSON-RPC over stdio, driven the way an MCP client would.
use serde_json::{json, Value};
use snip::{db, mcp};

fn tmp_conn(name: &str) -> rusqlite::Connection {
    let path = std::env::temp_dir().join(format!("snip-mcp-test-{}-{name}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    db::open(&path).unwrap()
}

/// Send each message as one line, return the reply lines.
fn session(conn: &rusqlite::Connection, messages: &[Value]) -> Vec<Value> {
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let mut out = Vec::new();
    mcp::serve(conn, input.as_bytes(), &mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("every output line is one JSON message"))
        .collect()
}

fn call(conn: &rusqlite::Connection, tool: &str, args: Value) -> Value {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                     "params": {"name": tool, "arguments": args}});
    mcp::handle(conn, &msg).unwrap()["result"].clone()
}

#[test]
fn handshake_and_tool_listing() {
    let conn = tmp_conn("handshake");
    let replies = session(&conn, &[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": "p", "method": "ping"}),
    ]);
    assert_eq!(replies.len(), 3, "notifications get no reply");

    let init = &replies[0];
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "snip");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let tools: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap()
        .iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(tools, ["search_notes", "get_note", "list_tags", "create_note", "append_to_note", "update_note"]);
    for t in replies[1]["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(t["description"].as_str().unwrap().len() > 10);
    }
    assert!(!tools.contains(&"delete_note"), "deletion stays a human action");

    assert_eq!(replies[2], json!({"jsonrpc": "2.0", "id": "p", "result": {}}));
}

#[test]
fn unknown_protocol_version_gets_the_latest() {
    let conn = tmp_conn("version");
    let r = mcp::handle(&conn, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "1999-01-01"}})).unwrap();
    assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn protocol_errors() {
    let conn = tmp_conn("errors");
    let mut out = Vec::new();
    mcp::serve(&conn, "not json\n\n[1,2]\n".as_bytes(), &mut out).unwrap();
    let replies: Vec<Value> = String::from_utf8(out).unwrap().lines()
        .map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 2, "blank lines are skipped");
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["error"]["code"], -32600);

    let r = mcp::handle(&conn, &json!({"jsonrpc": "2.0", "id": 3, "method": "nope"})).unwrap();
    assert_eq!(r["error"]["code"], -32601);
    let r = mcp::handle(&conn, &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "delete_note", "arguments": {"id": 1}}})).unwrap();
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn create_search_read_append_update() {
    let conn = tmp_conn("crud");
    let r = call(&conn, "create_note", json!({
        "title": "Meeting notes", "body": "Discuss the roadmap\n", "tags": ["work", "#Q4 work"]}));
    assert_eq!(r["isError"], false);
    let id = r["structuredContent"]["id"].as_i64().unwrap();
    assert_eq!(r["structuredContent"]["tags"], json!(["work", "Q4"]));
    // text content mirrors the structured result for clients that only read text
    let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, r["structuredContent"]);

    let r = call(&conn, "search_notes", json!({"query": "meet road"}));
    assert_eq!(r["structuredContent"]["count"], 1);
    let hit = &r["structuredContent"]["notes"][0];
    assert_eq!(hit["id"], id);
    assert_eq!(hit["snippet"], "Discuss the roadmap");
    assert!(hit["updated"].as_str().unwrap().ends_with('Z'));
    assert_eq!(call(&conn, "search_notes", json!({"tag": "#Q4"}))["structuredContent"]["count"], 1);
    assert_eq!(call(&conn, "search_notes", json!({"tag": "home"}))["structuredContent"]["count"], 0);

    call(&conn, "append_to_note", json!({"id": id, "text": "- ship the MCP server"}));
    let r = call(&conn, "get_note", json!({"id": id}));
    assert_eq!(r["structuredContent"]["body"], "Discuss the roadmap\n\n- ship the MCP server");

    // partial update: title only, body and tags kept
    call(&conn, "update_note", json!({"id": id, "title": "Roadmap meeting"}));
    let note = db::get(&conn, id).unwrap().unwrap();
    assert_eq!(note.title, "Roadmap meeting");
    assert_eq!(note.tags, "work Q4");
    assert!(note.body.ends_with("MCP server"));
    // tags replace
    call(&conn, "update_note", json!({"id": id, "tags": ["done"]}));
    assert_eq!(db::get(&conn, id).unwrap().unwrap().tags, "done");
    assert_eq!(call(&conn, "list_tags", json!({}))["structuredContent"]["tags"], json!(["done"]));

    // FTS stays in sync with MCP writes
    assert_eq!(call(&conn, "search_notes", json!({"query": "mcp"}))["structuredContent"]["count"], 1);
}

#[test]
fn bad_arguments_are_tool_errors_the_model_can_read() {
    let conn = tmp_conn("badargs");
    for (tool, args, needle) in [
        ("create_note", json!({}), "title is required"),
        ("create_note", json!({"title": "   "}), "title is empty"),
        ("create_note", json!({"title": "two\nlines"}), "single line"),
        ("create_note", json!({"title": "x", "tags": "work"}), "array"),
        ("get_note", json!({"id": "1"}), "integer"),
        ("get_note", json!({"id": 999}), "no note with id 999"),
        ("append_to_note", json!({"id": 999, "text": "x"}), "no note"),
        ("search_notes", json!({"limit": 0}), "limit"),
        ("search_notes", json!({"query": 5}), "must be a string"),
    ] {
        let r = call(&conn, tool, args.clone());
        assert_eq!(r["isError"], true, "{tool} {args}");
        let msg = r["content"][0]["text"].as_str().unwrap();
        assert!(msg.contains(needle), "{tool} {args}: {msg}");
    }
    assert!(db::list(&conn, "", "", 10).unwrap().is_empty(), "nothing was written");
}

#[test]
fn iso_time_formats_utc() {
    assert_eq!(mcp::iso_time(0), "1970-01-01T00:00:00Z");
    assert_eq!(mcp::iso_time(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(mcp::iso_time(1_790_959_740), "2026-10-02T16:49:00Z");
    assert_eq!(mcp::iso_time(-1), "1969-12-31T23:59:59Z");
}
