# blazingly-aasa-mcp

MCP server and CLI for debugging Apple Universal Links. Answers "why doesn't this link open my
app?" in one call.

```bash
npx blazingly-aasa-mcp check example.com "https://example.com/buy/42" \
  --app ABCDE12345.com.example.app
```

As an MCP server, point your client at it with no arguments:

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

This package is a launcher. On install it downloads the Rust binary built for your platform from
the matching GitHub release and verifies its SHA-256 before use. If the download fails — no
network, a proxy — the install still succeeds and the command explains what happened when you run
it. `cargo install blazingly-aasa-mcp` is the source route.

Prebuilt for macOS (arm64, x64), Linux (x64, arm64), and Windows (x64).

Full documentation: **https://github.com/sergii-ziborov/blazingly-aasa-mcp**
