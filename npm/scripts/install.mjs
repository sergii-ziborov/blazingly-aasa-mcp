// Downloads the release binary for this platform.
//
// Runs as postinstall. It must never fail the install: a machine without network, or behind a
// proxy, should still end up with a working `npm install` and a clear message when the command is
// actually run. The launcher re-checks and explains.

import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

import { binaryName, target, unsupportedMessage } from "./platform.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));

const REPO = "sergii-ziborov/blazingly-aasa-mcp";
const triple = target();

if (!triple) {
  console.warn(unsupportedMessage());
  process.exit(0);
}

const name = `blazingly-aasa-${triple}`;
const base = `https://github.com/${REPO}/releases/download/v${pkg.version}`;
const binDir = join(root, "bin");
const binPath = join(binDir, binaryName());

async function fetchOrThrow(url) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText} for ${url}`);
  }
  return response;
}

try {
  // The checksum is published beside the archive and is what makes downloading a binary at install
  // time defensible at all.
  const expected = (await (await fetchOrThrow(`${base}/${name}.tar.gz.sha256`)).text())
    .trim()
    .split(/\s+/)[0];

  const staging = await mkdtemp(join(tmpdir(), "blazingly-aasa-"));
  const archive = join(staging, `${name}.tar.gz`);
  const response = await fetchOrThrow(`${base}/${name}.tar.gz`);
  await pipeline(Readable.fromWeb(response.body), createWriteStream(archive));

  const actual = createHash("sha256").update(await readFile(archive)).digest("hex");
  if (actual !== expected) {
    throw new Error(`checksum mismatch: expected ${expected}, got ${actual}`);
  }

  // tar is present on every platform this package supports, Windows included since 1809.
  execFileSync("tar", ["-xzf", archive, "-C", staging], { stdio: "ignore" });

  await mkdir(binDir, { recursive: true });
  await rename(join(staging, name, binaryName()), binPath);
  if (process.platform !== "win32") {
    await chmod(binPath, 0o755);
  }
  await writeFile(join(binDir, ".version"), pkg.version);
  await rm(staging, { recursive: true, force: true });

  console.log(`blazingly-aasa ${pkg.version} installed for ${triple}`);
} catch (error) {
  // Deliberately not a failure. Say what happened and how to proceed.
  console.warn(
    [
      `blazingly-aasa: could not download the ${triple} binary for v${pkg.version}.`,
      `  ${error.message}`,
      "",
      "The command will explain this again when run. To install from source instead:",
      "  cargo install blazingly-aasa-mcp",
    ].join("\n"),
  );
}
