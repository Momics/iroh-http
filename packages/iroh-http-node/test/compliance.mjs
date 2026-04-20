/**
 * Cross-runtime HTTP compliance tests — Node.js adapter.
 *
 * Reads the shared fixture file and runs every case against the Node napi-rs
 * adapter.  The runner logic lives in tests/http-compliance/runner.ts and is
 * shared with the Deno and Tauri adapters.
 *
 * Run:  node test/compliance.mjs
 * Prerequisites: lib.js must be compiled (`tsc` in this package directory).
 */

import { createNode } from "../lib.js";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

// ── Load compliance fixtures ──────────────────────────────────────────────────

const __dir = dirname(fileURLToPath(import.meta.url));
const casesPath = join(__dir, "../../../tests/http-compliance/cases.json");
const cases = JSON.parse(readFileSync(casesPath, "utf8"));

// ── Inline echo server (mirrors runner.ts handleComplianceRequest) ─────────────────────

function handleComplianceRequest(req) {
  const url = new URL(req.url);
  const parts = url.pathname.split("/").filter(Boolean);

  if (parts[0] === "status" && parts[1]) {
    const code = parseInt(parts[1], 10);
    return new Response(null, { status: isNaN(code) ? 400 : code });
  }
  if (parts[0] === "echo" && parts.length === 1) {
    return new Response(req.body, { status: 200 });
  }
  if (parts[0] === "echo-path") {
    return new Response(url.pathname, { status: 200 });
  }
  if (parts[0] === "echo-method") {
    return new Response(req.method, { status: 200 });
  }
  if (parts[0] === "echo-length") {
    return req.arrayBuffer().then(
      (buf) => new Response(String(buf.byteLength), { status: 200 }),
    );
  }
  if (parts[0] === "header" && parts[1]) {
    return new Response(req.headers.get(parts[1]) ?? "", { status: 200 });
  }
  if (parts[0] === "set-header" && parts[1] && parts[2]) {
    return new Response(null, {
      status: 200,
      headers: { [parts[1]]: parts[2] },
    });
  }
  if (parts[0] === "set-headers" && parts[1]) {
    const n = parseInt(parts[1], 10);
    if (!isNaN(n) && n >= 0) {
      const hdrs = {};
      for (let i = 0; i < n; i++) hdrs[`x-h-${i}`] = `v${i}`;
      return new Response(null, { status: 200, headers: hdrs });
    }
  }
  if (parts[0] === "echo-query") {
    return new Response(url.search, { status: 200 });
  }
  if (parts[0] === "echo-header-count") {
    const count = [...req.headers].length;
    return new Response(String(count), { status: 200 });
  }
  if (parts[0] === "stream" && parts[1]) {
    const n = parseInt(parts[1], 10);
    if (!isNaN(n) && n >= 0) return new Response(new Uint8Array(n), { status: 200 });
  }
  return new Response("not found", { status: 404 });
}

// ── Assertion helpers ─────────────────────────────────────────────────────────

async function assertResponse(resp, expected) {
  if (resp.status !== expected.status)
    return `status: got ${resp.status}, want ${expected.status}`;
  if (expected.bodyExact !== undefined) {
    const text = await resp.text();
    return text !== expected.bodyExact
      ? `body: got ${JSON.stringify(text)}, want ${JSON.stringify(expected.bodyExact)}`
      : null;
  }
  if (expected.bodyNot !== undefined) {
    const text = await resp.text();
    return text === expected.bodyNot
      ? `body must not equal ${JSON.stringify(expected.bodyNot)}`
      : null;
  }
  if (expected.bodyNotEmpty) {
    const text = await resp.text();
    return text ? null : "body: expected non-empty";
  }
  if (expected.bodyLengthExact !== undefined) {
    const buf = await resp.arrayBuffer();
    return buf.byteLength !== expected.bodyLengthExact
      ? `body length: got ${buf.byteLength}, want ${expected.bodyLengthExact}`
      : null;
  }
  if (expected.headers) {
    for (const [k, v] of Object.entries(expected.headers)) {
      const actual = resp.headers.get(k);
      if (actual !== v)
        return `header ${k}: got ${JSON.stringify(actual)}, want ${JSON.stringify(v)}`;
    }
    await resp.body?.cancel();
    return null;
  }
  await resp.body?.cancel();
  return null;
}

function buildBody(body) {
  if (body === null) return null;
  if (typeof body === "string") return body;
  return new Uint8Array(body.fill);
}

// ── Runner ────────────────────────────────────────────────────────────────────

async function main() {
  console.log("iroh-http compliance tests — Node.js adapter");
  console.log(`  ${cases.length} cases\n`);

  const server = await createNode();
  const client = await createNode();

  let passed = 0;
  let failed = 0;
  const failures = [];

  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, handleComplianceRequest);

    for (const c of cases) {
      let resp;
      try {
        resp = await client.fetch(serverId, `httpi://compliance.test${c.request.path}`, {
          method: c.request.method,
          headers: c.request.headers,
          body: buildBody(c.request.body),
          directAddrs: serverAddrs,
        });
      } catch (e) {
        const reason = `fetch threw: ${e?.message ?? e}`;
        failed++;
        failures.push({ id: c.id, reason });
        console.log(`  FAIL  ${c.id}: ${reason}`);
        continue;
      }

      const failure = await assertResponse(resp, c.response);
      if (failure) {
        failed++;
        failures.push({ id: c.id, reason: failure });
        console.log(`  FAIL  ${c.id}: ${failure}`);
      } else {
        passed++;
        console.log(`  pass  ${c.id}`);
      }
    }

    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

main().catch((e) => {
  console.error("Fatal:", e);
  process.exit(1);
});
