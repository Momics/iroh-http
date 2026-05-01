//! `HandleLayerError` — convert tower-layer errors to HTTP responses.
//!
//! Split out of `mod.rs` per Slice C.7 of #182.
//!
//! ADR-013 ("Lean on the ecosystem") justification: tower itself, tower-http,
//! and hyper-util do not ship an "error → response" adapter. axum has
//! `axum::error_handling::HandleErrorLayer`, but pulling axum into the runtime
//! just for this seam would invert the dependency direction (axum sits *on
//! top of* tower; iroh-http-core lives one level lower). `HandleLayerError` is
//! a ~50-line bespoke layer that exists solely because that gap in the
//! ecosystem hasn't been filled — every other layer in the serve stack is a
//! stock `tower-http` / `tower` building block.
//!
//! `ConcurrencyLimitLayer`, `TimeoutLayer`, and `LoadShedLayer` return errors
//! rather than `Response` values when they reject a request. This adapter
//! catches them and renders an HTTP response so hyper only ever sees
//! `Ok(Response)`:
//!
//!   tower::timeout::error::Elapsed      → 408 Request Timeout
//!   tower::load_shed::error::Overloaded → 503 Service Unavailable
//!   anything else                        → 500 Internal Server Error

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http::StatusCode;
use tower::Service;

use crate::{Body, BoxError};

/// `Layer` form: insert in any `tower::ServiceBuilder` pipeline that contains
/// a `TimeoutLayer` and/or `LoadShedLayer` to convert their errors into HTTP
/// responses. Wraps the inner service with [`HandleLayerError`].
#[derive(Clone, Default)]
pub(crate) struct HandleLayerErrorLayer;

impl<S> tower::Layer<S> for HandleLayerErrorLayer {
    type Service = HandleLayerError<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HandleLayerError(inner)
    }
}

#[derive(Clone)]
pub(crate) struct HandleLayerError<S>(S);

impl<S, Req> Service<Req> for HandleLayerError<S>
where
    S: Service<Req, Response = hyper::Response<Body>>,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
{
    type Response = hyper::Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // `poll_ready` signals *backpressure readiness*, not request-time
        // capacity. Layer errors produced at request time (Overloaded, Elapsed)
        // arrive in `call()` and are converted to HTTP responses there.
        //
        // If the inner service returns `Err` from `poll_ready` the service is
        // in a broken state. Returning `Ok(())` here would be a lie — the next
        // `call()` would fail in unexpected ways. Instead we log, schedule an
        // immediate re-poll, and return `Pending` so the connection's own
        // request-timeout can eventually close it cleanly.
        match self.0.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => {
                let e: BoxError = e.into();
                tracing::error!(
                    "iroh-http: inner service poll_ready failed ({e}); \
                     treating as not-ready so the request timeout can close the connection"
                );
                // Wake immediately — caller will re-poll until the request
                // timeout fires or the connection is dropped.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let fut = self.0.call(req);
        Box::pin(async move {
            match fut.await {
                Ok(r) => Ok(r),
                Err(e) => {
                    let e = e.into();
                    let status = if e.is::<tower::timeout::error::Elapsed>() {
                        StatusCode::REQUEST_TIMEOUT
                    } else if e.is::<tower::load_shed::error::Overloaded>() {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        tracing::warn!("iroh-http: unexpected tower error: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    };
                    let body_bytes: &'static [u8] = match status {
                        StatusCode::REQUEST_TIMEOUT => b"request timed out",
                        StatusCode::SERVICE_UNAVAILABLE => b"server at capacity",
                        _ => b"internal server error",
                    };
                    Ok(hyper::Response::builder()
                        .status(status)
                        .body(Body::full(Bytes::from_static(body_bytes)))
                        .expect("valid error response"))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::future::{ready, Ready};
    use std::task::{Context, Poll};

    use super::*;

    /// A service whose `poll_ready` always returns `Err`.
    struct AlwaysErrorReady;

    impl Service<hyper::Request<Body>> for AlwaysErrorReady {
        type Response = hyper::Response<Body>;
        type Error = BoxError;
        type Future = Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Err("simulated inner poll_ready failure".into()))
        }

        fn call(&mut self, _req: hyper::Request<Body>) -> Self::Future {
            ready(Ok(hyper::Response::new(Body::empty())))
        }
    }

    /// Regression for #179: `HandleLayerError::poll_ready` must NOT return
    /// `Poll::Ready(Ok(()))` when the inner service returns `Err` from
    /// `poll_ready`. Before the fix it silently reported "I'm ready" —
    /// lying to the caller and deferring the broken state to `call()`.
    ///
    /// The current fix returns `Poll::Pending` (+ wakeup) so the connection
    /// can time out rather than proceeding into a broken `call`.
    #[test]
    fn poll_ready_error_is_not_silently_swallowed() {
        let inner = AlwaysErrorReady;
        let mut svc: HandleLayerError<AlwaysErrorReady> = HandleLayerError(inner);

        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        let result: Poll<Result<(), Infallible>> = svc.poll_ready(&mut cx);

        assert!(
            matches!(result, Poll::Pending),
            "poll_ready must return Poll::Pending (not Poll::Ready(Ok(()))) \
             when the inner service errors — regression for #179"
        );
    }
}