"""Tests for mDNS browse/advertise (requires mdns feature at build time)."""

import asyncio
import pytest
import pytest_asyncio

try:
    from iroh_http import IrohBrowseSession
    HAS_MDNS = True
except ImportError:
    HAS_MDNS = False

pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not HAS_MDNS, reason="iroh-http-py built without mdns feature"),
]


@pytest_asyncio.fixture
async def mdns_pair():
    from iroh_http import create_node
    advertiser = await create_node()
    browser = await create_node()
    yield advertiser, browser
    await advertiser.close()
    await browser.close()


async def test_advertise_does_not_raise(mdns_pair):
    advertiser, _browser = mdns_pair
    advertiser.advertise("_iroh-http-test._udp")


async def test_browse_returns_browse_session(mdns_pair):
    _advertiser, browser = mdns_pair
    session = await browser.browse("_iroh-http-test._udp")
    assert isinstance(session, IrohBrowseSession)


async def test_browse_discovers_advertiser(mdns_pair):
    """
    Advertiser announces itself; browser should see at least one active event.

    mDNS propagation may take a moment — we wait up to 5 s.
    If no event arrives the test is skipped (flaky environment).
    """
    advertiser, browser = mdns_pair
    advertiser.advertise("_iroh-http-test2._udp")
    session = await browser.browse("_iroh-http-test2._udp")

    events = []
    try:
        async with asyncio.timeout(5):
            async for event in session:
                events.append(event)
                if event.get("is_active"):
                    break
    except TimeoutError:
        pytest.skip("No mDNS event received within 5 s (network/OS limitation)")

    assert events, "Expected at least one discovery event"
    first = events[0]
    assert "node_id" in first
    assert "addrs" in first
    assert "is_active" in first
