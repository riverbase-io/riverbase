//! Query engine and resources (read side).

pub mod binding;
pub mod coverage;
/// Engine; module.
pub mod engine;
pub mod interface;
/// Log; module.
pub mod log;
pub mod lower;
pub mod primitives;
/// Resource; module.
pub mod resource;
/// Session; module.
pub mod session;
pub mod statement;

pub use binding::{FieldSource, JoinDef, QueryBinding};
pub use coverage::{
    assert_sort_columns_covered, const_slice_contains, const_str_eq,
    debug_validate_sortable_order_coverage, first_missing_column,
};
pub use engine::{PolicyFilterProvider, QueryEngine, QueryEngineArgs};
pub use interface::{
    build_resource_meta, field_sortable, FieldDef, FilterDef, Operator, ParamDef, Preset,
    QueryInterface,
};
pub use log::append_query_log;
pub use lower::{
    lower_item, lower_list, pagination_meta, project_and_relabel, with_identifier_filter,
};
pub use primitives::{FrontendQuery, QueryRequest, SortDirection};
pub use resource::{
    PolicyDecision, PolicyRequirement, QueryAccess, QueryHttpMeta, QueryResource,
    QueryResourceKind, QueryRouteMeta, ReportOutput,
};
pub use session::QuerySession;
pub use statement::{parse_operator_statement, parse_statement, Mode, OperatorStatement};
