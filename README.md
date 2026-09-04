# blazingly-aasa-mcp

**Apple Universal Links diagnostics, as an MCP server and as a command line.** Answers "why doesn't
this link open my app?" in one call.

[![CI](https://github.com/sergii-ziborov/blazingly-aasa-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/sergii-ziborov/blazingly-aasa-mcp/actions/workflows/ci.yml)
![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue)
![license MIT](https://img.shields.io/badge/license-MIT-blue)

---

A universal link that stops working gives you nothing to go on. The file looks fine. The app is
installed. The link opens Safari anyway.

This tells you which of the dozen possible reasons it actually is (domain and app identifier below
are placeholders; the `compare` transcript further down is real output):

```
$ blazingly-aasa check example.com "https://example.com/help/1?articleNumber=481" \
    --app ABCDE12345.com.example.app

source:       https://example.com/.well-known/apple-app-site-association (well-known)
status:       200
content-type: application/json
size:         412 bytes

NO_MATCH

application: ABCDE12345.com.example.app
domain:      example.com
url:         https://example.com/help/1?articleNumber=481

reason:
  the entries that apply to ABCDE12345.com.example.app have no rule matching this URL

closest failure:
  detail #0, rule #1
  [ok  ] path
         url:     /help/1
         pattern: /help/*
         wildcard match
  [FAIL] query[articleNumber]
         url:     481
         pattern: ????
         pattern did not match
```

The rule is right there, and so is the component that failed.

## What it is

A thin shell over [`blazingly-aasa`](https://github.com/sergii-ziborov/blazingly-aasa), which owns
all `apple-app-site-association` semantics and deliberately has no network. This crate adds the
three things that library refuses to carry — an HTTPS client, the MCP protocol, and opinions about
presentation — and nothing else.

The two front ends are not two implementations. `blazingly-aasa check` and the
`check_universal_link` tool call the same function, so they cannot disagree about an answer.

## Install

```bash
npx blazingly-aasa-mcp --help     # no Rust needed
cargo install blazingly-aasa-mcp        # or from source
```

## As an MCP server

Point your client at the binary with no arguments. For Claude Code:

```bash
claude mcp add blazingly-aasa -- npx -y blazingly-aasa-mcp
```

Or in a client config file:

```json
{
  "mcpServers": {
    "blazingly-aasa": {
      "command": "npx",
      "args": ["-y", "blazingly-aasa-mcp"]
    }
  }
}
```

The npm package is a launcher: it downloads the Rust binary built for your platform from the
matching GitHub release and verifies its SHA-256 before use. Prebuilt for macOS (arm64, x64),
Linux (x64, arm64), and Windows (x64).

Five tools. Three reach the network and two do not, and every description says which, so an agent
does not have to guess.

| Tool | Network | Required | Optional | What it answers |
| --- | :-: | --- | --- | --- |
| `check_universal_link` | yes | `domain`, `url` | `app_id` | Does this domain's file let this app open this URL, and why? |
| `fetch_association_file` | yes | `domain` | — | What is served, how is it hosted, and does it validate? |
| `compare_origin_and_cdn` | yes | `domain` | — | Is Apple's CDN serving something different from what I publish? |
| `validate_association_file` | no | `content` | — | Lint a file I already have. |
| `explain_match` | no | `content`, `domain`, `url` | `app_id` | Match a URL against a file I already have. |

Two of them do double duty: **omit `app_id`** and instead of "does this app get this URL" you are
told *every* app the URL reaches.

`content` takes the file itself, so the offline pair works on a draft that is not deployed
anywhere — a pull request, a local build, a file pasted into the chat.

## As a command line

The same five answers, without an MCP client. `--json` emits the structured result instead of
formatted text, so the same answers drop into CI.

```bash
blazingly-aasa check example.com "https://example.com/buy/42" --app ABCDE12345.com.example.app
blazingly-aasa check example.com "https://example.com/buy/42"        # which apps reach it?
blazingly-aasa fetch example.com [--cdn]
blazingly-aasa compare example.com
blazingly-aasa validate ./apple-app-site-association
blazingly-aasa explain ./apple-app-site-association example.com "https://example.com/buy/42"
```

`explain` accepts `-` to read the file from standard input.

**Exit status is the point.** It is non-zero when the answer is bad news — a miss, a validation
error, an origin and CDN that disagree — which is what makes this usable as a build step rather
than something a human has to read.

Linting a file with one mistake in it (`examples/broken.json`, a query dictionary with a boolean
in it):

```text

apps:
  ABCDE12345.com.example.app                   applinks

diagnostics:  1 error(s), 2 warning(s)
  error [AASA150] applinks.details[0].components[0].?.flag: query predicate is a boolean, but Apple documents only string patterns here
  help: Apple ignores the entire query dictionary when any predicate is not a string, so every query constraint in this rule stops applying and the rule matches more URLs, not fewer. Replace every predicate with a string pattern.
  warning [AASA180] applinks.details[0].components[0]: this rule constrains no URL component, so it matches every URL
  help: it opens the whole domain for this app; add `/`, `?`, or `#` if that was not intended
  warning [AASA190] applinks.details[0].components[1]: rule #0 already matches every URL, so this rule never runs
  help: the first matching rule wins; move this rule above the catch-all
exit=1
```

One mistake, three diagnostics, and the chain is the interesting part: Apple discards the whole
`?` dictionary because one predicate is not a string, which leaves that rule constraining nothing,
which makes the rule after it unreachable. A rule the author believed was narrow is in fact open.

Explaining a near miss (`examples/demo.json`, whose second rule wants a four-character
`articleNumber`):

```text
NO_MATCH

application: ABCDE12345.com.example.app
domain:      example.com
url:         https://example.com/help/1?articleNumber=481

reason:
  the entries that apply to ABCDE12345.com.example.app have no rule matching this URL

closest failure:
  detail #0, rule #1
  [ok  ] path
         url:     /help/1
         pattern: /help/*
         wildcard match
  [FAIL] query[articleNumber]
         url:     481
         pattern: ????
         pattern did not match

apps reached: none
exit=1
```

The trace names the rule that came closest and the single component that failed, with the pattern
and the input side by side. `scripts/check_examples.sh` diffs both of these against
`examples/expected/`, and CI runs it, so the blocks above cannot drift from what the binary prints.

## The one that finds the hard bugs

`compare_origin_and_cdn` reads both the file you serve and the file Apple's CDN is currently
handing to devices, then compares **effective policy** rather than bytes. Reformatting or reordering keys
reports no change; a stale CDN copy reports exactly what changed:

```
$ blazingly-aasa compare www.apple.com
...
identical: the CDN is serving the same file.
```

This is the "it works on my machine but not on a device" bug, and it is invisible to every
validator that only reads your origin.

## What it does about hosting

Apple requires the file to be served over HTTPS, as JSON, with **no redirects**. So this never
follows one — following it would hide the misconfiguration you are looking for:

```
$ blazingly-aasa fetch airbnb.com
error: https://airbnb.com/.well-known/apple-app-site-association: HTTP 301

status:       301
redirect:     https://www.airbnb.com/.well-known/apple-app-site-association
  ! the server replied 301 and tried to redirect; Apple requires the association file to be
    served with no redirects
```

It also reports a wrong `Content-Type`, a file served from the older root path instead of
`.well-known`, and Apple's CDN diagnostic headers (`Apple-Failure-Reason` and friends) when reading
from the CDN.

## What it will not tell you

**Whether a link actually opens an app.** A result describes what the association file permits.
Opening also depends on the app being installed, on its Associated Domains entitlement naming the
domain, and on what the device has cached. No file can tell you those, and this does not pretend
to.

Every answer is scoped that way on purpose — see
[the library's parity notes](https://github.com/sergii-ziborov/blazingly-aasa/blob/main/docs/parity.md)
for which behaviours are verified against Apple's own `swcutil` (139 of 140 conformance cases) and
which remain a reading of an underspecified sentence.

## Safety of the network side

The caller names a **domain**, never a URL, and only the three documented locations are ever
requested. Three layers stop a tool call being steered at an internal address:

**The name.** IP literals, `localhost`, `.local`, and anything not fully qualified are refused
before a socket is opened.

**The address.** A name is not an address: `evil.example` is a well-formed public hostname that can
resolve to `127.0.0.1`, `10.0.0.1`, or the cloud metadata endpoint at `169.254.169.254`. DNS
answers are filtered against loopback, private, link-local, carrier-grade NAT, multicast, and
reserved ranges — IPv6 included, and IPv4-mapped IPv6 unwrapped, since `::ffff:127.0.0.1` reaches
loopback. The filtering happens *inside* the resolver, so the client connects to exactly the
addresses that were vetted and there is no second lookup for DNS rebinding to poison.

**The route.** Proxies are disabled. `ureq` reads `HTTPS_PROXY` from the environment by default,
and a proxied request resolves the name at the proxy rather than here — which would route around
both layers above.

Redirects are never followed, which is both Apple's requirement and one less way to reach somewhere
unnamed. Requests are bounded by a timeout and a 128 KiB body ceiling, adjustable with `--timeout`
and `--max-bytes`.

This matters more here than for an ordinary CLI: an MCP server's arguments can come from a
repository, an issue, or a README that an agent was asked to act on.

## Dependencies

`blazingly-aasa` for semantics, [`mcport`](https://crates.io/crates/mcport) for MCP, `ureq` for
HTTPS, `serde`. No async runtime anywhere in the tree — `mcport` and `blazingly-json` are
Tokio-free by design, and `ureq` is a blocking client, so the whole thing is threads and syscalls.

MSRV is 1.85, not the library's 1.78: the rustls stack `ureq` pulls in reaches `zeroize 1.9`, which
is edition 2024. That is verified against a 1.85 toolchain in CI rather than assumed.

## Development

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo +1.85 check --lib
```

Tests drive the real tool catalog over in-memory streams, so a schema or handler change is caught
here rather than by a client one rejected call at a time. Nothing in the test suite touches the
network.

## License

MIT. See [LICENSE](LICENSE).
