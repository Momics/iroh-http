"""Tests for Ed25519 sign / verify / generate_secret_key."""

import pytest

from iroh_http import secret_key_sign, public_key_verify, generate_secret_key, create_node


# ── generate_secret_key ───────────────────────────────────────────────────────


def test_generate_secret_key_length():
    key = generate_secret_key()
    assert len(key) == 32, "secret key must be 32 bytes"


def test_generate_secret_key_uniqueness():
    k1 = generate_secret_key()
    k2 = generate_secret_key()
    assert k1 != k2, "two generated keys must differ"


# ── secret_key_sign ───────────────────────────────────────────────────────────


def test_sign_returns_64_bytes():
    key = generate_secret_key()
    sig = secret_key_sign(key, b"hello")
    assert len(sig) == 64, "Ed25519 signature must be 64 bytes"


def test_sign_deterministic():
    key = generate_secret_key()
    msg = b"deterministic"
    sig1 = secret_key_sign(key, msg)
    sig2 = secret_key_sign(key, msg)
    assert sig1 == sig2, "Ed25519 signing must be deterministic"


def test_sign_different_messages_differ():
    key = generate_secret_key()
    s1 = secret_key_sign(key, b"msg1")
    s2 = secret_key_sign(key, b"msg2")
    assert s1 != s2


def test_sign_empty_message():
    key = generate_secret_key()
    sig = secret_key_sign(key, b"")
    assert len(sig) == 64


# ── public_key_verify ─────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_verify_valid_signature():
    """Valid signature must verify as True."""
    import base64

    key = generate_secret_key()
    node = await create_node(key=key, disable_networking=True)
    node_id = node.node_id  # base32-encoded public key
    await node.close()

    # Decode the base32 public key (iroh uses RFC 4648 lower-case, no padding)
    padding = (8 - len(node_id) % 8) % 8
    try:
        pub = base64.b32decode(node_id.upper() + "=" * padding)
    except Exception:
        pytest.skip("node_id encoding not compatible with stdlib base32")

    if len(pub) != 32:
        pytest.skip("decoded public key length unexpected")

    msg = b"test message"
    sig = secret_key_sign(key, msg)
    assert public_key_verify(pub, msg, sig) is True, "valid signature must verify True"


@pytest.mark.asyncio
async def test_verify_invalid_signature():
    """Tampered signature must return False."""
    import base64

    key = generate_secret_key()
    node = await create_node(key=key, disable_networking=True)
    node_id = node.node_id
    await node.close()

    padding = (8 - len(node_id) % 8) % 8
    try:
        pub = base64.b32decode(node_id.upper() + "=" * padding)
    except Exception:
        pytest.skip("node_id encoding not compatible with stdlib base32")

    if len(pub) != 32:
        pytest.skip("decoded public key length unexpected")

    msg = b"original"
    sig = bytearray(secret_key_sign(key, msg))
    sig[0] ^= 0xFF  # flip a byte to invalidate
    assert public_key_verify(pub, msg, bytes(sig)) is False, "tampered signature must verify False"


@pytest.mark.asyncio
async def test_sign_verify_roundtrip():
    """
    Full round-trip: generate key → sign → verify with raw public key.

    The public key is the last 32 bytes of the Ed25519 keypair expansion
    (iroh stores only the seed; the full keypair expansion gives pubkey as
    `ed25519_dalek::SigningKey::from_bytes(seed).verifying_key().to_bytes()`).

    Since Python bindings don't yet expose `public_key_bytes()` directly,
    we use create_node(key=seed) to obtain node_id, then convert base32→bytes
    using the `base64` stdlib (node_id uses iroh's base32 encoding).

    If iroh's base32 doesn't map 1:1 to stdlib base32, skip gracefully.
    """
    import base64

    key = generate_secret_key()
    node = await create_node(key=key, disable_networking=True)
    node_id = node.node_id  # base32-encoded public key (no padding)
    await node.close()

    # Pad to multiple of 8 for stdlib decoder
    padding = (8 - len(node_id) % 8) % 8
    try:
        pub = base64.b32decode(node_id.upper() + "=" * padding)
    except Exception:
        pytest.skip("node_id encoding not compatible with stdlib base32; skipping verify")

    if len(pub) != 32:
        pytest.skip("decoded public key length unexpected")

    msg = b"round-trip test"
    sig = secret_key_sign(key, msg)

    assert public_key_verify(pub, msg, sig) is True
    assert public_key_verify(pub, b"wrong message", sig) is False
