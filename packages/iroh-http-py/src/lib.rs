//! Python bindings for iroh-http.
//!
//! Exports `create_node`, `IrohNode`, `IrohRequest`, `IrohResponse` via PyO3.

// The `?` operator always applies `From::from` even when types already match.
// In PyO3 async functions that pattern causes spurious `useless_conversion` hits.
#![allow(clippy::useless_conversion)]

use std::sync::Arc;

use bytes::Bytes;
use iroh_http_core::{
    server::respond,
    stream::{finish_body, make_body_channel, next_chunk, send_chunk},
    IrohEndpoint, NodeOptions,
};
use pyo3::{
    exceptions::PyRuntimeError,
    prelude::*,
    types::{PyBytes, PyDict},
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn py_err(e: impl std::fmt::Display) -> PyErr {
    PyErr::new::<PyRuntimeError, _>(e.to_string())
}

// ── IrohResponse ─────────────────────────────────────────────────────────────

/// Response returned by `IrohNode.fetch`.
#[pyclass]
struct IrohResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body_handle: u32,
    url: String,
}

#[pymethods]
impl IrohResponse {
    /// HTTP status code.
    #[getter]
    fn status(&self) -> u16 {
        self.status
    }

    /// Response headers as a list of `(name, value)` tuples.
    #[getter]
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    /// Final URL of the responding peer.
    #[getter]
    fn url(&self) -> &str {
        &self.url
    }

    /// Read the full response body and return it as `bytes`.
    fn bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.body_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut buf = Vec::new();
            loop {
                match next_chunk(handle).await.map_err(py_err)? {
                    None => break,
                    Some(b) => buf.extend_from_slice(&b),
                }
            }
            Python::with_gil(|py| Ok(PyBytes::new_bound(py, &buf).into_any().unbind()))
        })
    }

    /// Read the full response body and decode it as UTF-8.
    fn text<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.body_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut buf = Vec::new();
            loop {
                match next_chunk(handle).await.map_err(py_err)? {
                    None => break,
                    Some(b) => buf.extend_from_slice(&b),
                }
            }
            String::from_utf8(buf).map_err(|e| py_err(format!("UTF-8 decode error: {e}")))
        })
    }

    /// Read the full response body and parse it as JSON, returning a Python object.
    fn json<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.body_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut buf = Vec::new();
            loop {
                match next_chunk(handle).await.map_err(py_err)? {
                    None => break,
                    Some(b) => buf.extend_from_slice(&b),
                }
            }
            let text =
                String::from_utf8(buf).map_err(|e| py_err(format!("UTF-8 decode error: {e}")))?;
            Python::with_gil(|py| {
                let json_mod = py.import_bound("json")?;
                Ok(json_mod.call_method1("loads", (text,))?.into_any().unbind())
            })
        })
    }
}

// ── HandlerResponse ──────────────────────────────────────────────────────────

/// Response value object returned by a `serve` handler.
///
/// Handlers may return either a `HandlerResponse` instance or a plain dict
/// with `status`, `headers`, and `body` keys.
///
/// ```python
/// async def handler(req):
///     return HandlerResponse(200, b"hello", [("content-type", "text/plain")])
/// ```
#[pyclass(name = "HandlerResponse")]
#[derive(Clone)]
struct HandlerResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[pymethods]
impl HandlerResponse {
    /// Create a handler response.
    ///
    /// Args:
    ///     status: HTTP status code (default: 200).
    ///     body:   Response body bytes (default: b"").
    ///     headers: List of ``(name, value)`` header tuples (default: []).
    #[new]
    #[pyo3(signature = (status=200, body=None, headers=None))]
    fn new(status: u16, body: Option<Vec<u8>>, headers: Option<Vec<(String, String)>>) -> Self {
        Self {
            status,
            headers: headers.unwrap_or_default(),
            body: body.unwrap_or_default(),
        }
    }

    /// HTTP status code.
    #[getter]
    fn status(&self) -> u16 {
        self.status
    }

    /// Response headers as a list of `(name, value)` tuples.
    #[getter]
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    /// Response body bytes.
    #[getter]
    fn body(&self) -> Vec<u8> {
        self.body.clone()
    }
}

/// Incoming request passed to the `serve` handler.
#[pyclass]
struct IrohRequest {
    pub req_body_handle: u32,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub remote_node_id: String,
}

