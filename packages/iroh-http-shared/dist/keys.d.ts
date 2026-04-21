/**
 * PublicKey and SecretKey — typed wrappers around Ed25519 key material.
 *
 * Uses an inline RFC 4648 base32 codec and `crypto.subtle`
 * for Ed25519 signature verification (Node 18+, Deno, modern browsers).
 */
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
export declare class PublicKey {
    #private;
    private constructor();
    /** Copy of the raw 32-byte key material. */
    get bytes(): Uint8Array;
    /** Lowercase base32 representation (the "node ID"). */
    toString(): string;
    /** `true` when both keys contain identical byte sequences. */
    equals(other: PublicKey): boolean;
    /**
     * Parse from a base32 string (case-insensitive).
     * Throws `TypeError` if the string is not a valid 32-byte base32 key.
     */
    static fromString(s: string): PublicKey;
    /** Construct from 32 raw bytes. Copies the input. */
    static fromBytes(bytes: Uint8Array): PublicKey;
    /**
     * Verify an Ed25519 signature over `data`.
     * Returns `false` rather than throwing when the signature is invalid.
     */
    verify(data: Uint8Array, signature: Uint8Array): Promise<boolean>;
}
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
export declare class SecretKey {
    #private;
    private constructor();
    /** Copy of the raw 32-byte secret key material. */
    toBytes(): Uint8Array;
    /** Base32 representation of the secret key bytes. */
    toString(): string;
    /**
     * The associated public key.
     *
     * Available immediately if the `SecretKey` was constructed via
     * `_fromBytesWithPublicKey` (as `buildNode` does), otherwise requires a
     * call to `derivePublicKey()` first — accessing this getter before that
     * throws a `TypeError`.
     */
    get publicKey(): PublicKey;
    /** Generate a fresh random key using `crypto.getRandomValues`. */
    static generate(): SecretKey;
    /** Construct from 32 raw bytes. Copies the input. */
    static fromBytes(bytes: Uint8Array): SecretKey;
    /** Parse from a base32 string (case-insensitive). */
    static fromString(s: string): SecretKey;
    /**
     * Internal helper used by `buildNode` when the public key is already known
     * from the endpoint info returned by Rust — avoids an extra async round-trip.
     * @internal
     */
    static _fromBytesWithPublicKey(bytes: Uint8Array, publicKey: PublicKey): SecretKey;
    /**
     * Derive the corresponding public key using Web Crypto (Ed25519).
     * Caches the result so subsequent calls to `this.publicKey` are synchronous.
     */
    derivePublicKey(): Promise<PublicKey>;
    /**
     * Sign `data` with this Ed25519 secret key.
     * Returns a 64-byte signature.
     */
    sign(data: Uint8Array): Promise<Uint8Array>;
}
/**
 * Resolve a `PublicKey | string` argument to a base32 string suitable for
 * passing to the FFI layer.
 */
export declare function resolveNodeId(peer: PublicKey | string): string;
//# sourceMappingURL=keys.d.ts.map