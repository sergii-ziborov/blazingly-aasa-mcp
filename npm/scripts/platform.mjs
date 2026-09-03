// Maps this machine to the release artifact built for it.
//
// Kept in one place because the launcher and the installer must agree exactly; a mismatch here
// produces a 404 from GitHub that looks like a broken release rather than an unsupported platform.

export const TARGETS = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "win32-x64": "x86_64-pc-windows-msvc",
};

/** The Rust target triple for this platform, or `null` when there is no build for it. */
export function target(platform = process.platform, arch = process.arch) {
  return TARGETS[`${platform}-${arch}`] ?? null;
}

/** The name of the executable inside the archive. */
export function binaryName(platform = process.platform) {
  return platform === "win32" ? "blazingly-aasa.exe" : "blazingly-aasa";
}

/** A message that says what to do, rather than only what failed. */
export function unsupportedMessage(platform = process.platform, arch = process.arch) {
  return [
    `blazingly-aasa has no prebuilt binary for ${platform}-${arch}.`,
    "",
    "Supported: " + Object.keys(TARGETS).join(", "),
    "",
    "Install from source instead:",
    "  cargo install blazingly-aasa-mcp",
  ].join("\n");
}