#[pymethods]
impl IrohRequest {
    #[getter]
    fn method(&self) -> &str {
        &self.method
    }

    #[getter]
    fn url(&self) -> &str {
        &self.url
    }

    #[getter]
    fn remote_node_id(&self) -> &str {
        &self.remote_node_id
    }

    #[getter]
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    /// Read and return the full request body as `bytes`.
    fn body<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.req_body_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut buf = Vec::new();
            loop {
                match next_chunk(handle).await.map_err(py_err)? {
                    None => break,
                    Some(b) => buf.extend_from_slice(&b),
                }
            }
            Python::with_gil(|py| Ok(PyBytes::new_bound(py, &buf).into_any().unbind()))
        })
    }
}

// ── IrohBidiStream ──────────────────────────────────────────────────────────

/// A bidirectional byte stream.
///
/// Use `write(data)` to send, iterate with `async for chunk in stream:` to read,
/// and `close()` when done.
#[pyclass]
struct IrohBidiStream {
    read_handle: u32,
    write_handle: u32,
}

// ── IrohUniStream ───────────────────────────────────────────────────────────

/// A unidirectional (send-only) byte stream.
///
/// Use `write(data)` to send and `close()` when done.
#[pyclass]
struct IrohUniStream {
    write_handle: u32,
}

#[pymethods]
impl IrohUniStream {
    /// Write bytes to the stream.
    fn write<'py>(&self, py: Python<'py>, data: Vec<u8>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.write_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            send_chunk(handle, Bytes::from(data))
                .await
                .map_err(py_err)?;
            Ok(())
        })
    }

    /// Close (finish) the write side of the stream.
    fn close(&self) -> PyResult<()> {
        finish_body(self.write_handle).map_err(py_err)
    }
}

#[pymethods]
impl IrohBidiStream {
    /// Write bytes to the stream.
    fn write<'py>(&self, py: Python<'py>, data: Vec<u8>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.write_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            send_chunk(handle, Bytes::from(data))
                .await
                .map_err(py_err)?;
            Ok(())
        })
    }

    /// Read the next chunk. Returns None at EOF.
    fn read<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.read_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match next_chunk(handle).await.map_err(py_err)? {
                None => Ok(Python::with_gil(|py| py.None())),
                Some(b) => {
                    Python::with_gil(|py| Ok(PyBytes::new_bound(py, &b).into_any().unbind()))
                }
            }
        })
    }

    /// Close (finish) the write side of the stream.
    fn close(&self) -> PyResult<()> {
        finish_body(self.write_handle).map_err(py_err)
    }

    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.read_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match next_chunk(handle).await.map_err(py_err)? {
                None => Err(pyo3::exceptions::PyStopAsyncIteration::new_err(())),
                Some(b) => {
                    Python::with_gil(|py| Ok(PyBytes::new_bound(py, &b).into_any().unbind()))
                }
            }
        })
    }
}

// ── IrohBrowseSession ────────────────────────────────────────────────────────

/// An active mDNS browse session.
///
/// Use `async for event in session:` to iterate over discovery events.
/// Each event is a dict with keys `is_active` (bool), `node_id` (str), `addrs` (list of str).
#[cfg(feature = "mdns")]
#[pyclass]
struct IrohBrowseSession {
    inner: tokio::sync::Mutex<iroh_http_discovery::BrowseSession>,
}

#[cfg(feature = "mdns")]
#[pymethods]
impl IrohBrowseSession {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let ptr = self as *const IrohBrowseSession as usize;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // SAFETY: IrohBrowseSession is held on the Python heap for its lifetime.
            let session = unsafe { &*(ptr as *const IrohBrowseSession) };
            match session.inner.lock().await.next_event().await {
                None => Err(pyo3::exceptions::PyStopAsyncIteration::new_err(())),
                Some(ev) => Python::with_gil(|py| {
                    let dict = pyo3::types::PyDict::new_bound(py);
                    dict.set_item("is_active", ev.is_active)?;
                    dict.set_item("node_id", &ev.node_id)?;
                    dict.set_item("addrs", ev.addrs)?;
                    Ok(dict.into_any().unbind())
                }),
            }
        })
    }
}

