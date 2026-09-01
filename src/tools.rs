//! The tool implementations.
//!
//! Both front ends route through here. The MCP server and the command line are not two
//! implementations of the same answers — a `blazingly-aasa check` result and a
//! `check_universal_link` tool result come out of one function with one set of bounds.
//!
//! Every result is a `serde` structure rather than formatted text, because the MCP side needs
//! structured content and the command line renders the same values in `render`.

use blazingly_aasa::{
    AasaDocument, CompiledAasa, Diagnostic, MatchDecision, MatchResult, ParseOptions,
    SemanticChange, Service,
};
use serde::Serialize;

use crate::fetch::{self, FetchOptions, Fetched, Hosting, Source};

/// What went wrong, in a shape both front ends can render.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    /// A human-readable explanation.
    pub error: String,
    /// Transport facts, when the failure happened after a response arrived.
    ///
    /// Boxed so that `Result<_, Failure>` stays small on the success path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosting: Option<Box<Hosting>>,
}

impl Failure {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            hosting: None,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}

type Outcome<T> = Result<T, Failure>;

/// One app's verdict for a URL.
#[derive(Debug, Clone, Serialize)]
pub struct AppVerdict {
    /// The application identifier.
    pub app_id: String,
    /// `match`, `exclude`, or `no_match`.
    pub decision: &'static str,
}

/// The answer to "why doesn't this link open my app?".
#[derive(Debug, Clone, Serialize)]
pub struct LinkCheck {
    /// The domain that was read.
    pub domain: String,
    /// The URL under test.
    pub url: String,
    /// Transport facts about the file that was read.
    pub hosting: Hosting,
    /// Diagnostics from validating the file.
    pub diagnostics: Vec<Diagnostic>,
    /// The decision for the requested app, when one was named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<&'static str>,
    /// A human-readable trace of that decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// The services the file grants the requested app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<Service>>,
    /// Every app the URL reaches. Always present, and the whole answer when no app was named.
    pub apps: Vec<AppVerdict>,
    /// Context that does not change the decision, such as a non-HTTPS scheme.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A file that was read and validated.
#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    /// The domain that was read.
    pub domain: String,
    /// Transport facts.
    pub hosting: Hosting,
    /// Diagnostics from validating the file.
    pub diagnostics: Vec<Diagnostic>,
    /// Whether any diagnostic was an error.
    pub has_errors: bool,
    /// Every app named anywhere in the file, with the services it is granted.
    pub apps: Vec<AppServices>,
}

/// One app and what the file grants it.
#[derive(Debug, Clone, Serialize)]
pub struct AppServices {
    /// The application identifier.
    pub app_id: String,
    /// The services it appears under.
    pub services: Vec<Service>,
}

/// The result of comparing what a site serves against what Apple's CDN serves.
#[derive(Debug, Clone, Serialize)]
pub struct CdnComparison {
    /// The domain that was read.
    pub domain: String,
    /// Transport facts for the origin.
    pub origin: Hosting,
    /// Transport facts for the CDN.
    pub apple_cdn: Hosting,
    /// Whether the two make the same decisions for every app.
    pub equivalent: bool,
    /// The behavioural differences, empty when equivalent.
    pub changes: Vec<SemanticChange>,
    /// Whether the two files are also byte-for-byte identical in structure.
    pub structurally_identical: bool,
}

/// Validation of a document supplied by the caller.
#[derive(Debug, Clone, Serialize)]
pub struct Validation {
    /// Diagnostics, most severe first.
    pub diagnostics: Vec<Diagnostic>,
    /// Whether any diagnostic was an error.
    pub has_errors: bool,
    /// Every app named anywhere in the file, with the services it is granted.
    pub apps: Vec<AppServices>,
}

/// A match against a document supplied by the caller.
#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    /// The decision for the requested app, when one was named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<&'static str>,
    /// A human-readable trace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// Every app the URL reaches.
    pub apps: Vec<AppVerdict>,
    /// Context that does not change the decision.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

const fn decision_name(decision: MatchDecision) -> &'static str {
    match decision {
        MatchDecision::Match => "match",
        MatchDecision::Exclude => "exclude",
        MatchDecision::NoMatch => "no_match",
    }
}

fn compile(bytes: &[u8], limit: usize) -> Outcome<CompiledAasa> {
    let options = ParseOptions::new().max_bytes(limit);
    AasaDocument::parse_with(bytes, &options)
        .map(|document| document.compile())
        .map_err(|error| Failure::new(error.to_string()))
}

fn app_services(compiled: &CompiledAasa) -> Vec<AppServices> {
    let mut apps: Vec<String> = Vec::new();
    for service in [
        Service::AppLinks,
        Service::WebCredentials,
        Service::AppClips,
        Service::ActivityContinuation,
    ] {
        for app_id in compiled.apps_for_service(service) {
            if !apps.iter().any(|existing| existing == app_id) {
                apps.push(app_id.to_owned());
            }
        }
    }
    apps.sort_unstable();
    apps.into_iter()
        .map(|app_id| AppServices {
            services: compiled.services_for_app(&app_id),
            app_id,
        })
        .collect()
}

