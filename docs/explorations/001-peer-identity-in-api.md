---
id: "001"
title: "Peer identity exposure in the fetch API"
status: open
date: 2026-04-13
area: api | identity
tags: [identity, fetch, public-key, verification]
---

# [001] Peer identity exposure in the fetch API

## Context

Every Iroh node has a cryptographic identity — an Ed25519 public key — baked
into the transport layer. This means a `fetch` call in iroh-http is not just
"GET this resource from this address." It is "GET this resource from this
cryptographically verified peer." That distinction is fundamental and has no
equivalent in standard HTTP.

Currently the JS/Python caller addresses a peer by node key in the URL
(`httpi://<node-key>/path`), but the response object returned by `fetch` does
not surface any identity information. The verified peer key lives only inside
the Rust core.

## Questions

1. Should the verified peer public key be surfaced to the caller on a
   successful response — and if so, where (a custom header, a response
   property, a sidecar object)?
2. Should `fetch` accept an optional assertion — "only complete this request if
   the responding peer matches this key" — so the caller can pin to an expected
   identity?
3. Is identity pinning a first-class API concern, or should it be built as a
   middleware/wrapper on top of the existing surface?

## What we know

- The QUIC handshake in Iroh authenticates the remote node; the verified public
  key is available to the Rust core after the connection is established.
- Standard `fetch` has no concept of a verified remote identity; surfacing this
  would be a deliberate extension to the familiar interface.
- The `httpi://` URL scheme already encodes the target node key; the request
  therefore implicitly pins identity. The question is whether that pinning is
  *visible* and *assertable* from the caller side.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Add `x-iroh-peer-key` response header | Familiar header model, no API change | Headers are mutable; callers might not notice it |
| Extend the response object with a `peerKey` property | Explicit, typed, discoverable | Diverges from the standard `Response` interface |
| Pass an `expectedPeer` option to `fetch` | Explicit pinning at the call site | Adds coupling; how to handle mismatch — reject or error? |
| Do nothing / document that URL already pins | Zero API complexity | Invisible to callers; easy to misuse |

## Implications

- Affects all four FFI surfaces (Node, Deno, Tauri, Python) and their type
  definitions.
- Touches the wire boundary: if peer key is surfaced as a response property it
  must cross the FFI serialization layer.
- Has security implications: a caller that doesn't check identity may assume
  they have verified provenance when they don't.

## Next steps

- [ ] Survey how other identity-aware transports (e.g. SSH, Noise Protocol
  libraries) surface peer identity to application code.
- [ ] Prototype a `peerKey` property on the response object and evaluate
  ergonomics against the four runtimes.
- [ ] Decide whether identity assertion belongs in the core or in a
  higher-level wrapper.
