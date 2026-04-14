# Job Dispatch

Peers advertise compute capacity. Others submit work — rendering, transcoding,
inference, indexing, verification. Results come back over the same iroh
connection. No job queue server, no cloud function platform, no broker.

## The insight

"Serverless" computing still runs on servers — just someone else's, billed by
the millisecond. iroh-http lets idle devices offer their spare CPU, GPU, or
specialised hardware to peers they trust, without any intermediary routing,
billing infrastructure, or account setup.

A laptop that's rendering nothing at 2am can accept a friend's video
transcode job. A Raspberry Pi cluster in a maker space can offer distributed
indexing. A machine with a local LLM can handle inference requests from the
same home network.

```
Client              Worker A (GPU available)
  │                       │
  │  POST /job/transcode  │
  │──────────────────────►│
  │                       │  [processes]
  │◄── 200 result bytes ──│
  │                       │

No queue. No cloud. Just HTTP over iroh.
```

## Job descriptor

```ts
interface JobRequest {
  type: string;           // e.g. "transcode", "render", "index", "infer"
  payload: unknown;       // job-specific input
  maxDurationMs?: number; // client's timeout budget
  priority?: 'low' | 'normal' | 'high';
}

interface JobResult {
  jobId: string;
  status: 'ok' | 'error';
  output?: unknown;
  error?: string;
  durationMs: number;
  workerNodeId: string;   // for audit / retry routing
}
```

## Worker side

Workers announce the job types they accept and the resources they have:

```ts
const SUPPORTED_JOBS = ['transcode', 'thumbnail', 'index-text'];
let busySlots = 0;
const MAX_CONCURRENT = 2;

node.serve({}, async (req) => {
  const url = new URL(req.url);

  // GET /.well-known/capabilities — advertise job types (see capability-advertisement.md)
  if (req.method === 'GET' && url.pathname === '/.well-known/capabilities') {
    return Response.json({
      nodeId: node.nodeId(),
      version: 1,
      roles: [{
        role: 'compute',
        supportedJobs: SUPPORTED_JOBS,
        availableSlots: MAX_CONCURRENT - busySlots,
        cpuCores: navigator.hardwareConcurrency ?? 4,
      }],
      publishedAt: Date.now(),
    });
  }

  // POST /job/:type — submit a job
  const match = url.pathname.match(/^\/job\/([a-z\-]+)$/);
  if (req.method === 'POST' && match) {
    const jobType = match[1];
    if (!SUPPORTED_JOBS.includes(jobType)) {
      return new Response('Unsupported job type', { status: 422 });
    }
    if (busySlots >= MAX_CONCURRENT) {
      return new Response('Worker at capacity', {
        status: 503,
        headers: { 'Retry-After': '5' },
      });
    }

    const request: JobRequest = await req.json();
    const jobId = crypto.randomUUID();
    const start = Date.now();
    busySlots++;

    try {
      const output = await runJob(jobType, request.payload, {
        signal: AbortSignal.timeout(request.maxDurationMs ?? 60_000),
      });

      const result: JobResult = {
        jobId,
        status: 'ok',
        output,
        durationMs: Date.now() - start,
        workerNodeId: node.nodeId(),
      };
      return Response.json(result);
    } catch (err) {
      return Response.json({
        jobId,
        status: 'error',
        error: String(err),
        durationMs: Date.now() - start,
        workerNodeId: node.nodeId(),
      } satisfies JobResult, { status: 500 });
    } finally {
      busySlots--;
    }
  }

  return new Response('Not Found', { status: 404 });
});

// Pluggable job runners — replace with real implementations
async function runJob(
  type: string,
  payload: unknown,
  opts: { signal: AbortSignal },
): Promise<unknown> {
  switch (type) {
    case 'transcode': return transcodeVideo(payload as any, opts.signal);
    case 'thumbnail': return generateThumbnail(payload as any, opts.signal);
    case 'index-text': return indexText(payload as any, opts.signal);
    default: throw new Error(`Unknown job type: ${type}`);
  }
}
```

## Client side: submit and wait

```ts
async function submitJob(
  node: IrohNode,
  workerNodeId: string,
  job: JobRequest,
): Promise<JobResult> {
  const res = await node.fetch(`iroh://${workerNodeId}/job/${job.type}`, {
    method: 'POST',
    body: JSON.stringify(job),
    headers: { 'Content-Type': 'application/json' },
    signal: AbortSignal.timeout((job.maxDurationMs ?? 60_000) + 5000),
  });

  if (res.status === 503) {
    const retryAfter = Number(res.headers.get('retry-after') ?? '5');
    throw new RetriableError(`Worker busy, retry after ${retryAfter}s`, retryAfter);
  }

  const result: JobResult = await res.json();
  if (result.status === 'error') throw new Error(result.error);
  return result;
}

