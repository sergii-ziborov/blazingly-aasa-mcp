//! The npm launcher and the release workflow must agree on which platforms exist.
//!
//! They are two files in two languages, and a mismatch does not fail anywhere obvious: the
//! launcher asks GitHub for an artifact that was never built, and the user sees a 404 that looks
//! like a broken release rather than an unsupported platform. This is cheap insurance.

use std::collections::BTreeSet;
use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Rust target triples look like `arch-vendor-os[-abi]`; both files spell them identically.
fn triples(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .filter(|word| {
            word.matches('-').count() >= 2
                && (word.starts_with("aarch64-") || word.starts_with("x86_64-"))
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_launcher_and_the_release_workflow_build_the_same_platforms() {
    let built = triples(&read(".github/workflows/release-binaries.yml"));
    let expected = triples(&read("npm/scripts/platform.mjs"));

    assert!(
        !built.is_empty(),
        "no targets found in the release workflow"
    );
    assert_eq!(
        expected, built,
        "npm/scripts/platform.mjs and release-binaries.yml disagree about platforms"
    );
}

#[test]
fn the_npm_package_version_matches_the_crate() {
    let package = read("npm/package.json");
    let crate_version = env!("CARGO_PKG_VERSION");
    assert!(
        package.contains(&format!("\"version\": \"{crate_version}\"")),
        "npm/package.json must be version {crate_version}: the launcher downloads from the \
         release tagged v<that version>"
    );
}

#[test]
fn the_launcher_never_fails_an_install() {
    let install = read("npm/scripts/install.mjs");
    assert!(
        install.contains("process.exit(0)") && install.contains("console.warn"),
        "an unsupported platform must warn and exit zero, not break `npm install`"
    );
    assert!(
        install.contains("checksum mismatch"),
        "a downloaded binary must be checksum-verified before it is kept"
    );
}
