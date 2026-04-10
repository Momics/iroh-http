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
  // Pass through already-classified errors unchanged.
  if (raw instanceof IrohError) return raw;

  let msg: string;
  let code: string | null = null;

  if (typeof raw === "string") {
    // Try to detect structured `{"code":"...","message":"..."}` JSON emitted
    // by `iroh_http_core::classify_error_json`.
    if (raw.startsWith("{")) {
      try {
        const parsed = JSON.parse(raw) as Record<string, unknown>;
        if (typeof parsed.code === "string" && typeof parsed.message === "string") {
          code = parsed.code;
          msg  = parsed.message;
        } else {
          msg = raw;
        }
      } catch {
        msg = raw;
      }
    } else {
      msg = raw;
    }
  } else if (raw instanceof Error) {
    // napi/Tauri may wrap the JSON string inside an Error.message.
    const m = raw.message;
    if (m.startsWith("{")) {
      try {
        const parsed = JSON.parse(m) as Record<string, unknown>;
        if (typeof parsed.code === "string" && typeof parsed.message === "string") {
          code = parsed.code;
          msg  = parsed.message;
        } else {
          msg = m;
        }
      } catch {
        msg = m;
      }
    } else {
      msg = m;
    }
  } else {
    msg = String(raw);
  }

  if (code) {
    return classifyByCode(code, msg);
  }

  // ── Legacy regex fallback (for strings not yet using classify_error_json) ──
  return classifyByRegex(msg);
}

function classifyByCode(code: string, msg: string): IrohError {
  switch (code) {
    case "TIMEOUT":          return new IrohConnectError(msg, code);
    case "DNS_FAILURE":      return new IrohConnectError(msg, code);
    case "ALPN_MISMATCH":    return new IrohConnectError(msg, code);
    case "REFUSED":          return new IrohConnectError(msg, code);
    case "UPGRADE_REJECTED": return new IrohProtocolError(msg, code);
    case "PARSE_FAILURE":    return new IrohProtocolError(msg, code);
    case "TOO_MANY_HEADERS": return new IrohProtocolError(msg, code);
    case "INVALID_HANDLE":   return new IrohStreamError(msg, code);
    case "WRITER_DROPPED":   return new IrohStreamError(msg, code);
    case "READER_DROPPED":   return new IrohStreamError(msg, code);
    case "STREAM_RESET":     return new IrohStreamError(msg, code);
    case "INVALID_KEY":      return new IrohBindError(msg, code);
    case "ENDPOINT_FAILURE": return new IrohBindError(msg, code);
    default:                 return new IrohError(msg, code);
  }
}

function classifyByRegex(msg: string): IrohError {
  // ── Connect errors ──────────────────────────────────────────────────────────
  if (/\bconnect\b/i.test(msg)) {
    if (/timed?\s*out/i.test(msg))         return new IrohConnectError(msg, "TIMEOUT");
    if (/dns|resolv/i.test(msg))           return new IrohConnectError(msg, "DNS_FAILURE");
    if (/reset|refused|closed/i.test(msg)) return new IrohConnectError(msg, "REFUSED");
    if (/alpn/i.test(msg))                 return new IrohConnectError(msg, "ALPN_MISMATCH");
    return new IrohConnectError(msg, "REFUSED");
  }
  if (/timed?\s*out/i.test(msg))           return new IrohConnectError(msg, "TIMEOUT");
  if (/alpn/i.test(msg))                   return new IrohConnectError(msg, "ALPN_MISMATCH");
  if (/upgrade\s*rejected|non-101|101/i.test(msg))
                                           return new IrohProtocolError(msg, "UPGRADE_REJECTED");

  // ── Protocol errors ─────────────────────────────────────────────────────────
  if (/parse\s*(response|request)?\s*head/i.test(msg))
                                           return new IrohProtocolError(msg, "PARSE_FAILURE");
  if (/too\s*many\s*headers/i.test(msg))   return new IrohProtocolError(msg, "TOO_MANY_HEADERS");

  // ── Stream / handle errors ──────────────────────────────────────────────────
  if (/invalid\b.*handle|unknown\b.*handle/i.test(msg))
                                           return new IrohStreamError(msg, "INVALID_HANDLE");
  if (/writer\s*dropped|reader\s*dropped/i.test(msg))
                                           return new IrohStreamError(msg, "WRITER_DROPPED");
  if (/stream\s*reset/i.test(msg))         return new IrohStreamError(msg, "STREAM_RESET");
  if (/chunk|body/i.test(msg))             return new IrohStreamError(msg, "STREAM_RESET");

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
  // If already a structured error, promote to IrohBindError preserving the code.
  const classified = classifyError(raw);
  if (classified instanceof IrohBindError) return classified;
  // Re-wrap with bind context so callers always get IrohBindError.
  return new IrohBindError(classified.message, classified.code);
}
