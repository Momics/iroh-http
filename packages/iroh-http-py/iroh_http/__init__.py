"""
iroh-http — peer-to-peer HTTP over QUIC (Iroh).

Re-exports the native Rust extension (`iroh_http_py`) under a clean
top-level namespace.

Usage::

    import asyncio
    from iroh_http import create_node

    async def main():
        node = await create_node()
        print(node.node_id)

        response = await node.fetch(peer_id, "httpi://peer/api/data")
        body = await response.bytes()

        async def handler(request):
            body = await request.body()
            return {"status": 200, "headers": [], "body": b"hello"}

        node.serve(handler)
        await node.close()

    asyncio.run(main())
"""

from .iroh_http_py import (  # noqa: F401
    create_node,
    IrohNode,
    IrohServeHandle,
    IrohRequest,
    IrohResponse,
    IrohSession,
    IrohBidiStream,
    IrohUniStream,
    HandlerResponse,
    secret_key_sign,
    public_key_verify,
    generate_secret_key,
)

try:
    from .iroh_http_py import IrohBrowseSession  # noqa: F401
    _has_browse_session = True
except ImportError:
    _has_browse_session = False  # mdns feature not enabled

__all__ = [
    "create_node",
    "IrohNode",
    "IrohServeHandle",
    "IrohRequest",
    "IrohResponse",
    "IrohSession",
    "IrohBidiStream",
    "IrohUniStream",
    "HandlerResponse",
    "secret_key_sign",
    "public_key_verify",
    "generate_secret_key",
    # IrohBrowseSession appended below when the mdns feature is present
]

if _has_browse_session:
    __all__.append("IrohBrowseSession")
