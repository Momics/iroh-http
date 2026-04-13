/**
 * PublicKey and SecretKey — typed wrappers around Ed25519 key material.
 *
 * Uses `@scure/base` for RFC 4648 base32 encoding and `crypto.subtle`
 * for Ed25519 signature verification (Node 18+, Deno, modern browsers).
 */

import { base32 } from "@scure/base";

// ── Base32 codec ──────────────────────────────────────────────────────────────
//
// Iroh node IDs are RFC 4648 base32 (a-z2-7, no padding, lowercase).
// @scure/base's `base32` codec uses padding; we strip it on encode and re-add
// it on decode so the library stays happy while wire format stays unpadded.

const base32Encode = (b: Uint8Array): string =>
  base32.encode(b).replace(/=+$/, "").toLowerCase();
const base32Decode = (s: string): Uint8Array => {
  const upper = s.toUpperCase();
  // RFC 4648 §6: base32 groups are 5 chars; pad to next multiple of 8.
  const pad = (8 - (upper.length % 8)) % 8;
  return base32.decode(upper + "=".repeat(pad));
};

// ── Web Crypto algorithm descriptor ──────────────────────────────────────────

const ED25519: EcKeyAlgorithm = {
  name: "Ed25519",
} as unknown as EcKeyAlgorithm;

// ── PublicKey ─────────────────────────────────────────────────────────────────

/**
 * A node's public identity — its stable network address.
 *
 * Immutable. Can be created from a base32 string or raw bytes, and can
 * be used to verify Ed25519 signatures and compare identities.
 *
 * @example
 * ```ts
 * const pk = PublicKey.fromString(nodeIdString);
 * console.log(pk.toString()); // base32 node ID
 * console.log(pk.bytes);      // Uint8Array(32)
 *
 * if (pk.equals(otherKey)) { /* same node *\/ }
 * ```
 */
export class PublicKey {
  readonly #bytes: Uint8Array<ArrayBuffer>;
  // Lazy-cached base32 string.
  #str: string | null = null;

  private constructor(bytes: Uint8Array) {
    if (bytes.length !== 32) {
      throw new TypeError(`PublicKey must be 32 bytes, got ${bytes.length}`);
    }
    this.#bytes = bytes.slice();
  }

  /** Copy of the raw 32-byte key material. */
  get bytes(): Uint8Array {
    return this.#bytes.slice();
  }

