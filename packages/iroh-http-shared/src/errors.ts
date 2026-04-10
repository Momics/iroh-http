/**
 * Structured error classes for iroh-http.
 *
 * All errors from the Rust/FFI layer are classified through `classifyError`
 * before reaching user code.  This gives callers `instanceof` checks and
 * machine-readable `.code` strings.
 */

// ── Base class ────────────────────────────────────────────────────────────────

/** Base class for all iroh-http errors. */
export class IrohError extends Error {
  /** Machine-readable error code string (e.g. `"TIMEOUT"`, `"INVALID_HANDLE"`). */
  readonly code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = "IrohError";
    this.code = code;
    // Restore prototype chain in transpiled environments.
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

// ── Subclasses ────────────────────────────────────────────────────────────────

/** Failed to bind or create an Iroh endpoint. */
export class IrohBindError extends IrohError {
  constructor(message: string, code: string) {
    super(message, code);
    this.name = "IrohBindError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** Failed to connect to a remote peer. */
export class IrohConnectError extends IrohError {
  constructor(message: string, code: string) {
    super(message, code);
    this.name = "IrohConnectError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** A body read or write stream failed mid-transfer. */
export class IrohStreamError extends IrohError {
  constructor(message: string, code: string) {
    super(message, code);
    this.name = "IrohStreamError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** HTTP framing / protocol error. */
export class IrohProtocolError extends IrohError {
  constructor(message: string, code: string) {
    super(message, code);
    this.name = "IrohProtocolError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

// ── Classification ────────────────────────────────────────────────────────────

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
export function classifyError(raw: string | unknown): IrohError {
  const msg = raw instanceof Error ? raw.message : String(raw);

  // ── Connect errors ──────────────────────────────────────────────────────────
  if (/\bconnect\b/i.test(msg)) {
    if (/timed?\s*out/i.test(msg))     return new IrohConnectError(msg, "TIMEOUT");
    if (/dns|resolv/i.test(msg))       return new IrohConnectError(msg, "DNS_FAILURE");
    if (/reset|refused|closed/i.test(msg)) return new IrohConnectError(msg, "REFUSED");
    if (/alpn/i.test(msg))             return new IrohConnectError(msg, "ALPN_MISMATCH");
    return new IrohConnectError(msg, "REFUSED");
  }
  if (/timed?\s*out/i.test(msg))       return new IrohConnectError(msg, "TIMEOUT");
  if (/alpn/i.test(msg))               return new IrohConnectError(msg, "ALPN_MISMATCH");
  if (/upgrade\s*rejected|non-101|101/i.test(msg))
                                       return new IrohProtocolError(msg, "UPGRADE_REJECTED");

  // ── Protocol errors ─────────────────────────────────────────────────────────
  if (/parse\s*(response|request)?\s*head/i.test(msg))
                                       return new IrohProtocolError(msg, "PARSE_FAILURE");
  if (/too\s*many\s*headers/i.test(msg))
                                       return new IrohProtocolError(msg, "TOO_MANY_HEADERS");

  // ── Stream / handle errors ──────────────────────────────────────────────────
  if (/invalid\b.*handle|unknown\b.*handle/i.test(msg))
                                       return new IrohStreamError(msg, "INVALID_HANDLE");
  if (/writer\s*dropped|reader\s*dropped/i.test(msg))
                                       return new IrohStreamError(msg, "WRITER_DROPPED");
  if (/stream\s*reset/i.test(msg))     return new IrohStreamError(msg, "STREAM_RESET");
  if (/chunk|body/i.test(msg))         return new IrohStreamError(msg, "STREAM_RESET");

  // ── Bind errors ─────────────────────────────────────────────────────────────
  if (/bind|endpoint|invalid\s*key|key\s*bytes/i.test(msg))
                                       return new IrohBindError(msg, "ENDPOINT_FAILURE");

  // ── Fallback ────────────────────────────────────────────────────────────────
  return new IrohError(msg, "UNKNOWN");
}

/**
 * Classify a bind/create error specifically.
 * Used by platform adapters for `createEndpoint` rejections.
 */
export function classifyBindError(raw: string | unknown): IrohBindError {
  const msg = raw instanceof Error ? raw.message : String(raw);
  if (/invalid\s*key|key\s*bytes|wrong\s*length/i.test(msg))
                                       return new IrohBindError(msg, "INVALID_KEY");
  return new IrohBindError(msg, "ENDPOINT_FAILURE");
}
