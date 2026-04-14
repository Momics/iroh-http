/**
 * Shared utility functions for iroh-http adapters.
 *
 * Centralises helpers that were previously duplicated across Node, Deno,
 * and Tauri adapter code.
 */

import type { RelayMode } from "./bridge.js";

// ── Relay mode normalisation ──────────────────────────────────────────────────

export interface NormalisedRelay {
  relayMode: string | undefined;
  relays: string[] | null;
  disableNetworking: boolean;
}

/**
 * Normalise a {@link RelayMode} value into the shape expected by the Rust FFI
 * layer.
 */
export function normaliseRelayMode(mode?: RelayMode): NormalisedRelay {
  if (mode === "disabled") {
    return { relayMode: "disabled", relays: [], disableNetworking: true };
  }
  if (mode === "default" || mode === undefined) {
    return { relayMode: undefined, relays: null, disableNetworking: false };
  }
  if (mode === "staging") {
    return { relayMode: "staging", relays: null, disableNetworking: false };
  }
  if (Array.isArray(mode)) {
    return { relayMode: "custom", relays: mode, disableNetworking: false };
  }
  return { relayMode: "custom", relays: [mode], disableNetworking: false };
}

// ── Base64 encoding ───────────────────────────────────────────────────────────

/**
 * Encode a `Uint8Array` to a base64 string.
 *
 * Uses chunked `String.fromCharCode` to avoid call-stack limits on large
 * buffers.
 */
export function encodeBase64(u8: Uint8Array): string {
  const CHUNK = 0x8000; // 32 KB — safe for String.fromCharCode spread
  const parts: string[] = [];
  for (let i = 0; i < u8.length; i += CHUNK) {
    parts.push(String.fromCharCode(...u8.subarray(i, i + CHUNK)));
  }
  return btoa(parts.join(""));
}

/** Decode a base64 string to a `Uint8Array`. */
export function decodeBase64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
