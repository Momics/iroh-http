# Internals

Technical documentation for contributors to iroh-http-core.

| Document | What it covers |
|----------|----------------|
| [http-engine.md](http-engine.md) | hyper/tower integration, request lifecycle, body channel bridge, duplex |
| [resource-handles.md](resource-handles.md) | u64 slotmap handle system, registries, lifecycle, stale handle safety |
| [connection-pool.md](connection-pool.md) | moka-backed pool, single-flight, stale connection handling, ALPN segregation |
| [wire-format.md](wire-format.md) | Wire encoding, ALPN versioning, duplex handshake |

Start with [../architecture.md](../architecture.md) for the component overview before diving into these.
