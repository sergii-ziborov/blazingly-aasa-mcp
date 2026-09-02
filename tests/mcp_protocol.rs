//! Drives the real tool catalog over in-memory streams.
//!
//! These tests use the same `mcp::build` the binary serves, so a change to a schema or a handler
//! is checked here rather than discovered by a client one rejected call at a time. Nothing here
//! touches the network: the two offline tools cover the protocol surface, and the network tools
//! are covered by `fetch`'s own guard tests.

use blazingly_aasa_mcp::{fetch::FetchOptions, mcp};
use serde_json::Value;

/// Feeds `requests` through the server and returns one parsed response per reply.
fn exchange(requests: &[Value]) -> Vec<Value> {
    let mut input = String::new();
    for request in requests {
        input.push_str(&serde_json::to_string(request).unwrap());
        input.push('\n');
    }
    let mut output = Vec::new();
    mcp::build(FetchOptions::default())
        .serve_streams(std::io::Cursor::new(input.into_bytes()), &mut output)
        .expect("in-memory streams should not fail");

    String::from_utf8(output)
        .expect("responses should be UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each response should be JSON"))
        .collect()
}

fn initialize() -> Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}
        }
    })
}

fn call(id: u32, name: &str, arguments: &Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
}

const DOCUMENT: &str = r#"{"applinks":{"details":[{
    "appIDs": ["ABCDE12345.com.example.app"],
    "components": [
        {"/": "/help/website/*", "exclude": true},
        {"/": "/help/*", "?": {"articleNumber": "????"}},
        {"/": "buy/*"}
    ]
}]},"webcredentials":{"apps":["ABCDE12345.com.example.app"]}}"#;

const APP: &str = "ABCDE12345.com.example.app";

#[test]
fn initialize_reports_identity_and_a_supported_protocol() {
    let responses = exchange(&[initialize()]);
    let result = &responses[0]["result"];
    assert_eq!(result["serverInfo"]["name"], "blazingly-aasa");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(result["protocolVersion"].is_string());
    assert!(
        result["instructions"]
            .as_str()
            .unwrap()
            .contains("check_universal_link"),
        "the instructions should point at the tool to start with"
    );
}

#[test]
fn the_catalog_is_small_and_every_schema_is_well_formed() {
    let responses = exchange(&[
        initialize(),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ]);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();

    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec![
            "check_universal_link",
            "fetch_association_file",
            "compare_origin_and_cdn",
            "validate_association_file",
            "explain_match",
        ]
    );

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.len() > 60,
            "{name} needs a description a model can choose from"
        );
        // An agent should not have to guess which tools cost a network round trip.
        let networked = matches!(
            name,
            "check_universal_link" | "fetch_association_file" | "compare_origin_and_cdn"
        );
        assert_eq!(
            description.contains("Reaches the network"),
            networked,
            "{name} must say whether it reaches the network"
        );

        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{name} schema");
        assert_eq!(
            schema["additionalProperties"], false,
            "{name} should reject unknown arguments"
        );
        for required in schema["required"].as_array().unwrap() {
            let key = required.as_str().unwrap();
            assert!(
                schema["properties"][key].is_object(),
                "{name} requires `{key}` but does not describe it"
            );
        }
        for (key, property) in schema["properties"].as_object().unwrap() {
            assert!(
                property["description"]
                    .as_str()
                    .is_some_and(|d| d.len() > 10),
                "{name}.{key} needs a description"
            );
        }
    }
}

#[test]
fn explain_match_returns_a_decision_and_a_trace() {
    let responses = exchange(&[
        initialize(),
        call(
            2,
            "explain_match",
            &serde_json::json!({
                "content": DOCUMENT,
                "domain": "example.com",
                "url": "https://example.com/help/1?articleNumber=481",
                "app_id": APP
            }),
        ),
    ]);
    let result = &responses[1]["result"];
    assert_ne!(result["isError"], true);
    let content = &result["structuredContent"];
    assert_eq!(content["decision"], "no_match");
    let explanation = content["explanation"].as_str().unwrap();
    assert!(explanation.contains("NO_MATCH"));
    assert!(
        explanation.contains("articleNumber"),
        "the trace should name the component that failed:\n{explanation}"
    );
}

#[test]
fn omitting_the_app_id_asks_which_apps_a_url_reaches() {
    let responses = exchange(&[
        initialize(),
        call(
            2,
            "explain_match",
            &serde_json::json!({
                "content": DOCUMENT,
                "domain": "example.com",
                "url": "https://example.com/help/1?articleNumber=4815"
            }),
        ),
    ]);
    let content = &responses[1]["result"]["structuredContent"];
    assert!(content["decision"].is_null());
    let apps = content["apps"].as_array().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0]["app_id"], APP);
    assert_eq!(apps[0]["decision"], "match");
}

#[test]
fn validate_reports_stable_codes() {
    let responses = exchange(&[
        initialize(),
        call(
            2,
            "validate_association_file",
            &serde_json::json!({"content": DOCUMENT}),
        ),
    ]);
    let content = &responses[1]["result"]["structuredContent"];
    assert_eq!(content["has_errors"], false);
    let codes: Vec<&str> = content["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    // The document is clean: `buy/*` is a legal path pattern, since swcutil matches it against
    // `/buy/42`. This asserted AASA191 until the oracle disproved that lint.
    assert!(codes.is_empty(), "expected a silent report, got {codes:?}");
    assert_eq!(content["apps"][0]["app_id"], APP);
}

#[test]
fn a_broken_document_is_a_tool_error_not_a_protocol_error() {
    let responses = exchange(&[
        initialize(),
        call(
            2,
            "validate_association_file",
            &serde_json::json!({"content": "{ not json"}),
        ),
    ]);
    let result = &responses[1]["result"];
    assert_eq!(
        result["isError"], true,
        "a bad document is the caller's problem, reported as a tool error"
    );
    assert!(responses[1]["error"].is_null(), "not a JSON-RPC error");
}

#[test]
fn a_missing_required_argument_is_reported_readably() {
    let responses = exchange(&[
        initialize(),
        call(
            2,
            "explain_match",
            &serde_json::json!({"domain": "example.com"}),
        ),
    ]);
    let response = &responses[1];
    let rendered = serde_json::to_string(response).unwrap();
    assert!(
        rendered.contains("content") || rendered.contains("required"),
        "the reply should name what was missing: {rendered}"
    );
}

#[test]
fn an_unknown_tool_is_refused() {
    let responses = exchange(&[initialize(), call(2, "not_a_tool", &serde_json::json!({}))]);
    let rendered = serde_json::to_string(&responses[1]).unwrap();
    assert!(
        rendered.contains("not_a_tool") || responses[1]["error"].is_object(),
        "unknown tools must be refused: {rendered}"
    );
}