// ── IrohSession ──────────────────────────────────────────────────────────────
///
/// Use `create_bidirectional_stream()` to open streams.
#[pyclass]
struct IrohSession {
    session_handle: u32,
}

#[pymethods]
impl IrohSession {
    /// Open a new bidirectional stream on this session.
    fn create_bidirectional_stream<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let duplex = iroh_http_core::session_create_bidi_stream(handle)
                .await
                .map_err(py_err)?;
            Ok(IrohBidiStream {
                read_handle: duplex.read_handle,
                write_handle: duplex.write_handle,
            })
        })
    }

    /// Open a new unidirectional (send-only) stream on this session.
    fn create_unidirectional_stream<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let write_handle = iroh_http_core::session_create_uni_stream(handle)
                .await
                .map_err(py_err)?;
            Ok(IrohUniStream { write_handle })
        })
    }

    /// Send a datagram on this session.
    fn send_datagram<'py>(&self, py: Python<'py>, data: Vec<u8>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            iroh_http_core::session_send_datagram(handle, &data).map_err(py_err)?;
            Ok(())
        })
    }

    /// Receive the next datagram. Returns None when the session closes.
    fn recv_datagram<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match iroh_http_core::session_recv_datagram(handle)
                .await
                .map_err(py_err)?
            {
                None => Ok(Python::with_gil(|py| py.None())),
                Some(b) => {
                    Python::with_gil(|py| Ok(PyBytes::new_bound(py, &b).into_any().unbind()))
                }
            }
        })
    }

    /// Get the maximum datagram payload size, or None if unsupported.
    #[getter]
    fn max_datagram_size(&self) -> PyResult<Option<usize>> {
        iroh_http_core::session_max_datagram_size(self.session_handle).map_err(py_err)
    }

    /// Close this session with an optional close code and reason.
    #[pyo3(signature = (close_code=0, reason=""))]
    fn close<'py>(
        &self,
        py: Python<'py>,
        close_code: u32,
        reason: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        let reason = reason.to_owned();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            iroh_http_core::session_close(handle, close_code, &reason).map_err(py_err)?;
            Ok(())
        })
    }

    /// Wait for the session handshake to complete.
    ///
    /// Resolves immediately for iroh sessions (handshake completes during connect).
    fn ready<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            iroh_http_core::session_ready(handle)
                .await
                .map_err(py_err)?;
            Ok(())
        })
    }

    /// Wait for the session to close and return (close_code, reason).
    ///
    /// Blocks until the connection is closed by either side.
    fn closed<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let info = iroh_http_core::session_closed(handle)
                .await
                .map_err(py_err)?;
            Ok((info.close_code, info.reason))
        })
    }

    /// Accept the next incoming bidirectional stream from the remote peer.
    ///
    /// Returns an `IrohBidiStream`, or `None` when the session closes.
    fn next_bidirectional_stream<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match iroh_http_core::session_next_bidi_stream(handle)
                .await
                .map_err(py_err)?
            {
                Some(duplex) => Ok(Python::with_gil(|py| {
                    IrohBidiStream {
                        read_handle: duplex.read_handle,
                        write_handle: duplex.write_handle,
                    }
                    .into_py(py)
                })),
                None => Ok(Python::with_gil(|py| py.None())),
            }
        })
    }

    /// Accept the next incoming unidirectional (receive-only) stream.
    ///
    /// Returns an `IrohBidiStream` with a read handle (write handle is unused),
    /// or `None` when the session closes.
    fn next_unidirectional_stream<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match iroh_http_core::session_next_uni_stream(handle)
                .await
                .map_err(py_err)?
            {
                Some(read_handle) => Ok(Python::with_gil(|py| {
                    IrohBidiStream {
                        read_handle,
                        write_handle: 0,
                    }
                    .into_py(py)
                })),
                None => Ok(Python::with_gil(|py| py.None())),
            }
        })
    }

    fn __aenter__<'py>(slf: PyRef<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let handle = slf.session_handle;
        let py = slf.py();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            iroh_http_core::session_ready(handle)
                .await
                .map_err(py_err)?;
            Ok(IrohSession {
                session_handle: handle,
            })
        })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: &Bound<'py, PyAny>,
        _exc_val: &Bound<'py, PyAny>,
        _exc_tb: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let handle = self.session_handle;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            iroh_http_core::session_close(handle, 0, "").map_err(py_err)?;
            Ok(())
        })
    }
}

