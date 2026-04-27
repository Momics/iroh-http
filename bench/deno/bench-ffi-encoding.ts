/**
 * FFI boundary encoding benchmark — JSON vs MessagePack vs raw struct (#130).
 *
 * Measures serialization/deserialization overhead for representative payloads
 * that cross the Deno FFI boundary:
 *   - Fetch response metadata (status, headers, bodyHandle, url)
 *   - Serve request event (method, url, headers, handles)
 *   - Small dispatch responses (ok/err envelopes)
 *
 * Run: deno bench --allow-all bench/deno/bench-ffi-encoding.ts
 */

// ── MessagePack (minimal encode/decode for benchmarking) ──────────────────────
// Using @std/msgpack from JSR for a fair comparison against JSON.

import { encode as msgpackEncode, decode as msgpackDecode } from "jsr:@std/msgpack@1";

const enc = new TextEncoder();
const dec = new TextDecoder();

// ── Representative payloads ───────────────────────────────────────────────────

/** Typical fetch response metadata (what rawFetch returns) */
const FETCH_RESPONSE = {
  ok: {
    status: 200,
    headers: [
      ["content-type", "application/json"],
      ["content-length", "1234"],
      ["x-request-id", "abc123def456"],
    ],
    bodyHandle: 42,
    url: "httpi://7bqhyr4mdk3taenl6ap4ufcu4xnaqnph4iagdtqcjjx5bftc64fa/api/v1/data",
    inlineBody: null,
  },
};

/** Fetch response with inline body (small response optimization from #126) */
const FETCH_RESPONSE_INLINE = {
  ok: {
    status: 200,
    headers: [
      ["content-type", "application/json"],
      ["content-length", "11"],
    ],
    bodyHandle: 0,
    url: "httpi://7bqhyr4mdk3taenl6ap4ufcu4xnaqnph4iagdtqcjjx5bftc64fa/ping",
    inlineBody: "eyJvayI6dHJ1ZX0=", // {"ok":true} base64
  },
};

/** Serve request event (incoming request metadata) */
const SERVE_REQUEST = {
  reqHandle: 7,
  reqBodyHandle: 8,
  resBodyHandle: 9,
  isBidi: false,
  method: "POST",
  url: "httpi://7bqhyr4mdk3taenl6ap4ufcu4xnaqnph4iagdtqcjjx5bftc64fa/api/data",
  headers: [
    ["content-type", "application/json"],
    ["content-length", "256"],
    ["authorization", "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"],
    ["x-request-id", "req-abc-123"],
  ],
  remoteNodeId: "7bqhyr4mdk3taenl6ap4ufcu4xnaqnph4iagdtqcjjx5bftc64fa",
};

/** Small OK envelope (e.g. sendChunk, finishBody) */
const SMALL_OK = { ok: {} };

/** Error envelope */
const ERROR_RESPONSE = {
  err: {
    code: "CONNECTION_CLOSED",
    message: "peer disconnected before response was complete",
  },
};

/** Outbound fetch request payload */
const FETCH_REQUEST = {
  endpointHandle: 1,
  nodeId: "7bqhyr4mdk3taenl6ap4ufcu4xnaqnph4iagdtqcjjx5bftc64fa",
  url: "httpi://7bqhyr4mdk3taenl6ap4ufcu4xnaqnph4iagdtqcjjx5bftc64fa/api/v1/resource?page=2&limit=50",
  method: "GET",
  headers: [
    ["accept", "application/json"],
    ["x-custom-header", "some-value"],
  ],
  reqBodyHandle: null,
  fetchToken: 12345,
  directAddrs: ["127.0.0.1:12345", "192.168.1.100:54321"],
};

// ── Pre-encode for decode benchmarks ──────────────────────────────────────────

const FETCH_RESPONSE_JSON = enc.encode(JSON.stringify(FETCH_RESPONSE));
const FETCH_RESPONSE_MSGPACK = msgpackEncode(FETCH_RESPONSE);
const FETCH_RESPONSE_INLINE_JSON = enc.encode(JSON.stringify(FETCH_RESPONSE_INLINE));
const FETCH_RESPONSE_INLINE_MSGPACK = msgpackEncode(FETCH_RESPONSE_INLINE);
const SERVE_REQUEST_JSON = enc.encode(JSON.stringify(SERVE_REQUEST));
const SERVE_REQUEST_MSGPACK = msgpackEncode(SERVE_REQUEST);
const SMALL_OK_JSON = enc.encode(JSON.stringify(SMALL_OK));
const SMALL_OK_MSGPACK = msgpackEncode(SMALL_OK);
const FETCH_REQUEST_JSON = enc.encode(JSON.stringify(FETCH_REQUEST));
const FETCH_REQUEST_MSGPACK = msgpackEncode(FETCH_REQUEST);

// ── Size comparison ───────────────────────────────────────────────────────────

