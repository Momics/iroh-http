//! Diagnostic test — captures iroh trace output during a connect timeout.

use iroh_http_core::{session_accept, session_connect, IrohEndpoint, NodeOptions};

/// Reproduce exactly what bidi_stream.rs does, in this binary — NO tracing.
#[tokio::test]
async fn diag_session_connect() {
    let opts = || NodeOptions {
        disable_networking: true,
        ..Default::default()
    };
    let a = IrohEndpoint::bind(opts()).await.unwrap();
    let b = IrohEndpoint::bind(opts()).await.unwrap();

    let b_id = b.node_id().to_string();
    let b_addrs: Vec<std::net::SocketAddr> = b.raw().addr().ip_addrs().cloned().collect();
    eprintln!("=== B addrs: {b_addrs:?}");

    let b_clone = b.clone();
    let b_handle = tokio::spawn(async move {
        let sess = session_accept(&b_clone).await.unwrap();
        eprintln!("=== B: accepted session: {sess:?}");
    });

    eprintln!("=== Attempting session_connect...");
    let connect_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        session_connect(&a, &b_id, Some(&b_addrs)),
    )
    .await;

    match &connect_result {
        Ok(Ok(handle)) => eprintln!("=== session_connect succeeded! handle={handle}"),
        Ok(Err(e)) => eprintln!("=== session_connect FAILED: {e}"),
        Err(_) => eprintln!("=== session_connect TIMED OUT after 10s"),
    }

    assert!(
        connect_result.is_ok() && connect_result.unwrap().is_ok(),
        "session_connect failed"
    );
    b_handle.abort();
}
