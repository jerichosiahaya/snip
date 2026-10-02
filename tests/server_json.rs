//! server.json (the MCP Registry listing) must match the crate it points to.
use serde_json::Value;

#[test]
fn registry_listing_matches_the_crate() {
    let doc: Value = serde_json::from_str(include_str!("../server.json")).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(doc["version"], version, "bump server.json version with Cargo.toml");

    let pkg = &doc["packages"][0];
    assert_eq!(pkg["registryType"], "cargo");
    assert_eq!(pkg["identifier"], env!("CARGO_PKG_NAME"));
    assert_eq!(pkg["version"], version, "bump packages[0].version with Cargo.toml");
    assert_eq!(pkg["packageArguments"][0]["value"], "mcp");

    // the registry verifies crate ownership by this visible line in the README
    let name = doc["name"].as_str().unwrap();
    assert!(include_str!("../README.md").contains(&format!("mcp-name: {name}")));
    assert!(doc["description"].as_str().unwrap().len() <= 100, "registry limit");
}
