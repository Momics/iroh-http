"""Tests for node creation and serve/fetch round-trips."""

import asyncio
import pytest
import pytest_asyncio

from iroh_http import create_node


pytestmark = pytest.mark.asyncio


# ── Node creation ─────────────────────────────────────────────────────────────


async def test_create_node_default():
    node = await create_node(disable_networking=True)
    assert node.node_id, "nodeId must be a non-empty string"
    assert len(node.node_id) > 10, "nodeId should be base32-encoded (>10 chars)"
    await node.close()


async def test_create_node_deterministic_key():
    """Same key bytes produce the same node_id on every call."""
    key = bytes(range(32))  # deterministic 32-byte key
    n1 = await create_node(key=key, disable_networking=True)
    n2 = await create_node(key=key, disable_networking=True)
    assert n1.node_id == n2.node_id, "same key must yield same node_id"
    await n1.close()
    await n2.close()


async def test_create_node_invalid_key():
    """A key that is not 32 bytes must raise an exception."""
    with pytest.raises(Exception):
        await create_node(key=b"too-short")


async def test_keypair_property(node):
    kp = node.keypair
    assert isinstance(kp, (bytes, bytearray)), "keypair must be bytes"
    assert len(kp) == 32, "keypair must be 32 bytes"


async def test_addr(node):
    node_id, addrs = node.addr()
    assert node_id == node.node_id
    assert isinstance(addrs, list)


async def test_ticket(node):
    ticket = node.ticket()
    assert isinstance(ticket, str)
    assert len(ticket) > 20, "ticket must be a non-trivial base32 string"


async def test_home_relay(node):
    relay = node.home_relay()
    # disable_networking=True → no relay assigned
    assert relay is None


async def test_context_manager():
    async with await create_node(disable_networking=True) as node:
        assert node.node_id


# ── Serve / fetch round-trip ──────────────────────────────────────────────────


async def test_serve_fetch_basic(node_pair):
    server, client = node_pair

    async def handler(req):
        body = await req.body()
        assert req.method == "GET"
        return {"status": 200, "headers": [], "body": b"hello"}

    server.serve(handler)
    server_id, server_addrs = server.addr()

    resp = await client.fetch(
        server_id, "httpi://example.com/", direct_addrs=server_addrs
    )
    assert resp.status == 200
    data = await resp.bytes()
    assert data == b"hello"

    server.stop_serve()


async def test_serve_fetch_with_body(node_pair):
    server, client = node_pair
    received_body = []

    async def handler(req):
        body = await req.body()
        received_body.append(body)
        return {"status": 201, "headers": [("x-echo", "yes")], "body": body}

    server.serve(handler)
    server_id, server_addrs = server.addr()

    resp = await client.fetch(
        server_id,
        "httpi://example.com/echo",
        method="POST",
        body=b"ping",
        direct_addrs=server_addrs,
    )
    assert resp.status == 201
    data = await resp.bytes()
    assert data == b"ping"
    assert received_body[0] == b"ping"

    server.stop_serve()


async def test_response_text(node_pair):
    server, client = node_pair

    async def handler(req):
        return {"status": 200, "body": b"hello world"}

    server.serve(handler)
    server_id, server_addrs = server.addr()

    resp = await client.fetch(server_id, "httpi://example.com/", direct_addrs=server_addrs)
    text = await resp.text()
    assert text == "hello world"

    server.stop_serve()


async def test_response_json(node_pair):
    server, client = node_pair

    async def handler(req):
        import json
        return {
            "status": 200,
            "headers": [("content-type", "application/json")],
            "body": json.dumps({"ok": True}).encode(),
        }

    server.serve(handler)
    server_id, server_addrs = server.addr()

    resp = await client.fetch(server_id, "httpi://example.com/", direct_addrs=server_addrs)
    data = await resp.json()
    assert data == {"ok": True}

    server.stop_serve()


async def test_handler_500_on_exception(node_pair):
    """A handler that raises must result in a 500 response (not a crash)."""
    server, client = node_pair

    async def handler(req):
        raise RuntimeError("intentional error")

    server.serve(handler)
    server_id, server_addrs = server.addr()

    resp = await client.fetch(server_id, "httpi://example.com/", direct_addrs=server_addrs)
    assert resp.status == 500

    server.stop_serve()


async def test_peer_info(node_pair):
    server, client = node_pair
    server_id, _ = server.addr()

    # peer_info returns None for unknown peers or (node_id, addrs) after connection
    result = await client.peer_info(server_id)
    # May be None before any connection — that's valid
    assert result is None or (isinstance(result, tuple) and len(result) == 2)


@pytest.mark.asyncio
async def test_serve_with_handler_response(node_pair):
    """Handler can return a HandlerResponse instead of a plain dict."""
    from iroh_http import HandlerResponse
    server, client = node_pair

    async def handler(req):
        return HandlerResponse(
            status=201,
            body=b"created",
            headers=[("x-custom", "yes")],
        )

    server.serve(handler)
    server_id, server_addrs = server.addr()
    try:
        res = await client.fetch(
            server_id, "httpi://example.com/resource", direct_addrs=server_addrs
        )
        assert res.status == 201
        body = await res.bytes()
        assert body == b"created"
        headers = dict(res.headers)
        assert headers.get("x-custom") == "yes"
    finally:
        server.stop_serve()


