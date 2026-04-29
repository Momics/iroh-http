#![allow(dead_code)]

use iroh_http_core::{IrohEndpoint, NetworkingOptions, NodeOptions};

/// Create a pair of locally-connected endpoints (relay disabled, loopback only).
pub async fn make_pair() -> (IrohEndpoint, IrohEndpoint) {
    let opts = || NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let server = IrohEndpoint::bind(opts()).await.unwrap();
    let client = IrohEndpoint::bind(opts()).await.unwrap();
    (server, client)
}

pub fn node_id(ep: &IrohEndpoint) -> String {
    ep.node_id().to_string()
}

/// Get the server's direct socket addresses so the client can connect.
pub fn server_addrs(ep: &IrohEndpoint) -> Vec<std::net::SocketAddr> {
    ep.raw().addr().ip_addrs().cloned().collect()
}

/// Helper: create a pair where the server has custom NodeOptions.
pub async fn make_pair_custom_server(server_opts: NodeOptions) -> (IrohEndpoint, IrohEndpoint) {
    let server = IrohEndpoint::bind(server_opts).await.unwrap();
    let client = IrohEndpoint::bind(NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            bind_addrs: vec!["127.0.0.1:0".into()],
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap();
    (server, client)
}
