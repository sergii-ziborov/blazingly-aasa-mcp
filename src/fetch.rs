//! Retrieving association files over HTTPS.
//!
//! Everything network-facing lives here, which is the whole reason this crate exists separately
//! from `blazingly-aasa`: that crate takes bytes and explicit context and has no opinions about
//! transport. This one has opinions, and they are worth stating.
//!
//! **Redirects are never followed.** Apple requires the file to be served with no redirects, so
//! following one would hide a real misconfiguration. It also removes a redirect-based way to reach
//! somewhere the caller did not name.
//!
//! **Only HTTPS, only public hostnames, only the two documented paths.** The caller supplies a
//! domain, never a URL, so this cannot be pointed at an arbitrary endpoint.
//!
//! **A name is not an address.** [`check_domain`] rejects an IP literal or `localhost`, but
//! `evil.example` is a well-formed public hostname that can resolve to `127.0.0.1` or
//! `169.254.169.254`. The address itself is vetted in [`crate::resolver`], which hands ureq only
//! addresses outside the local network — and, because ureq connects to exactly what the resolver
//! returns, leaves no second lookup for DNS rebinding to poison.
//!
//! **No proxies.** ureq reads `HTTPS_PROXY` from the environment by default, and a proxied request
//! resolves the name at the proxy rather than here, which would route around all of the above.

use std::fmt;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::resolver::{PublicOnlyResolver, Rejections};

/// Where an association file was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// `https://<domain>/.well-known/apple-app-site-association`, the location Apple documents.
    WellKnown,
    /// `https://<domain>/apple-app-site-association`, the older location Apple still accepts.
    Root,
    /// `https://app-site-association.cdn-apple.com/a/v1/<domain>`, what devices actually receive.
    AppleCdn,
}

impl Source {
    /// The URL this source reads for `domain`.
    #[must_use]
    pub fn url(self, domain: &str) -> String {
        match self {
            Self::WellKnown => format!("https://{domain}/.well-known/apple-app-site-association"),
            Self::Root => format!("https://{domain}/apple-app-site-association"),
            Self::AppleCdn => format!("https://app-site-association.cdn-apple.com/a/v1/{domain}"),
        }
    }

    /// A short label for output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WellKnown => "well-known",
            Self::Root => "root",
            Self::AppleCdn => "apple-cdn",
        }
    }
}

/// What the transport observed, separate from what the document says.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Hosting {
    /// Which location was read.
    pub source: Source,
    /// The URL requested.
    pub url: String,
    /// HTTP status code.
    pub status: u16,
    /// `Content-Type`, when the server sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Body size in bytes.
    pub bytes: usize,
    /// Where the server tried to redirect to. Apple requires no redirects, so this is a finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirected_to: Option<String>,
    /// Apple CDN diagnostic headers, when reading from the CDN.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub apple_headers: Vec<(String, String)>,
    /// Transport-level findings, in the order they were noticed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A successful read.
pub struct Fetched {
    /// The bytes as served.
    pub body: Vec<u8>,
    /// What the transport observed.
    pub hosting: Hosting,
}

/// A read that did not produce a document.
#[derive(Debug)]
pub struct FetchError {
    /// Which location was attempted. Kept for callers that report per-source failures.
    #[allow(dead_code)]
    pub source: Source,
    /// The URL requested.
    pub url: String,
    /// What went wrong.
    pub message: String,
    /// Transport facts, when the request got far enough to produce any.
    pub hosting: Option<Hosting>,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.url, self.message)
    }
}

impl std::error::Error for FetchError {}

/// Limits applied to every request.
#[derive(Debug, Clone, Copy)]
pub struct FetchOptions {
    /// Whole-request timeout.
    pub timeout: Duration,
    /// Largest body accepted, in bytes.
    pub max_bytes: usize,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            // Matches blazingly-aasa's own default ceiling.
            max_bytes: 128 * 1024,
        }
    }
}

/// Apple's CDN reports why it rejected a file through these headers.
const APPLE_HEADERS: &[&str] = &[
    "apple-failure-reason",
    "apple-failure-details",
    "apple-from",
    "apple-try-direct",
];

/// Rejects anything that is not a plain public hostname.
///
/// The caller names a domain, not a URL, so this is the only place an address enters. An IP
/// literal, `localhost`, a `.local` name, or anything with a port, path, or credentials is refused
/// before a socket is opened.
///
/// # Errors
///
/// Returns a description of what is wrong with `domain`.
pub fn check_domain(domain: &str) -> Result<(), String> {
    if domain.is_empty() {
        return Err("domain is empty".to_owned());
    }
    if domain.len() > 253 {
        return Err("domain is longer than 253 characters".to_owned());
    }
    if let Some(bad) = domain
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '.' || *c == '-'))
    {
        return Err(format!(
            "`{domain}` contains `{bad}`; pass a bare hostname such as example.com, not a URL"
        ));
    }
    if domain.parse::<IpAddr>().is_ok() {
        return Err(format!(
            "`{domain}` is an IP address; association files are served by name"
        ));
    }
    let lower = domain.to_ascii_lowercase();
    // Compare the final label rather than a suffix: `mylocal.com` is a perfectly good domain.
    let last_label = lower.rsplit('.').next().unwrap_or_default();
    if lower == "localhost" || last_label == "localhost" || last_label == "local" {
        return Err(format!("`{domain}` is not a public host"));
    }
    if !lower.contains('.') {
        return Err(format!(
            "`{domain}` has no dot, so it is not a fully qualified domain name"
        ));
    }
    if lower.starts_with('.') || lower.ends_with('.') || lower.contains("..") {
        return Err(format!("`{domain}` is not a well-formed domain name"));
    }
    Ok(())
}

