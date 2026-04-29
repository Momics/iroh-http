#!/usr/bin/env node
// Verify that no entry in package-lock.json is a bare {"optional": true}
// stub without a version. Such entries crash `npm ci` on platforms that did
// not produce them, with `TypeError: Invalid Version:` from semver.
//
// Root cause: when `npm install --package-lock-only --omit=optional` runs,
// platform-specific native binary optional deps (e.g. iroh-http-node-darwin-x64
// on a macOS arm64 host) get written as `{"optional": true}` with no version
// field. See issue #154.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const lockPath = resolve(here, "..", "package-lock.json");
const lock = JSON.parse(readFileSync(lockPath, "utf8"));

const broken = [];
for (const [pkgPath, entry] of Object.entries(lock.packages ?? {})) {
  if (!pkgPath) continue; // root entry
  if (entry.link || entry.workspace) continue;
  if (typeof entry.version === "string" && entry.version.length > 0) continue;
  broken.push(pkgPath);
}

if (broken.length > 0) {
  console.error(
    "package-lock.json contains entries with no version (will crash `npm ci` on other platforms):",
  );
  for (const p of broken) console.error(`  ${p}`);
  console.error(
    "\nRegenerate the lockfile with all platform optional deps available, or",
  );
  console.error("rerun scripts/version.sh which now resolves them explicitly.");
  process.exit(1);
}

console.log(`package-lock.json: ${Object.keys(lock.packages ?? {}).length} entries, all valid.`);