console.log("\n=== Payload sizes (bytes) ===");
console.log(`${"Payload".padEnd(30)} ${"JSON".padStart(6)} ${"MsgPack".padStart(8)} ${"Ratio".padStart(7)}`);
console.log("-".repeat(55));
for (const [name, json, mp] of [
  ["Fetch response", FETCH_RESPONSE_JSON, FETCH_RESPONSE_MSGPACK],
  ["Fetch response (inline)", FETCH_RESPONSE_INLINE_JSON, FETCH_RESPONSE_INLINE_MSGPACK],
  ["Serve request", SERVE_REQUEST_JSON, SERVE_REQUEST_MSGPACK],
  ["Small OK", SMALL_OK_JSON, SMALL_OK_MSGPACK],
  ["Fetch request", FETCH_REQUEST_JSON, FETCH_REQUEST_MSGPACK],
] as const) {
  const ratio = (mp.byteLength / json.byteLength * 100).toFixed(0);
  console.log(
    `${(name as string).padEnd(30)} ${String(json.byteLength).padStart(6)} ${String(mp.byteLength).padStart(8)} ${(ratio + "%").padStart(7)}`,
  );
}
console.log("");

// ── Benchmarks: Encode (Rust → bytes) ─────────────────────────────────────────

Deno.bench("encode/fetch-response/json", () => {
  enc.encode(JSON.stringify(FETCH_RESPONSE));
});

Deno.bench("encode/fetch-response/msgpack", () => {
  msgpackEncode(FETCH_RESPONSE);
});

Deno.bench("encode/fetch-response-inline/json", () => {
  enc.encode(JSON.stringify(FETCH_RESPONSE_INLINE));
});

Deno.bench("encode/fetch-response-inline/msgpack", () => {
  msgpackEncode(FETCH_RESPONSE_INLINE);
});

Deno.bench("encode/serve-request/json", () => {
  enc.encode(JSON.stringify(SERVE_REQUEST));
});

Deno.bench("encode/serve-request/msgpack", () => {
  msgpackEncode(SERVE_REQUEST);
});

Deno.bench("encode/small-ok/json", () => {
  enc.encode(JSON.stringify(SMALL_OK));
});

Deno.bench("encode/small-ok/msgpack", () => {
  msgpackEncode(SMALL_OK);
});

Deno.bench("encode/fetch-request/json", () => {
  enc.encode(JSON.stringify(FETCH_REQUEST));
});

Deno.bench("encode/fetch-request/msgpack", () => {
  msgpackEncode(FETCH_REQUEST);
});

// ── Benchmarks: Decode (bytes → JS object) ────────────────────────────────────

Deno.bench("decode/fetch-response/json", () => {
  JSON.parse(dec.decode(FETCH_RESPONSE_JSON));
});

Deno.bench("decode/fetch-response/msgpack", () => {
  msgpackDecode(FETCH_RESPONSE_MSGPACK);
});

Deno.bench("decode/fetch-response-inline/json", () => {
  JSON.parse(dec.decode(FETCH_RESPONSE_INLINE_JSON));
});

Deno.bench("decode/fetch-response-inline/msgpack", () => {
  msgpackDecode(FETCH_RESPONSE_INLINE_MSGPACK);
});

Deno.bench("decode/serve-request/json", () => {
  JSON.parse(dec.decode(SERVE_REQUEST_JSON));
});

Deno.bench("decode/serve-request/msgpack", () => {
  msgpackDecode(SERVE_REQUEST_MSGPACK);
});

Deno.bench("decode/small-ok/json", () => {
  JSON.parse(dec.decode(SMALL_OK_JSON));
});

Deno.bench("decode/small-ok/msgpack", () => {
  msgpackDecode(SMALL_OK_MSGPACK);
});

Deno.bench("decode/fetch-request/json", () => {
  JSON.parse(dec.decode(FETCH_REQUEST_JSON));
});

Deno.bench("decode/fetch-request/msgpack", () => {
  msgpackDecode(FETCH_REQUEST_MSGPACK);
});

// ── Benchmarks: Full round-trip (encode + decode) ─────────────────────────────

Deno.bench("roundtrip/fetch-response/json", () => {
  const bytes = enc.encode(JSON.stringify(FETCH_RESPONSE));
  JSON.parse(dec.decode(bytes));
});

Deno.bench("roundtrip/fetch-response/msgpack", () => {
  const bytes = msgpackEncode(FETCH_RESPONSE);
  msgpackDecode(bytes);
});

Deno.bench("roundtrip/serve-request/json", () => {
  const bytes = enc.encode(JSON.stringify(SERVE_REQUEST));
  JSON.parse(dec.decode(bytes));
});

Deno.bench("roundtrip/serve-request/msgpack", () => {
  const bytes = msgpackEncode(SERVE_REQUEST);
  msgpackDecode(bytes);
});
