"""
iroh-http Python example.

Usage:
  python main.py server
  python main.py client <peer-id>
"""

import asyncio
import sys
import iroh_http


async def main() -> None:
    mode = sys.argv[1] if len(sys.argv) > 1 else None
    peer_id = sys.argv[2] if len(sys.argv) > 2 else None

    node = await iroh_http.create_node()
    print("Node ID:", node.node_id)

    if mode == "server":
        async def handler(req: iroh_http.IrohRequest) -> dict:
            path = req.url.split("://", 1)[-1].split("/", 1)[-1]
            path = "/" + path if not path.startswith("/") else path
            print(f"Incoming: {req.method} {path}")
            return {
                "status": 200,
                "headers": [("content-type", "text/plain")],
                "body": f"Hello from Python iroh-http! Path: {path}".encode(),
            }

        node.serve(handler)
        print("Serving. Share your node ID with the client.")
        input("Press Enter to stop...\n")
        await node.close()

    elif mode == "client" and peer_id:
        res = await node.fetch(peer_id, "/hello")
        print("Status:", res.status)
        print("Body:", await res.text())
        await node.close()

    else:
        print("Usage: python main.py server | python main.py client <peer-id>")
        sys.exit(1)


asyncio.run(main())
