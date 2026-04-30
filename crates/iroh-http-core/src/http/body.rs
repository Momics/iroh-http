//! Unified HTTP body type for `iroh-http-core`.
//!
//! Per [ADR-014](../../docs/adr/014-runtime-architecture.md) every HTTP body
//! flowing through this crate — request bodies, response bodies, and bodies
//! emerging from any tower-http layer — is wrapped in a single newtype:
//! [`Body`]. This collapses the type-system tax that fallible middleware
//! (compression, decompression, timeout) used to impose on the wiring code,
//! and gives every layer a single concrete `B = Body` to compose against.
//!
//! The error type is intentionally [`BoxError`] (not `Infallible`) so that
//! body adapters introduced by tower-http (decompression failures, timeout
//! frame errors, etc.) can flow through the body without forcing the wiring
//! to invent a new `B` parameter at every seam. All current body sources are
//! infallible at construction time and convert into `BoxError` trivially.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Frame, SizeHint};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::BodyExt;

/// Boxed dynamic error used by [`Body`] and the serve service contract.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Single HTTP body type used everywhere in `iroh-http-core`.
///
/// Wraps an [`UnsyncBoxBody`] of [`Bytes`] frames with a [`BoxError`] error
/// channel. `Sync` is intentionally not required — neither hyper nor the
/// tower-http layers we compose need it, and dropping it widens the set of
/// body adapters we can box without ceremony.
pub struct Body(UnsyncBoxBody<Bytes, BoxError>);

impl Body {
    /// An empty body (no frames, end-of-stream immediately).
    pub fn empty() -> Self {
        Self::new(http_body_util::Empty::<Bytes>::new())
    }

    /// A complete body of the given bytes, sent as a single frame.
    pub fn full<B: Into<Bytes>>(bytes: B) -> Self {
        Self::new(http_body_util::Full::new(bytes.into()))
    }

    /// Wrap any `http_body::Body` whose data are [`Bytes`] and whose error
    /// converts into [`BoxError`].
    pub fn new<B>(body: B) -> Self
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        Self(body.map_err(Into::into).boxed_unsync())
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.get_mut().0).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }
}
