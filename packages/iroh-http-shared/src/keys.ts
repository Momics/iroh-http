/**
 * PublicKey and SecretKey — typed wrappers around Ed25519 key material.
 *
 * Uses an inline RFC 4648 base32 codec and `crypto.subtle`
 * for Ed25519 signature verification (Node 18+, Deno, modern browsers).
 */

// ── Base32 codec ──────────────────────────────────────────────────────────────
//
// Iroh node IDs are RFC 4648 base32 (a-z2-7, no padding, lowercase).
// Implemented inline to avoid any external dependency.

const B32_ALPHA = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const B32_LOOKUP = new Uint8Array(256).fill(0xff);
for (let i = 0; i < B32_ALPHA.length; i++) {
  B32_LOOKUP[B32_ALPHA.charCodeAt(i)] = i;
}

const base32Encode = (b: Uint8Array): string => {
  let out = "", buf = 0, bits = 0;
  for (const byte of b) {
    buf = (buf << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += B32_ALPHA[(buf >> bits) & 0x1f];
    }
  }
  if (bits > 0) out += B32_ALPHA[(buf << (5 - bits)) & 0x1f];
  return out.toLowerCase();
};

const base32Decode = (s: string): Uint8Array => {
  const out: number[] = [];
  let buf = 0, bits = 0;
  for (const ch of s.toUpperCase()) {
    if (ch === "=") break;
    const val = B32_LOOKUP[ch.charCodeAt(0)];
    if (val === 0xff) throw new Error(`Invalid base32 character: ${ch}`);
    buf = (buf << 5) | val;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((buf >> bits) & 0xff);
    }
  }
  return new Uint8Array(out);
};

// ── Web Crypto algorithm descriptor ──────────────────────────────────────────

const ED25519: EcKeyAlgorithm = {
  name: "Ed25519",
} as unknown as EcKeyAlgorithm;

// ── PKCS8 helper ─────────────────────────────────────────────────────────────

/** DER-encoded PKCS8 prefix for an Ed25519 private key (RFC 8410). */
const ED25519_PKCS8_PREFIX = new Uint8Array([
  0x30,
  0x2e,
  0x02,
  0x01,
  0x00,
  0x30,
  0x05,
  0x06,
  0x03,
  0x2b,
  0x65,
  0x70,
  0x04,
  0x22,
  0x04,
  0x20,
]);

/** Wrap a 32-byte Ed25519 seed in PKCS8 DER encoding for Web Crypto import. */
function ed25519Pkcs8(seed: Uint8Array): ArrayBuffer {
  const buf = new Uint8Array(ED25519_PKCS8_PREFIX.length + 32);
  buf.set(ED25519_PKCS8_PREFIX);
  buf.set(seed, ED25519_PKCS8_PREFIX.length);
  return buf.buffer;
}
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
    // Import via PKCS8 (works on Node, Deno, browsers — JWK without `x` fails on Node).
    const cryptoKey = await crypto.subtle.importKey(
      "pkcs8",
      ed25519Pkcs8(this.#bytes),
      ED25519,
      true,
      ["sign"],
    );
    const pubJwk = await crypto.subtle.exportKey("jwk", cryptoKey);
    const pubBytes = Uint8Array.from(
      atob((pubJwk.x as string).replace(/-/g, "+").replace(/_/g, "/")),
      (c) => c.charCodeAt(0),
    );
    this.#publicKey = PublicKey.fromBytes(pubBytes);
    return this.#publicKey;
  }

  /**
   * Sign `data` with this Ed25519 secret key.
   * Returns a 64-byte signature.
   */
  async sign(data: Uint8Array): Promise<Uint8Array> {
    const cryptoKey = await crypto.subtle.importKey(
      "pkcs8",
      ed25519Pkcs8(this.#bytes),
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
