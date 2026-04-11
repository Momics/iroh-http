//! Python bindings for iroh-http.
//!
//! Exports `create_node`, `IrohNode`, `IrohRequest`, `IrohResponse` via PyO3.

use std::sync::Arc;

use bytes::Bytes;
use iroh_http_core::{
    server::{respond, ServeOptions},
    stream::{finish_body, next_chunk, send_chunk, make_body_channel},
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
    status:      u16,
    headers:     Vec<(String, String)>,
    body_handle: u32,
    url:         String,
}

#[pymethods]
impl IrohResponse {
    /// HTTP status code.
    #[getter]
    fn status(&self) -> u16 { self.status }

    /// Response headers as a list of `(name, value)` tuples.
    #[getter]
    fn headers(&self) -> Vec<(String, String)> { self.headers.clone() }

    /// Final URL of the responding peer.
    #[getter]
    fn url(&self) -> &str { &self.url }

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
            Python::with_gil(|py| {
                Ok(PyBytes::new_bound(py, &buf).into_any().unbind())
            })
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
            String::from_utf8(buf)
                .map_err(|e| py_err(format!("UTF-8 decode error: {e}")))
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
            let text = String::from_utf8(buf)
                .map_err(|e| py_err(format!("UTF-8 decode error: {e}")))?;
            Python::with_gil(|py| {
                let json_mod = py.import_bound("json")?;
                Ok(json_mod.call_method1("loads", (text,))?.into_any().unbind())
            })
        })
    }
}

// ── IrohRequest ──────────────────────────────────────────────────────────────

/// Incoming request passed to the `serve` handler.
#[pyclass]
struct IrohRequest {
    pub req_body_handle: u32,
    pub method:          String,
    pub url:             String,
    pub headers:         Vec<(String, String)>,
    pub remote_node_id:  String,
}

#[pymethods]
impl IrohRequest {
    #[getter]
    fn method(&self) -> &str { &self.method }

    #[getter]
    fn url(&self) -> &str { &self.url }

    #[getter]
    fn remote_node_id(&self) -> &str { &self.remote_node_id }

    #[getter]
    fn headers(&self) -> Vec<(String, String)> { self.headers.clone() }

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
            Python::with_gil(|py| {
                Ok(PyBytes::new_bound(py, &buf).into_any().unbind())
            })
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

    /// Send an HTTP request to a remote peer.
    ///
    /// `peer_id` is the base32-encoded public key of the target node.
    /// Returns an `IrohResponse` coroutine.
    #[pyo3(signature = (peer_id, url, method="GET", headers=None, body=None, direct_addrs=None))]
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
        let ep      = self.ep.clone();
        let method  = method.to_owned();
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
            let res = iroh_http_core::fetch(&ep, &peer_id, &url, &method, &headers, body_reader, None, addrs.as_deref())
                .await
                .map_err(py_err)?;
            Ok(IrohResponse {
                status:      res.status,
                headers:     res.headers,
                body_handle: res.body_handle,
                url:         res.url,
            })
        })
    }

    /// Register an `async def handler(request: IrohRequest)` and start accepting
    /// incoming requests in the background.
    ///
    /// The handler must return a dict with keys `status` (int), `headers`
    /// (list of `(name, value)` tuples), and `body` (bytes).
    fn serve(&self, _py: Python<'_>, handler: PyObject) -> PyResult<()> {
        let ep      = self.ep.clone();
        let handler = Arc::new(handler);

        // Use an mpsc channel so the synchronous `on_request` callback can
        // hand payloads off to an async polling loop without blocking.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<iroh_http_core::RequestPayload>(64);

        let handle = iroh_http_core::serve(
            ep.clone(),
            ServeOptions::default(),
            move |payload| {
                let tx = tx.clone();
                // `on_request` is synchronous; spawn to avoid blocking the accept task.
                tokio::spawn(async move {
                    let _ = tx.send(payload).await;
                });
            },
        );
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

    /// Close the endpoint and release all resources.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let ep = self.ep.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            ep.close().await;
            Ok(())
        })
    }
}

// ── Serve request handler ─────────────────────────────────────────────────────

async fn handle_request(handler: Arc<PyObject>, payload: iroh_http_core::RequestPayload) {
    let req_handle    = payload.req_handle;
    let res_body_handle = payload.res_body_handle;

    // Build the IrohRequest and call the Python handler to get a coroutine.
    let fut = Python::with_gil(|py| {
        let ireq = IrohRequest {
            req_body_handle: payload.req_body_handle,
            method:          payload.method.clone(),
            url:             payload.url.clone(),
            headers:         payload.headers.clone(),
            remote_node_id:  payload.remote_node_id.clone(),
        };
        let py_req = Bound::new(py, ireq).map_err(py_err)?;
        let coro   = handler.call1(py, (py_req,)).map_err(|e| py_err(e))?;
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

    let outcome = Python::with_gil(|py| -> PyResult<(u16, Vec<(String, String)>, Vec<u8>)> {
        let obj = py_result?;
        let dict = obj.bind(py).downcast::<PyDict>()?.clone();
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
///   key          — 32 bytes (Ed25519 secret key).  Omit to generate a fresh identity.
///   idle_timeout — milliseconds before idle connections are closed.
///   relays       — list of custom relay server URL strings.
///   dns_discovery — custom DNS discovery server URL.
#[pyfunction]
#[pyo3(signature = (key=None, idle_timeout=None, relays=None, dns_discovery=None, disable_networking=false))]
fn create_node<'py>(
    py: Python<'py>,
    key: Option<Vec<u8>>,
    idle_timeout: Option<u64>,
    relays: Option<Vec<String>>,
    dns_discovery: Option<String>,
    disable_networking: bool,
) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let opts = NodeOptions {
            key:                    key.and_then(|k| k.try_into().ok()),
            idle_timeout_ms:        idle_timeout,
            relays:                 relays.unwrap_or_default(),
            dns_discovery,
            capabilities:           Vec::new(),
            channel_capacity:       None,
            max_chunk_size_bytes:   None,
            max_consecutive_errors: None,
            discovery:              None,
            disable_networking,
            drain_timeout_ms:       None,
            handle_ttl_ms:          None,
            max_pooled_connections: None,
        };
        let ep = IrohEndpoint::bind(opts).await.map_err(py_err)?;
        Ok(IrohNode { ep })
    })
}

// ── Module ────────────────────────────────────────────────────────────────────

#[pymodule]
fn iroh_http_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(create_node, m)?)?;
    m.add_class::<IrohNode>()?;
    m.add_class::<IrohRequest>()?;
    m.add_class::<IrohResponse>()?;
    Ok(())
}
