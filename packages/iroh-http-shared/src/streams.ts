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
export function makeReadable(
  bridge: Bridge,
  handle: bigint,
  onClose?: () => void,
): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      const chunk = await bridge.nextChunk(handle);
      if (chunk === null) {
        controller.close();
        onClose?.();
      } else {
        controller.enqueue(chunk);
      }
    },
    cancel() {
      bridge.cancelRequest(handle);
      onClose?.();
    },
  });
}

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
export async function pipeToWriter(
  bridge: Bridge,
  stream: ReadableStream<Uint8Array>,
  handle: bigint,
): Promise<void> {
  const reader = stream.getReader();
  try {
    let pending: Promise<void> | null = null;
    while (true) {
      const { value, done } = await reader.read();
      if (pending) await pending;
      if (done) break;
      if (value && value.byteLength > 0) {
        pending = bridge.sendChunk(handle, value);
      }
    }
    if (pending) await pending;
  } finally {
    reader.releaseLock();
    await bridge.finishBody(handle);
  }
}

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
export function bodyInitToStream(
  body: BodyInit | null | undefined,
): ReadableStream<Uint8Array> | null {
  if (body == null) return null;
  if (body instanceof ReadableStream) return body as ReadableStream<Uint8Array>;
  if (body instanceof Uint8Array) {
    return singleChunkStream(body);
  }
  if (body instanceof ArrayBuffer) {
    return singleChunkStream(new Uint8Array(body));
  }
  if (typeof body === "string") {
    return singleChunkStream(new TextEncoder().encode(body));
  }
  if (body instanceof Blob) {
    return body.stream() as ReadableStream<Uint8Array>;
  }
  if (body instanceof FormData) {
    throw new TypeError(
      "FormData request bodies are not supported by iroh-http (v1). " +
        "Serialise the form data manually and pass a string or Uint8Array body instead.",
    );
  }
  if (body instanceof URLSearchParams) {
    return singleChunkStream(new TextEncoder().encode(body.toString()));
  }
  // Catch-all for other ArrayBufferView subtypes (Int16Array, Float64Array, DataView, etc.)
  // Must come after the Uint8Array check so the common case stays on the fast path.
  if (ArrayBuffer.isView(body)) {
    return singleChunkStream(
      new Uint8Array((body as ArrayBufferView).buffer, (body as ArrayBufferView).byteOffset, (body as ArrayBufferView).byteLength),
    );
  }
  throw new TypeError(
    `Unsupported BodyInit type: ${Object.prototype.toString.call(body)}. ` +
      `Supported types: ReadableStream, Uint8Array, ArrayBufferView, ArrayBuffer, string, Blob, URLSearchParams.`,
  );
}

function singleChunkStream(data: Uint8Array): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(data);
      controller.close();
    },
  });
}
