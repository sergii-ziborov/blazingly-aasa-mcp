//! The command line, exercised offline.
//!
//! The point of these is the shared-path guarantee: a `blazingly-aasa explain` answer and an
//! `explain_match` tool result must come from one function. If someone reimplements one of them,
//! these and `mcp_protocol.rs` will disagree.

use std::io::Write;
use std::process::{Command, Stdio};

const DOCUMENT: &str = r#"{"applinks":{"details":[{
    "appIDs": ["ABCDE12345.com.example.app"],
    "components": [
        {"/": "/help/website/*", "exclude": true},
        {"/": "/help/*", "?": {"articleNumber": "????"}}
    ]
}]}}"#;

const APP: &str = "ABCDE12345.com.example.app";

struct Output {
    stdout: String,
    stderr: String,
    success: bool,
}

/// Runs the binary with `arguments`, feeding `DOCUMENT` on stdin so no temporary file is needed.
fn run(arguments: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_blazingly-aasa"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary should start");
    if let Some(mut stdin) = child.stdin.take() {
        // Commands that never read stdin -- `fetch`, `--help` -- can exit before this write
        // lands, and a broken pipe there is the expected outcome rather than a failure. Dropping
        // the handle afterwards gives EOF to the commands that do read.
        let _ = stdin.write_all(DOCUMENT.as_bytes());
    }
    let output = child.wait_with_output().expect("the binary should finish");
    Output {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    }
}

#[test]
fn help_lists_every_command() {
    let output = run(&["--help"]);
    assert!(output.success);
    for command in ["check", "fetch", "compare", "validate", "explain"] {
        assert!(output.stdout.contains(command), "help omits {command}");
    }
    assert!(
        output.stdout.contains("Associated Domains entitlement"),
        "help should say what a result does not prove"
    );
}

#[test]
fn version_is_the_crate_version() {
    let output = run(&["--version"]);
    assert!(output.success);
    assert!(output.stdout.trim().ends_with(env!("CARGO_PKG_VERSION")));
}

#[test]
fn validate_reads_stdin_and_succeeds_on_a_clean_file() {
    let output = run(&["validate", "-"]);
    assert!(output.success, "{}", output.stderr);
    assert!(output.stdout.contains(APP));
    assert!(
        output.stdout.contains("diagnostics:  none"),
        "{}",
        output.stdout
    );
}

#[test]
fn validate_exits_non_zero_when_the_file_has_errors() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_blazingly-aasa"))
        .args(["validate", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(br#"{"applinks":{"details":[{"components":[{"/":"/*"}]}]}}"#)
            .expect("validate reads stdin, so this write must land");
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "an error-level diagnostic should fail"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("AASA110"));
}

#[test]
fn explain_matches_and_exits_zero() {
    let output = run(&[
        "explain",
        "-",
        "example.com",
        "https://example.com/help/1?articleNumber=4815",
        "--app",
        APP,
    ]);
    assert!(output.success, "{}{}", output.stdout, output.stderr);
    assert!(output.stdout.contains("MATCH"));
}

#[test]
fn explain_reports_the_failing_component_and_exits_non_zero() {
    let output = run(&[
        "explain",
        "-",
        "example.com",
        "https://example.com/help/1?articleNumber=481",
        "--app",
        APP,
    ]);
    assert!(!output.success, "a miss should not exit zero");
    assert!(output.stdout.contains("NO_MATCH"));
    assert!(
        output.stdout.contains("articleNumber"),
        "the trace should name the failing component:\n{}",
        output.stdout
    );
}

#[test]
fn json_output_is_machine_readable() {
    let output = run(&[
        "explain",
        "-",
        "example.com",
        "https://example.com/help/1?articleNumber=4815",
        "--app",
        APP,
        "--json",
    ]);
    let parsed: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("--json should emit JSON");
    assert_eq!(parsed["decision"], "match");
    assert_eq!(parsed["apps"][0]["app_id"], APP);
}

#[test]
fn a_refused_domain_never_reaches_the_network() {
    // The guard runs before any socket is opened, so this is fast and offline.
    for domain in [
        "localhost",
        "127.0.0.1",
        "169.254.169.254",
        "https://example.com",
    ] {
        let output = run(&["fetch", domain]);
        assert!(!output.success, "{domain} should be refused");
        assert!(
            !output.stderr.contains("timed out"),
            "{domain} should be refused before any request: {}",
            output.stderr
        );
    }
}

#[test]
fn an_unknown_command_explains_itself() {
    let output = run(&["frobnicate"]);
    assert!(!output.success);
    assert!(output.stderr.contains("frobnicate"));
    assert!(output.stderr.contains("blazingly-aasa check"));
}
