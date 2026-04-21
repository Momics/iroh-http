/**
 * Structured error classes for iroh-http.
 *
 * All errors from the Rust/FFI layer are classified through `classifyError`
 * before reaching user code.  This gives callers `instanceof` checks and
 * machine-readable `.code` strings.
 */
/**
 * Base class for all iroh-http errors.
 *
 * Every error from the Rust/FFI layer is an `IrohError` or subclass, providing
 * a machine-readable `.code` string for programmatic handling.
 *
 * The `.name` property follows `DOMException` naming conventions where a
 * direct analogue exists, so existing web-platform error-handling patterns
 * work without modification:
 *
 * | Subclass            | `.name`        | DOMException equivalent     |
 * |---------------------|----------------|-----------------------------|
 * | IrohAbortError      | "AbortError"   | DOMException AbortError     |
 * | IrohConnectError    | "NetworkError" | DOMException NetworkError   |
 * | IrohBindError       | "NetworkError" | DOMException NetworkError   |
 * | IrohArgumentError   | "TypeError"    | DOMException TypeError      |
 * | IrohStreamError     | "IrohStreamError"  | (no direct analogue)    |
 * | IrohProtocolError   | "IrohProtocolError" | (no direct analogue)   |
 *
 * @example
 * ```ts
 * try {
 *   await node.fetch(peer, '/api');
 * } catch (e) {
 *   if (e instanceof IrohError) {
 *     console.error(`[${e.code}] ${e.message}`);
 *   }
 *   // Web-platform pattern also works:
 *   if (e.name === "NetworkError") { /* peer unreachable *\/ }
 *   if (e.name === "AbortError")   { /* user cancelled  *\/ }
 * }
 * ```
 */
export declare class IrohError extends Error {
    /** Machine-readable error code string (e.g. `"TIMEOUT"`, `"INVALID_HANDLE"`). */
    readonly code: string;
    constructor(message: string, code: string);
}
/**
 * Failed to bind or create an Iroh endpoint.
 *
 * Thrown by `createNode()` when the QUIC endpoint cannot be created —
 * for example, the UDP port is already in use or the secret key is invalid.
 *
 * @example
 * ```ts
 * try {
 *   const node = await createNode({ bindAddr: '0.0.0.0:4433' });
 * } catch (e) {
 *   if (e instanceof IrohBindError) {
 *     console.error('Bind failed:', e.code, e.message);
 *   }
 * }
 * ```
 */
export declare class IrohBindError extends IrohError {
    constructor(message: string, code: string);
}
/**
 * Failed to connect to a remote peer.
 *
 * Covers DNS resolution, timeout, connection refused, and ALPN mismatch errors.
 *
 * @example
 * ```ts
 * try {
 *   const res = await node.fetch(peerId, '/api');
 * } catch (e) {
 *   if (e instanceof IrohConnectError && e.code === 'TIMEOUT') {
 *     console.error('Peer unreachable — try again later');
 *   }
 * }
 * ```
 */
export declare class IrohConnectError extends IrohError {
    constructor(message: string, code: string);
}
/** A body read or write stream failed mid-transfer (reset, timeout, or cancelled). */
export declare class IrohStreamError extends IrohError {
    constructor(message: string, code: string);
}
/** HTTP framing / protocol error — the peer sent malformed headers or rejected an upgrade. */
export declare class IrohProtocolError extends IrohError {
    constructor(message: string, code: string);
}
/** The operation was aborted via `AbortSignal`.  Mirrors the web platform `AbortError`. */
export declare class IrohAbortError extends IrohError {
    constructor(message?: string);
}
/** Invalid argument passed by the caller. */
export declare class IrohArgumentError extends IrohError {
    constructor(message: string, code?: string);
}
/** Slab handle is invalid or expired. */
export declare class IrohHandleError extends IrohStreamError {
    constructor(message: string, code?: string);
}
/**
 * Map a raw Rust error string to the appropriate structured error class.
 *
 * The Rust layer uses stable, prefixed error messages (e.g. `"connect: …"`,
 * `"parse response head: …"`).  This function uses those prefixes plus a few
 * keyword tests to pick the right subclass and code.
 *
 * Long-term the Rust side should emit structured `{ code, message }` JSON,
 * but string-prefix matching is sufficient while the error messages are
 * under our control.
 */
export declare function classifyError(raw: string | unknown): IrohError;
/**
 * Classify a bind/create error specifically.
 * Used by platform adapters for `createEndpoint` rejections.
 */
export declare function classifyBindError(raw: string | unknown): IrohBindError;
//# sourceMappingURL=errors.d.ts.map