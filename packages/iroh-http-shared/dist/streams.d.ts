/**
 * Web-standard stream helpers.
 *
 * `makeReadable` and `pipeToWriter` are the two primitives that map
 * integer body handles to `ReadableStream` / `WritableStream` abstractions.
 */
import type { Bridge } from "./bridge.js";
/**
 * Wrap a `BodyReader` handle in a web-standard `ReadableStream<Uint8Array>`.
 *
 * Pulls from the bridge via `nextChunk` on each `pull` request.
 * The stream closes automatically when `nextChunk` returns `null`.
 *
 * @param bridge  Platform bridge implementation.
 * @param handle  Slab handle for the `BodyReader` to read from.
 * @param onClose Optional callback invoked when the stream reaches EOF or is cancelled.
 * @returns A `ReadableStream<Uint8Array>` backed by the body channel.
 */
export declare function makeReadable(bridge: Bridge, handle: bigint, onClose?: () => void): ReadableStream<Uint8Array>;
/**
 * Drain a `ReadableStream<Uint8Array>` into a `BodyWriter` handle.
 *
 * Calls `sendChunk` for each chunk, then `finishBody` when the stream ends.
 * Errors from either side are propagated to the returned `Promise`.
 *
 * @param bridge  Platform bridge implementation.
 * @param stream  The `ReadableStream` to consume.
 * @param handle  Slab handle for the `BodyWriter` to write to.
 * @returns Resolves when the entire stream has been piped and finished.
 */
export declare function pipeToWriter(bridge: Bridge, stream: ReadableStream<Uint8Array>, handle: bigint): Promise<void>;
/**
 * Coerce a `BodyInit` to a `ReadableStream<Uint8Array>`, or `null` for empty bodies.
 *
 * Supports `ReadableStream`, `Uint8Array`, any `ArrayBufferView` (e.g. `Int16Array`,
 * `DataView`), `ArrayBuffer`, `string`, `Blob`, and `URLSearchParams`.
 * Throws for `FormData` (not supported in iroh-http v1) and for any other type.
 *
 * @param body  The body value to coerce.
 * @returns A `ReadableStream<Uint8Array>`, or `null` if the body is empty.
 * @throws {TypeError} If `body` is a `FormData` instance or an unsupported type.
 */
export declare function bodyInitToStream(body: BodyInit | null | undefined): ReadableStream<Uint8Array> | null;
//# sourceMappingURL=streams.d.ts.map