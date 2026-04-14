# Signed Caching

Use Ed25519 signatures to make ETag-based caching tamper-evident. The server
signs the response body; a valid `304 Not Modified` proves the cached copy is
still authentic.

## The pattern

1. On the first request the server signs the body and returns it with:
   - `ETag: <base64url(sig)>` — the signature over the body bytes
   - `Cache-Control: max-age=...`
2. On subsequent requests the browser/client sends `If-None-Match: <etag>`.
3. The server re-generates the body, re-signs it, and compares. If the
   signature matches the provided ETag, the content has not changed → `304`.
   If different → `200` with the new body and new ETag.

Because the ETag **is** a signature, any tampered intermediate cache will
fail the comparison without the server needing a separate hash store.

## Implementation

```ts
function signedCache(secretKey: SecretKey, handler: Handler): Handler {
  return async (req) => {
    // Run the real handler to get fresh content
    const upstream = await handler(req);
    if (!upstream.ok) return upstream;

    const body = new Uint8Array(await upstream.arrayBuffer());
    const sig = secretKey.sign(body);
    const etag = `"${toBase64Url(sig)}"`;

    // Conditional request?
    const ifNoneMatch = req.headers.get('if-none-match');
    if (ifNoneMatch === etag) {
      return new Response(null, {
        status: 304,
        headers: { ETag: etag },
      });
    }

    return new Response(body, {
      status: 200,
      headers: {
        ...Object.fromEntries(upstream.headers),
        ETag: etag,
        'Cache-Control': upstream.headers.get('cache-control') ?? 'no-cache',
      },
    });
  };
}
```

## Client-side verification

A client that received an ETag can verify the body at any time — even from a
local disk cache — without contacting the server:

```ts
async function verifyFromCache(
  body: Uint8Array,
  etag: string,
  issuer: PublicKey,
): Promise<boolean> {
  const sig = fromBase64Url(etag.replace(/"/g, ''));
  return issuer.verify(body, sig);
}
```

## Streaming variant

For large streaming responses, sign a hash of the body and include the
signature in a [trailer header](../features/trailer-headers.md):

```ts
async function streamWithTrailerSig(
  secretKey: SecretKey,
  body: ReadableStream<Uint8Array>,
): Promise<Response> {
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const chunks: Uint8Array[] = [];

  // Tee the stream to accumulate for signing
  const [forHash, forBody] = body.tee();
  const writer = writable.getWriter();

  (async () => {
    for await (const chunk of forHash) chunks.push(chunk);
    const full = concat(chunks);
    const sig = secretKey.sign(full);
    writer.close();
  })();

  return new Response(forBody, {
    headers: {
      'Trailer': 'x-body-sig',
    },
    // Trailer appended by iroh-http framing layer after body completes
    trailers: async () => ({
      'x-body-sig': toBase64Url(await sigPromise),
    }),
  });
}
```

## Notes

- Signing the raw bytes is straightforward; signing a hash is equivalent but
  faster for large bodies. Either is fine — pick one and be consistent.
- This pattern is most useful for public content that many peers cache. For
  private content, the transport-layer identity (`Peer-Id`) is usually
  sufficient.
- `304` responses do not carry a body, so the client must hold the original
  response. Cache the body and the ETag together.
