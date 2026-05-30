// SPDX-License-Identifier: Apache-2.0
//! VPS-side decryption of a browser-sealed bot token.
//!
//! Mirrors `keisei-marketplace/src/lib/crypto-box.ts::sealBoxToVps`. The
//! browser seals the token with XChaCha20-Poly1305, the key being derived
//! via HKDF-SHA256 from x25519-ECDH between the browser's ephemeral private
//! and our VPS public (registered in marketplace at provisioning time).
//!
//! Decomposed into three cubes per Constructor Pattern (provision_decrypt.rs
//! used to be 402 LOC, audit H5 2026-05-30):
//!
//! - [`pem`] — PKCS#8 PEM parse / write + base64 helpers
//! - [`crypto`] — `SealedBlob` type + `decrypt_blob` (ECDH + HKDF + AEAD)
//! - [`cli`] — filesystem orchestration: `decrypt_and_export` + `genkeys`
//!
//! Public API is re-exported below for backward compatibility with
//! existing callers (`crate::provision_decrypt::SealedBlob`, etc.).

pub mod cli;
pub mod crypto;
pub mod pem;

// Backward-compat re-exports — keep call sites unchanged.
pub use cli::{decrypt_and_export, genkeys};
pub use crypto::{decrypt_blob, SealedBlob};

#[cfg(test)]
mod tests {
    use super::cli::{decrypt_and_export, genkeys};
    use super::crypto::{decrypt_blob, SealedBlob, HKDF_INFO};
    use super::pem::{b64decode, parse_x25519_pkcs8_pem, pem_begin, pem_end, write_x25519_pkcs8_pem};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
    use hkdf::Hkdf;
    use sha2::Sha256;
    use x25519_dalek::{PublicKey, StaticSecret};

