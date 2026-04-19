use criterion::{criterion_group, criterion_main, Criterion};
use iroh_http_core::{
    fetch, serve, server::ServeOptions, IrohEndpoint, NodeOptions, RequestPayload,
};

async fn make_pair() -> (IrohEndpoint, IrohEndpoint) {
    let opts = || NodeOptions {
        disable_networking: true,
        bind_addrs: vec!["127.0.0.1:0".into()],
        ..Default::default()
    };
    let server = IrohEndpoint::bind(opts()).await.unwrap();
    let client = IrohEndpoint::bind(opts()).await.unwrap();
    (server, client)
}

fn bench_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("latency/iroh/1kb", |b| {
        b.to_async(&rt).iter(|| async {
            let (server, client) = make_pair().await;
            let server_id = server.node_id().to_string();
            let addrs = server.raw().addr().ip_addrs().cloned().collect::<Vec<_>>();
            let body = bytes::Bytes::from(vec![0x61; 1024]);

            serve(
                server.clone(),
                ServeOptions::default(),
                move |payload: RequestPayload| {
                    iroh_http_core::server::respond(
                        server.handles(),
                        payload.req_handle,
                        200,
                        vec![("content-length".into(), "1024".into())],
                    )
                    .unwrap();
                    let server = server.clone();
                    let body = body.clone();
                    tokio::spawn(async move {
                        server
                            .handles()
                            .send_chunk(payload.res_body_handle, body)
                            .await
                            .unwrap();
                        server
                            .handles()
                            .finish_body(payload.res_body_handle)
                            .unwrap();
                    });
                },
            );

            let res = fetch(
                &client,
                &server_id,
                "/latency",
                "GET",
                &[],
                None,
                None,
                Some(&addrs),
            )
            .await
            .unwrap();

            while client
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
