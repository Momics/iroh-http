//! Diagnostic test — captures iroh trace output during a connect timeout.

use iroh_http_core::{IrohEndpoint, NetworkingOptions, NodeOptions, Session};

/// Reproduce exactly what bidi_stream.rs does, in this binary — NO tracing.
///
/// Marked `#[ignore]` because it is a diagnostic/investigative test used to
/// reproduce connect-timeout scenarios.  Run explicitly with:
///   cargo test -p iroh-http-core diag_session_connect -- --ignored
#[tokio::test]
#[ignore = "diagnostic only — run explicitly when investigating connect timeouts"]
async fn diag_session_connect() {
    let opts = || NodeOptions {
        networking: NetworkingOptions {
            disabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let a = IrohEndpoint::bind(opts()).await.unwrap();
    let b = IrohEndpoint::bind(opts()).await.unwrap();

    let b_id = b.node_id().to_string();
    let b_addrs: Vec<std::net::SocketAddr> = b.raw().addr().ip_addrs().cloned().collect();
    eprintln!("=== B addrs: {b_addrs:?}");

    let b_clone = b.clone();
    let b_handle = tokio::spawn(async move {
        let sess = Session::accept(b_clone).await.unwrap();
        eprintln!(
            "=== B: accepted session handle: {:?}",
            sess.map(|s| s.handle())
        );
    });

    eprintln!("=== Attempting Session::connect...");
    let connect_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        Session::connect(a, &b_id, Some(&b_addrs)),
    )
    .await;

    match &connect_result {
        Ok(Ok(session)) => eprintln!(
            "=== Session::connect succeeded! handle={}",
            session.handle()
        ),
        Ok(Err(e)) => eprintln!("=== Session::connect FAILED: {e}"),
        Err(_) => eprintln!("=== Session::connect TIMED OUT after 10s"),
    }

    assert!(
        connect_result.is_ok() && connect_result.unwrap().is_ok(),
        "Session::connect failed"
    );
    b_handle.abort();
}
