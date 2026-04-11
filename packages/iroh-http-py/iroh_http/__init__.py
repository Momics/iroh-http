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
    IrohRequest,
    IrohResponse,
    IrohSession,
    IrohBidiStream,
    IrohUniStream,
)

__all__ = ["create_node", "IrohNode", "IrohRequest", "IrohResponse", "IrohSession", "IrohBidiStream", "IrohUniStream"]
