//! Throughput and latency benchmarks for iroh-http-core.
//!
//! Run:
//!   cargo bench -p iroh-http-core
//!
//! Each benchmark uses loopback QUIC (relay disabled, 127.0.0.1:0) so results
//! reflect the iroh-http stack rather than external network conditions.

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use iroh_http_core::{
    fetch, make_body_channel, respond, serve, IrohEndpoint, NetworkingOptions, NodeOptions,
    RequestPayload, ServeOptions,
};

// ── Fixture helpers ───────────────────────────────────────────────────────────

fn local_opts() -> NodeOptions {
    NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn make_pair() -> (IrohEndpoint, IrohEndpoint) {
    let server = IrohEndpoint::bind(local_opts()).await.unwrap();
    let client = IrohEndpoint::bind(local_opts()).await.unwrap();
    (server, client)
}

fn direct_addrs(ep: &IrohEndpoint) -> Vec<std::net::SocketAddr> {
    ep.raw().addr().ip_addrs().cloned().collect()
}

/// Set up a minimal echo server: reads the request body and returns 200.
fn start_echo_server(server_ep: IrohEndpoint) {
    let sep = server_ep.clone();
    serve(
        server_ep,
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let sep2 = sep.clone();
            tokio::spawn(async move {
                // Drain the request body
                while sep2
                    .handles()
                    .next_chunk(payload.req_body_handle)
                    .await
                    .unwrap()
                    .is_some()
                {}
                respond(sep2.handles(), payload.req_handle, 200, vec![]).unwrap();
                sep2.handles().finish_body(payload.res_body_handle).unwrap();
            });
        },
    );
}

// ── bench 1: connection establishment ────────────────────────────────────────

fn bench_connection_establishment(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("connection_establishment", |b| {
        b.to_async(&rt)
            .iter_with_large_drop(|| async { make_pair().await });
    });
}

// ── bench 2: GET request latency (no body) ───────────────────────────────────

fn bench_fetch_get_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (server_ep, client_ep, server_id, server_addrs) = rt.block_on(async {
        let (server_ep, client_ep) = make_pair().await;
        let id = server_ep.node_id().to_string();
        let a = direct_addrs(&server_ep);
        (server_ep, client_ep, id, a)
    });

    let _guard = rt.enter();
    start_echo_server(server_ep);

    c.bench_function("fetch_get_latency", |b| {
        b.to_async(&rt).iter(|| async {
            let res = fetch(
                &client_ep,
                &server_id,
                "/bench",
                "GET",
                &[],
                None, // no request body
                None, // no fetch token
                Some(&server_addrs),
            )
            .await
            .unwrap();
            // Drain the empty response body to free the handle.
            client_ep
                .handles()
                .next_chunk(res.body_handle)
                .await
                .unwrap();
        });
    });
}

// ── bench 3: POST body throughput (1 KB, 64 KB, 1 MB) ───────────────────────

fn bench_post_body_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (server_ep, client_ep, server_id, server_addrs) = rt.block_on(async {
        let (server_ep, client_ep) = make_pair().await;
        let id = server_ep.node_id().to_string();
        let a = direct_addrs(&server_ep);
        (server_ep, client_ep, id, a)
    });

    let _guard = rt.enter();
    start_echo_server(server_ep);

    let mut group = c.benchmark_group("throughput/post_body");
    for size in [1_024usize, 64 * 1_024, 1_024 * 1_024, 10 * 1_024 * 1_024] {
        group.throughput(Throughput::Bytes(size as u64));
        if size >= 10 * 1_024 * 1_024 {
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &sz| {
            let chunk = Bytes::from(vec![0x42u8; sz]);
            let client = client_ep.clone();
            let id = server_id.clone();
            let addrs = server_addrs.clone();
            b.to_async(&rt).iter(|| {
                let chunk = chunk.clone();
                let client = client.clone();
                let id = id.clone();
                let addrs = addrs.clone();
                async move {
                    // Build a one-shot body channel and write our chunk.
                    let (writer, reader) = client.handles().make_body_channel();
                    let write_handle = client.handles().insert_writer(writer).unwrap();
                    // Write+finish in a background task so fetch can proceed.
                    let client2 = client.clone();
                    tokio::spawn(async move {
                        client2
                            .handles()
                            .send_chunk(write_handle, chunk)
                            .await
                            .unwrap();
                        client2.handles().finish_body(write_handle).unwrap();
                    });

                    let res = fetch(
                        &client,
                        &id,
                        "/upload",
                        "POST",
                        &[],
                        Some(reader),
                        None, // no fetch token
                        Some(&addrs),
                    )
                    .await
                    .unwrap();
                    // Drain the empty response body.
                    client.handles().next_chunk(res.body_handle).await.unwrap();
                }
            });
        });
    }
    group.finish();
}

// ── bench 4: response body streaming (server → client) ───────────────────────

