/**
 * PublicKey — typed wrapper around an Ed25519 public key.
 *
 * Immutable. Can be created from a base32 string or raw bytes, and can
 * be used to verify Ed25519 signatures and compare identities.
 */

import { base32Decode, base32Encode } from "./base32.js";

const ED25519: EcKeyAlgorithm = {
  name: "Ed25519",
} as unknown as EcKeyAlgorithm;

/**
 * A node's public identity — its stable network address.
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

  /**
   * Parse a peer ID string (as found in the `Peer-Id` request header) into
   * a `PublicKey`.
   *
   * Semantically equivalent to `fromString()` but makes intent explicit
   * when working with incoming request headers.
   *
   * @example
   * ```ts
   * const peer = PublicKey.fromPeerId(req.headers.get("Peer-Id")!);
   * await node.fetch(peer.toURL("/ping"));
   * ```
   */
  static fromPeerId(id: string): PublicKey {
    return new PublicKey(base32Decode(id));
  }

  /** Construct from 32 raw bytes. Copies the input. */
  static fromBytes(bytes: Uint8Array): PublicKey {
    return new PublicKey(bytes);
  }

  /**
   * Construct an `httpi://` URL string for this peer.
   *
   * @param path Optional path to append (e.g. `"/ping"`). Defaults to `"/"`.
   * @returns A full `httpi://` URL suitable for `node.fetch()` or the WHATWG
   *          `URL` constructor.
   *
   * @example
   * ```ts
   * peer.toURL("/ping")   // → "httpi://tvtswinq.../ping"
   * peer.toURL()          // → "httpi://tvtswinq.../"
   * new URL("/api", peer.toURL()) // works with WHATWG URL
   * ```
   */
  toURL(path?: string): string {
    const base = `httpi://${this.toString()}`;
    if (path == null || path === "") return `${base}/`;
    // Ensure exactly one slash between host and path.
    if (path.startsWith("/")) return `${base}${path}`;
    return `${base}/${path}`;
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

/**
 * Resolve a `PublicKey | string` argument to a base32 node-id string
 * suitable for passing to the FFI layer.
 *
 * Accepts:
 * - a `PublicKey` instance — `.toString()` is called.
 * - a bare base32 public-key string (e.g. `"tvtswinq..."`).
 * - a full `httpi://` URL (e.g. `"httpi://tvtswinq.../some/path"`) — the
 *   hostname is extracted via the WHATWG `URL` parser. The path, query and
 *   fragment, if any, are ignored: this helper resolves *identity*, not a
 *   request target.
 *
 * Throws `TypeError` for `http://` / `https://` URLs to match the rejection
 * in `node.fetch()` — iroh-http is a separate scheme on purpose.
 */
export function resolveNodeId(peer: PublicKey | string): string {
  if (typeof peer !== "string") return peer.toString();
  if (/^httpi:\/\//i.test(peer)) {
    // WHATWG URL handles hostname normalisation (lower-casing, IDN, etc.).
    return new URL(peer).hostname;
  }
  if (/^https?:\/\//i.test(peer)) {
    throw new TypeError(
      `iroh-http requires the "httpi://" scheme, not "${
        peer.slice(0, peer.indexOf("://") + 3)
      }". ` +
        `Use peer.toURL() or pass the bare base32 node-id.`,
    );
  }
  return peer;
}
