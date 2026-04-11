"""Shared pytest fixtures for iroh-http Python tests."""

import asyncio
import pytest
import pytest_asyncio

from iroh_http import create_node


@pytest_asyncio.fixture
async def node():
    """A node with networking disabled — suitable for offline/unit tests."""
    n = await create_node(disable_networking=True)
    yield n
    await n.close()


@pytest_asyncio.fixture
async def node_pair():
    """Two fully-networked nodes that can reach each other via direct addresses."""
    a = await create_node()
    b = await create_node()
    yield a, b
    await a.close()
    await b.close()