fn agent(options: FetchOptions, rejections: Rejections) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(options.timeout))
        // Apple requires the file to be served without redirects, so following one would hide the
        // misconfiguration this tool exists to find.
        .max_redirects(0)
        .max_redirects_will_error(false)
        .https_only(true)
        // A proxy would do the name lookup itself, which is exactly what PublicOnlyResolver is
        // there to prevent. ureq picks proxies up from the environment unless told not to.
        .proxy(None)
        .user_agent(concat!("blazingly-aasa-mcp/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::default(),
        PublicOnlyResolver::new(rejections),
    )
}

/// Reads one association file.
///
/// # Errors
///
/// Returns [`FetchError`] for a refused domain, a transport failure, a non-200 status, or a body
/// above the configured ceiling.
pub fn fetch(
    source: Source,
    domain: &str,
    options: FetchOptions,
) -> Result<Fetched, Box<FetchError>> {
    let url = source.url(domain);
    if let Err(message) = check_domain(domain) {
        return Err(Box::new(FetchError {
            source,
            url,
            message,
            hosting: None,
        }));
    }

    // The resolver records why it refused an address; without it a blocked host is
    // indistinguishable from a typo.
    let rejections: Rejections = Arc::new(Mutex::new(Vec::new()));
    let response = agent(options, rejections.clone())
        .get(&url)
        .call()
        .map_err(|error| {
            let refused = rejections
                .lock()
                .ok()
                .and_then(|reasons| reasons.first().cloned());
            Box::new(FetchError {
                source,
                url: url.clone(),
                message: refused.unwrap_or_else(|| error.to_string()),
                hosting: None,
            })
        })?;

    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let content_type = header("content-type");
    let redirected_to = header("location");
    let apple_headers: Vec<(String, String)> = APPLE_HEADERS
        .iter()
        .filter_map(|name| header(name).map(|value| ((*name).to_owned(), value)))
        .collect();

    let mut notes = Vec::new();
    if (300..400).contains(&status) {
        notes.push(format!(
            "the server replied {status} and tried to redirect; Apple requires the association \
             file to be served with no redirects"
        ));
    }
    if let Some(content_type) = &content_type {
        let base = content_type.split(';').next().unwrap_or("").trim();
        if !base.eq_ignore_ascii_case("application/json")
            && !base.eq_ignore_ascii_case("application/pkcs7-mime")
        {
            notes.push(format!(
                "Content-Type is `{content_type}`; Apple expects application/json"
            ));
        }
    } else {
        notes.push("the server sent no Content-Type".to_owned());
    }

    let mut response = response;
    let body = response
        .body_mut()
        .with_config()
        .limit(options.max_bytes as u64)
        .read_to_vec()
        .map_err(|error| {
            Box::new(FetchError {
                source,
                url: url.clone(),
                message: format!(
                    "could not read the body within the {} byte limit: {error}",
                    options.max_bytes
                ),
                hosting: None,
            })
        })?;

    let hosting = Hosting {
        source,
        url: url.clone(),
        status,
        content_type,
        bytes: body.len(),
        redirected_to,
        apple_headers,
        notes,
    };

    if status != 200 {
        return Err(Box::new(FetchError {
            source,
            url,
            message: format!("HTTP {status}"),
            hosting: Some(hosting),
        }));
    }
    Ok(Fetched { body, hosting })
}

/// Reads from `.well-known`, falling back to the older root location.
///
/// # Errors
///
/// Returns the `.well-known` failure when neither location works, since that is the one Apple
/// documents and the one worth fixing.
pub fn fetch_origin(domain: &str, options: FetchOptions) -> Result<Fetched, Box<FetchError>> {
    match fetch(Source::WellKnown, domain, options) {
        Ok(found) => Ok(found),
        Err(well_known) => match fetch(Source::Root, domain, options) {
            Ok(mut found) => {
                found.hosting.notes.push(
                    "served from /apple-app-site-association; Apple documents \
                     /.well-known/apple-app-site-association"
                        .to_owned(),
                );
                Ok(found)
            }
            Err(_) => Err(well_known),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_match_the_documented_locations() {
        assert_eq!(
            Source::WellKnown.url("example.com"),
            "https://example.com/.well-known/apple-app-site-association"
        );
        assert_eq!(
            Source::AppleCdn.url("example.com"),
            "https://app-site-association.cdn-apple.com/a/v1/example.com"
        );
    }

    #[test]
    fn only_public_hostnames_are_accepted() {
        assert!(check_domain("example.com").is_ok());
        assert!(check_domain("a.b.example.co.uk").is_ok());

        for bad in [
            "",
            "localhost",
            "app.localhost",
            "printer.local",
            "127.0.0.1",
            "::1",
            "169.254.169.254",
            "example.com:8080",
            "https://example.com",
            "example.com/path",
            "user@example.com",
            "nodots",
            ".example.com",
            "example..com",
        ] {
            assert!(check_domain(bad).is_err(), "{bad} should be refused");
        }
    }
}
