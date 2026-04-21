/**
 * Shared utility functions for iroh-http adapters.
 *
 * Centralises helpers that were previously duplicated across Node, Deno,
 * and Tauri adapter code.
 */
import type { RelayMode } from "./bridge.js";
export interface NormalisedRelay {
    relayMode: string | undefined;
    relays: string[] | null;
    disableNetworking: boolean;
}
/**
 * Normalise a {@link RelayMode} value into the shape expected by the Rust FFI
 * layer.
 */
export declare function normaliseRelayMode(mode?: RelayMode): NormalisedRelay;
/**
 * Encode a `Uint8Array` to a base64 string.
 *
 * Uses chunked `String.fromCharCode` to avoid call-stack limits on large
 * buffers.
 */
export declare function encodeBase64(u8: Uint8Array): string;
/** Decode a base64 string to a `Uint8Array`. */
export declare function decodeBase64(s: string): Uint8Array;
//# sourceMappingURL=utils.d.ts.map