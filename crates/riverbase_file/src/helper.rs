use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn hash_n_length(content: &[u8]) -> (String, i64) {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let digest = hasher.finalize();
    let hash = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    (hash, content.len() as i64)
}

/// Sanitize a resource name for use as an object-key segment.
///
/// Keeps `[A-Za-z0-9_-]`. Empty or missing values become `_`.
fn sanitize_resource(resource: Option<&str>) -> String {
    let cleaned: String = resource
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

/// First two bytes of SHA-256(media_id) as `{aa}/{bb}` hex shards.
fn media_id_shard(id: &Uuid) -> String {
    let digest = Sha256::digest(id.as_bytes());
    format!("{:02x}/{:02x}", digest[0], digest[1])
}

/// Build an object key for a new media blob.
///
/// `{resource}/{aa}/{bb}/{media_id}{ext}` under an optional `root_path` prefix
/// inside the OpenDal filesystem root (not the OS directory).
pub fn build_media_path(
    root_path: &str,
    resource: Option<&str>,
    id: &Uuid,
    filename: &str,
) -> String {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{s}"))
        .unwrap_or_default();
    let resource = sanitize_resource(resource);
    let shard = media_id_shard(id);
    let id_hex = id.simple().to_string();
    let name = format!("{resource}/{shard}/{id_hex}{ext}");
    let prefix = root_path.trim().trim_matches('/');
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

/// Normalize a stored `fspath` to an OpenDal object key under the filesystem root.
///
/// Legacy rows may embed a one-level `media/` prefix (e.g. `./media/{token}.jpg`);
/// those resolve incorrectly once `fs_root` points at the real media directory.
/// Nested keys (`{resource}/{aa}/{bb}/{id}.ext`) are kept as-is.
pub fn storage_object_key(fspath: &str) -> String {
    let trimmed = fspath.trim().trim_start_matches("./").trim_matches('/');
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [] => fspath.to_string(),
        [file] => (*file).to_string(),
        ["media", file] => (*file).to_string(),
        _ => trimmed.to_string(),
    }
}

pub fn guess_mime(filename: &str) -> String {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        Some("csv") => "text/csv",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("html") | Some("htm") => "text/html",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id() -> Uuid {
        Uuid::parse_str("d9118f28-0014-44ed-b6ce-1784da150288").expect("uuid")
    }

    #[test]
    fn build_media_path_shards_on_sha256_of_media_id() {
        let id = test_id();
        let path = build_media_path("", Some("article"), &id, "archive.html");
        let shard = media_id_shard(&id);
        assert_eq!(
            path,
            format!("article/{shard}/d9118f28001444edb6ce1784da150288.html")
        );
        assert!(!path.starts_with("article/d9/11/"), "{path}");
    }

    #[test]
    fn build_media_path_prefixes_root_path() {
        let id = test_id();
        let path = build_media_path("uploads", Some("article"), &id, "archive.html");
        let shard = media_id_shard(&id);
        assert_eq!(
            path,
            format!("uploads/article/{shard}/d9118f28001444edb6ce1784da150288.html")
        );
    }

    #[test]
    fn build_media_path_missing_resource_uses_underscore() {
        let id = test_id();
        let path = build_media_path("", None, &id, "photo.jpg");
        let shard = media_id_shard(&id);
        assert_eq!(
            path,
            format!("_/{shard}/d9118f28001444edb6ce1784da150288.jpg")
        );
    }

    #[test]
    fn build_media_path_sanitizes_resource() {
        let id = test_id();
        let path = build_media_path("", Some("../article!"), &id, "a.html");
        assert!(path.starts_with("article/"), "{path}");
    }

    #[test]
    fn storage_object_key_strips_legacy_directory_prefix() {
        assert_eq!(
            storage_object_key("./media/1e6ceae1f2f049808aa9aa5658b898dc.jpg"),
            "1e6ceae1f2f049808aa9aa5658b898dc.jpg"
        );
        assert_eq!(
            storage_object_key("media/1e6ceae1f2f049808aa9aa5658b898dc.jpg"),
            "1e6ceae1f2f049808aa9aa5658b898dc.jpg"
        );
        assert_eq!(
            storage_object_key("1e6ceae1f2f049808aa9aa5658b898dc.jpg"),
            "1e6ceae1f2f049808aa9aa5658b898dc.jpg"
        );
    }

    #[test]
    fn storage_object_key_keeps_nested_hash_paths() {
        let key = "article/3a/f1/d9118f28001444edb6ce1784da150288.html";
        assert_eq!(storage_object_key(key), key);
        assert_eq!(
            storage_object_key("./article/3a/f1/d9118f28001444edb6ce1784da150288.html"),
            key
        );
        assert_eq!(
            storage_object_key("uploads/article/3a/f1/d9118f28001444edb6ce1784da150288.html"),
            "uploads/article/3a/f1/d9118f28001444edb6ce1784da150288.html"
        );
    }
}