  /** Lowercase base32 representation (the "node ID"). */
  toString(): string {
    if (this.#str === null) this.#str = base32Encode(this.#bytes);
    return this.#str;
  }

  /** `true` when both keys contain identical byte sequences. */
  equals(other: PublicKey): boolean {
    if (this.#bytes.length !== other.#bytes.length) return false;
    let diff = 0;
    for (let i = 0; i < this.#bytes.length; i++) {
      diff |= this.#bytes[i] ^ other.#bytes[i];
    }
    return diff === 0;
  }

  /**
   * Parse from a base32 string (case-insensitive).
   * Throws `TypeError` if the string is not a valid 32-byte base32 key.
   */
  static fromString(s: string): PublicKey {
    return new PublicKey(base32Decode(s));
  }

  /** Construct from 32 raw bytes. Copies the input. */
  static fromBytes(bytes: Uint8Array): PublicKey {
    return new PublicKey(bytes);
  }

  /**
   * Verify an Ed25519 signature over `data`.
   * Returns `false` rather than throwing when the signature is invalid.
   */
  async verify(data: Uint8Array, signature: Uint8Array): Promise<boolean> {
    try {
      const key = await crypto.subtle.importKey(
        "raw",
        this.#bytes,
        ED25519,
        false,
        ["verify"],
      );
      return await crypto.subtle.verify(
        ED25519,
        key,
        signature.slice(),
        data.slice(),
      );
    } catch {
      return false;
    }
  }
}

// ── SecretKey ─────────────────────────────────────────────────────────────────

/**
 * An Ed25519 secret key.
 *
 * Persist `toBytes()` to restore identity across restarts.
 * The associated `publicKey` is derived lazily on first access.
 *
 * @example Save and restore identity:
 * ```ts
 * // First run — generate and save:
 * const node = await createNode();
 * localStorage.setItem('key', btoa(String.fromCharCode(...node.secretKey.toBytes())));
 *
 * // Subsequent runs — restore:
 * const raw = Uint8Array.from(atob(localStorage.getItem('key')!), c => c.charCodeAt(0));
 * const node2 = await createNode({ key: raw });
 * ```
 */
export class SecretKey {
  readonly #bytes: Uint8Array<ArrayBuffer>;
  #publicKey: PublicKey | null = null;

  private constructor(bytes: Uint8Array) {
    if (bytes.length !== 32) {
      throw new TypeError(`SecretKey must be 32 bytes, got ${bytes.length}`);
    }
    this.#bytes = bytes.slice();
  }

  /** Copy of the raw 32-byte secret key material. */
  toBytes(): Uint8Array {
    return this.#bytes.slice();
  }

  /** Base32 representation of the secret key bytes. */
  toString(): string {
    return base32Encode(this.#bytes);
  }

  /**
   * The associated public key.
   *
   * Available immediately if the `SecretKey` was constructed via
   * `_fromBytesWithPublicKey` (as `buildNode` does), otherwise requires a
   * call to `derivePublicKey()` first — accessing this getter before that
   * throws a `TypeError`.
   */
  get publicKey(): PublicKey {
    if (this.#publicKey === null) {
      throw new TypeError(
        "publicKey not yet available — call await secretKey.derivePublicKey() first",
      );
    }
    return this.#publicKey;
  }

  /** Generate a fresh random key using `crypto.getRandomValues`. */
  static generate(): SecretKey {
    const bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    return new SecretKey(bytes);
  }

  /** Construct from 32 raw bytes. Copies the input. */
  static fromBytes(bytes: Uint8Array): SecretKey {
    return new SecretKey(bytes);
  }

  /** Parse from a base32 string (case-insensitive). */
  static fromString(s: string): SecretKey {
    return new SecretKey(base32Decode(s));
  }

  /**
   * Internal helper used by `buildNode` when the public key is already known
   * from the endpoint info returned by Rust — avoids an extra async round-trip.
   * @internal
   */
  static _fromBytesWithPublicKey(
    bytes: Uint8Array,
    publicKey: PublicKey,
  ): SecretKey {
    const sk = new SecretKey(bytes);
    sk.#publicKey = publicKey;
    return sk;
  }

  /**
   * Derive the corresponding public key using Web Crypto (Ed25519).
   * Caches the result so subsequent calls to `this.publicKey` are synchronous.
   */
  async derivePublicKey(): Promise<PublicKey> {
    if (this.#publicKey !== null) return this.#publicKey;
    // Web Crypto does not support raw Ed25519 key derivation directly.
    // We use the JWK format to import the private key and export the public half.
    const jwk: JsonWebKey = {
      kty: "OKP",
      crv: "Ed25519",
      d: btoa(String.fromCharCode(...this.#bytes)).replace(/\+/g, "-").replace(
        /\//g,
        "_",
      ).replace(/=+$/, ""),
      key_ops: ["sign"],
    };
    const cryptoKey = await crypto.subtle.importKey("jwk", jwk, ED25519, true, [
      "sign",
    ]);
    const pubJwk = await crypto.subtle.exportKey(
      "jwk",
      await crypto.subtle.importKey(
        "jwk",
        { ...jwk, d: undefined, key_ops: ["verify"] },
        ED25519,
        true,
        ["verify"],
      ),
    );
    const pubBytes = Uint8Array.from(
      atob((pubJwk.x as string).replace(/-/g, "+").replace(/_/g, "/")),
      (c) => c.charCodeAt(0),
    );
    void cryptoKey; // used above for type narrowing
    this.#publicKey = PublicKey.fromBytes(pubBytes);
    return this.#publicKey;
  }

  /**
   * Sign `data` with this Ed25519 secret key.
   * Returns a 64-byte signature.
   */
  async sign(data: Uint8Array): Promise<Uint8Array> {
    const jwk: JsonWebKey = {
      kty: "OKP",
      crv: "Ed25519",
      d: btoa(String.fromCharCode(...this.#bytes)).replace(/\+/g, "-").replace(
        /\//g,
        "_",
      ).replace(/=+$/, ""),
      key_ops: ["sign"],
    };
    const cryptoKey = await crypto.subtle.importKey(
      "jwk",
      jwk,
      ED25519,
      false,
      ["sign"],
    );
    const sig = await crypto.subtle.sign(ED25519, cryptoKey, data.slice());
    return new Uint8Array(sig);
  }
}

// ── Helpers exported for use inside iroh-http-shared ─────────────────────────

/**
 * Resolve a `PublicKey | string` argument to a base32 string suitable for
 * passing to the FFI layer.
 */
export function resolveNodeId(peer: PublicKey | string): string {
  return typeof peer === "string" ? peer : peer.toString();
}
