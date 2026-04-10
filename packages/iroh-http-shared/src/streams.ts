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
 */
export function makeReadable(bridge: Bridge, handle: number, onClose?: () => void): ReadableStream<Uint8Array> {
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
 */
export async function pipeToWriter(
  bridge: Bridge,
  stream: ReadableStream<Uint8Array>,
  handle: number
): Promise<void> {
  const reader = stream.getReader();
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value && value.byteLength > 0) {
        await bridge.sendChunk(handle, value);
      }
    }
  } finally {
    reader.releaseLock();
    await bridge.finishBody(handle);
  }
}

/**
 * Coerce a `BodyInit` to a `ReadableStream<Uint8Array>`, or `null` for empty bodies.
 */
export function bodyInitToStream(
  body: BodyInit | null | undefined
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
      "Serialise the form data manually and pass a string or Uint8Array body instead."
    );
  }
  if (body instanceof URLSearchParams) {
    return singleChunkStream(new TextEncoder().encode(body.toString()));
  }
  return null;
}

function singleChunkStream(data: Uint8Array): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(data);
      controller.close();
    },
  });
}