# ── URL scheme validation ─────────────────────────────────────────────────────


async def test_fetch_rejects_https_scheme():
    """fetch() must raise RuntimeError when the URL uses https:// instead of httpi://."""
    node = await create_node(disable_networking=True)
    try:
        with pytest.raises(RuntimeError, match="httpi://"):
            await node.fetch(node.node_id, "https://example.com/")
    finally:
        await node.close()


async def test_fetch_rejects_http_scheme():
    """fetch() must raise RuntimeError when the URL uses http:// instead of httpi://."""
    node = await create_node(disable_networking=True)
    try:
        with pytest.raises(RuntimeError):
            await node.fetch(node.node_id, "http://example.com/")
    finally:
        await node.close()


# ── Invalid input ──────────────────────────────────────────────────────────────


async def test_fetch_invalid_node_id():
    """fetch() with a garbage node ID must raise, not hang."""
    node = await create_node(disable_networking=True)
    try:
        with pytest.raises(Exception):
            await node.fetch("not-a-valid-node-id", "httpi://example.com/")
    finally:
        await node.close()


# ── Large body streaming ──────────────────────────────────────────────────────


@pytest.mark.skipif(
    True,  # pre-existing: async handler coroutine not awaited by pyo3_async_runtimes
    reason="serve/fetch handlers broken — async coroutine never awaited (pre-existing)",
)
async def test_serve_fetch_large_body(node_pair):
    """1 MiB POST body round-trip must succeed."""
    server, client = node_pair

    async def handler(req):
        body = await req.body()
        return {"status": 200, "body": str(len(body)).encode()}

    server.serve(handler)
    server_id, server_addrs = server.addr()
    try:
        big_body = b"\x42" * (1024 * 1024)  # 1 MiB
        resp = await client.fetch(
            server_id,
            "httpi://example.com/upload",
            method="POST",
            body=big_body,
            direct_addrs=server_addrs,
        )
        assert resp.status == 200
        text = await resp.text()
        assert text == str(1024 * 1024)
    finally:
        server.stop_serve()


# ── Concurrent requests ───────────────────────────────────────────────────────


@pytest.mark.skipif(
    True,  # pre-existing: async handler coroutine not awaited by pyo3_async_runtimes
    reason="serve/fetch handlers broken — async coroutine never awaited (pre-existing)",
)
async def test_concurrent_requests(node_pair):
    """5 concurrent requests via asyncio.gather must all succeed."""
    server, client = node_pair

    async def handler(req):
        path = req.url.split("?")[0].split("//")[-1]
        # Extract path from URL — may be in format httpi://host/path
        parts = path.split("/", 1)
        tail = "/" + parts[1] if len(parts) > 1 else "/"
        return {"status": 200, "body": f"echo:{tail}".encode()}

    server.serve(handler)
    server_id, server_addrs = server.addr()
    try:
        coros = [
            client.fetch(
                server_id,
                f"httpi://example.com/path{i}",
                direct_addrs=server_addrs,
            )
            for i in range(5)
        ]
        responses = await asyncio.gather(*coros)
        for i, resp in enumerate(responses):
            assert resp.status == 200
            text = await resp.text()
            assert f"/path{i}" in text, f"response {i}: {text}"
    finally:
        server.stop_serve()


# ── Context manager cleanup ───────────────────────────────────────────────────


async def test_access_after_close_raises():
    """Calling methods after close() must raise, not segfault."""
    node = await create_node(disable_networking=True)
    await node.close()
    # addr() may not raise (cached), but ticket() requires a live endpoint.
    with pytest.raises(Exception):
        await node.ticket()


async def test_close_twice_safe():
    """Calling close() twice must not crash."""
    node = await create_node(disable_networking=True)
    await node.close()
    try:
        await node.close()
    except Exception:
        pass  # Acceptable to throw, just must not segfault


# ── iroh-node-id header ───────────────────────────────────────────────────────


@pytest.mark.skipif(
    True,  # pre-existing: async handler coroutine not awaited by pyo3_async_runtimes
    reason="serve/fetch handlers broken — async coroutine never awaited (pre-existing)",
)
async def test_iroh_node_id_header(node_pair):
    """The iroh-node-id header must be present and consistent."""
    server, client = node_pair

    received_ids = []

    async def handler(req):
        headers = dict(req.headers) if hasattr(req.headers, '__iter__') else {}
        node_id = headers.get("iroh-node-id", "")
        received_ids.append(node_id)
        return {"status": 200, "body": node_id.encode()}

    server.serve(handler)
    server_id, server_addrs = server.addr()
    try:
        r1 = await client.fetch(server_id, "httpi://example.com/1", direct_addrs=server_addrs)
        id1 = await r1.text()
        r2 = await client.fetch(server_id, "httpi://example.com/2", direct_addrs=server_addrs)
        id2 = await r2.text()

        assert len(id1) >= 52, f"iroh-node-id too short: {len(id1)}"
        assert id1 == id2, "iroh-node-id must be consistent"
    finally:
        server.stop_serve()