fn bench_response_body_streaming(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (server_ep, client_ep, server_id, server_addrs) = rt.block_on(async {
        let (server_ep, client_ep) = make_pair().await;
        let id = server_ep.node_id().to_string();
        let a = direct_addrs(&server_ep);
        (server_ep, client_ep, id, a)
    });

    // Server sends a fixed-size response body
    let _guard = rt.enter();
    let sep = server_ep.clone();
    serve(
        server_ep,
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let sep2 = sep.clone();
            tokio::spawn(async move {
                // Parse body size from path: /bench/<n>
                let n: usize = payload
                    .url
                    .rsplit('/')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                // Drain request body first
                while sep2
                    .handles()
                    .next_chunk(payload.req_body_handle)
                    .await
                    .unwrap()
                    .is_some()
                {}
                respond(sep2.handles(), payload.req_handle, 200, vec![]).unwrap();
                if n > 0 {
                    sep2.handles()
                        .send_chunk(payload.res_body_handle, Bytes::from(vec![0u8; n]))
                        .await
                        .unwrap();
                }
                sep2.handles().finish_body(payload.res_body_handle).unwrap();
            });
        },
    );

    let mut group = c.benchmark_group("throughput/response_body");
    for size in [1_024usize, 64 * 1_024, 1_024 * 1_024, 10 * 1_024 * 1_024] {
        group.throughput(Throughput::Bytes(size as u64));
        if size >= 10 * 1_024 * 1_024 {
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &sz| {
            let client = client_ep.clone();
            let id = server_id.clone();
            let addrs = server_addrs.clone();
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let id = id.clone();
                let addrs = addrs.clone();
                async move {
                    let url = format!("/bench/{sz}");
                    let res = fetch(&client, &id, &url, "GET", &[], None, None, Some(&addrs))
                        .await
                        .unwrap();
                    // Drain the full response body.
                    while client
                        .handles()
                        .next_chunk(res.body_handle)
                        .await
                        .unwrap()
                        .is_some()
                    {}
                }
            });
        });
    }
    group.finish();
}

// ── bench 5: multiplexing (concurrent fetches) ──────────────────────────────

fn bench_multiplex(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (server_ep, client_ep, server_id, server_addrs) = rt.block_on(async {
        let (server_ep, client_ep) = make_pair().await;
        let id = server_ep.node_id().to_string();
        let a = direct_addrs(&server_ep);
        (server_ep, client_ep, id, a)
    });

    let _guard = rt.enter();
    start_echo_server(server_ep);

    let mut group = c.benchmark_group("multiplex");
    for concurrency in [8usize, 32] {
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            &concurrency,
            |b, &n| {
                let client = client_ep.clone();
                let id = server_id.clone();
                let addrs = server_addrs.clone();
                b.to_async(&rt).iter(|| {
                    let client = client.clone();
                    let id = id.clone();
                    let addrs = addrs.clone();
                    async move {
                        let futs: Vec<_> = (0..n)
                            .map(|_| {
                                let client = client.clone();
                                let id = id.clone();
                                let addrs = addrs.clone();
                                async move {
                                    let res = fetch(
                                        &client,
                                        &id,
                                        "/bench",
                                        "GET",
                                        &[],
                                        None,
                                        None,
                                        Some(&addrs),
                                    )
                                    .await
                                    .unwrap();
                                    client.handles().next_chunk(res.body_handle).await.unwrap();
                                }
                            })
                            .collect();
                        futures::future::join_all(futs).await;
                    }
                });
            },
        );
    }
    group.finish();
}

// ── bench 6: handle allocation (alloc_body_writer) ───────────────────────────

fn bench_handle_allocation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let ep = rt.block_on(IrohEndpoint::bind(local_opts())).unwrap();

    let mut group = c.benchmark_group("handle_ops");
    group.bench_function("alloc_body_writer", |b| {
        b.iter(|| {
            // alloc_body_writer allocates a (writer, reader) pair and inserts
            // the writer into the slab, returning a u64 handle.
            let (h, _reader) = ep.handles().alloc_body_writer().unwrap();
            // finish_body removes the writer from self.writers, freeing the
            // slot.  cancel_reader (the previous call here) operated on the
            // readers slab, which is wrong: the reader is returned directly
            // from alloc_body_writer and is never inserted into the store.
            ep.handles().finish_body(h).unwrap();
        });
    });

    group.bench_function("make_body_channel", |b| {
        b.iter(|| {
            let (_writer, _reader) = make_body_channel();
        });
    });

    group.finish();
}

// ── Benchmark group registration ──────────────────────────────────────────────

criterion_group!(
    benches,
    bench_connection_establishment,
    bench_fetch_get_latency,
    bench_post_body_throughput,
    bench_response_body_streaming,
    bench_multiplex,
    bench_handle_allocation,
);
criterion_main!(benches);
