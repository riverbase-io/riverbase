use crate::RiverbaseResult;

/// Fully-qualified API name: `{namespace}/{key}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FqName {
    /// Namespace.
    pub namespace: String,
    /// Key.
    pub key: String,
}

/// Fq.
pub fn fq(namespace: &str, key: &str) -> FqName {
    FqName {
        namespace: namespace.to_string(),
        key: key.to_string(),
    }
}

/// Parse fq.
pub fn parse_fq(value: &str) -> RiverbaseResult<FqName> {
    let (namespace, key) = value
        .split_once('/')
        .ok_or_else(|| crate::errors::C00_007.with_data(format!("invalid fq: {value}")))?;
    if namespace.is_empty() || key.is_empty() {
        return Err(crate::errors::C00_007.with_data(format!("invalid fq: {value}")));
    }
    Ok(FqName {
        namespace: namespace.to_string(),
        key: key.to_string(),
    })
}

impl std::fmt::Display for FqName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.namespace, self.key)
    }
}
