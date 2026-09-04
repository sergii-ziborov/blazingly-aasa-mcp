//! Human-readable rendering for the command line.
//!
//! `push_str(&format!(...))` rather than `write!` throughout: rendering runs once per invocation,
//! never in a loop, and the temporary allocation clippy objects to buys readability that matters
//! more here than it costs.
#![allow(clippy::format_push_string)]

//!
//! The MCP side returns structured content; this turns the same values into something a person
//! reads in a terminal. Both come from one set of tool functions, so the two front ends cannot
//! disagree about an answer.

use blazingly_aasa::{Diagnostic, Severity};

use crate::fetch::Hosting;
use crate::tools::{
    AppServices, AppVerdict, CdnComparison, Explanation, Failure, FileReport, LinkCheck, Validation,
};

fn hosting(out: &mut String, facts: &Hosting) {
    out.push_str(&format!(
        "source:       {} ({})\n",
        facts.url,
        facts.source.label()
    ));
    out.push_str(&format!("status:       {}\n", facts.status));
    if let Some(content_type) = &facts.content_type {
        out.push_str(&format!("content-type: {content_type}\n"));
    }
    out.push_str(&format!("size:         {} bytes\n", facts.bytes));
    if let Some(location) = &facts.redirected_to {
        out.push_str(&format!("redirect:     {location}\n"));
    }
    for (name, value) in &facts.apple_headers {
        out.push_str(&format!("{name}: {value}\n"));
    }
    for note in &facts.notes {
        out.push_str(&format!("  ! {note}\n"));
    }
}

fn diagnostics(out: &mut String, list: &[Diagnostic]) {
    if list.is_empty() {
        out.push_str("\ndiagnostics:  none\n");
        return;
    }
    let errors = list
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = list
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    out.push_str(&format!(
        "\ndiagnostics:  {errors} error(s), {warnings} warning(s)\n"
    ));
    for diagnostic in list {
        out.push_str(&format!("  {diagnostic}\n"));
    }
}

fn verdicts(out: &mut String, apps: &[AppVerdict]) {
    if apps.is_empty() {
        out.push_str("\napps reached: none\n");
        return;
    }
    out.push_str("\napps reached:\n");
    for app in apps {
        out.push_str(&format!("  {:<10} {}\n", app.decision, app.app_id));
    }
}

fn app_list(out: &mut String, apps: &[AppServices]) {
    if apps.is_empty() {
        out.push_str("\napps:         none\n");
        return;
    }
    out.push_str("\napps:\n");
    for app in apps {
        let services: Vec<&str> = app.services.iter().map(|s| s.key()).collect();
        out.push_str(&format!("  {:<44} {}\n", app.app_id, services.join(", ")));
    }
}

/// Renders a failure, including the transport facts when the request got far enough to produce
/// any.
///
/// A bare "HTTP 301" is the least useful half of that answer: the redirect target and the fact that
/// Apple forbids redirects at all are what the reader needs.
#[must_use]
pub fn failure(failure: &Failure) -> String {
    let mut out = format!("error: {}\n", failure.error);
    if let Some(facts) = failure.hosting.as_deref() {
        out.push('\n');
        hosting(&mut out, facts);
    }
    out
}

/// Renders a link check.
#[must_use]
pub fn link_check(check: &LinkCheck) -> String {
    let mut out = String::new();
    out.push_str(&format!("domain:       {}\n", check.domain));
    out.push_str(&format!("url:          {}\n", check.url));
    hosting(&mut out, &check.hosting);
    if let Some(explanation) = &check.explanation {
        out.push('\n');
        out.push_str(explanation);
    }
    if let Some(services) = &check.services {
        let names: Vec<&str> = services.iter().map(|s| s.key()).collect();
        out.push_str(&format!(
            "\nservices:     {}\n",
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(", ")
            }
        ));
    }
    verdicts(&mut out, &check.apps);
    diagnostics(&mut out, &check.diagnostics);
    for note in &check.notes {
        out.push_str(&format!("\nnote: {note}\n"));
    }
    out
}

/// Renders a fetched file report.
#[must_use]
pub fn file_report(report: &FileReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("domain:       {}\n", report.domain));
    hosting(&mut out, &report.hosting);
    app_list(&mut out, &report.apps);
    diagnostics(&mut out, &report.diagnostics);
    out
}

/// Renders an origin/CDN comparison.
#[must_use]
pub fn comparison(comparison: &CdnComparison) -> String {
    let mut out = String::new();
    out.push_str(&format!("domain:       {}\n\n", comparison.domain));
    out.push_str("origin\n");
    hosting(&mut out, &comparison.origin);
    out.push_str("\napple cdn\n");
    hosting(&mut out, &comparison.apple_cdn);
    out.push('\n');

    if comparison.equivalent {
        out.push_str(if comparison.structurally_identical {
            "identical: the CDN is serving the same file.\n"
        } else {
            "equivalent: the two files are written differently but behave identically.\n"
        });
        return out;
    }
    out.push_str(&format!(
        "DIFFERENT: {} policy change(s). Apple's CDN may be serving an older file.\n\n",
        comparison.changes.len()
    ));
    for change in &comparison.changes {
        out.push_str(&format!("{change}\n"));
    }
    out
}

/// Renders an offline validation.
#[must_use]
pub fn validation(validation: &Validation) -> String {
    let mut out = String::new();
    app_list(&mut out, &validation.apps);
    diagnostics(&mut out, &validation.diagnostics);
    out
}

/// Renders an offline match.
#[must_use]
pub fn explanation(explanation: &Explanation) -> String {
    let mut out = String::new();
    if let Some(text) = &explanation.explanation {
        out.push_str(text);
    }
    verdicts(&mut out, &explanation.apps);
    for note in &explanation.notes {
        out.push_str(&format!("\nnote: {note}\n"));
    }
    out
}
