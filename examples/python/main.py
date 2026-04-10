"""
iroh-http Python example.

Usage:
  python main.py server
  python main.py client <peer-id>
"""

import sys
import iroh_http

mode = sys.argv[1] if len(sys.argv) > 1 else None
peer_id = sys.argv[2] if len(sys.argv) > 2 else None

node = iroh_http.create_node()
print("Node ID:", node.node_id())

if mode == "server":
    def handler(req):
        print(f"Incoming: {req.method} {req.path}")
        return iroh_http.Response(200, b"Hello from Python iroh-http!")

    node.serve(handler)
    print("Serving. Share your node ID with the client.")
    input("Press Enter to stop...\n")
elif mode == "client" and peer_id:
    res = node.fetch(peer_id, "/hello")
    print("Status:", res.status)
    print("Body:", res.text())
    node.close()
else:
    print("Usage: python main.py server | python main.py client <peer-id>")
    sys.exit(1)
