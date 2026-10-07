use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use super::Domain;
use crate::base::RiverbaseResult;
use crate::command::CommandKind;
use crate::query::QueryResourceKind;

#[derive(Debug, Clone, Serialize)]
/// Domain manifest structure.
pub struct DomainManifest {
    /// Namespace.
    pub namespace: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: Option<String>,
    /// Commands.
    pub commands: Vec<CommandContract>,
    /// Queries.
    pub queries: Vec<QueryContract>,
    /// Services.
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
/// Command contract structure.
pub struct CommandContract {
    /// Key.
    pub key: String,
    /// Title.
    pub title: String,
    /// Kind.
    pub kind: &'static str,
    /// Resources.
    pub resources: Vec<String>,
    /// Scope required.
    pub scope_required: bool,
    /// Allowed zones.
    pub allowed_zones: Vec<String>,
    /// Schema.
    pub schema: Value,
}

#[derive(Debug, Clone, Serialize)]
/// Query contract structure.
pub struct QueryContract {
    /// Resource.
    pub resource: String,
    /// Title.
    pub title: String,
    /// Kind.
    pub kind: &'static str,
    /// Scope required.
    pub scope_required: bool,
    /// Allowed zones.
    pub allowed_zones: Vec<String>,
}

/// Build domain manifest.
pub async fn build_domain_manifest<D: Domain + ?Sized>(
    domain: &D,
) -> RiverbaseResult<DomainManifest> {
    let mut commands = Vec::new();
    if let Some(command_engine) = domain.command_capability() {
        command_engine.ensure_namespace(domain.namespace())?;
        let mut command_keys = command_engine.commands().await?;
        command_keys.sort();
        reject_duplicates(domain.namespace(), "command", &command_keys)?;
        commands.reserve(command_keys.len());
        for key in command_keys {
            let meta = command_engine.command_meta(&key).ok_or_else(|| {
                crate::errors::DOM_009.with_data(format!("{}:{key}", domain.namespace()))
            })?;
            if meta.key != key {
                return Err(crate::errors::DOM_010.with_data(format!(
                    "{}: registry={key}, metadata={}",
                    domain.namespace(),
                    meta.key
                )));
            }
            if meta.resources.is_empty() {
                return Err(
                    crate::errors::DOM_011.with_data(format!("{}:{key}", domain.namespace()))
                );
            }
            commands.push(CommandContract {
                key: key.clone(),
                title: meta.title,
                kind: match meta.kind {
                    CommandKind::Collection => "collection",
                    CommandKind::Object => "object",
                    CommandKind::ObjectLink => "object-link",
                    CommandKind::ObjectHook => "object-hook",
                },
                resources: meta.resources,
                scope_required: meta.scope.required,
                allowed_zones: meta.allowed_zones,
                schema: command_engine.command_info(&key).unwrap_or(Value::Null),
            });
        }
    }

    let mut queries = Vec::new();
    if let Some(query_engine) = domain.query_capability() {
        query_engine.ensure_namespace(domain.namespace())?;
        let mut query_names = query_engine.queries().await?;
        query_names.sort();
        reject_duplicates(domain.namespace(), "query", &query_names)?;
        let query_name_set = query_names.into_iter().collect::<BTreeSet<_>>();
        queries = query_engine
            .query_scope_metas()
            .await?
            .into_iter()
            .map(|meta| QueryContract {
                resource: meta.resource,
                title: meta.title,
                kind: match meta.kind {
                    QueryResourceKind::Query => "query",
                    QueryResourceKind::Report => "report",
                },
                scope_required: meta.scope.required,
                allowed_zones: meta.allowed_zones,
            })
            .collect::<Vec<_>>();
        let route_name_set = queries
            .iter()
            .map(|contract| contract.resource.clone())
            .collect::<BTreeSet<_>>();
        if query_name_set != route_name_set {
            return Err(crate::errors::DOM_012.with_data(serde_json::json!({
                "namespace": domain.namespace(),
                "registered": query_name_set,
                "routes": route_name_set,
            })));
        }
    }
    queries.sort_by(|left, right| left.resource.cmp(&right.resource));
    reject_duplicates(
        domain.namespace(),
        "query route",
        &queries
            .iter()
            .map(|contract| contract.resource.clone())
            .collect::<Vec<_>>(),
    )?;
    let services = match domain.service_dyn() {
        Some(service) => {
            let mut services = service.items().await?;
            services.sort();
            reject_duplicates(domain.namespace(), "service", &services)?;
            services
        }
        None => Vec::new(),
    };

    Ok(DomainManifest {
        namespace: domain.namespace().to_string(),
        title: domain.title().to_string(),
        description: domain.description().map(str::to_string),
        commands,
        queries,
        services,
    })
}

fn reject_duplicates(namespace: &str, kind: &str, values: &[String]) -> RiverbaseResult<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(crate::errors::DOM_013.with_data(format!("{namespace}:{kind}:{value}")));
        }
    }
    Ok(())
}
