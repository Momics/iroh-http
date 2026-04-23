/**
 * Client-only compliance runner — Node.js
 *
 * Reads a server public key from argv and runs all cases against it.
 * Used for cross-runtime testing (e.g., Deno server ↔ Node client).
 *
 * Usage:
 *   node tests/client-node.mjs <server-public-key> [--filter <pattern>] [--bail]
 */

import { readFile } from "node:fs/promises";
import { createNode } from "../../packages/iroh-http-node/lib.js";
import { assertResponse } from "./assertions.mjs";

const serverAddr = process.argv[2];
if (!serverAddr) {
  console.error("Usage: node tests/client-node.mjs <server-public-key>");
  process.exit(1);
}

const args = process.argv.slice(3);
const filterPattern = getArg(args, "--filter");
const bail = args.includes("--bail");
const verbose = args.includes("--verbose");
const timeout = parseInt(getArg(args, "--timeout") ?? "30000", 10);

function getArg(a, flag) {
  const idx = a.indexOf(flag);
  return idx !== -1 && idx + 1 < a.length ? a[idx + 1] : null;
}

const casesRaw = await readFile(new URL("./cases.json", import.meta.url), "utf-8");
const allCases = JSON.parse(casesRaw).filter((c) => c.id);
let cases = filterPattern
  ? allCases.filter((c) => c.id.includes(filterPattern))
  : allCases;

const client = await createNode();
console.log(`Client public key: ${client.publicKey.toString()}`);
console.log(`Target server: ${serverAddr}\n`);

function buildBody(bodySpec) {
  if (bodySpec === null || bodySpec === undefined) return undefined;
  if (typeof bodySpec === "string") return bodySpec;
  if (typeof bodySpec === "object" && bodySpec.fill)
    return new Uint8Array(bodySpec.fill);
  return undefined;
}

function buildHeaders(headersSpec) {
  const h = {};
  if (!headersSpec) return h;
  for (const [k, v] of Object.entries(headersSpec)) {
    h[k] = typeof v === "object" && v.fill ? "x".repeat(v.fill) : v;
  }
  return h;
}

async function runSingleRequest(req) {
  const body = buildBody(req.body);
  const headers = buildHeaders(req.headers);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeout);
  try {
    const res = await client.fetch(serverAddr, req.path, {
      method: req.method,
      headers,
      body,
      signal: controller.signal,
    });
    const buf = await res.arrayBuffer();
    return {
      res,
      bodyText: new TextDecoder().decode(buf),
      bodyLength: buf.byteLength,
    };
  } finally {
    clearTimeout(timer);
  }
}

let passed = 0,
  failed = 0;
const failedCases = [];
const startTime = Date.now();

for (const tc of cases) {
  if (tc.requests || tc.sequential || tc.concurrent > 1 || tc.repeat > 1) {
    // Skip complex tests in cross-runtime mode for now
    continue;
  }

  const label = `[${tc.id}] ${tc.description ?? ""}`;
  try {
    const { res, bodyText, bodyLength } = await runSingleRequest(tc.request);
    const result = assertResponse(tc, res, bodyText, bodyLength);
    if (result.pass) {
      passed++;
      if (verbose) console.log(`  ✓ ${label}`);
    } else {
      failed++;
      failedCases.push(tc.id);
      console.log(`  ✗ ${label}`);
      result.failures.forEach((f) => console.log(`      ${f}`));
      if (bail) break;
    }
  } catch (err) {
    failed++;
    failedCases.push(tc.id);
    console.log(`  ✗ ${label} — ${err.message}`);
    if (bail) break;
  }
}

const elapsed = ((Date.now() - startTime) / 1000).toFixed(2);
console.log("\n" + "─".repeat(60));
console.log(`Results: ${passed} passed, ${failed} failed (${elapsed}s)`);
if (failedCases.length > 0) {
  console.log("\nFailed cases:");
  failedCases.forEach((id) => console.log(`  - ${id}`));
}

try { client.shutdown?.(); } catch {}
process.exit(failed > 0 ? 1 : 0);
