//! In-memory SOPS decryption by piping through the `sops` binary.
//!
//! Ciphertext is written to the child's stdin and plaintext is read from its
//! stdout; nothing is written to a path (`/dev/stdin` is the inherited pipe).
//! Key backends (age, PGP, AWS KMS, GCP KMS, Vault) are delegated to `sops` via
//! the inherited process environment.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use zeroize::Zeroizing;

use super::spec::SopsMode;
use crate::base::RiverbaseResult;
use crate::config::ConfigPayloadFormat;

/// Decrypt `ciphertext` according to `mode`. In [`SopsMode::Auto`] the payload is
/// only sent to `sops` when it looks encrypted; otherwise it is returned as-is.
pub async fn maybe_decrypt(
    ciphertext: Zeroizing<Vec<u8>>,
    format: ConfigPayloadFormat,
    mode: SopsMode,
) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
    match mode {
        SopsMode::Off => Ok(ciphertext),
        SopsMode::Auto if !looks_encrypted(&ciphertext) => Ok(ciphertext),
        SopsMode::Auto | SopsMode::Force => decrypt(&ciphertext, format).await,
    }
}

/// SOPS `--input-type`/`--output-type` for a payload format. SOPS has no TOML
/// mode, so TOML is handled as opaque `binary`.
fn sops_type(format: ConfigPayloadFormat) -> &'static str {
    match format {
        ConfigPayloadFormat::Yaml => "yaml",
        ConfigPayloadFormat::Json => "json",
        ConfigPayloadFormat::Toml => "binary",
    }
}

/// Heuristic detection of a SOPS-encrypted payload (structured or binary).
fn looks_encrypted(bytes: &[u8]) -> bool {
    contains(bytes, b"ENC[") || (contains(bytes, b"sops") && contains(bytes, b"mac"))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

async fn decrypt(
    ciphertext: &[u8],
    format: ConfigPayloadFormat,
) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
    let type_arg = sops_type(format);
    let mut child = Command::new("sops")
        .arg("--decrypt")
        .arg("--input-type")
        .arg(type_arg)
        .arg("--output-type")
        .arg(type_arg)
        .arg("/dev/stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| crate::errors::CFG_110.with_data(format!("spawn sops: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(ciphertext)
            .await
            .map_err(|e| crate::errors::CFG_172.with_data(format!("write sops stdin: {e}")))?;
        // Close stdin so sops sees EOF.
        let _ = stdin.shutdown().await;
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| crate::errors::CFG_111.with_data(format!("wait sops: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::errors::CFG_173.with_data(format!(
            "exit={:?}: {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    Ok(Zeroizing::new(output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_enc_marker() {
        assert!(looks_encrypted(b"key: ENC[AES256_GCM,data:...]"));
        assert!(looks_encrypted(br#"{"data":"x","sops":{"mac":"ENC[..]"}}"#));
        assert!(!looks_encrypted(b"db_url = \"postgres://x\""));
    }

    #[tokio::test]
    async fn off_mode_passes_through() {
        let input = Zeroizing::new(b"log_level = \"warn\"".to_vec());
        let out = maybe_decrypt(input, ConfigPayloadFormat::Toml, SopsMode::Off)
            .await
            .expect("passthrough");
        assert_eq!(&out[..], b"log_level = \"warn\"");
    }

    #[tokio::test]
    async fn auto_mode_skips_plaintext() {
        let input = Zeroizing::new(b"log_level = \"warn\"".to_vec());
        let out = maybe_decrypt(input, ConfigPayloadFormat::Toml, SopsMode::Auto)
            .await
            .expect("passthrough");
        assert_eq!(&out[..], b"log_level = \"warn\"");
    }

    fn has_sops() -> bool {
        std::process::Command::new("sops")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn force_mode_errors_on_non_sops_payload() {
        if !has_sops() {
            eprintln!("skipping force_mode_errors_on_non_sops_payload: sops not installed");
            return;
        }
        let input = Zeroizing::new(b"key: ENC[not-a-real-sops-file]\n".to_vec());
        let err = match maybe_decrypt(input, ConfigPayloadFormat::Yaml, SopsMode::Force).await {
            Ok(_) => panic!("expected SOPS decryption to fail"),
            Err(e) => e,
        };
        assert_eq!(err.errcode.as_str(), "CFG-111");
    }
}
