//! Declarative OpenAPI overrides for command/query route metadata.

/// OpenAPI catalog and explorer overrides stamped onto HTTP operations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenApiMeta {
    /// Primary Riverbase catalog tag (e.g. `"riverbase:audit"`). When unset, the operation kind default applies.
    pub tag: Option<String>,
    /// When `Some(false)`, sets `x-explorer: false` on the operation.
    pub explorer: Option<bool>,
    /// When `Some(true)`, sets `x-internal: true` on the operation.
    pub internal: Option<bool>,
    /// When true, marks the OpenAPI operation as `deprecated`.
    pub deprecated: bool,
}

impl OpenApiMeta {
    /// Set tag and return self.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Set explorer and return self.
    pub fn with_explorer(mut self, explorer: bool) -> Self {
        self.explorer = Some(explorer);
        self
    }

    /// Set internal and return self.
    pub fn with_internal(mut self, internal: bool) -> Self {
        self.internal = Some(internal);
        self
    }

    /// Set deprecated and return self.
    pub fn with_deprecated(mut self, deprecated: bool) -> Self {
        self.deprecated = deprecated;
        self
    }
}
