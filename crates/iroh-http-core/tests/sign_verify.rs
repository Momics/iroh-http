//! Sign / verify integration tests.

#[test]
fn sign_verify_round_trip() {
    let key = iroh::SecretKey::generate(&mut rand::rng());
    let data = b"hello world";
    let sig = key.sign(data);
    assert!(key.public().verify(data, &sig).is_ok());
}

#[test]
fn verify_rejects_bad_signature() {
    let key = iroh::SecretKey::generate(&mut rand::rng());
    let data = b"hello world";
    let mut sig_bytes = key.sign(data).to_bytes();
    sig_bytes[0] ^= 0xFF; // corrupt
    let sig = iroh::Signature::from_bytes(&sig_bytes);
    assert!(key.public().verify(data, &sig).is_err());
}

#[test]
fn generate_produces_unique_keys() {
    let k1 = iroh::SecretKey::generate(&mut rand::rng());
    let k2 = iroh::SecretKey::generate(&mut rand::rng());
    assert_ne!(k1.to_bytes(), k2.to_bytes());
}