// ── IrohNode ─────────────────────────────────────────────────────────────────

/// An Iroh peer-to-peer HTTP node.
#[pyclass]
struct IrohNode {
    ep: IrohEndpoint,
}

#[pymethods]
impl IrohNode {
    /// The node's public key as a lowercase base32 string.
    #[getter]
    fn node_id(&self) -> &str {
        self.ep.node_id()
    }

    /// The raw 32-byte secret key.  Persist this to restore identity.
    #[getter]
    fn keypair<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.ep.secret_key_bytes())
    }

    /// Open a session (QUIC connection) to a remote peer.
    ///
    /// Returns an `IrohSession` that can open bidirectional streams.
    #[pyo3(signature = (peer_id, direct_addrs=None))]
    fn connect<'py>(
        &self,
        py: Python<'py>,
        peer_id: String,
        direct_addrs: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let addrs: Option<Vec<std::net::SocketAddr>> = direct_addrs.map(|v| {
                v.iter()
                    .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
                    .collect()
            });
            let handle = iroh_http_core::session_connect(&ep, &peer_id, addrs.as_deref())
                .await
                .map_err(py_err)?;
            Ok(IrohSession {
                session_handle: handle,
            })
        })
    }

    /// Send an HTTP request to a remote peer.
    ///
    /// `peer_id` is the base32-encoded public key of the target node.
    /// Returns an `IrohResponse` coroutine.
    #[pyo3(signature = (peer_id, url, method="GET", headers=None, body=None, direct_addrs=None))]
    #[allow(clippy::too_many_arguments)]
    fn fetch<'py>(
        &self,
        py: Python<'py>,
        peer_id: String,
        url: String,
        method: &str,
        headers: Option<Vec<(String, String)>>,
        body: Option<Vec<u8>>,
        direct_addrs: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        let method = method.to_owned();
        let headers = headers.unwrap_or_default();

        // Wire up the optional body through a channel so the core fetch can
        // stream it concurrently with reading the response head.
        let body_reader = if let Some(body_bytes) = body {
            let (writer, reader) = make_body_channel();
            tokio::spawn(async move {
                let _ = writer.send_chunk(Bytes::from(body_bytes)).await;
                // BodyWriter drops here, signalling EOF to the reader.
            });
            Some(reader)
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let addrs: Option<Vec<std::net::SocketAddr>> = direct_addrs.map(|v| {
                v.iter()
                    .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
                    .collect()
            });
            let res = iroh_http_core::fetch(
                &ep,
                &peer_id,
                &url,
                &method,
                &headers,
                body_reader,
                None,
                addrs.as_deref(),
            )
            .await
            .map_err(py_err)?;
            Ok(IrohResponse {
                status: res.status,
                headers: res.headers,
                body_handle: res.body_handle,
                url: res.url,
            })
        })
    }

    /// Register an `async def handler(request: IrohRequest)` and start accepting
    /// incoming requests in the background.
    ///
    /// The handler may return either a `HandlerResponse` instance or a plain
    /// dict with keys `status` (int), `headers` (list of ``(name, value)``
    /// tuples), and `body` (bytes).
    fn serve(&self, _py: Python<'_>, handler: PyObject) -> PyResult<()> {
        let ep = self.ep.clone();
        let handler = Arc::new(handler);

        // Use an mpsc channel so the synchronous `on_request` callback can
        // hand payloads off to an async polling loop without blocking.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<iroh_http_core::RequestPayload>(64);

        let handle = iroh_http_core::serve(ep.clone(), ep.serve_options(), move |payload| {
            let tx = tx.clone();
            // `on_request` is synchronous; spawn to avoid blocking the accept task.
            tokio::spawn(async move {
                let _ = tx.send(payload).await;
            });
        });
        ep.set_serve_handle(handle);

        // Polling task: receives each payload, calls the Python handler, sends response.
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                let h = Arc::clone(&handler);
                tokio::spawn(async move {
                    handle_request(h, payload).await;
                });
            }
        });

        Ok(())
    }

    /// Stop the serve loop (graceful shutdown), without closing the endpoint.
    fn stop_serve(&self) {
        self.ep.stop_serve();
    }

    /// Close the endpoint and release all resources.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            ep.close().await;
            Ok(())
        })
    }

    /// Full node address: node ID + relay URL(s) + direct socket addresses.
    /// Returns a dict with `id` (str) and `addrs` (list of str).
    fn addr(&self) -> (String, Vec<String>) {
        let info = self.ep.node_addr();
        (info.id, info.addrs)
    }

    /// Generate a shareable ticket string encoding this node's current address.
    fn ticket(&self) -> String {
        iroh_http_core::node_ticket(&self.ep)
    }

    /// Home relay URL, or None if not connected to a relay.
    fn home_relay(&self) -> Option<String> {
        self.ep.home_relay()
    }

    /// Known addresses for a remote peer, or None if unknown.
    /// Returns a tuple of (node_id, addrs) if found.
    fn peer_info<'py>(&self, py: Python<'py>, node_id: String) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(ep
                .peer_info(&node_id)
                .await
                .map(|info| (info.id, info.addrs)))
        })
    }

    /// Per-peer connection statistics with path information.
    /// Returns a dict with `relay` (bool), `relay_url` (str|None), `paths` (list of dicts).
    fn peer_stats<'py>(&self, py: Python<'py>, node_id: String) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let stats = ep.peer_stats(&node_id).await;
            Ok(stats.map(|s| {
                let paths: Vec<(bool, String, bool)> = s
                    .paths
                    .into_iter()
                    .map(|p| (p.relay, p.addr, p.active))
                    .collect();
                (s.relay, s.relay_url, paths)
            }))
        })
    }

    /// Start discovering peers on the local network via mDNS.
    ///
    /// Returns an async iterable `IrohBrowseSession`.  Iterate over it with
    /// `async for event in node.browse():`.  Each event is a dict with
    /// `is_active` (bool), `node_id` (str), `addrs` (list of str).
    ///
    /// Raises `RuntimeError` if the `mdns` feature is not enabled.
    #[pyo3(signature = (service_name="iroh-http"))]
    fn browse<'py>(&self, py: Python<'py>, service_name: &str) -> PyResult<Bound<'py, PyAny>> {
        #[cfg(feature = "mdns")]
        {
            let ep = self.ep.clone();
            let svc = service_name.to_string();
            let _py_unused = (); // suppress unused-variable warning in non-mdns builds
            return pyo3_async_runtimes::tokio::future_into_py(py, async move {
                let session = iroh_http_discovery::start_browse(ep.raw(), &svc)
                    .await
                    .map_err(py_err)?;
                Ok(IrohBrowseSession {
                    inner: tokio::sync::Mutex::new(session),
                })
            });
        }
        #[cfg(not(feature = "mdns"))]
        {
            let _ = (py, service_name);
            Err(py_err("iroh-http-py compiled without the 'mdns' feature"))
        }
    }

    /// Advertise this node on the local network via mDNS until `stop()` is called.
    ///
    /// Returns immediately.  The advertisement continues in the background until
    /// `node.stop_advertise()` is called or the node is closed.
    ///
    /// Raises `RuntimeError` if the `mdns` feature is not enabled.
    #[pyo3(signature = (service_name="iroh-http"))]
    fn advertise(&self, service_name: &str) -> PyResult<()> {
        #[cfg(feature = "mdns")]
        {
            iroh_http_discovery::start_advertise(self.ep.raw(), service_name)
                .map(|_session| ()) // session kept alive by the endpoint's address_lookup
                .map_err(py_err)
        }
        #[cfg(not(feature = "mdns"))]
        {
            let _ = service_name;
            Err(py_err("iroh-http-py compiled without the 'mdns' feature"))
        }
    }

    fn __aenter__<'py>(slf: PyRef<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let ep = slf.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(IrohNode { ep }) })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: &Bound<'py, PyAny>,
        _exc_val: &Bound<'py, PyAny>,
        _exc_tb: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            ep.close().await;
            Ok(())
        })
    }
}

