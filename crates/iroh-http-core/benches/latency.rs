#![allow(clippy::disallowed_types)] // test/bench file — RequestPayload and friends are valid here
use criterion::{criterion_group, criterion_main, Criterion};
use iroh_http_core::{
    fetch, respond, serve, IrohEndpoint, NetworkingOptions, NodeOptions, RequestPayload,
    ServeOptions,
};

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

fn direct_addrs(ep: &IrohEndpoint) -> Vec<std::net::SocketAddr> {
    ep.raw().addr().ip_addrs().cloned().collect()
}

fn bench_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Create the node pair once; all iterations measure warm-path latency only.
    let (server_ep, client_ep, server_id, server_addrs) = rt.block_on(async {
        let server_ep = IrohEndpoint::bind(local_opts()).await.unwrap();
        let client_ep = IrohEndpoint::bind(local_opts()).await.unwrap();
        let id = server_ep.node_id().to_string();
        let addrs = direct_addrs(&server_ep);
        (server_ep, client_ep, id, addrs)
    });

    let _guard = rt.enter();
    let body = bytes::Bytes::from(vec![0x61; 1024]);
    let sep = server_ep.clone();
    serve(
        server_ep,
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let sep2 = sep.clone();
            let body = body.clone();
            respond(
                sep2.handles(),
                payload.req_handle,
                200,
                vec![("content-length".into(), "1024".into())],
            )
            .unwrap();
            tokio::spawn(async move {
                sep2.handles()
                    .send_chunk(payload.res_body_handle, body)
                    .await
                    .unwrap();
                sep2.handles().finish_body(payload.res_body_handle).unwrap();
            });
        },
    );

    c.bench_function("latency/iroh/1kb", |b| {
        b.to_async(&rt).iter(|| async {
            let res = fetch(
                &client_ep,
                &server_id,
                "/latency",
                "GET",
                &[],
                None,
                None,
                Some(&server_addrs),
                None,
                true,
                None, // max_response_body_bytes
            )
            .await
            .unwrap();

            while client_ep
                .handles()
                .next_chunk(res.body_handle)
                .await
                .unwrap()
                .is_some()
            {}
        })
    });
}

criterion_group!(benches, bench_latency);
criterion_main!(benches);
