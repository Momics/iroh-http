# iroh-http Test Suite

A comprehensive test suite for [`@momics/iroh-http`](https://github.com/Momics/iroh-http) covering HTTP compliance, node lifecycle, error handling, and stress testing across all supported runtimes (Node.js, Deno, Tauri).

## Test categories

### 1. HTTP compliance (`http-compliance/`)

Data-driven tests validating HTTP semantics (RFC 9110). Extends the upstream `tests/http-compliance/cases.json` (35 cases) to **102 test cases**:

| Category | Cases | What it tests |
|---|---|---|
| **Status Codes** | 22 | All major HTTP status codes across 2xx/3xx/4xx/5xx |
| **HTTP Methods** | 9 | GET, POST, PUT, PATCH, DELETE, OPTIONS, HEAD, custom verbs |
| **Body Handling** | 19 | Echo, UTF-8, emoji, JSON, newlines, special chars, sizes 1B→5MB |
| **Request Headers** | 9 | Custom, case-insensitive, empty, long (8KiB), content-type |
| **Response Headers** | 5 | Single/multiple headers, Content-Type forwarding |
| **Peer-Id Security** | 4 | Injection, anti-spoofing, consistency, format validation |
| **Path Handling** | 12 | Trailing slashes, query strings, URL encoding, dot segments |
| **Concurrent** | 4 | 3/5/10 parallel requests, concurrent POST with body |
| **Sequential** | 3 | Connection reuse, mixed methods, mixed status codes |
| **Content-Type** | 4 | JSON, text/plain, octet-stream, charset |
| **Streaming** | 4 | 64KB, 256KB, 1MB, 5MB streamed responses |
| **Edge Cases** | 7 | Unusual status codes, long paths, many headers |

### 2. Lifecycle (`lifecycle/`)

Imperative tests for node creation, shutdown, and resource management:

- `createNode()` returns valid node with publicKey, secretKey
- publicKey is consistent across accesses
- Two nodes get different keys
- `node.addr()` returns correct info
- `node.close()` resolves cleanly, `node.closed` promise settles
- Double close is idempotent (no crash)
- `serve()` returns handle, handler receives valid Request objects
- `fetch()` returns valid Response with readable body
- 10 sequential create/close cycles (leak detection)

### 3. Error handling (`errors/`)

Tests error scenarios and recovery:

- Handler throws → client gets error/500
- Handler returns rejected promise → client gets error
- Server stays alive after handler error (recovery)
- Fetch to unknown peer → rejection
- Fetch with pre-aborted signal → immediate rejection
- Fetch with mid-request abort → cancellation
- Handler returning non-Response → no crash
- Null body on 200 → no crash
- Peer-Id header cannot be spoofed by client

### 4. Stress (`stress/`)

Tests behavior under load:

- 50 concurrent GETs → all succeed
- 20 concurrent POSTs with body → all echo correctly
- 100 sequential GETs → connection reuse
- 1MB body round-trip with byte-level verification
- 5 concurrent 256KB transfers
- 20 rapid create/close cycles with fetch in each
- 5 independent node pairs running concurrently
- 30 concurrent mixed-method requests

## Architecture

```
tests/
├── harness.mjs                  # Shared test harness (suite/test/assert)
├── run-all.sh                   # Run all categories for Node + Deno
├── README.md
│
├── http-compliance/             # Data-driven HTTP tests
│   ├── cases.json               # 102 test cases (upstream-compatible)
│   ├── handler.mjs              # Shared compliance server routes
│   ├── assertions.mjs           # Shared assertion engine
│   ├── run-node.mjs             # Node same-process runner
│   ├── run-deno.ts              # Deno same-process runner
│   ├── run-tauri.ts             # Tauri webview runner
│   ├── tauri-test.html          # Tauri test entry page
│   ├── server-node.mjs          # Standalone Node server (cross-runtime)
│   ├── server-deno.ts           # Standalone Deno server
│   ├── client-node.mjs          # Standalone Node client
│   ├── client-deno.ts           # Standalone Deno client
│   └── run-cross.sh             # Cross-runtime orchestrator
│
├── lifecycle/                   # Node lifecycle tests
│   ├── test-node.mjs
│   └── test-deno.ts
│
├── errors/                      # Error handling tests
│   ├── test-node.mjs
│   └── test-deno.ts
│
└── stress/                      # Stress / load tests
    ├── test-node.mjs
    └── test-deno.ts
```

### Design principles

1. **DRY** — Shared `handler.mjs`, `assertions.mjs`, and `harness.mjs` eliminate duplicated logic across runtimes.

2. **Upstream-compatible** — `cases.json` is a superset of the upstream format. Extended fields (`concurrent`, `sequential`, `bodyContains`, etc.) are ignored by runners that don't support them.

3. **Categorized** — Each test concern lives in its own directory. Run one category or all of them.

4. **CI-friendly** — All scripts exit with code 1 on failure. The `run-all.sh` orchestrator runs everything. The CI workflow runs each category as a separate step for clear failure attribution.

## Quick start

### Run everything

```bash
./tests/run-all.sh
```

### Run by category

```bash
# Node only, specific category
./tests/run-all.sh --node --category lifecycle

# Deno only, multiple categories
./tests/run-all.sh --deno --category http-compliance,stress
```

### Run individual test scripts

```bash
# HTTP compliance (Node)
cd node && npm install
node ../tests/http-compliance/run-node.mjs --verbose

# Lifecycle (Deno)
deno run -A tests/lifecycle/test-deno.ts

# Error handling (Node)
cd node && node ../tests/errors/test-node.mjs

# Stress (Deno)
deno run -A tests/stress/test-deno.ts

# Cross-runtime HTTP compliance
./tests/http-compliance/run-cross.sh
```

### Tauri (in webview)

```bash
cd tauri && npm install
cargo tauri dev
# Navigate to /tests/http-compliance/tauri-test.html
```

URL params: `?filter=status&verbose=true&bail=true&ci=true`

### Filtering (HTTP compliance only)

```bash
node tests/http-compliance/run-node.mjs --filter status --verbose
node tests/http-compliance/run-node.mjs --bail
deno run -A tests/http-compliance/run-deno.ts --filter peer-id
```

## Test case format (HTTP compliance)

```jsonc
{
  "id": "unique-test-id",
  "description": "Human-readable description",
  "request": {
    "method": "GET",
    "path": "/route",
    "headers": { "x-custom": "value" },
    "body": null | "string" | { "fill": 65536 }
  },
  "response": {
    "status": 200,
    "bodyExact": "exact match",
    "bodyNotEmpty": true,
    "bodyNot": "must not equal",
    "bodyContains": "substring",
    "bodyMatchesRegex": "^[a-z]+$",
    "bodyLengthExact": 1024,
    "bodyMinLength": 1,
    "headers": { "content-type": "application/json" }
  }
}
```

### Extended fields (not in upstream)

| Field | Type | Description |
|---|---|---|
| `concurrent` | number | Run N copies of the request in parallel |
| `sequential` | number | Run N copies sequentially |
| `repeat` | number | Run N copies with optional body equality assertion |
| `requests` | array | Multi-step sequential test with per-step expectations |
| `assertAllBodiesEqual` | boolean | Assert all repeated responses have identical bodies |

## Contributing upstream

This test suite is designed to be contributed to [`Momics/iroh-http`](https://github.com/Momics/iroh-http).

To merge:
1. Copy `http-compliance/cases.json` entries into upstream's `tests/http-compliance/cases.json`
2. Port `handler.mjs` routes into the upstream compliance servers
3. Port `assertions.mjs` logic into the upstream runner
4. Add `lifecycle/`, `errors/`, `stress/` as new test categories under `tests/`