async fn handle_request(handler: Arc<PyObject>, payload: iroh_http_core::RequestPayload) {
    let req_handle = payload.req_handle;
    let res_body_handle = payload.res_body_handle;

    // Build the IrohRequest and call the Python handler to get a coroutine.
    let fut = Python::with_gil(|py| {
        let ireq = IrohRequest {
            req_body_handle: payload.req_body_handle,
            method: payload.method.clone(),
            url: payload.url.clone(),
            headers: payload.headers.clone(),
            remote_node_id: payload.remote_node_id.clone(),
        };
        let py_req = Bound::new(py, ireq).map_err(py_err)?;
        let coro = handler.call1(py, (py_req,)).map_err(py_err)?;
        pyo3_async_runtimes::tokio::into_future(coro.into_bound(py))
    });

    let fut = match fut {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("[iroh-http-py] handler setup error: {e}");
            send_500(req_handle, res_body_handle);
            return;
        }
    };

    let py_result = fut.await;

    type HandlerOutcome = (u16, Vec<(String, String)>, Vec<u8>);
    let outcome = Python::with_gil(|py| -> PyResult<HandlerOutcome> {
        let obj = py_result?;
        let bound = obj.bind(py);

        // Accept HandlerResponse instance or a plain dict.
        if let Ok(hr) = bound.extract::<HandlerResponse>() {
            return Ok((hr.status, hr.headers, hr.body));
        }

        let dict = bound.downcast::<PyDict>().map_err(|_| {
            py_err("handler must return a HandlerResponse or a dict with 'status', 'headers', 'body'")
        })?.clone();
        let status: u16 = dict
            .get_item("status")?
            .ok_or_else(|| py_err("handler result missing 'status'"))?
            .extract()?;
        let headers: Vec<(String, String)> = dict
            .get_item("headers")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        let body: Vec<u8> = dict
            .get_item("body")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default();
        Ok((status, headers, body))
    });

    match outcome {
        Err(e) => {
            tracing::error!("[iroh-http-py] handler error: {e}");
            send_500(req_handle, res_body_handle);
        }
        Ok((status, headers, body)) => {
            if let Err(e) = respond(req_handle, status, headers) {
                tracing::error!("[iroh-http-py] respond error: {e}");
                return;
            }
            if !body.is_empty() {
                if let Err(e) = send_chunk(res_body_handle, Bytes::from(body)).await {
                    tracing::error!("[iroh-http-py] send_chunk error: {e}");
                    return;
                }
            }
            if let Err(e) = finish_body(res_body_handle) {
                tracing::error!("[iroh-http-py] finish_body error: {e}");
            }
        }
    }
}

