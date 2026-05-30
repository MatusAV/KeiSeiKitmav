// SPDX-License-Identifier: Apache-2.0
//! Filesystem-level orchestration: read PEM + sealed blob from disk,
//! decrypt, and write the resulting `BOT_TOKEN=<...>` into an env file.
//! Also: generate a fresh X25519 keypair on disk.
//!
//! Contract:
//!   - `/etc/keisei-vps.key`   — PKCS#8 PEM x25519 private (`openssl genpkey -algorithm X25519`)
//!   - `/etc/keisei-blob.json` — `{"ciphertext":"<b64u>","nonce":"<b64u>","ephPub":"<b64u>"}`
//!   - result                  — `BOT_TOKEN=<plaintext>\n` appended to the env file

use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand_core::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use super::crypto::{decrypt_blob, SealedBlob};
use super::pem::write_x25519_pkcs8_pem;

/// Read the VPS key + the sealed blob from disk, decrypt, and write
/// `BOT_TOKEN=<plaintext>` (and `TELEGRAM_BOT_TOKEN=` mirror) into
/// `env_out_path`. Idempotent: pre-existing `BOT_TOKEN=` / `TELEGRAM_BOT_TOKEN=`
/// lines are stripped before the new one is appended.
pub fn decrypt_and_export(
    vps_key_path: &Path,
    blob_path: &Path,
    env_out_path: &Path,
) -> Result<()> {
    let pem = std::fs::read_to_string(vps_key_path)
        .with_context(|| format!("read vps key {}", vps_key_path.display()))?;
    let blob_str = std::fs::read_to_string(blob_path)
        .with_context(|| format!("read blob {}", blob_path.display()))?;
    let blob: SealedBlob =
        serde_json::from_str(&blob_str).context("parse sealed blob JSON")?;

    let pt = decrypt_blob(&pem, &blob)?;
    let token = std::str::from_utf8(&pt).context("decrypted plaintext is not UTF-8")?;
    let token = token.trim();
    if token.is_empty() {
        bail!("decrypted token is empty");
    }

    let existing = std::fs::read_to_string(env_out_path).unwrap_or_default();
    let mut filtered: Vec<String> = existing
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("BOT_TOKEN=") && !t.starts_with("TELEGRAM_BOT_TOKEN=")
        })
        .map(|s| s.to_string())
        .collect();
    filtered.push(format!("BOT_TOKEN={token}"));
    filtered.push(format!("TELEGRAM_BOT_TOKEN={token}"));
    let mut content = filtered.join("\n");
    content.push('\n');
    std::fs::write(env_out_path, content)
        .with_context(|| format!("write env {}", env_out_path.display()))?;

    Ok(())
}

/// Generate a fresh X25519 keypair: writes the private as PKCS#8 PEM (mode
/// 0o400 on Unix) to `key_path`, returns the base64-encoded public key.
pub fn genkeys(key_path: &Path) -> Result<String> {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    let mut priv_raw: [u8; 32] = secret.to_bytes();
    let pem = write_x25519_pkcs8_pem(&priv_raw);
    priv_raw.zeroize();

    std::fs::write(key_path, pem.as_bytes())
        .with_context(|| format!("write {}", key_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o400));
    }

    Ok(STANDARD.encode(public.as_bytes()))
}
