//! Apple Universal Links diagnostics, as an MCP server and as a command line.
//!
//! A thin shell over `blazingly-aasa`: this crate owns the network, the protocol, and the
//! rendering, and nothing else. All association-file semantics live in the library, so the same
//! answers are available without dragging JSON-RPC or an HTTP client along.
//!
//! The two front ends are not two implementations. Commands route through the same functions the
//! tools call, so a `blazingly-aasa check` answer and a `check_universal_link` tool result come
//! from one code path.
//!
//! Run with no arguments to serve MCP over stdio. On that transport stdout carries JSON-RPC frames
//! and nothing else, so every diagnostic goes to stderr.

use blazingly_aasa_mcp::{fetch, mcp, render, tools};

use std::io::Read;
use std::process::ExitCode;
use std::time::Duration;

use fetch::{FetchOptions, Source};

const USAGE: &str = "\
blazingly-aasa - Apple Universal Links diagnostics

  blazingly-aasa                     serve MCP over stdio (default)
  blazingly-aasa check <domain> <url> [--app <id>]
                                     why doesn't this link open the app?
  blazingly-aasa fetch <domain> [--cdn]
                                     fetch and validate the association file
  blazingly-aasa compare <domain>    origin against what Apple's CDN serves
  blazingly-aasa validate <file|->   validate a file you already have
  blazingly-aasa explain <file|-> <domain> <url> [--app <id>]
                                     match a URL against a file you already have

Options
  --app <id>       application identifier; omit to be told every app a URL reaches
  --cdn            read Apple's CDN copy instead of the site's own file
  --json           emit the structured result instead of formatted text
  --timeout <s>    request timeout in seconds (default 10)
  --max-bytes <n>  largest accepted file (default 131072)
  -h, --help       this text
  -V, --version    version

Results describe what the association file permits. Whether a link actually opens an app also
depends on the app being installed and on its Associated Domains entitlement, which no file can
tell you.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(code) => code,
        Err(message) => {
            // `report` already renders its own `error:` line; plain messages need one.
            if message.starts_with("error: ") {
                eprint!("{message}");
            } else {
                eprintln!("error: {message}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Reports a tool failure with its transport facts intact.
///
/// `Failure` carries the hosting details for exactly the cases where they explain the error -- a
/// redirect, a 404, an unexpected content type -- so collapsing it to a string here would throw
/// away the useful half.
fn report(failure: &tools::Failure, json: bool) -> String {
    if json {
        blazingly_json::to_string_pretty(failure).unwrap_or_else(|_| failure.error.clone())
    } else {
        render::failure(failure)
    }
}

struct Flags {
    app: Option<String>,
    cdn: bool,
    json: bool,
    options: FetchOptions,
}

fn parse_flags(arguments: &[String]) -> Result<(Vec<String>, Flags), String> {
    let mut positional = Vec::new();
    let mut flags = Flags {
        app: None,
        cdn: false,
        json: false,
        options: FetchOptions::default(),
    };
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        let value = |name: &str| -> Result<String, String> {
            arguments
                .get(index + 1)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match argument {
            "--app" => {
                flags.app = Some(value("--app")?);
                index += 2;
            }
            "--timeout" => {
                let seconds: u64 = value("--timeout")?
                    .parse()
                    .map_err(|_| "--timeout needs a whole number of seconds".to_owned())?;
                flags.options.timeout = Duration::from_secs(seconds);
                index += 2;
            }
            "--max-bytes" => {
                flags.options.max_bytes = value("--max-bytes")?
                    .parse()
                    .map_err(|_| "--max-bytes needs a whole number".to_owned())?;
                index += 2;
            }
            "--cdn" => {
                flags.cdn = true;
                index += 1;
            }
            "--json" => {
                flags.json = true;
                index += 1;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other}"));
            }
            other => {
                positional.push(other.to_owned());
                index += 1;
            }
        }
    }
    Ok((positional, flags))
}

fn read_source(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|error| format!("could not read stdin: {error}"))?;
        return Ok(buffer);
    }
    std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))
}

/// Prints either the structured value or the rendered text, and reports whether the answer was
/// clean enough to exit zero.
fn emit<T: serde::Serialize>(
    value: &T,
    rendered: impl FnOnce() -> String,
    json: bool,
    ok: bool,
) -> Result<ExitCode, String> {
    if json {
        let text = blazingly_json::to_string_pretty(value)
            .map_err(|error| format!("could not serialize the result: {error}"))?;
        println!("{text}");
    } else {
        print!("{}", rendered());
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn run(arguments: &[String]) -> Result<ExitCode, String> {
    if arguments.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    if arguments.iter().any(|a| a == "-V" || a == "--version") {
        println!("blazingly-aasa {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    if arguments.is_empty() {
        // Default mode: an MCP client launches this binary with no arguments.
        return mcp::serve(FetchOptions::default())
            .map(|()| ExitCode::SUCCESS)
            .map_err(|error| error.to_string());
    }

    let (positional, flags) = parse_flags(&arguments[1..])?;
    let need = |count: usize, usage: &str| -> Result<(), String> {
        if positional.len() == count {
            Ok(())
        } else {
            Err(format!("usage: blazingly-aasa {usage}"))
        }
    };

    match arguments[0].as_str() {
        "serve" => mcp::serve(flags.options)
            .map(|()| ExitCode::SUCCESS)
            .map_err(|error| error.to_string()),
        "check" => {
            need(2, "check <domain> <url> [--app <id>]")?;
            let check = tools::check_universal_link(
                &positional[0],
                flags.app.as_deref(),
                &positional[1],
                flags.options,
            )
            .map_err(|failure| report(&failure, flags.json))?;
            let ok = check
                .decision
                .map_or(!check.apps.is_empty(), |d| d == "match");
            emit(&check, || render::link_check(&check), flags.json, ok)
        }
        "fetch" => {
            need(1, "fetch <domain> [--cdn]")?;
            let source = if flags.cdn {
                Source::AppleCdn
            } else {
                Source::WellKnown
            };
            let report = tools::fetch_association_file(&positional[0], source, flags.options)
                .map_err(|failure| report(&failure, flags.json))?;
            let ok = !report.has_errors;
            emit(&report, || render::file_report(&report), flags.json, ok)
        }
        "compare" => {
            need(1, "compare <domain>")?;
            let comparison = tools::compare_origin_and_cdn(&positional[0], flags.options)
                .map_err(|failure| report(&failure, flags.json))?;
            let ok = comparison.equivalent;
            emit(
                &comparison,
                || render::comparison(&comparison),
                flags.json,
                ok,
            )
        }
        "validate" => {
            need(1, "validate <file|->")?;
            let content = read_source(&positional[0])?;
            let validation = tools::validate_association_file(&content, flags.options.max_bytes)
                .map_err(|failure| report(&failure, flags.json))?;
            let ok = !validation.has_errors;
            emit(
                &validation,
                || render::validation(&validation),
                flags.json,
                ok,
            )
        }
        "explain" => {
            need(3, "explain <file|-> <domain> <url> [--app <id>]")?;
            let content = read_source(&positional[0])?;
            let explanation = tools::explain_match(
                &content,
                &positional[1],
                flags.app.as_deref(),
                &positional[2],
                flags.options.max_bytes,
            )
            .map_err(|failure| report(&failure, flags.json))?;
            let ok = explanation
                .decision
                .map_or(!explanation.apps.is_empty(), |d| d == "match");
            emit(
                &explanation,
                || render::explanation(&explanation),
                flags.json,
                ok,
            )
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}