fn send_500(req_handle: u32, res_body_handle: u32) {
    let _ = respond(req_handle, 500, vec![]);
    let _ = finish_body(res_body_handle);
}

// ── create_node ───────────────────────────────────────────────────────────────

/// Create an Iroh node.
///
/// Parameters:
///   key                      — 32-byte Ed25519 secret key.  Omit to generate a fresh identity.
///   idle_timeout             — Milliseconds before idle connections are closed.
///   relays                   — List of custom relay server URL strings.
///   dns_discovery            — Custom DNS discovery server URL.
///   disable_networking       — If True, binds locally only (no relay, no DNS).
///   relay_mode               — Relay mode string: "default", "staging", "disabled", or a URL.
///   bind_addrs               — List of socket addresses to bind to (e.g. ["0.0.0.0:0"]).
///   proxy_url                — HTTP proxy URL for outbound connections.
///   proxy_from_env           — Use HTTP_PROXY / HTTPS_PROXY environment variables.
///   keylog                   — Enable TLS key logging (for Wireshark debugging).
///   compression_level        — Zstd compression level (1–22).  Enables compression.
///   compression_min_body_bytes — Skip compression for bodies smaller than this (default 512).
///   max_concurrency          — Maximum simultaneous in-flight requests (default 64).
///   max_connections_per_peer — Maximum connections from a single peer (default 8).
///   request_timeout          — Per-request timeout in milliseconds (default 60000, 0 = disabled).
///   max_request_body_bytes   — Reject request bodies larger than this (default unlimited).
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (key=None, idle_timeout=None, relays=None, dns_discovery=None, disable_networking=false, relay_mode=None, bind_addrs=None, proxy_url=None, proxy_from_env=false, keylog=false, compression_level=None, compression_min_body_bytes=None, max_concurrency=None, max_connections_per_peer=None, request_timeout=None, max_request_body_bytes=None))]
fn create_node<'py>(
    py: Python<'py>,
    key: Option<Vec<u8>>,
    idle_timeout: Option<u64>,
    relays: Option<Vec<String>>,
    dns_discovery: Option<String>,
    disable_networking: bool,
    relay_mode: Option<String>,
    bind_addrs: Option<Vec<String>>,
    proxy_url: Option<String>,
    proxy_from_env: bool,
    keylog: bool,
    #[allow(unused_variables)] compression_level: Option<i32>,
    #[allow(unused_variables)] compression_min_body_bytes: Option<usize>,
    max_concurrency: Option<usize>,
    max_connections_per_peer: Option<usize>,
    request_timeout: Option<u64>,
    max_request_body_bytes: Option<usize>,
) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let opts = NodeOptions {
            key: match key {
                Some(k) => {
                    let arr: [u8; 32] = k.try_into().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err(
                            "secret key must be exactly 32 bytes",
                        )
                    })?;
                    Some(arr)
                }
                None => None,
            },
            idle_timeout_ms: idle_timeout,
            relay_mode,
            relays: relays.unwrap_or_default(),
            bind_addrs: bind_addrs.unwrap_or_default(),
            dns_discovery,
            dns_discovery_enabled: true,
            capabilities: Vec::new(),
            channel_capacity: None,
            max_chunk_size_bytes: None,
            max_consecutive_errors: None,
            disable_networking,
            drain_timeout_ms: None,
            handle_ttl_ms: None,
            max_pooled_connections: None,
            pool_idle_timeout_ms: None,
            max_header_size: None,
            proxy_url,
            proxy_from_env,
            keylog,
            max_concurrency,
            max_connections_per_peer,
            request_timeout_ms: request_timeout,
            max_request_body_bytes,
            drain_timeout_secs: None,
            #[cfg(feature = "compression")]
            compression: if compression_level.is_some() || compression_min_body_bytes.is_some() {
                Some(iroh_http_core::CompressionOptions {
                    level: compression_level.unwrap_or(3),
                    min_body_bytes: compression_min_body_bytes.unwrap_or(512),
                })
            } else {
                None
            },
        };
        let ep = IrohEndpoint::bind(opts).await.map_err(py_err)?;
        Ok(IrohNode { ep })
    })
}

