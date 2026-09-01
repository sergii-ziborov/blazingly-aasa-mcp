//! The MCP surface.
//!
//! Five tools, two of which do double duty: omitting `app_id` turns "does this app get this URL"
//! into "which apps does this URL reach". A small catalog is easier for a model to use correctly
//! than a large one.
//!
//! Three tools reach the network and two do not, and the descriptions say which, because an agent
//! deciding between them should not have to guess.

use mcport::{json, McpServer, ToolReply, Value};
use serde::Serialize;

use crate::fetch::{FetchOptions, Source};
use crate::tools;

/// Wraps a tool outcome, so a failure reaches the model as a readable message rather than a
/// protocol error.
fn reply<T: Serialize>(outcome: Result<T, tools::Failure>) -> ToolReply {
    match outcome {
        Ok(value) => ToolReply::structured(value),
        Err(failure) => match blazingly_json::to_string(&failure) {
            Ok(rendered) => ToolReply::error(rendered),
            Err(_) => ToolReply::error(failure.error),
        },
    }
}

fn text(arguments: &Value, key: &str) -> Result<String, tools::Failure> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| tools::Failure {
            error: format!("`{key}` is required and must be a string"),
            hosting: None,
        })
}

fn optional(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

const DOMAIN_PROPERTY: &str = "The bare domain the file is served for, such as example.com. \
                               Not a URL, not an IP address.";
const APP_ID_PROPERTY: &str = "Application identifier as <Application Identifier Prefix>.\
                               <Bundle Identifier>, for example ABCDE12345.com.example.app. \
                               Omit to be told every app the URL reaches.";

/// Builds the tool catalog.
///
/// Separate from [`serve`] so tests can drive the same catalog over in-memory streams instead of
/// spawning a process.
#[must_use]
pub fn build(options: FetchOptions) -> McpServer {
    let server = McpServer::new("blazingly-aasa", env!("CARGO_PKG_VERSION")).instructions(
        "Debugs Apple Universal Links by reading a domain's apple-app-site-association file \
             and explaining what it does. Start with check_universal_link: it answers \"why \
             doesn't this link open my app?\" in one call. Use compare_origin_and_cdn when a link \
             works locally but not on a device, since Apple's CDN can be serving an older file. \
             validate_association_file and explain_match take file contents directly and never \
             touch the network. Results describe what the file permits; whether a link actually \
             opens an app also depends on the app being installed and its Associated Domains \
             entitlement, which no file can tell you.",
    );
    with_offline_tools(with_network_tools(server, options), options.max_bytes)
}

/// The three tools that make an HTTPS request.
fn with_network_tools(server: McpServer, options: FetchOptions) -> McpServer {
    server
        .tool(
            "check_universal_link",
            "Fetches a domain's association file and explains whether it lets an app open a URL. \
             Reaches the network. Returns the decision, a component-by-component trace of why, \
             the file's diagnostics, and every app the URL reaches.",
            json!({
                "type": "object",
                "properties": {
                    "domain": {"type": "string", "description": DOMAIN_PROPERTY},
                    "url": {
                        "type": "string",
                        "description": "The full https URL under test, whose host must be the domain."
                    },
                    "app_id": {"type": "string", "description": APP_ID_PROPERTY}
                },
                "required": ["domain", "url"],
                "additionalProperties": false
            }),
            move |arguments: Value| {
                reply((|| {
                    let domain = text(&arguments, "domain")?;
                    let url = text(&arguments, "url")?;
                    let app_id = optional(&arguments, "app_id");
                    tools::check_universal_link(&domain, app_id.as_deref(), &url, options)
                })())
            },
        )
        .tool(
            "fetch_association_file",
            "Fetches and validates a domain's association file. Reaches the network. Reports HTTP \
             status, Content-Type, size, whether the server redirected (Apple forbids it), every \
             app named in the file, and all diagnostics.",
            json!({
                "type": "object",
                "properties": {
                    "domain": {"type": "string", "description": DOMAIN_PROPERTY},
                    "source": {
                        "type": "string",
                        "enum": ["origin", "apple_cdn"],
                        "description": "origin reads the site itself; apple_cdn reads what Apple's \
                                        CDN is currently handing to devices. Defaults to origin."
                    }
                },
                "required": ["domain"],
                "additionalProperties": false
            }),
            move |arguments: Value| {
                reply((|| {
                    let domain = text(&arguments, "domain")?;
                    let source = match optional(&arguments, "source").as_deref() {
                        Some("apple_cdn") => Source::AppleCdn,
                        _ => Source::WellKnown,
                    };
                    tools::fetch_association_file(&domain, source, options)
                })())
            },
        )
        .tool(
            "compare_origin_and_cdn",
            "Compares the association file a site serves against the one Apple's CDN is handing \
             to devices. Reaches the network. Compares behaviour rather than bytes, so \
             reformatting reports no change while a stale CDN copy does. Use this when a link \
             works in testing but not on a real device.",
            json!({
                "type": "object",
                "properties": {
                    "domain": {"type": "string", "description": DOMAIN_PROPERTY}
                },
                "required": ["domain"],
                "additionalProperties": false
            }),
            move |arguments: Value| {
                reply((|| {
                    let domain = text(&arguments, "domain")?;
                    tools::compare_origin_and_cdn(&domain, options)
                })())
            },
        )
}

/// The two tools that work on contents the caller already holds.
fn with_offline_tools(server: McpServer, limit: usize) -> McpServer {
    server
        .tool(
            "validate_association_file",
            "Validates association file contents you already have. No network. Returns stable \
             AASA### diagnostic codes covering unreachable rules, catch-alls that open a whole \
             domain by accident, path patterns that can never match, and broken substitution \
             variables.",
            json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The complete apple-app-site-association file as text."
                    }
                },
                "required": ["content"],
                "additionalProperties": false
            }),
            move |arguments: Value| {
                reply((|| {
                    let content = text(&arguments, "content")?;
                    tools::validate_association_file(&content, limit)
                })())
            },
        )
        .tool(
            "explain_match",
            "Matches a URL against association file contents you already have. No network. \
             Returns the decision and a component-by-component trace naming the rule that decided \
             and the exact component that failed.",
            json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The complete apple-app-site-association file as text."
                    },
                    "domain": {"type": "string", "description": DOMAIN_PROPERTY},
                    "url": {"type": "string", "description": "The full https URL under test."},
                    "app_id": {"type": "string", "description": APP_ID_PROPERTY}
                },
                "required": ["content", "domain", "url"],
                "additionalProperties": false
            }),
            move |arguments: Value| {
                reply((|| {
                    let content = text(&arguments, "content")?;
                    let domain = text(&arguments, "domain")?;
                    let url = text(&arguments, "url")?;
                    let app_id = optional(&arguments, "app_id");
                    tools::explain_match(&content, &domain, app_id.as_deref(), &url, limit)
                })())
            },
        )
}

/// Serves the tool catalog over stdio until EOF.
///
/// # Errors
///
/// Returns only stdio failures. A bad request is answered, not fatal.
pub fn serve(options: FetchOptions) -> std::io::Result<()> {
    build(options).serve()
}
