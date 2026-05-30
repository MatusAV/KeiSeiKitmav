// SPDX-License-Identifier: Apache-2.0
//! XChaCha20-Poly1305 + HKDF-SHA256 + x25519-ECDH unsealing of a
//! browser-sealed token.
//!
//! Mirrors `keisei-marketplace/src/lib/crypto-box.ts::sealBoxToVps` on the
//! browser side: the user's ephemeral X25519 secret + our VPS X25519 public
//! produce a shared secret via ECDH; HKDF-SHA256 derives the 32-byte
//! XChaCha20-Poly1305 key (info=HKDF_INFO).

use anyhow::{anyhow, bail, Result};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use serde::Deserialize;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use super::pem::{b64decode, parse_x25519_pkcs8_pem};

pub const HKDF_INFO: &[u8] = b"keibuddy-token-v1";

#[derive(Debug, Deserialize)]
pub struct SealedBlob {
    #[serde(rename = "ciphertext", alias = "ciphertextB64")]
    pub ciphertext_b64: String,
    #[serde(rename = "nonce", alias = "nonceB64")]
    pub nonce_b64: String,
    #[serde(rename = "ephPub", alias = "ephPubB64")]
    pub eph_pub_b64: String,
}

/// Decrypt a `SealedBlob` using the VPS X25519 private key (PKCS#8 PEM).
/// Zeroises the raw key + derived AEAD key before returning.
pub fn decrypt_blob(vps_priv_pem: &str, blob: &SealedBlob) -> Result<Vec<u8>> {
    let mut priv_raw = parse_x25519_pkcs8_pem(vps_priv_pem)?;
    let vps_secret = StaticSecret::from(priv_raw);
    priv_raw.zeroize();

    let eph_pub_bytes = b64decode(&blob.eph_pub_b64)?;
    if eph_pub_bytes.len() != 32 {
        bail!("ephPub must be 32 bytes, got {}", eph_pub_bytes.len());
    }
    let mut eph_arr = [0u8; 32];
    eph_arr.copy_from_slice(&eph_pub_bytes);
    let eph_pub = PublicKey::from(eph_arr);

    let shared = vps_secret.diffie_hellman(&eph_pub);

    let hk = Hkdf::<Sha256>::new(None, shared.as_bytes());
    let mut key = [0u8; 32];
    hk.expand(HKDF_INFO, &mut key)
        .map_err(|e| anyhow!("HKDF expand failed: {e}"))?;

    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce_bytes = b64decode(&blob.nonce_b64)?;
    if nonce_bytes.len() != 24 {
        bail!("nonce must be 24 bytes, got {}", nonce_bytes.len());
    }
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ct = b64decode(&blob.ciphertext_b64)?;
    let pt = cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|_| anyhow!("XChaCha20-Poly1305 decryption failed (wrong key or tamper)"))?;

    key.zeroize();
    Ok(pt)
}
