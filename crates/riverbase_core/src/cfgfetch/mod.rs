//! In-memory remote config fetching with optional SOPS decryption.
//!
//! Pipeline: [`resolve_source`] picks a backend from the URI scheme,
//! [`ConfigSource::fetch`] pulls the (possibly encrypted) bytes into memory, and
//! [`sops`] decrypts them by piping through the `sops` binary. No fetched or
//! decrypted bytes are ever written to disk, and all buffers are
//! [`Zeroizing`](zeroize::Zeroizing) so they are wiped on drop.
//!
//! Feature flags select backends: `cfgfetch-https`, `cfgfetch-git`,
//! `cfgfetch-s3`, `cfgfetch-oci` (or `cfgfetch-all`). The base `cfgfetch`
//! feature always provides the `file://` source and the SOPS pipeline.

mod sops;
pub mod source;
mod spec;

pub use source::{resolve_source, ConfigSource};
pub use spec::{RemoteConfigSpec, SopsMode, ENV_CONFIG_FORMAT, ENV_CONFIG_SOPS, ENV_CONFIG_URI};

use zeroize::Zeroizing;

use crate::base::RiverbaseResult;

/// Fetch the config described by `spec` and return decrypted plaintext bytes.
///
/// The returned buffer is zeroized on drop. Callers typically pass it to
/// [`RiverbaseConfig::from_bytes`](crate::config::RiverbaseConfig::from_bytes).
pub async fn fetch_and_decrypt(spec: &RemoteConfigSpec) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
    let source = resolve_source(&spec.uri)?;
    let ciphertext = source.fetch().await?;
    sops::maybe_decrypt(ciphertext, spec.format, spec.sops).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_scheme_is_rejected() {
        let err = match resolve_source("weird://example") {
            Ok(_) => panic!("expected an unsupported-scheme error"),
            Err(e) => e,
        };
        assert_eq!(err.errcode.as_str(), "CFG-120");
    }

    #[tokio::test]
    async fn file_source_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("riverbase.toml");
        std::fs::write(&path, b"log_level = \"warn\"").expect("write");

        let spec =
            RemoteConfigSpec::new(format!("file://{}", path.display())).with_sops(SopsMode::Off);
        let bytes = fetch_and_decrypt(&spec).await.expect("fetch");
        assert_eq!(&bytes[..], b"log_level = \"warn\"");

        let cfg = crate::config::RiverbaseConfig::from_bytes(&bytes, spec.format).expect("parse");
        assert_eq!(cfg.log_level, "warn");
    }

    fn has_binary(name: &str) -> bool {
        std::process::Command::new(name)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    async fn run_with_stdin(
        program: &str,
        args: &[&str],
        input: &[u8],
    ) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>) {
        use tokio::io::AsyncWriteExt;
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn child");
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input).await.expect("write stdin");
            let _ = stdin.shutdown().await;
        }
        let out = child.wait_with_output().await.expect("wait child");
        (out.status, out.stdout, out.stderr)
    }

    /// Full pipeline against the real `sops` + `age` binaries: generate an age
    /// key, encrypt a config in memory, fetch it via `file://`, and decrypt +
    /// parse it back through the pipeline. Skips when the binaries are absent.
    #[tokio::test]
    async fn sops_age_round_trip() {
        if !has_binary("sops") || !has_binary("age-keygen") {
            eprintln!("skipping sops_age_round_trip: sops/age-keygen not installed");
            return;
        }

        let keygen = std::process::Command::new("age-keygen")
            .output()
            .expect("run age-keygen");
        assert!(keygen.status.success(), "age-keygen failed");
        let key_text = String::from_utf8_lossy(&keygen.stdout);
        let recipient = key_text
            .lines()
            .find(|l| l.contains("public key:"))
            .and_then(|l| l.rsplit("public key:").next())
            .map(|s| s.trim().to_string())
            .expect("age public key");
        let secret = key_text
            .lines()
            .find(|l| l.starts_with("AGE-SECRET-KEY-"))
            .map(|s| s.trim().to_string())
            .expect("age secret key");

        std::env::set_var("SOPS_AGE_KEY", &secret);

        let dir = tempfile::tempdir().expect("tempdir");

        // Structured YAML (sops yaml mode), with an `flrs` section.
        let yaml_plain = b"flrs:\n  log_level: warn\n  bind_addr: 127.0.0.1:9999\n";
        let (status, ciphertext, stderr) = run_with_stdin(
            "sops",
            &[
                "--encrypt",
                "--age",
                &recipient,
                "--input-type",
                "yaml",
                "--output-type",
                "yaml",
                "/dev/stdin",
            ],
            yaml_plain,
        )
        .await;
        assert!(
            status.success(),
            "sops yaml encrypt failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(
            ciphertext.windows(4).any(|w| w == b"ENC["),
            "expected sops ciphertext markers"
        );
        let yaml_path = dir.path().join("enc.yaml");
        std::fs::write(&yaml_path, &ciphertext).expect("write yaml ciphertext");

        let yaml_spec = RemoteConfigSpec::new(format!("file://{}", yaml_path.display()))
            .with_format(crate::config::ConfigPayloadFormat::Yaml)
            .with_sops(SopsMode::Auto);
        let yaml_cfg = crate::config::RiverbaseConfig::load_remote(&yaml_spec)
            .await
            .expect("load yaml remote");
        assert_eq!(yaml_cfg.log_level, "warn");
        assert_eq!(yaml_cfg.bind_addr, "127.0.0.1:9999");

        // Whole-file TOML via sops binary mode (TOML has no native sops mode).
        let toml_plain = b"log_level = \"debug\"\nbind_addr = \"0.0.0.0:7000\"\n";
        let (status, ciphertext, stderr) = run_with_stdin(
            "sops",
            &[
                "--encrypt",
                "--age",
                &recipient,
                "--input-type",
                "binary",
                "--output-type",
                "binary",
                "/dev/stdin",
            ],
            toml_plain,
        )
        .await;
        assert!(
            status.success(),
            "sops binary encrypt failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        let toml_path = dir.path().join("enc.toml.sops");
        std::fs::write(&toml_path, &ciphertext).expect("write toml ciphertext");

        let toml_spec = RemoteConfigSpec::new(format!("file://{}", toml_path.display()))
            .with_format(crate::config::ConfigPayloadFormat::Toml)
            .with_sops(SopsMode::Auto);
        let toml_cfg = crate::config::RiverbaseConfig::load_remote(&toml_spec)
            .await
            .expect("load toml remote");
        assert_eq!(toml_cfg.log_level, "debug");
        assert_eq!(toml_cfg.bind_addr, "0.0.0.0:7000");

        std::env::remove_var("SOPS_AGE_KEY");
    }
}
