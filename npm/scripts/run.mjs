#!/usr/bin/env node
// Runs the downloaded binary, passing stdio straight through.
//
// MCP clients launch this over stdio and expect JSON-RPC frames on stdout and nothing else, so
// every diagnostic here goes to stderr.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { binaryName, target, unsupportedMessage } from "./platform.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const binPath = join(root, "bin", binaryName());

if (!existsSync(binPath)) {
  const triple = target();
  console.error(
    triple
      ? [
          `blazingly-aasa: the ${triple} binary for v${pkg.version} is not installed.`,
          "",
          "The postinstall download did not complete -- often no network, or a proxy.",
          "Reinstall to retry, or install from source:",
          "  cargo install blazingly-aasa-mcp",
        ].join("\n")
      : unsupportedMessage(),
  );
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`blazingly-aasa: could not run ${binPath}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
