"""
Smoke test — verifies the native module loads and basic operations work.

Run: python tests/smoke.py

Note: the package must be installed first:
    maturin develop --features pyo3/extension-module
    # or:
    pip install -e .
"""

import asyncio
import sys


async def main() -> None:
    try:
        from iroh_http import create_node, generate_secret_key
    except ImportError as e:
        print(f"❌ Import failed: {e}")
        print("   Build the package first: maturin develop --features pyo3/extension-module")
        sys.exit(1)

    print("1. create_node...")
    node = await create_node(disable_networking=True)
    assert node.node_id, "node_id should be a non-empty string"
    assert len(node.node_id) > 10, "node_id should be base32-encoded"
    print(f"   node_id = {node.node_id}")

    print("2. keypair...")
    kp = node.keypair
    assert isinstance(kp, bytes), "keypair should be bytes"
    assert len(kp) == 32, f"keypair should be 32 bytes, got {len(kp)}"

    print("3. generate_secret_key...")
    sk2 = generate_secret_key()
    assert sk2 is not None
    assert isinstance(sk2, bytes), "generate_secret_key should return bytes"
    assert len(sk2) == 32, "secret key should be 32 bytes"

    print("4. close...")
    await node.close()

    print("\n✅ All smoke tests passed.")


if __name__ == "__main__":
    asyncio.run(main())