    fn seal(plaintext: &[u8], vps_pub_b64: &str) -> SealedBlob {
        let vps_pub_bytes = STANDARD.decode(vps_pub_b64).unwrap();
        let mut vp = [0u8; 32];
        vp.copy_from_slice(&vps_pub_bytes);
        let vps_pub = PublicKey::from(vp);

        let eph_secret = StaticSecret::random_from_rng(rand_core::OsRng);
        let eph_pub = PublicKey::from(&eph_secret);

        let shared = eph_secret.diffie_hellman(&vps_pub);
        let hk = Hkdf::<Sha256>::new(None, shared.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(HKDF_INFO, &mut key).unwrap();

        let cipher = XChaCha20Poly1305::new((&key).into());
        let nonce_bytes = rand_xchacha_nonce();
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ct = cipher.encrypt(nonce, plaintext).unwrap();

        SealedBlob {
            ciphertext_b64: STANDARD.encode(&ct),
            nonce_b64: STANDARD.encode(nonce_bytes),
            eph_pub_b64: STANDARD.encode(eph_pub.as_bytes()),
        }
    }

    fn rand_xchacha_nonce() -> [u8; 24] {
        use rand_core::RngCore;
        let mut n = [0u8; 24];
        rand_core::OsRng.fill_bytes(&mut n);
        n
    }

    #[test]
    fn roundtrip_seal_then_decrypt() {
        let tmpdir = tempdir_unique();
        let key_path = tmpdir.join("vps.key");
        let pub_b64 = genkeys(&key_path).unwrap();
        let pem = std::fs::read_to_string(&key_path).unwrap();

        // Fixture intentionally generic: the round-trip test verifies bytes
        // travel unchanged through seal/decrypt; the content is irrelevant.
        // Previously this LOOKED like a Telegram bot token and tripped
        // GitHub Secret Scanning (Copilot scanner false-positive 2026-05-29).
        let secret = "kei-buddy-roundtrip-fixture-not-a-secret";
        let blob = seal(secret.as_bytes(), &pub_b64);

        let pt = decrypt_blob(&pem, &blob).unwrap();
        assert_eq!(std::str::from_utf8(&pt).unwrap(), secret);
    }

    #[test]
    fn decrypt_and_export_writes_env_file() {
        let tmpdir = tempdir_unique();
        let key_path = tmpdir.join("vps.key");
        let blob_path = tmpdir.join("blob.json");
        let env_path = tmpdir.join("keisei.env");
        std::fs::write(&env_path, "LLM_API_BASE=https://api.keisei.app\n").unwrap();

        let pub_b64 = genkeys(&key_path).unwrap();
        let secret = "kei-buddy-export-fixture-not-a-secret";
        let blob = seal(secret.as_bytes(), &pub_b64);
        let blob_json = format!(
            r#"{{"ciphertext":"{}","nonce":"{}","ephPub":"{}"}}"#,
            blob.ciphertext_b64, blob.nonce_b64, blob.eph_pub_b64
        );
        std::fs::write(&blob_path, blob_json).unwrap();

        decrypt_and_export(&key_path, &blob_path, &env_path).unwrap();

        let env = std::fs::read_to_string(&env_path).unwrap();
        assert!(env.contains("LLM_API_BASE=https://api.keisei.app"));
        assert!(env.contains(&format!("BOT_TOKEN={secret}")));
        assert!(env.contains(&format!("TELEGRAM_BOT_TOKEN={secret}")));
    }

    #[test]
    fn decrypt_and_export_replaces_existing_token() {
        let tmpdir = tempdir_unique();
        let key_path = tmpdir.join("vps.key");
        let blob_path = tmpdir.join("blob.json");
        let env_path = tmpdir.join("keisei.env");
        std::fs::write(&env_path, "BOT_TOKEN=stale\nLLM_API_BASE=x\n").unwrap();

        let pub_b64 = genkeys(&key_path).unwrap();
        let secret = "fresh:token";
        let blob = seal(secret.as_bytes(), &pub_b64);
        let blob_json = format!(
            r#"{{"ciphertextB64":"{}","nonceB64":"{}","ephPubB64":"{}"}}"#,
            blob.ciphertext_b64, blob.nonce_b64, blob.eph_pub_b64
        );
        std::fs::write(&blob_path, blob_json).unwrap();

        decrypt_and_export(&key_path, &blob_path, &env_path).unwrap();

        let env = std::fs::read_to_string(&env_path).unwrap();
        assert!(!env.contains("stale"));
        assert!(env.contains("BOT_TOKEN=fresh:token"));
        assert!(env.contains("LLM_API_BASE=x"));
    }

    #[test]
    fn decrypt_rejects_wrong_key() {
        let tmpdir = tempdir_unique();
        let key_path = tmpdir.join("vps.key");
        let pub_b64 = genkeys(&key_path).unwrap();

        let other_key_path = tmpdir.join("other.key");
        let _ = genkeys(&other_key_path).unwrap();
        let wrong_pem = std::fs::read_to_string(&other_key_path).unwrap();

        let blob = seal(b"secret", &pub_b64);
        let err = decrypt_blob(&wrong_pem, &blob).err().unwrap();
        assert!(err.to_string().contains("decryption failed"));
    }

    #[test]
    fn pem_roundtrip() {
        let raw = [42u8; 32];
        let pem = write_x25519_pkcs8_pem(&raw);
        let parsed = parse_x25519_pkcs8_pem(&pem).unwrap();
        assert_eq!(parsed, raw);
    }

    #[test]
    fn b64decode_accepts_urlsafe_and_standard() {
        let standard = "SGVsbG8gd29ybGQ=";
        let urlsafe = "SGVsbG8gd29ybGQ";
        assert_eq!(b64decode(standard).unwrap(), b"Hello world");
        assert_eq!(b64decode(urlsafe).unwrap(), b"Hello world");
    }

    #[test]
    fn parse_rejects_wrong_length_der() {
        // Exactly 32 bytes — too short for the PKCS#8 v1 wrapper.
        let bad_pem = format!(
            "{}\n{}\n{}\n",
            pem_begin(),
            STANDARD.encode([0u8; 32]),
            pem_end()
        );
        let err = parse_x25519_pkcs8_pem(&bad_pem).err().unwrap();
        assert!(err.to_string().contains("48 bytes"));
    }

    #[test]
    fn parse_rejects_wrong_oid() {
        // 48 bytes (correct length) but OID is not X25519
        // (e.g. Ed25519: 0x2b 0x65 0x70).
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        der.extend_from_slice(&[0u8; 32]);
        let bad_pem = format!(
            "{}\n{}\n{}\n",
            pem_begin(),
            STANDARD.encode(&der),
            pem_end()
        );
        let err = parse_x25519_pkcs8_pem(&bad_pem).err().unwrap();
        assert!(err.to_string().contains("X25519"));
    }

    fn tempdir_unique() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!("kei-buddy-test-{pid}-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
