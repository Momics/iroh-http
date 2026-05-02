#!/usr/bin/env node
// Verify — and auto-repair — bare {"optional": true} stubs in
// package-lock.json.  Such entries crash `npm ci` on other platforms with
// `TypeError: Invalid Version:` from semver.
//
// Why they appear: `npm install --package-lock-only` cannot resolve
// platform-specific optional deps whose version hasn't been published to npm
// yet (the chicken-and-egg of napi-rs version bumps).  It writes bare stubs
// for every platform it can't install locally.
//
// Fix: for each bare stub whose name matches a local platform package under
// packages/iroh-http-node/npm/, fill in the metadata from the local
// package.json.  `resolved` and `integrity` are omitted — CI publishes the
// tarballs and `npm ci` fetches the real ones.  The key requirement is that
// `version` is present so semver doesn't choke.
//
// See issue #154.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const lockPath = resolve(root, "package-lock.json");
const lock = JSON.parse(readFileSync(lockPath, "utf8"));

let repaired = 0;
const unfixable = [];

for (const [pkgPath, entry] of Object.entries(lock.packages ?? {})) {
  if (!pkgPath) continue; // root entry
  if (entry.link || entry.workspace) continue;
  if (typeof entry.version === "string" && entry.version.length > 0) continue;

  // Try to auto-repair from local platform package.json.
  // Stubs:  packages/iroh-http-node/node_modules/@momics/iroh-http-node-darwin-arm64
  // Source: packages/iroh-http-node/npm/darwin-arm64/package.json
  const match = pkgPath.match(
    /packages\/iroh-http-node\/node_modules\/@momics\/iroh-http-node-(.+)$/,
  );
  if (!match) {
    unfixable.push(pkgPath);
    continue;
  }

  const localPkgPath = join(root, "packages", "iroh-http-node", "npm", match[1], "package.json");
  if (!existsSync(localPkgPath)) {
    unfixable.push(pkgPath);
    continue;
  }

  const local = JSON.parse(readFileSync(localPkgPath, "utf8"));
  entry.version = local.version;
  if (local.license) entry.license = local.license;
  if (local.cpu) entry.cpu = local.cpu;
  if (local.os) entry.os = local.os;
  entry.optional = true;
  repaired++;
  console.log(`  ✓ repaired ${pkgPath} → ${local.version}`);
}

if (repaired > 0) {
  writeFileSync(lockPath, JSON.stringify(lock, null, 2) + "\n");
}

if (unfixable.length > 0) {
  console.error(
    "package-lock.json contains entries with no version that could not be auto-repaired:",
  );
  for (const p of unfixable) console.error(`  ${p}`);
  process.exit(1);
}

const total = Object.keys(lock.packages ?? {}).length;
console.log(`package-lock.json: ${total} entries, all valid${repaired > 0 ? ` (${repaired} repaired)` : ""}.`);
