/**
 * iroh-http-shared/adapter — types intended for platform adapter authors only.
 *
 * Import this sub-path in adapter packages (iroh-http-node, iroh-http-deno,
 * iroh-http-tauri) rather than importing `@internal` symbols from the root
 * package entry point.
 *
 *   import type { Bridge, RawFetchFn, ... } from "@momics/iroh-http-shared/adapter";
 */

export type {
  Bridge,
  FfiRequest,
  FfiResponseHead,
  FfiResponse,
  RequestPayload,
  RawServeFn,
  RawFetchFn,
  AllocBodyWriterFn,
  FfiDuplexStream,
  RawConnectFn,
} from "./bridge.js";

export type { RawSessionFns } from "./session.js";
