//! Base primitives shared across riverbase crates.

mod aggregate;
mod domain;
mod engine;
mod error;
mod fq;
mod ids;
mod json;
mod mode;
mod response;
mod scope;
mod tenant;

pub use aggregate::AggregateRoot;
pub use domain::{
    domain_fields_for_entity_insert, domain_fields_from_json, domain_fields_from_payload,
    profile_id_from_claims_and_sub, uuid_from_json, with_json_domain_meta, with_json_etag,
    with_json_tenant, AggregateContext, AuditActor, DomainFields,
};
pub use engine::{Engine, EngineContext, EngineKind, InvocationMode};
pub use error::{
    BadRequestError, ConfigError, ConflictError, ErrorSpec, RiverbaseError, RiverbaseErrorCode,
    RiverbaseResult, ForbiddenError, IntoErrorData, InvalidArgumentError, NotFoundError,
    StorageError,
};
pub use fq::{fq, parse_fq, FqName};
pub use ids::{
    realm_uuid, tenant_id_from_config, tenant_uuid, uuid_from_text, CommandId, EntityId, Namespace,
    TrackerId, BASE_NAMESPACE, NAME_UUID_NAMESPACE,
};
pub use json::JsonMap;
pub use mode::ServiceMode;
pub use response::{
    command_envelope, command_meta, command_success_meta, etag_from_data, item_envelope,
    list_envelope, quoted_etag, report_envelope, success_envelope, success_envelope_typed,
    unquote_etag, ProblemDetails, API_CONTRACT_VERSION, DEFAULT_RESPONSE_TYPE, ENVELOPE_COMMAND,
    ENVELOPE_GENERIC, ENVELOPE_QUERY_ITEM, ENVELOPE_QUERY_LIST, ENVELOPE_REPORT, ERROR_TYPE_BASE,
};
pub use scope::{ScopeMap, ScopeMeta};
pub use tenant::{
    apply_tenant_stamp, authorize_tenant_transfer, carry_row_tenant, row_tenant_id,
    DomainTenantLookup, TenantAccess, TenantAccessContext, TenantAccessFn, TenantAccessKind,
    TenantPolicyResolver, TenantTransfer, ACCESS_DOMAIN_TENANT, ACCESS_PROFILE,
    ACCESS_PROFILE_ORGANIZATION, ACCESS_SYSTEM, ACCESS_USER, DEFAULT_TENANT_ACCESS_POLICY,
    DEFAULT_TENANT_STAMP_POLICY, STAMP_DOMAIN_TENANT, STAMP_PROFILE, STAMP_PROFILE_ORGANIZATION,
    STAMP_USER,
};
