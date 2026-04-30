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
        // If ConcurrencyLimitLayer is saturated AND LoadShed is present, it
        // returns Pending from poll_ready — LoadShed converts that to an
        // immediate Err(Overloaded). If LoadShed is absent, poll_ready blocks
        // until a slot opens. In both cases the inner service signals readiness
        // here; layer errors are handled in `call`, never surfaced via
        // `poll_ready`.
        match self.0.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Ok(())),
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