class RetriableError extends Error {
  constructor(message: string, public retryAfterSec: number) {
    super(message);
  }
}
```

## Discovering workers with capability advertisement

```ts
async function findWorkers(
  node: IrohNode,
  jobType: string,
): Promise<string[]> {
  // From capability-advertisement.md — peerCaps built from browse()
  return findPeers('compute', (r) =>
    Array.isArray((r as any).supportedJobs) &&
    (r as any).supportedJobs.includes(jobType) &&
    (r as any).availableSlots > 0,
  );
}
```

## Dispatch with retries and worker pool

```ts
async function dispatch(
  node: IrohNode,
  job: JobRequest,
  opts = { maxAttempts: 3 },
): Promise<JobResult> {
  const workers = await findWorkers(node, job.type);
  if (workers.length === 0) throw new Error(`No workers available for job type "${job.type}"`);

  let lastError: unknown;
  for (let attempt = 0; attempt < opts.maxAttempts; attempt++) {
    const worker = workers[attempt % workers.length];
    try {
      return await submitJob(node, worker, job);
    } catch (err) {
      lastError = err;
      if (err instanceof RetriableError) {
        await sleep(err.retryAfterSec * 1000);
        continue;
      }
      // Non-retriable — try next worker
    }
  }
  throw lastError;
}
```

## Racing for the fastest result

For embarrassingly parallel jobs (rendering 100 frames), scatter the work
across all available workers simultaneously:

```ts
async function scatter<T>(
  node: IrohNode,
  jobs: JobRequest[],
  jobType: string,
): Promise<JobResult[]> {
  const workers = await findWorkers(node, jobType);
  if (workers.length === 0) throw new Error('No workers');

  return Promise.all(
    jobs.map((job, i) =>
      submitJob(node, workers[i % workers.length], job),
    ),
  );
}
```

## Streaming results (long-running jobs)

For jobs that produce incremental output (rendering frames, transcribing audio
in chunks), use WebTransport bidi streams:

```ts
// Worker: stream partial results as they're ready
session.accept().then(async ({ readable, writable }) => {
  const reader = readable.getReader();
  const writer = writable.getWriter();

  const { type, payload } = JSON.parse(new TextDecoder().decode(
    (await reader.read()).value,
  ));

  for await (const chunk of runJobStreaming(type, payload)) {
    await writer.write(new TextEncoder().encode(JSON.stringify(chunk) + '\n'));
  }
  await writer.close();
});
```

See [webtransport](../features/webtransport.md) for the bidi stream API.

## Access control

Workers should only accept jobs from trusted clients. Options in increasing
strictness:

1. **Node ID allowlist** — check `Peer-Id` header against known peers
2. **Capability token** — require a `requireToken()` middleware; see
   [capability-tokens.md](capability-tokens.md)
3. **Attenuated chain** — scope tokens to specific job types; see
   [capability-attenuation.md](capability-attenuation.md)

```ts
node.serve({}, compose(
  requireToken(trustedPublicKey), // only authorised clients can submit jobs
  jobHandler,
));
```

## Failure modes

- **Worker goes offline mid-job**: the HTTP connection drops; the client
  receives a network error. Retry on a different worker. Long-running jobs
  should checkpoint and resume if possible.
- **Worker lies about capacity**: claims `availableSlots: 4` but immediately
  returns `503`. Treat `503` as capacity exhaustion, back off, and try another
  worker. Track per-worker failure rates.
- **Malicious input**: job payloads should be treated as untrusted. Run job
  execution in a sandbox (worker thread, container, Deno subprocess with
  limited permissions). Never `eval` a job payload.
- **Slow worker drags the whole job**: use `Promise.race` with a timeout and
  fall back to another worker if the first is too slow.

## Threat model

**Protects against:**
- Centralised job broker as a single point of failure or billing chokepoint
- Unencrypted job payloads in transit (iroh QUIC provides encryption)
- Submitting jobs to unintended workers (node ID is authenticated)

**Does not protect against:**
- A malicious worker returning incorrect results — verify outputs where
  possible (hash, re-run on a second worker)
- Resource exhaustion by a trusted-but-buggy client — rate limit by node ID
  (`Peer-Id` header) using the pattern in
  [middleware.md](middleware.md)
- Workers observing sensitive payloads — [sealed-messages.md](sealed-messages.md)
  shows how to encrypt payloads if workers shouldn't see the input

## When not to use this pattern

If you need guaranteed delivery, ordered execution, or durable job history,
you need a queue. This pattern is best-effort: if all workers are offline, the
job fails. Consider [offline-first.md](offline-first.md) for the queuing
layer that sits in front of this.

## See also

- [Capability advertisement](capability-advertisement.md) — how workers
  announce availability and clients discover them
- [Peer fallback](peer-fallback.md) — the retry loop that routes around
  unavailable workers
- [Offline-first](offline-first.md) — queue jobs when no workers are available;
  dispatch when they reappear
- [Ecosystem overview](ecosystem.md) — job dispatch is the compute layer of
  the full network stack
