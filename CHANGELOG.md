# Changelog

All notable changes to this project are documented here. This project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-09-03

### Fixed — security

- **A public hostname that resolves into the local network is now refused.** `check_domain`
  validates the *name* — it rejects an IP literal, `localhost`, and `.local` — and the README said
  that stopped a tool call reaching an internal address. It did not. `localtest.me` is a
  well-formed public hostname that resolves to `127.0.0.1`, and `10-0-0-1.sslip.io` to `10.0.0.1`.
  Both passed and were fetched.

  For a CLI that is untidy. For an MCP server it is a real boundary: the arguments can come from a
  repository, an issue, or a README that an agent was asked to act on.

  DNS answers are now filtered against loopback, private, link-local — which is where
  `169.254.169.254` lives — carrier-grade NAT, multicast and reserved ranges, with IPv6 covered and
  IPv4-mapped IPv6 unwrapped, because `::ffff:127.0.0.1` reaches loopback and passes an IPv6-only
  check.

  The filtering happens **inside the resolver**, which is the part that matters: `ureq` connects to
  exactly the addresses a resolver returns, so there is no second lookup left for DNS rebinding to
  poison. Resolve-then-validate-then-let-the-client-resolve-again would have looked equivalent and
  left the window open.

- **Proxies are disabled.** `ureq` reads `HTTPS_PROXY` from the environment by default, and a
  proxied request resolves the name at the proxy — routing around all of the above.

### Changed

- Depends on `blazingly-aasa` 0.1.1, which quotes a rule's own `comment` in the catch-all warning.
  A catch-all is frequently deliberate; GitHub's association file ends with one commented
  "Matches all remaining routes".

## [0.1.0] - 2026-09-02

First release. MCP server and CLI for debugging Apple Universal Links, built on `blazingly-aasa`.

- Five tools: `check_universal_link`, `fetch_association_file`, `compare_origin_and_cdn`,
  `validate_association_file`, `explain_match`. Three reach the network and two do not, and every
  description says which. Omitting `app_id` turns "does this app get this URL" into "which apps
  does this URL reach".
- A command line over the same functions, so the two front ends cannot disagree about an answer.
- Redirects are never followed: Apple requires the file to be served without them, so following one
  would hide the misconfiguration being looked for.
- Distributed as prebuilt binaries for macOS (arm64, x64), Linux (x64, arm64) and Windows (x64),
  launched from npm so no Rust toolchain is needed.

[0.1.1]: https://github.com/sergii-ziborov/blazingly-aasa-mcp/releases/tag/v0.1.1
[0.1.0]: https://github.com/sergii-ziborov/blazingly-aasa-mcp/releases/tag/v0.1.0