// ── Key operations ────────────────────────────────────────────────────────────

/// Sign arbitrary bytes with a 32-byte Ed25519 secret key.
/// Returns a 64-byte signature.
#[pyfunction]
fn secret_key_sign(secret_key: Vec<u8>, data: Vec<u8>) -> PyResult<Vec<u8>> {
    let key_bytes: [u8; 32] = secret_key
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("secret key must be 32 bytes"))?;
    Ok(iroh_http_core::secret_key_sign(&key_bytes, &data).to_vec())
}

/// Verify a 64-byte Ed25519 signature against a 32-byte public key.
/// Returns True on success, False on failure.
#[pyfunction]
fn public_key_verify(public_key: Vec<u8>, data: Vec<u8>, signature: Vec<u8>) -> PyResult<bool> {
    let key_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("public key must be 32 bytes"))?;
    let sig_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("signature must be 64 bytes"))?;
    Ok(iroh_http_core::public_key_verify(
        &key_bytes, &data, &sig_bytes,
    ))
}

/// Generate a fresh Ed25519 secret key. Returns 32 raw bytes.
#[pyfunction]
fn generate_secret_key() -> Vec<u8> {
    iroh_http_core::generate_secret_key().to_vec()
}

// ── Module ────────────────────────────────────────────────────────────────────

#[pymodule]
fn iroh_http_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(create_node, m)?)?;
    m.add_function(wrap_pyfunction!(secret_key_sign, m)?)?;
    m.add_function(wrap_pyfunction!(public_key_verify, m)?)?;
    m.add_function(wrap_pyfunction!(generate_secret_key, m)?)?;
    m.add_class::<IrohNode>()?;
    m.add_class::<IrohRequest>()?;
    m.add_class::<IrohResponse>()?;
    m.add_class::<HandlerResponse>()?;
    m.add_class::<IrohSession>()?;
    m.add_class::<IrohBidiStream>()?;
    m.add_class::<IrohUniStream>()?;
    #[cfg(feature = "mdns")]
    m.add_class::<IrohBrowseSession>()?;
    Ok(())
}
