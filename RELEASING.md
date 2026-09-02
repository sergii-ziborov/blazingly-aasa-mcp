# Releasing

This crate depends on `blazingly-aasa`, and **that crate must be on crates.io first**. The manifest
carries both a `version` and a pinned `git` revision: local builds use the revision, so the binary
always builds against known semantics, while `cargo publish` drops the git source and depends on
the published version. `cargo package` refuses to run until the version exists, which is the
intended guard.

See [the library's RELEASING.md](https://github.com/sergii-ziborov/blazingly-aasa/blob/main/RELEASING.md)
for step one.

## Before releasing

```bash
cargo fmt --all --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo +1.85 check --lib --bins
```

No test reaches the network. Check the live paths by hand against a domain that actually serves an
association file:

```bash
cargo run -- fetch www.apple.com
cargo run -- compare www.apple.com
```

## Publishing

One-time: add a crates.io API token as the repository secret `CARGO_REGISTRY_TOKEN`, under an
environment named `crates-io`.

```bash
cargo update -p blazingly-aasa    # resolve the published version
cargo package                     # now succeeds
cargo publish --dry-run
git tag v0.1.0 && git push origin v0.1.0
```

Once the library is on crates.io, the `git` and `rev` keys can be dropped from the dependency
entirely, leaving a plain version requirement. Keep them only while tracking an unreleased change
in the engine.

## Versioning

The engine's matching behaviour is this crate's behaviour. When `blazingly-aasa` changes what a
rule decides, say so here too — someone running `blazingly-aasa validate` in CI will see different
output, and the version bump is the only warning they get.
