//! Signed command link tokens (`:link`), compatible with Python `itsdangerous.URLSafeSerializer`.

use std::io::Read;

use chrono::{DateTime, Utc};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;

use crate::base::RiverbaseResult;
use crate::config::LinkTokenConfig;

/// Payload embedded in a `:link` token (matches Python `CommandTokenData`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandTokenData {
    /// Resource.
    pub resource: String,
    /// Identifier.
    pub identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Scope.
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Command or event payload.
    pub payload: Option<Value>,
    /// Expires at.
    pub expires_at: DateTime<Utc>,
}

/// Encode a signed URL-safe command link token.
pub fn encode_command_token(
    data: &CommandTokenData,
    config: &LinkTokenConfig,
) -> RiverbaseResult<String> {
    let json =
        serde_json::to_vec(data).map_err(|e| crate::errors::LNK_001.with_data(e.to_string()))?;
    let payload = url_safe_payload(&json)?;
    sign_payload(&payload, config)
}

/// Decode and verify a `:link` token; returns forbidden on bad signature or expiry.
pub fn decode_command_token(
    token: &str,
    config: &LinkTokenConfig,
) -> RiverbaseResult<CommandTokenData> {
    let payload = unsign_payload(token, config)?;
    let json = decode_url_safe_payload(&payload)?;
    let data: CommandTokenData = serde_json::from_slice(&json)
        .map_err(|e| crate::errors::LNK_002.with_data(format!("payload json: {e}")))?;
    if data.expires_at < Utc::now() {
        return Err(crate::errors::LNK_003.with_data("expires_at is in the past"));
    }
    Ok(data)
}

const TOKEN_VERSION_PREFIX: &str = "v2.";

fn derive_signing_key(config: &LinkTokenConfig) -> RiverbaseResult<Vec<u8>> {
    let mut hasher = Sha256::new();
    hasher.update(config.require_salt()?.as_bytes());
    hasher.update(b"signer");
    hasher.update(config.require_secret()?.as_bytes());
    Ok(hasher.finalize().to_vec())
}

fn sign_payload(payload: &[u8], config: &LinkTokenConfig) -> RiverbaseResult<String> {
    let key = derive_signing_key(config)?;
    let sig = hmac_sha256(&key, payload);
    let sig_b64 = base64_url_encode(&sig);
    Ok(format!(
        "{TOKEN_VERSION_PREFIX}{}.{}",
        String::from_utf8_lossy(payload),
        String::from_utf8_lossy(&sig_b64)
    ))
}

fn unsign_payload(token: &str, config: &LinkTokenConfig) -> RiverbaseResult<Vec<u8>> {
    let token = token
        .strip_prefix(TOKEN_VERSION_PREFIX)
        .ok_or_else(|| crate::errors::LNK_011.with_data("legacy token"))?;
    let (value, sig) = token
        .rsplit_once('.')
        .ok_or_else(|| crate::errors::LNK_004.with_data("invalid token"))?;
    let value = value.as_bytes();
    let sig = base64_url_decode(sig.as_bytes())
        .map_err(|_| crate::errors::LNK_005.with_data("invalid token"))?;
    let key = derive_signing_key(config)?;
    let expected = hmac_sha256(&key, value);
    if !constant_time_eq(&expected, &sig) {
        return Err(crate::errors::LNK_006.with_data("invalid token"));
    }
    Ok(value.to_vec())
}

pub(crate) fn hmac_sha256(key: &[u8], value: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 64;
    let mut k = key.to_vec();
    if k.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(&k);
        k = h.finalize().to_vec();
    }
    if k.len() < BLOCK {
        k.resize(BLOCK, 0);
    }
    let mut ipad = vec![0x36u8; BLOCK];
    let mut opad = vec![0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(value);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(inner);
    outer.finalize().to_vec()
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub(crate) fn url_safe_payload(json: &[u8]) -> RiverbaseResult<Vec<u8>> {
    let compressed = zlib_compress(json)?;
    let mut b64 = base64_url_encode(&compressed);
    let mut out = vec![b'.'];
    out.append(&mut b64);
    Ok(out)
}

pub(crate) fn decode_url_safe_payload(payload: &[u8]) -> RiverbaseResult<Vec<u8>> {
    let (compressed, body) = if payload.first() == Some(&b'.') {
        (true, &payload[1..])
    } else {
        (false, payload)
    };
    let bytes = base64_url_decode(body)
        .map_err(|_| crate::errors::LNK_007.with_data("payload base64 decode failed"))?;
    if compressed {
        zlib_decompress(&bytes)
            .map_err(|e| crate::errors::LNK_008.with_data(format!("zlib decompress: {e}")))
    } else {
        Ok(bytes)
    }
}

fn zlib_compress(input: &[u8]) -> RiverbaseResult<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .map_err(|e| crate::errors::LNK_009.with_data(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| crate::errors::LNK_010.with_data(e.to_string()))
}

fn zlib_decompress(input: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(input);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

pub(crate) fn base64_url_encode(data: &[u8]) -> Vec<u8> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    engine.encode(data).into_bytes()
}

pub(crate) fn base64_url_decode(data: &[u8]) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    engine.decode(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    /// Fixed vector from Python `encode_command_token` (itsdangerous URLSafeSerializer + msgspec JSON).
    const PYTHON_TOKEN: &str = ".eJwVzcEKwjAMBuB3yXkdbdzm1ufw5EVim0FBbOk6UMZ8dhPI4ecj-XNA5S3vNTB4aDlm6CBFfre0Jq5i1uFlGKermRd6mhB5NUpGTUlFbraQi1b8JBf6vjJF8AcQeHd2wJ-S5M-DmqygxclYGXdz6HH0iL2brcXlDucfULUpQw.5kFtqDcQO5tBsYKC2GsQYej1KVI";

    fn python_parity_config() -> LinkTokenConfig {
        #[allow(deprecated)]
        LinkTokenConfig {
            secret: Some(crate::config::DEFAULT_LINK_TOKEN_SECRET.to_string()),
            salt: Some(crate::config::DEFAULT_LINK_TOKEN_SALT.to_string()),
        }
    }

    #[test]
    fn encode_without_secret_fails() {
        let err = encode_command_token(
            &CommandTokenData {
                resource: "order".into(),
                identifier: "id".into(),
                scope: None,
                payload: None,
                expires_at: Utc::now(),
            },
            &LinkTokenConfig::default(),
        )
        .expect_err("missing secret");
        assert!(
            err.errcode.as_str() == "CFG-146" || err.errcode.as_str() == "CFG-147",
            "missing secret or salt, got {}",
            err.errcode
        );
    }

    #[test]
    fn rejects_sha1_python_generated_token() {
        let cfg = python_parity_config();
        let err = unsign_payload(PYTHON_TOKEN, &cfg).expect_err("sha1 rejected");
        assert_eq!(err.errcode.as_str(), "LNK-011");
    }

    #[test]
    fn roundtrip_encode_decode() {
        let cfg = python_parity_config();
        let data = CommandTokenData {
            resource: "order".into(),
            identifier: "01234567-89ab-cdef-0123-456789abcdef".into(),
            scope: None,
            payload: Some(serde_json::json!({"confirm": true})),
            expires_at: Utc::now() + Duration::hours(2),
        };
        let token = encode_command_token(&data, &cfg).expect("encode");
        let decoded = decode_command_token(&token, &cfg).expect("decode");
        assert_eq!(decoded.resource, data.resource);
        assert_eq!(decoded.identifier, data.identifier);
        assert_eq!(decoded.payload, data.payload);
    }
}
