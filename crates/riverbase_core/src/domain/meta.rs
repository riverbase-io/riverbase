use crate::base::{fq, Namespace};

/// Business domain descriptor: one namespace with coherent command + query surface.
#[derive(Debug, Clone)]
pub struct DomainMeta {
    /// Namespace.
    pub namespace: Namespace,
    /// Title.
    pub title: String,
    /// Description.
    pub description: Option<String>,
}

impl DomainMeta {
    /// Construct a new value.
    pub fn new(namespace: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            namespace: Namespace::new(namespace),
            title: title.into(),
            description: None,
        }
    }

    /// Set description and return self.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Command fq.
    pub fn command_fq(&self, cmdkey: &str) -> crate::base::FqName {
        fq(&self.namespace.0, cmdkey)
    }

    /// Namespace str.
    pub fn namespace_str(&self) -> &str {
        &self.namespace.0
    }
}
