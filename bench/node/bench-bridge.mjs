/**
 * iroh-http Node.js FFI bridge benchmark — isolates Rust↔JS data crossing.
 *
 * Measures the overhead of sendChunk/nextChunk across the napi-rs boundary
 * WITHOUT any QUIC transport — uses a local body channel loopback.
 *
 * Scenarios:
 *   1. sendChunk + nextChunk round-trip (1 KB, 64 KB, 1 MB)
 *   2. Streaming throughput: 1 MB as 16 × 64 KB chunks
 *   3. allocBodyWriter + finishBody lifecycle
 *
 * Run: node bench/node/bench-bridge.mjs
 */

import { bench, group, run } from "mitata";
import {
  createEndpoint,
  jsAllocBodyWriter,
  jsSendChunk,
  jsNextChunk,
  jsFinishBody,
  closeEndpoint,
} from "../../packages/iroh-http-node/index.js";

// ── Payloads ──────────────────────────────────────────────────────────────────

function payload(size) {
  return new Uint8Array(size).fill(0x42);
}

const PAYLOAD_1K = payload(1_024);
const PAYLOAD_64K = payload(64 * 1_024);
const PAYLOAD_1M = payload(1_024 * 1_024);

// ── Setup — use raw napi bindings, no IrohNode wrapper ────────────────────────

const info = await createEndpoint({ disableNetworking: true, bindAddrs: ["127.0.0.1:0"] });
const eh = info.endpointHandle;

try {
  // ── 1. Round-trip: allocBodyWriter → sendChunk → finishBody ────────────────

  for (const [label, data] of [
    ["1kb", PAYLOAD_1K],
    ["64kb", PAYLOAD_64K],
    ["1mb", PAYLOAD_1M],
  ]) {
    group(`bridge-roundtrip-${label}`, () => {
      bench(`bridge-roundtrip-${label}`, async () => {
        const writerHandle = jsAllocBodyWriter(eh);
        await jsSendChunk(eh, BigInt(writerHandle), data);
        jsFinishBody(eh, BigInt(writerHandle));
      });
    });
  }

  // ── 2. Streaming: 1 MB as 16 × 64 KB chunks ──────────────────────────────

  group("bridge-streaming-1mb", () => {
    bench("bridge-streaming-1mb", async () => {
      const writerHandle = jsAllocBodyWriter(eh);
      const bh = BigInt(writerHandle);
      for (let i = 0; i < 16; i++) {
        await jsSendChunk(eh, bh, PAYLOAD_64K);
      }
      jsFinishBody(eh, bh);
    });
  });

  // ── 3. Handle lifecycle: alloc + finish ───────────────────────────────────

  group("bridge-alloc-free", () => {
    bench("bridge-alloc-free", () => {
      const writerHandle = jsAllocBodyWriter(eh);
      jsFinishBody(eh, BigInt(writerHandle));
    });
  });

  await run();
} finally {
  await closeEndpoint(eh).catch(() => {});
}
