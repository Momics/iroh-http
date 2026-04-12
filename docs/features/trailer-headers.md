# Trailer Headers

## What

HTTP trailers are headers sent **after** the message body rather than before it. They are useful for metadata that is only known once the body has been fully produced — a checksum of the streamed bytes, a final status code for a long-running operation, or a server-generated request ID.

iroh-http supports trailers on both request and response, exposed as a non-standard extension on the standard `Request` and `Response` objects.

## API

### Reading request trailers (in a serve handler)

```ts
node.serve({}, async (req) => {
  const body = await req.text();

  // req.trailers is a Promise<Headers> that resolves once the body is consumed.
  const trailers = await req.trailers;
  const checksum = trailers.get('x-body-checksum');

  return new Response('ok');
});
```

`req.trailers` is a `Promise<Headers>`. It resolves to an empty `Headers` object when the sender sent no trailers.

### Sending response trailers (in a serve handler)

```ts
node.serve({}, async (req) => {
  const res = new Response(body);

  // Attach a non-standard `trailers` property — a function returning Headers.
  (res as any).trailers = () => new Headers({ 'x-body-checksum': checksum });

  return res;
});
```

### Reading response trailers (in fetch)

```ts
const res = await node.fetch(peer, '/stream');
const body = await res.text();

// res.trailers resolves once the body is fully consumed.
const trailers = await (res as any).trailers;
const checksum = trailers?.get('x-body-checksum');
```

## How it works

Trailers are negotiated implicitly: when the ALPN includes a trailers-capable variant (`iroh-http/1-trailers`, `iroh-http/1-full`), both sides exchange a trailer block after the body using the framing crate's `serialize_trailers` / `parse_trailers` functions. The framing is a simple `\r\n`-delimited header block in the same format as the request/response head.

On the JS side:
- `req.trailers` is attached to the `Request` object inside `makeServe` using `Object.defineProperty`.
- `res.trailers` is read from the `Response` object after the handler returns; if present, its return value is sent via `bridge.sendTrailers` once the response body is fully piped.

## Limitations

- Trailers are **not** part of the WHATWG `fetch` spec. The `.trailers` property is a non-standard extension. TypeScript callers must cast to `any` or extend the type themselves.
- Trailers are not available in duplex (`createBidirectionalStream`) mode. `reqTrailersHandle` is `0` (sentinel) in that case.
- The set of valid trailer names is not enforced — any header name is accepted. Note that the HTTP spec forbids `Content-Length`, `Transfer-Encoding`, and `Host` in trailers; iroh-http does not validate this.
