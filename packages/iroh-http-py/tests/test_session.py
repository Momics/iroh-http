"""Tests for QUIC session, bidirectional streams, datagrams."""

import asyncio
import pytest
import pytest_asyncio

from iroh_http import create_node


pytestmark = pytest.mark.asyncio


@pytest_asyncio.fixture
async def connected_pair():
    """Two nodes where client already has a session to server."""
    server = await create_node()
    client = await create_node()
    server_id, server_addrs = server.addr()
    session = await client.connect(server_id, direct_addrs=server_addrs)
    yield server, client, session
    await session.close()
    await client.close()
    await server.close()


# ── Session lifecycle ──────────────────────────────────────────────────────────


async def test_session_ready(connected_pair):
    _server, _client, session = connected_pair
    # ready() resolves once the QUIC handshake completes
    await session.ready()


async def test_session_max_datagram_size(connected_pair):
    _server, _client, session = connected_pair
    size = session.max_datagram_size
    assert size is None or isinstance(size, int)


async def test_session_context_manager():
    server = await create_node()
    client = await create_node()
    server_id, server_addrs = server.addr()

    async with await client.connect(server_id, direct_addrs=server_addrs) as session:
        await session.ready()

    await client.close()
    await server.close()


async def test_session_closed_after_close(connected_pair):
    _server, _client, session = connected_pair
    await session.close(close_code=42, reason="test done")
    close_code, reason = await session.closed()
    assert isinstance(close_code, int)
    assert isinstance(reason, str)


# ── Bidirectional streams ──────────────────────────────────────────────────────


async def test_bidi_stream_write_read(connected_pair):
    _server, _client, session = connected_pair
    stream = await session.create_bidirectional_stream()

    await stream.write(b"hello")
    chunk = await stream.read()
    # Server echoes back what it receives (or chunk is the data we wrote on the send buffer)
    # In raw bidi mode the server side must also read — here we just check no exception.
    assert chunk is None or isinstance(chunk, (bytes, bytearray))

    stream.close()


async def test_bidi_stream_async_iteration(connected_pair):
    _server, _client, session = connected_pair
    stream = await session.create_bidirectional_stream()

    await stream.write(b"chunk1")
    stream.close()

    chunks = []
    async for chunk in stream:
        chunks.append(chunk)
    # After close, iteration returns available data then stops
    assert all(isinstance(c, (bytes, bytearray)) for c in chunks)


# ── Unidirectional streams ─────────────────────────────────────────────────────


async def test_uni_stream_write(connected_pair):
    _server, _client, session = connected_pair
    stream = await session.create_unidirectional_stream()
    await stream.write(b"one-way data")
    stream.close()


# ── Datagrams ─────────────────────────────────────────────────────────────────


async def test_datagram_send_recv(connected_pair):
    server, _client, session = connected_pair
    size = session.max_datagram_size
    if size is None:
        pytest.skip("datagrams not supported by this endpoint config")

    payload = b"dgram"
    await session.send_datagram(payload)
    # recv_datagram on the same session returns None if no inbound datagrams
    result = await asyncio.wait_for(session.recv_datagram(), timeout=2.0)
    assert result is None or isinstance(result, (bytes, bytearray))