fn verdicts(compiled: &CompiledAasa, domain: &str, url: &str) -> Outcome<Vec<AppVerdict>> {
    compiled
        .apps_for_url(domain, url)
        .map(|apps| {
            apps.into_iter()
                .map(|(app_id, decision)| AppVerdict {
                    app_id,
                    decision: decision_name(decision),
                })
                .collect()
        })
        .map_err(|error| Failure::new(format!("invalid URL: {error}")))
}

fn matched(compiled: &CompiledAasa, domain: &str, app_id: &str, url: &str) -> Outcome<MatchResult> {
    compiled
        .match_url(domain, app_id, url)
        .map_err(|error| Failure::new(format!("invalid URL: {error}")))
}

fn read(source: Source, domain: &str, options: FetchOptions) -> Outcome<Fetched> {
    let result = if source == Source::WellKnown {
        fetch::fetch_origin(domain, options)
    } else {
        fetch::fetch(source, domain, options)
    };
    result.map_err(|error| Failure {
        error: error.to_string(),
        hosting: error.hosting.map(Box::new),
    })
}

/// Fetches a domain's association file and answers whether it lets an app open a URL.
///
/// With no `app_id`, the answer is every app the URL reaches.
///
/// # Errors
///
/// Returns [`Failure`] when the domain is refused, the file cannot be read, or the URL is unusable.
pub fn check_universal_link(
    domain: &str,
    app_id: Option<&str>,
    url: &str,
    options: FetchOptions,
) -> Outcome<LinkCheck> {
    let fetched = read(Source::WellKnown, domain, options)?;
    let compiled = compile(&fetched.body, options.max_bytes)?;
    let report = compiled.validate();

    let apps = verdicts(&compiled, domain, url)?;
    let (decision, explanation, services, notes) = match app_id {
        Some(app_id) => {
            let result = matched(&compiled, domain, app_id, url)?;
            (
                Some(decision_name(result.decision)),
                Some(result.to_string()),
                Some(compiled.services_for_app(app_id)),
                result.notes.clone(),
            )
        }
        None => (None, None, None, Vec::new()),
    };

    Ok(LinkCheck {
        domain: domain.to_owned(),
        url: url.to_owned(),
        hosting: fetched.hosting,
        diagnostics: report.diagnostics().to_vec(),
        decision,
        explanation,
        services,
        apps,
        notes,
    })
}

/// Fetches and validates a domain's association file.
///
/// # Errors
///
/// Returns [`Failure`] when the domain is refused or the file cannot be read or parsed.
pub fn fetch_association_file(
    domain: &str,
    source: Source,
    options: FetchOptions,
) -> Outcome<FileReport> {
    let fetched = read(source, domain, options)?;
    let compiled = compile(&fetched.body, options.max_bytes)?;
    let report = compiled.validate();
    Ok(FileReport {
        domain: domain.to_owned(),
        hosting: fetched.hosting,
        has_errors: report.has_errors(),
        diagnostics: report.diagnostics().to_vec(),
        apps: app_services(&compiled),
    })
}

/// Compares what a site serves against what Apple's CDN is handing to devices.
///
/// # Errors
///
/// Returns [`Failure`] when either file cannot be read or parsed.
pub fn compare_origin_and_cdn(domain: &str, options: FetchOptions) -> Outcome<CdnComparison> {
    let origin = read(Source::WellKnown, domain, options)?;
    let cdn = read(Source::AppleCdn, domain, options)?;

    let left = compile(&origin.body, options.max_bytes)?;
    let right = compile(&cdn.body, options.max_bytes)?;
    let diff = left.semantic_diff(&right);

    Ok(CdnComparison {
        domain: domain.to_owned(),
        origin: origin.hosting,
        apple_cdn: cdn.hosting,
        equivalent: diff.is_equivalent(),
        changes: diff.changes().to_vec(),
        structurally_identical: left.structural_equal(&right),
    })
}

/// Validates a document the caller already holds. No network.
///
/// # Errors
///
/// Returns [`Failure`] when the document cannot be parsed at all.
pub fn validate_association_file(content: &str, limit: usize) -> Outcome<Validation> {
    let compiled = compile(content.as_bytes(), limit)?;
    let report = compiled.validate();
    Ok(Validation {
        has_errors: report.has_errors(),
        diagnostics: report.diagnostics().to_vec(),
        apps: app_services(&compiled),
    })
}

/// Matches a URL against a document the caller already holds. No network.
///
/// With no `app_id`, the answer is every app the URL reaches.
///
/// # Errors
///
/// Returns [`Failure`] when the document or the URL cannot be parsed.
pub fn explain_match(
    content: &str,
    domain: &str,
    app_id: Option<&str>,
    url: &str,
    limit: usize,
) -> Outcome<Explanation> {
    let compiled = compile(content.as_bytes(), limit)?;
    let apps = verdicts(&compiled, domain, url)?;
    let (decision, explanation, notes) = match app_id {
        Some(app_id) => {
            let result = matched(&compiled, domain, app_id, url)?;
            (
                Some(decision_name(result.decision)),
                Some(result.to_string()),
                result.notes.clone(),
            )
        }
        None => (None, None, Vec::new()),
    };
    Ok(Explanation {
        decision,
        explanation,
        apps,
        notes,
    })
}
