//! Query-engine declarative macros.
//!
//! - [`query_resource!`](crate::query_resource) — query contract, storage mapping, and resource wiring
//! - [`report_resource!`](crate::report_resource) — report query contract (filters + params), binding, and resource wiring
//! - [`query_engine!`](crate::query_engine) — domain query engine struct + [`DomainQueryEngine`] impl
//!
//! See `docs/02-design/query/10-query-engine-design.md` §3.3.
//!
//! [`DomainQueryEngine`]: crate::domain::DomainQueryEngine

/// Declare a complete query resource: interface, binding, and [`QueryResource`] impl.
///
/// The resource type name is the full identifier (e.g. `TodoQueryResource`), not a prefix that
/// gets `QueryResource` appended. Companion types are derived by appending to the full name:
/// `{Resource}Interface` and `{Resource}Binding` (e.g. `TodoQueryResource` →
/// `TodoQueryResourceInterface`). These are an internal implementation detail of the macro.
///
/// In the `binding source { … }` section, list **overrides only**: logical field → physical
/// column when they differ (e.g. `id => "_id"`, `created => "_created"`). When the logical
/// field name matches the storage column, omit the entry — identity mapping is applied
/// automatically.
///
/// The emitted `{Resource}Key` carries [`ResourceKey::SORT_COLUMNS`](crate::datastore::ResourceKey::SORT_COLUMNS)
/// for compile-time coverage when `pg_domain_entity!` `resources:` names that key.
#[macro_export]
macro_rules! query_resource {
    (
        $resource:ident name $name:literal {
            meta {
                $($meta:tt)*
            }
            fields {
                $(field $field_name:ident {
                    preset: $preset:ident,
                    label: $field_label:literal
                    $(, $field_attr:ident)*
                })*
            }
            binding source $source:literal {
                $($binding_body:tt)*
            }
            $(errors {
                list: ($list_code:literal, $list_msg:literal),
                item: ($item_code:literal, $item_msg:literal),
            })?
            $(policy: $policy_requirement:ident $(($policy_reason:literal, $policy_signoff:literal))?,)?
            $(scope_policy ($pol_ctx:ident, $pol_scope:ident) {
                $($policy_body:tt)*
            })?
            $(tenant_scope: exempt ($tenant_reason:literal, $tenant_signoff:literal),)?
            $(handler ($handler_self:ident, $handler_ctx:ident, $handler_session:ident, $handler_request:ident, $handler_query:ident) {
                $($handler_body:tt)*
            })?
        }
    ) => {
        $crate::query_resource!(@emit $resource, $name, {
            meta { $($meta)* }
            $(field $field_name {
                preset: $preset,
                label: $field_label
                $(, $field_attr)*
            })*
            binding source $source {
                $($binding_body)*
            }
            list_code { $($list_code)? }
            list_msg { $($list_msg)? }
            item_code { $($item_code)? }
            item_msg { $($item_msg)? }
            policy { $($policy_requirement $(($policy_reason, $policy_signoff))?)? }
            scope_policy { $(($pol_ctx, $pol_scope) { $($policy_body)* })? }
            tenant_scope { $($tenant_reason, $tenant_signoff)? }
            handler { $(($handler_self, $handler_ctx, $handler_session, $handler_request, $handler_query) { $($handler_body)* })? }
        });
    };

    (@emit $resource:ident, $name:literal, {
        meta { $($meta:tt)* }
        $(field $field_name:ident {
            preset: $preset:ident,
            label: $field_label:literal
            $(, $field_attr:ident)*
        })*
        binding source $source:literal {
            $($binding_body:tt)*
        }
        list_code { $($list_code:literal)? }
        list_msg { $($list_msg:literal)? }
        item_code { $($item_code:literal)? }
        item_msg { $($item_msg:literal)? }
        policy { $($policy_requirement:ident $(($policy_reason:literal, $policy_signoff:literal))?)? }
        scope_policy { $(($pol_ctx:ident, $pol_scope:ident) { $($policy_body:tt)* })? }
        tenant_scope { $($tenant_reason:literal, $tenant_signoff:literal)? }
        handler { $(($handler_self:ident, $handler_ctx:ident, $handler_session:ident, $handler_request:ident, $handler_query:ident) { $($handler_body:tt)* })? }
    }) => {
        $crate::paste::paste! {
            $crate::query_resource!(@iface_impl [<$resource Interface>] {
                meta { $($meta)* }
                $(field $field_name {
                    preset: $preset,
                    label: $field_label
                    $(, $field_attr)*
                })*
            });

            $crate::query_resource!(@binding_impl [<$resource Binding>] source $source {
                $($binding_body)*
            });

            pub struct $resource<M: $crate::datastore::DataStore + 'static> {
                executor: ::std::sync::Arc<M>,
                interface: [<$resource Interface>],
                binding: [<$resource Binding>],
            }

            impl<M: $crate::datastore::DataStore + 'static> $resource<M> {
                /// Wire resource name declared in the resource macro.
                pub const NAME: &'static str = $name;
                /// Storage source key declared in `binding source`.
                pub const SOURCE: &'static str = $source;

                pub fn new(executor: ::std::sync::Arc<M>) -> Self {
                    Self {
                        executor,
                        interface: [<$resource Interface>],
                        binding: [<$resource Binding>],
                    }
                }
            }

            /// Compile-time resource key. The string `$name` is declared once on this resource.
            #[derive(Debug, Clone, Copy, Default)]
            pub struct [<$resource Key>];

            $crate::query_resource!(@key_impl [<$resource Key>] {
                name: $name,
                source: $source,
                meta { $($meta)* }
                $(field $field_name {
                    preset: $preset,
                    label: $field_label
                    $(, $field_attr)*
                })*
                binding {
                    $($binding_body)*
                }
            });

            #[::async_trait::async_trait]
            impl<M: $crate::datastore::DataStore + 'static> $crate::query::QueryResource for $resource<M> {
                fn name(&self) -> &str {
                    $name
                }

                fn interface(&self) -> &dyn $crate::query::QueryInterface {
                    &self.interface
                }

                fn binding(&self) -> &dyn $crate::query::QueryBinding {
                    &self.binding
                }

                fn policy_filter_enforced(&self) -> bool {
                    $crate::datastore::DataStore::enforces_policy_filter(
                        self.executor.as_ref(),
                        $crate::query::QueryBinding::source(&self.binding),
                    )
                }

                async fn execute_list(
                    &self,
                    query: &$crate::datastore::dsl::DataQuery,
                ) -> $crate::base::RiverbaseResult<Vec<serde_json::Value>> {
                    self.execute_list_with_total(query)
                        .await
                        .map(|(rows, _)| rows)
                }

                async fn execute_list_with_total(
                    &self,
                    query: &$crate::datastore::dsl::DataQuery,
                ) -> $crate::base::RiverbaseResult<(Vec<serde_json::Value>, i64)> {
                    self.executor
                        .query_list_with_total::<serde_json::Value>(query)
                        .await
                }

                async fn execute_item(
                    &self,
                    query: &$crate::datastore::dsl::DataQuery,
                    id: &str,
                ) -> $crate::base::RiverbaseResult<Option<serde_json::Value>> {
                    match self.executor.query_item::<serde_json::Value>(query, id).await {
                        Ok(row) => Ok(Some(row)),
                        Err(e) if $crate::datastore::is_not_found(&e) => Ok(None),
                        Err(e) => Err(e),
                    }
                }

                fn item_not_found_error_code(&self) -> &'static str {
                    $crate::query_resource!(@item_not_found_code $($item_code)?)
                }

                $crate::query_resource!(@policy_requirement $($policy_requirement $(($policy_reason, $policy_signoff))?)?);
                $crate::query_resource!(@reject_public_scope $($policy_requirement)? ; $($pol_ctx)?);
                $crate::query_resource!(@scope_policy_fn $( ($pol_ctx, $pol_scope) { $($policy_body)* } )? );
                $crate::query_resource!(@tenant_scope_fn $($tenant_reason, $tenant_signoff)?);
                $crate::query_resource!(@handler_fn $( ($handler_self, $handler_ctx, $handler_session, $handler_request, $handler_query) { $($handler_body)* } )? );

                fn debug_validate_order_coverage(&self) -> $crate::base::RiverbaseResult<()> {
                    $crate::query::debug_validate_sortable_order_coverage(
                        self.name(),
                        self.interface(),
                        self.binding(),
                        self.executor.as_ref(),
                    )
                }
            }
        }
    };

    (@handler_fn) => {};
    (@handler_fn ($handler_self:ident, $handler_ctx:ident, $handler_session:ident, $handler_request:ident, $handler_query:ident) { $($handler_body:tt)* }) => {
        fn execute_report<'a>(
            &'a self,
            $handler_ctx: &'a $crate::base::EngineContext,
            $handler_session: &'a $crate::query::QuerySession,
            $handler_request: &'a $crate::query::QueryRequest,
            $handler_query: &'a $crate::datastore::dsl::DataQuery,
        ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = $crate::base::RiverbaseResult<$crate::query::ReportOutput>> + Send + 'a>> {
            let $handler_self = self;
            Box::pin(async move { $($handler_body)* })
        }
    };

    // --- scope_policy hook generation ---
    //
    // Emits a `QueryResource::scope_policy` override only when a `scope_policy(ctx, url_scope) { … }`
    // block is declared; otherwise the trait default (`Ok(None)`) applies. The block body must
    // evaluate to `RiverbaseResult<Option<Expr>>` using **physical** column names.

    (@tenant_scope_fn) => {};
    (@tenant_scope_fn $tenant_reason:literal, $tenant_signoff:literal) => {
        fn tenant_scope_exempt(&self) -> bool {
            let _ = ($tenant_reason, $tenant_signoff);
            true
        }
    };

    (@scope_policy_fn) => {};
    (@scope_policy_fn ($pol_ctx:ident, $pol_scope:ident) { $($policy_body:tt)* }) => {
        fn has_scope_policy(&self) -> bool {
            true
        }

        fn scope_policy(
            &self,
            $pol_ctx: &$crate::base::EngineContext,
            $pol_scope: ::std::option::Option<&::serde_json::Value>,
        ) -> $crate::base::RiverbaseResult<::std::option::Option<$crate::datastore::dsl::Expr>> {
            $($policy_body)*
        }
    };

    (@policy_requirement) => {
        fn policy_requirement(&self) -> $crate::query::PolicyRequirement {
            $crate::query::PolicyRequirement::Required
        }
    };
    (@policy_requirement required) => {
        fn policy_requirement(&self) -> $crate::query::PolicyRequirement {
            $crate::query::PolicyRequirement::Required
        }
    };
    (@policy_requirement optional) => {
        fn policy_requirement(&self) -> $crate::query::PolicyRequirement {
            $crate::query::PolicyRequirement::Optional
        }
    };
    (@policy_requirement public) => {
        ::core::compile_error!(
            "policy: public requires a reason and sign-off: policy: public(\"why\", \"TICKET\")"
        );
        fn policy_requirement(&self) -> $crate::query::PolicyRequirement {
            $crate::query::PolicyRequirement::Required
        }
    };
    (@policy_requirement public ($policy_reason:literal, $policy_signoff:literal)) => {
        fn policy_requirement(&self) -> $crate::query::PolicyRequirement {
            $crate::query::PolicyRequirement::Public {
                reason: $policy_reason,
                signoff: $policy_signoff,
            }
        }
    };
    (@reject_public_scope) => {};
    (@reject_public_scope ; $($rest:tt)*) => {};
    (@reject_public_scope required ; $($rest:tt)*) => {};
    (@reject_public_scope optional ; $($rest:tt)*) => {};
    (@reject_public_scope public ; ) => {};
    (@reject_public_scope public ; $pol_ctx:ident) => {
        ::core::compile_error!(
            "policy: public cannot be combined with scope_policy; use Required if the constraint matters"
        );
    };

    // --- QueryInterface generation ---

    (@iface_impl $iface:ident {
        meta {
            $(title: $title:literal,)?
            $(description: $description:literal,)?
            default_order: [$($order_field:ident.$order_dir:ident),* $(,)?] $(,)?
            $(order_fields: [$($allow_field:ident),* $(,)?],)?
            $(scope: $scope:ident,)?
            $(allow_text_search: $allow_text_search:ident,)?
            $(openapi_tag: $openapi_tag:literal,)?
            $(openapi_explorer: $openapi_explorer:ident,)?
            $(openapi_internal: $openapi_internal:ident,)?
            $(allowed_zones: [ $($allowed_zone:literal),* $(,)? ],)?
            $(roles_required: [ $($role_req:literal),* $(,)? ],)?
        }
        $(field $field_name:ident {
            preset: $preset:ident,
            label: $field_label:literal
            $(, $field_attr:ident)*
        })*
    }) => {
        pub struct $iface;

        impl $crate::query::QueryInterface for $iface {
            fn fields(&self) -> &'static [$crate::query::interface::FieldDef] {
                static FIELDS: &[$crate::query::interface::FieldDef] = &[
                    $($crate::query_resource!(@field $field_name, $field_label, $preset $(, $field_attr)*),)*
                ];
                FIELDS
            }

            fn title(&self) -> &'static str {
                $crate::query_resource!(@title_val $($title)?)
            }

            fn description(&self) -> &'static str {
                $crate::query_resource!(@description_val $($description)?)
            }

            fn scope(&self) -> $crate::base::ScopeMeta {
                $crate::query_resource!(@scope_val $($scope)?)
            }

            fn default_order(&self) -> &'static [(&'static str, $crate::query::SortDirection)] {
                static ORDER: &[(&'static str, $crate::query::SortDirection)] = &[
                    $($crate::query_resource!(@order $order_field.$order_dir),)*
                ];
                ORDER
            }

            fn order_fields(&self) -> &'static [&'static str] {
                static ORDER_FIELDS: &[&str] = &[
                    $($(stringify!($allow_field),)*)?
                ];
                ORDER_FIELDS
            }

            fn allow_text_search(&self) -> bool {
                $crate::query_resource!(@allow_text_search_val $($allow_text_search)?)
            }

            fn openapi(&self) -> $crate::util::openapi_meta::OpenApiMeta {
                $crate::query_resource!(@openapi_meta_val
                    $(tag: $openapi_tag,)?
                    $(explorer: $openapi_explorer,)?
                    $(internal: $openapi_internal,)?
                )
            }

            fn allowed_zones(&self) -> Vec<String> {
                $crate::query_resource!(@allowed_zones_val $($($allowed_zone),*)?)
            }

            fn roles_required(&self) -> &'static [&'static str] {
                $crate::query_resource!(@roles_required_val $($($role_req),*)?)
            }
        }
    };

    (@allow_text_search_val) => { false };
    (@allow_text_search_val true) => { true };
    (@allow_text_search_val false) => { false };

    (@openapi_meta_val $(tag: $tag:literal,)? $(explorer: $exp:ident,)? $(internal: $int:ident,)?) => {
        $crate::util::openapi_meta::OpenApiMeta {
            tag: $crate::query_resource!(@opt_openapi_tag $($tag)?),
            explorer: $crate::query_resource!(@opt_openapi_explorer $($exp)?),
            internal: $crate::query_resource!(@opt_openapi_internal $($int)?),
            deprecated: false,
        }
    };

    (@allowed_zones_val) => { Vec::new() };
    (@allowed_zones_val $($zone:literal),+) => {
        vec![$($zone.to_string()),+]
    };

    (@roles_required_val) => { &[] };
    (@roles_required_val $($role:literal),+) => {
        {
            static ROLES: &[&str] = &[$($role),+];
            ROLES
        }
    };

    (@opt_openapi_tag) => { None };
    (@opt_openapi_tag $tag:literal) => { Some($tag.to_string()) };

    (@opt_openapi_explorer) => { None };
    (@opt_openapi_explorer true) => { Some(true) };
    (@opt_openapi_explorer false) => { Some(false) };

    (@opt_openapi_internal) => { None };
    (@opt_openapi_internal true) => { Some(true) };
    (@opt_openapi_internal false) => { Some(false) };

    (@scope_val) => { $crate::base::ScopeMeta::none() };
    (@scope_val none) => { $crate::base::ScopeMeta::none() };
    (@scope_val required) => { $crate::base::ScopeMeta::required(None) };

    (@title_val) => { "" };
    (@title_val $title:literal) => { $title };

    (@description_val) => { "" };
    (@description_val $description:literal) => { $description };

    (@order $field:ident.desc) => {
        (stringify!($field), $crate::query::SortDirection::Desc)
    };
    (@order $field:ident.asc) => {
        (stringify!($field), $crate::query::SortDirection::Asc)
    };

    (@field $name:ident, $label:literal, $preset:ident $(, $attr:ident)*) => {
        $crate::query_resource!(@apply_attrs
            $crate::query::interface::FieldDef::new(
                stringify!($name),
                $label,
                $crate::query_resource!(@preset $preset),
            )
            $(, $attr)*
        )
    };

    (@apply_attrs $field:expr $(,)?) => {
        $field
    };
    (@apply_attrs $field:expr, identifier $(, $rest:ident)*) => {
        $crate::query_resource!(@apply_attrs $field.identifier() $(, $rest)*)
    };
    (@apply_attrs $field:expr, hidden $(, $rest:ident)*) => {
        $crate::query_resource!(@apply_attrs $field.hidden() $(, $rest)*)
    };
    (@apply_attrs $field:expr, no_sort $(, $rest:ident)*) => {
        $crate::query_resource!(@apply_attrs $field.no_sort() $(, $rest)*)
    };
    (@apply_attrs $field:expr, sortable $(, $rest:ident)*) => {
        $crate::query_resource!(@apply_attrs $field.sortable() $(, $rest)*)
    };
    (@apply_attrs $field:expr, $unknown:ident $(, $rest:ident)*) => {
        compile_error!(concat!(
            "query_resource!: unknown field attribute `",
            stringify!($unknown),
            "` (expected identifier, hidden, no_sort, or sortable)"
        ));
    };

    (@preset Uuid) => { &$crate::query::interface::PRESET_UUID };
    (@preset String) => { &$crate::query::interface::PRESET_STRING };
    (@preset Boolean) => { &$crate::query::interface::PRESET_BOOLEAN };
    (@preset Datetime) => { &$crate::query::interface::PRESET_DATETIME };
    (@preset Date) => { &$crate::query::interface::PRESET_DATE };
    (@preset Integer) => { &$crate::query::interface::PRESET_INTEGER };
    (@preset Number) => { &$crate::query::interface::PRESET_NUMBER };
    (@preset Json) => { &$crate::query::interface::PRESET_JSON };
    (@preset Array) => { &$crate::query::interface::PRESET_ARRAY };
    (@preset Enum) => { &$crate::query::interface::PRESET_ENUM };
    (@preset Textsearch) => { &$crate::query::interface::PRESET_TEXTSEARCH };
    (@preset None) => { &$crate::query::interface::PRESET_NONE };
    (@preset $unknown:ident) => {
        compile_error!(concat!(
            "query_resource!: unknown preset `",
            stringify!($unknown),
            "` (expected Uuid, String, Boolean, Datetime, Date, Integer, Number, Json, Array, Enum, Textsearch, or None)"
        ));
    };

    // --- QueryBinding generation ---
    //
    // Explicit `$field => $mapping` entries are overrides only. Unlisted logical fields fall
    // through to `other => FieldSource::local(other)` (identity: field name = column name).

    (@binding_impl $binding:ident source $source:literal {
        $(join $join_name:ident -> $foreign_source:literal on $local_col:literal = $foreign_col:literal;)*
        $($field:ident => $mapping:tt),* $(,)?
    }) => {
        pub struct $binding;

        impl $crate::query::QueryBinding for $binding {
            fn source(&self) -> &'static str {
                $source
            }

            fn joins(&self) -> &'static [$crate::query::JoinDef] {
                static JOINS: &[$crate::query::JoinDef] = &[
                    $($crate::query_resource!(@join $join_name, $foreign_source, $local_col, $foreign_col),)*
                ];
                JOINS
            }

            fn identifier_column(&self) -> &str {
                $(
                    if stringify!($field) == "id" {
                        return $crate::query_resource!(@id_from_mapping $mapping);
                    }
                )*
                "id"
            }

            fn resolve(&self, field: &str) -> $crate::query::FieldSource {
                match field {
                    $(stringify!($field) => $crate::query_resource!(@resolve $mapping),)*
                    other => $crate::query::FieldSource::local(other.to_string()),
                }
            }
        }
    };

    (@join $name:ident, $foreign_source:literal, $local_col:literal, $foreign_col:literal) => {
        $crate::query::JoinDef {
            name: stringify!($name),
            foreign_source: $foreign_source,
            local_column: $local_col,
            foreign_column: $foreign_col,
            kind: $crate::datastore::dsl::JoinKind::Left,
        }
    };

    (@resolve $column:literal) => {
        $crate::query::FieldSource::local($column)
    };

    (@resolve $join:ident . $column:literal) => {
        $crate::query::FieldSource::joined(stringify!($join), $column)
    };

    (@id_from_mapping $column:literal) => {
        $column
    };
    (@id_from_mapping $join:ident . $column:literal) => {
        concat!(stringify!($join), ".", $column)
    };

    // --- Error code helpers ---

    (@list_code) => { "QRY-130" };
    (@list_code $code:literal) => { $code };

    (@list_msg) => { "Failed to list items." };
    (@list_msg $msg:literal) => { $msg };

    (@item_code) => { "QRY-131" };
    (@item_code $code:literal) => { $code };

    (@item_msg) => { "Failed to fetch item by id." };
    (@item_msg $msg:literal) => { $msg };

    (@item_not_found_code) => { "QRY-101" };
    (@item_not_found_code $code:literal) => { $code };

    // --- Compile-time SORT_COLUMNS on ResourceKey ---

    (@key_impl $key:ident {
        name: $name:literal,
        source: $source:literal,
        meta {
            $(title: $title:literal,)?
            $(description: $description:literal,)?
            default_order: [$($order_field:ident.$order_dir:ident),* $(,)?] $(,)?
            $(order_fields: [$($allow_field:ident),* $(,)?],)?
            $(scope: $scope:ident,)?
            $(allow_text_search: $allow_text_search:ident,)?
            $(openapi_tag: $openapi_tag:literal,)?
            $(openapi_explorer: $openapi_explorer:ident,)?
            $(openapi_internal: $openapi_internal:ident,)?
            $(allowed_zones: [ $($allowed_zone:literal),* $(,)? ],)?
            $(roles_required: [ $($role_req:literal),* $(,)? ],)?
        }
        $(field $field_name:ident {
            preset: $preset:ident,
            label: $field_label:literal
            $(, $field_attr:ident)*
        })*
        binding {
            $(join $join_name:ident -> $foreign_source:literal on $local_col:literal = $foreign_col:literal;)*
            $($bind_field:ident => $bind_map:tt),* $(,)?
        }
    }) => {
        impl $crate::datastore::ResourceKey for $key {
            const NAME: &'static str = $name;
            const SOURCE: &'static str = $source;
            const SORT_COLUMNS: &'static [&'static str] = $crate::query_resource!(@sort_columns_array
                ; allow [$($($allow_field),*)?]
                ; default [$($order_field),*]
                ; fields [$($field_name, $preset $(, $field_attr)* ;)*]
                ; bind { $($bind_field => $bind_map),* }
            );
        }

        #[allow(missing_docs)]
        impl $key {
            const ORDER_FIELDS: &'static [&'static str] =
                $crate::query_resource!(@str_slice [$($($allow_field),*)?]);
            const DEFAULT_ORDER_FIELDS: &'static [&'static str] =
                $crate::query_resource!(@str_slice [$($order_field),*]);

            const fn is_joined_field(field: &str) -> bool {
                $(
                    if $crate::query::const_str_eq(field, stringify!($bind_field)) {
                        return $crate::query_resource!(@joined_bool $bind_map);
                    }
                )*
                false
            }
        }

        $(
            const _: () = {
                let in_order_fields = $crate::query::const_slice_contains(
                    $key::ORDER_FIELDS,
                    stringify!($field_name),
                );
                let in_default = $crate::query::const_slice_contains(
                    $key::DEFAULT_ORDER_FIELDS,
                    stringify!($field_name),
                );
                let sortable = if !$key::ORDER_FIELDS.is_empty() {
                    in_order_fields || in_default
                } else {
                    $crate::query_resource!(@flag_bool $preset $(, $field_attr)*) || in_default
                };
                if sortable && $key::is_joined_field(stringify!($field_name)) {
                    panic!(
                        "query_resource!: sortable field maps to a joined column; mark it `no_sort`"
                    );
                }
            };
        )*
    };

    (@str_slice []) => { &[] };
    (@str_slice [$($name:ident),+ $(,)?]) => { &[$(stringify!($name)),+] };

    (@sort_columns_array ; allow [] ; default [$($def:ident),*] ; fields [$($field_name:ident, $preset:ident $(, $field_attr:ident)* ;)*] ; bind $bind:tt) => {
        &[
            $($crate::query_resource!(@sort_if_flag $field_name, $preset $(, $field_attr)* ; bind $bind),)*
            $($crate::query_resource!(@bind_lookup $def ; bind $bind),)*
        ]
    };
    (@sort_columns_array ; allow [$($allow:ident),+] ; default [$($def:ident),*] ; fields [$($field_name:ident, $preset:ident $(, $field_attr:ident)* ;)*] ; bind $bind:tt) => {
        &[
            $($crate::query_resource!(@bind_lookup $allow ; bind $bind),)*
            $($crate::query_resource!(@bind_lookup $def ; bind $bind),)*
        ]
    };

    (@flag_bool $preset:ident) => {
        $crate::query_resource!(@preset_sortable $preset)
    };
    (@flag_bool $preset:ident, identifier $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_bool $preset $(, $rest)*)
    };
    (@flag_bool $preset:ident, hidden $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_bool $preset $(, $rest)*)
    };
    (@flag_bool $preset:ident, sortable $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced true $(, $rest)*)
    };
    (@flag_bool $preset:ident, no_sort $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced false $(, $rest)*)
    };
    (@flag_bool $preset:ident, $unknown:ident $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_bool $preset $(, $rest)*)
    };

    (@flag_forced $flag:ident) => { $flag };
    (@flag_forced $flag:ident, identifier $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced $flag $(, $rest)*)
    };
    (@flag_forced $flag:ident, hidden $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced $flag $(, $rest)*)
    };
    (@flag_forced $flag:ident, sortable $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced true $(, $rest)*)
    };
    (@flag_forced $flag:ident, no_sort $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced false $(, $rest)*)
    };
    (@flag_forced $flag:ident, $unknown:ident $(, $rest:ident)*) => {
        $crate::query_resource!(@flag_forced $flag $(, $rest)*)
    };

    (@preset_sortable Uuid) => { true };
    (@preset_sortable String) => { true };
    (@preset_sortable Boolean) => { true };
    (@preset_sortable Datetime) => { true };
    (@preset_sortable Date) => { true };
    (@preset_sortable Integer) => { true };
    (@preset_sortable Number) => { true };
    (@preset_sortable Enum) => { true };
    (@preset_sortable Json) => { false };
    (@preset_sortable Array) => { false };
    (@preset_sortable Textsearch) => { false };
    (@preset_sortable None) => { false };
    (@preset_sortable $unknown:ident) => { false };

    (@joined_bool $col:literal) => { false };
    (@joined_bool $join:ident) => { true };
    (@joined_bool $other:tt) => { true };

    (@bind_col $col:literal) => { $col };
    (@bind_col $other:tt) => { stringify!($other) };

    (@bind_lookup $field:ident ; bind {}) => { stringify!($field) };
    (@bind_lookup $field:ident ; bind { $($bind_field:ident => $bind_map:tt),* $(,)? }) => {{
        let field = stringify!($field);
        if false {
            field
        } $(else if $crate::query::const_str_eq(field, stringify!($bind_field)) {
            $crate::query_resource!(@bind_col $bind_map)
        })*
        else {
            field
        }
    }};

    (@sort_if_flag $field:ident, $preset:ident ; bind $bind:tt) => {
        $crate::query_resource!(@sort_if_preset $field, $preset ; bind $bind)
    };
    (@sort_if_flag $field:ident, $preset:ident, identifier $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_if_flag $field, $preset $(, $rest)* ; bind $bind)
    };
    (@sort_if_flag $field:ident, $preset:ident, hidden $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_if_flag $field, $preset $(, $rest)* ; bind $bind)
    };
    (@sort_if_flag $field:ident, $preset:ident, sortable $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, yes $(, $rest)* ; bind $bind)
    };
    (@sort_if_flag $field:ident, $preset:ident, no_sort $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, no $(, $rest)* ; bind $bind)
    };
    (@sort_if_flag $field:ident, $preset:ident, $unknown:ident $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_if_flag $field, $preset $(, $rest)* ; bind $bind)
    };

    (@sort_forced $field:ident, yes ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_forced $field:ident, no ; bind $bind:tt) => { "" };
    (@sort_forced $field:ident, $flag:ident, identifier $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, $flag $(, $rest)* ; bind $bind)
    };
    (@sort_forced $field:ident, $flag:ident, hidden $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, $flag $(, $rest)* ; bind $bind)
    };
    (@sort_forced $field:ident, $flag:ident, sortable $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, yes $(, $rest)* ; bind $bind)
    };
    (@sort_forced $field:ident, $flag:ident, no_sort $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, no $(, $rest)* ; bind $bind)
    };
    (@sort_forced $field:ident, $flag:ident, $unknown:ident $(, $rest:ident)* ; bind $bind:tt) => {
        $crate::query_resource!(@sort_forced $field, $flag $(, $rest)* ; bind $bind)
    };

    // Keep in sync with PRESET_*.sortable (`query_resource_sort_preset_table_matches_statics`).
    (@sort_if_preset $field:ident, Uuid ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, String ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Boolean ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Datetime ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Date ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Integer ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Number ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Enum ; bind $bind:tt) => {
        $crate::query_resource!(@bind_lookup $field ; bind $bind)
    };
    (@sort_if_preset $field:ident, Json ; bind $bind:tt) => { "" };
    (@sort_if_preset $field:ident, Array ; bind $bind:tt) => { "" };
    (@sort_if_preset $field:ident, Textsearch ; bind $bind:tt) => { "" };
    (@sort_if_preset $field:ident, None ; bind $bind:tt) => { "" };
    (@sort_if_preset $field:ident, $unknown:ident ; bind $bind:tt) => { "" };
}

/// Resolve the compile-time key type emitted by [`query_resource!`] / [`report_resource!`].
///
/// `resource_name!(WorkerJobQueryResource)` is `WorkerJobQueryResourceKey`, whose
/// [`ResourceKey::NAME`](crate::datastore::ResourceKey::NAME) is the single string declaration.
#[macro_export]
macro_rules! resource_name {
    ($resource:ident) => {
        $crate::paste::paste! { [<$resource Key>] }
    };
}

/// Declare a report query resource: interface (filters + params), binding, and [`QueryResource`] impl.
///
/// Like [`query_resource!`], but exposes only `.rept` and `.meta` routes. Queryable columns are
/// declared under `filters { field … }` (same field syntax as `query_resource!`'s `fields` block).
/// Input parameters are declared under `params { param … { type: …, label: … } }` and surfaced in
/// `.meta` as a key/value schema (declarative; not lowered into the query automatically).
///
/// Companion types: `{Resource}Interface` and `{Resource}Binding`.
#[macro_export]
macro_rules! report_resource {
    (
        $resource:ident name $name:literal {
            meta {
                $($meta:tt)*
            }
            filters {
                $(field $field_name:ident {
                    preset: $preset:ident,
                    label: $field_label:literal
                    $(, $field_attr:ident)*
                })*
            }
            params {
                $(param $param_name:ident {
                    type: $param_type:ident,
                    label: $param_label:literal
                    $(, $param_attr:tt)*
                })*
            }
            binding source $source:literal {
                $($binding_body:tt)*
            }
            $(errors {
                report: ($report_code:literal, $report_msg:literal),
            })?
            $(policy: $policy_requirement:ident $(($policy_reason:literal, $policy_signoff:literal))?,)?
            $(scope_policy ($pol_ctx:ident, $pol_scope:ident) {
                $($policy_body:tt)*
            })?
            $(tenant_scope: exempt ($tenant_reason:literal, $tenant_signoff:literal),)?
            $(handler ($handler_self:ident, $handler_ctx:ident, $handler_session:ident, $handler_request:ident, $handler_query:ident) {
                $($handler_body:tt)*
            })?
        }
    ) => {
        $crate::report_resource!(@emit $resource, $name, {
            meta { $($meta)* }
            $(field $field_name {
                preset: $preset,
                label: $field_label
                $(, $field_attr)*
            })*
            $(param $param_name {
                type: $param_type,
                label: $param_label
                $(, $param_attr)*
            })*
            binding source $source {
                $($binding_body)*
            }
            report_code { $($report_code)? }
            report_msg { $($report_msg)? }
            policy { $($policy_requirement $(($policy_reason, $policy_signoff))?)? }
            scope_policy { $(($pol_ctx, $pol_scope) { $($policy_body)* })? }
            tenant_scope { $($tenant_reason, $tenant_signoff)? }
            handler { $(($handler_self, $handler_ctx, $handler_session, $handler_request, $handler_query) { $($handler_body)* })? }
        });
    };

    (@emit $resource:ident, $name:literal, {
        meta { $($meta:tt)* }
        $(field $field_name:ident {
            preset: $preset:ident,
            label: $field_label:literal
            $(, $field_attr:ident)*
        })*
        $(param $param_name:ident {
            type: $param_type:ident,
            label: $param_label:literal
            $(, $param_attr:tt)*
        })*
        binding source $source:literal {
            $($binding_body:tt)*
        }
        report_code { $($report_code:literal)? }
        report_msg { $($report_msg:literal)? }
        policy { $($policy_requirement:ident $(($policy_reason:literal, $policy_signoff:literal))?)? }
        scope_policy { $(($pol_ctx:ident, $pol_scope:ident) { $($policy_body:tt)* })? }
        tenant_scope { $($tenant_reason:literal, $tenant_signoff:literal)? }
        handler { $(($handler_self:ident, $handler_ctx:ident, $handler_session:ident, $handler_request:ident, $handler_query:ident) { $($handler_body:tt)* })? }
    }) => {
        $crate::paste::paste! {
            $crate::report_resource!(@iface_impl [<$resource Interface>] {
                meta { $($meta)* }
                $(field $field_name {
                    preset: $preset,
                    label: $field_label
                    $(, $field_attr)*
                })*
                $(param $param_name {
                    type: $param_type,
                    label: $param_label
                    $(, $param_attr)*
                })*
            });

            $crate::query_resource!(@binding_impl [<$resource Binding>] source $source {
                $($binding_body)*
            });

            pub struct $resource<M: $crate::datastore::DataStore + 'static> {
                executor: ::std::sync::Arc<M>,
                interface: [<$resource Interface>],
                binding: [<$resource Binding>],
            }

            impl<M: $crate::datastore::DataStore + 'static> $resource<M> {
                /// Wire resource name declared in the resource macro.
                pub const NAME: &'static str = $name;
                /// Storage source key declared in `binding source`.
                pub const SOURCE: &'static str = $source;

                pub fn new(executor: ::std::sync::Arc<M>) -> Self {
                    Self {
                        executor,
                        interface: [<$resource Interface>],
                        binding: [<$resource Binding>],
                    }
                }
            }

            /// Compile-time resource key. The string `$name` is declared once on this resource.
            #[derive(Debug, Clone, Copy, Default)]
            pub struct [<$resource Key>];

            $crate::query_resource!(@key_impl [<$resource Key>] {
                name: $name,
                source: $source,
                meta { $($meta)* }
                $(field $field_name {
                    preset: $preset,
                    label: $field_label
                    $(, $field_attr)*
                })*
                binding {
                    $($binding_body)*
                }
            });

            #[::async_trait::async_trait]
            impl<M: $crate::datastore::DataStore + 'static> $crate::query::QueryResource for $resource<M> {
                fn name(&self) -> &str {
                    $name
                }

                fn interface(&self) -> &dyn $crate::query::QueryInterface {
                    &self.interface
                }

                fn binding(&self) -> &dyn $crate::query::QueryBinding {
                    &self.binding
                }

                fn policy_filter_enforced(&self) -> bool {
                    $crate::datastore::DataStore::enforces_policy_filter(
                        self.executor.as_ref(),
                        $crate::query::QueryBinding::source(&self.binding),
                    )
                }

                fn route_kind(&self) -> $crate::query::QueryResourceKind {
                    $crate::query::QueryResourceKind::Report
                }

                async fn execute_list(
                    &self,
                    query: &$crate::datastore::dsl::DataQuery,
                ) -> $crate::base::RiverbaseResult<Vec<serde_json::Value>> {
                    self.execute_list_with_total(query)
                        .await
                        .map(|(rows, _)| rows)
                }

                async fn execute_list_with_total(
                    &self,
                    query: &$crate::datastore::dsl::DataQuery,
                ) -> $crate::base::RiverbaseResult<(Vec<serde_json::Value>, i64)> {
                    self.executor
                        .query_list_with_total::<serde_json::Value>(query)
                        .await
                }

                async fn execute_item(
                    &self,
                    query: &$crate::datastore::dsl::DataQuery,
                    id: &str,
                ) -> $crate::base::RiverbaseResult<Option<serde_json::Value>> {
                    match self.executor.query_item::<serde_json::Value>(query, id).await {
                        Ok(row) => Ok(Some(row)),
                        Err(e) if $crate::datastore::is_not_found(&e) => Ok(None),
                        Err(e) => Err(e),
                    }
                }

                $crate::query_resource!(@policy_requirement $($policy_requirement $(($policy_reason, $policy_signoff))?)?);
                $crate::query_resource!(@reject_public_scope $($policy_requirement)? ; $($pol_ctx)?);
                $crate::query_resource!(@scope_policy_fn $( ($pol_ctx, $pol_scope) { $($policy_body)* } )? );
                $crate::query_resource!(@tenant_scope_fn $($tenant_reason, $tenant_signoff)?);
                $crate::query_resource!(@handler_fn $( ($handler_self, $handler_ctx, $handler_session, $handler_request, $handler_query) { $($handler_body)* } )? );

                fn debug_validate_order_coverage(&self) -> $crate::base::RiverbaseResult<()> {
                    $crate::query::debug_validate_sortable_order_coverage(
                        self.name(),
                        self.interface(),
                        self.binding(),
                        self.executor.as_ref(),
                    )
                }
            }
        }
    };

    (@iface_impl $iface:ident {
        meta {
            $(title: $title:literal,)?
            $(description: $description:literal,)?
            default_order: [$($order_field:ident.$order_dir:ident),* $(,)?] $(,)?
            $(order_fields: [$($allow_field:ident),* $(,)?],)?
            $(scope: $scope:ident,)?
            $(allow_text_search: $allow_text_search:ident,)?
            $(openapi_tag: $openapi_tag:literal,)?
            $(openapi_explorer: $openapi_explorer:ident,)?
            $(openapi_internal: $openapi_internal:ident,)?
            $(allowed_zones: [ $($allowed_zone:literal),* $(,)? ],)?
            $(roles_required: [ $($role_req:literal),* $(,)? ],)?
        }
        $(field $field_name:ident {
            preset: $preset:ident,
            label: $field_label:literal
            $(, $field_attr:ident)*
        })*
        $(param $param_name:ident {
            type: $param_type:ident,
            label: $param_label:literal
            $(, $param_attr:tt)*
        })*
    }) => {
        pub struct $iface;

        impl $crate::query::QueryInterface for $iface {
            fn fields(&self) -> &'static [$crate::query::interface::FieldDef] {
                static FIELDS: &[$crate::query::interface::FieldDef] = &[
                    $($crate::query_resource!(@field $field_name, $field_label, $preset $(, $field_attr)*),)*
                ];
                FIELDS
            }

            fn params(&self) -> &'static [$crate::query::interface::ParamDef] {
                static PARAMS: &[$crate::query::interface::ParamDef] = &[
                    $($crate::report_resource!(@param $param_name, $param_label, $param_type $(, $param_attr)*),)*
                ];
                PARAMS
            }

            fn title(&self) -> &'static str {
                $crate::query_resource!(@title_val $($title)?)
            }

            fn description(&self) -> &'static str {
                $crate::query_resource!(@description_val $($description)?)
            }

            fn scope(&self) -> $crate::base::ScopeMeta {
                $crate::query_resource!(@scope_val $($scope)?)
            }

            fn default_order(&self) -> &'static [(&'static str, $crate::query::SortDirection)] {
                static ORDER: &[(&'static str, $crate::query::SortDirection)] = &[
                    $($crate::query_resource!(@order $order_field.$order_dir),)*
                ];
                ORDER
            }

            fn order_fields(&self) -> &'static [&'static str] {
                static ORDER_FIELDS: &[&str] = &[
                    $($(stringify!($allow_field),)*)?
                ];
                ORDER_FIELDS
            }

            fn allow_text_search(&self) -> bool {
                $crate::query_resource!(@allow_text_search_val $($allow_text_search)?)
            }

            fn openapi(&self) -> $crate::util::openapi_meta::OpenApiMeta {
                $crate::query_resource!(@openapi_meta_val
                    $(tag: $openapi_tag,)?
                    $(explorer: $openapi_explorer,)?
                    $(internal: $openapi_internal,)?
                )
            }

            fn allowed_zones(&self) -> Vec<String> {
                $crate::query_resource!(@allowed_zones_val $($($allowed_zone),*)?)
            }

            fn roles_required(&self) -> &'static [&'static str] {
                $crate::query_resource!(@roles_required_val $($($role_req),*)?)
            }
        }
    };

    (@param $name:ident, $label:literal, $preset:ident $(, $attr:tt)*) => {
        $crate::report_resource!(@apply_param_attrs
            $crate::query::interface::ParamDef::new(
                stringify!($name),
                $label,
                $crate::query_resource!(@preset $preset),
            )
            $(, $attr)*
        )
    };

    (@apply_param_attrs $param:expr $(,)?) => {
        $param
    };
    (@apply_param_attrs $param:expr, required $(, $($rest:tt)*)?) => {
        $crate::report_resource!(@apply_param_attrs $param.required() $(, $($rest)*)?)
    };
    (@apply_param_attrs $param:expr, default : $value:literal $(, $($rest:tt)*)?) => {
        $crate::report_resource!(@apply_param_attrs $param.default_value($value) $(, $($rest)*)?)
    };

    (@report_code) => { "QRY-140" };
    (@report_code $code:literal) => { $code };

    (@report_msg) => { "Failed to run report." };
    (@report_msg $msg:literal) => { $msg };
}

/// Declare a domain query engine: struct, `spawn`, accessors, and [`DomainQueryEngine`] impl.
///
/// Wraps a [`QueryEngine`] for a fixed set of [`query_resource!`]-generated resources, generating
/// the boilerplate that is otherwise hand-written per domain (see the todo-app `TodoQueryEngine`).
/// Optional [`report_resource!`] entries are registered from the `reports { … }` block.
/// Mirrors the engine form of [`command_engine!`](crate::command_engine).
///
/// The generated `spawn` is generic over a [`DataStore`] and registers every listed resource from a
/// single shared store handle (`Resource::new(store.clone())`). Extra `component:` fields are added
/// to the struct and appended to `spawn`'s parameter list. Add bespoke constructors (e.g. an
/// in-memory variant) in a separate `impl` block when a domain needs them.
///
/// ```ignore
/// riverbase_core::query_engine! {
///     engine TodoQueryEngine {
///         component: {},
///     }
///     resources {
///         TodoQueryResource,
///     }
/// }
/// ```
///
/// [`DomainQueryEngine`]: crate::domain::DomainQueryEngine
/// [`QueryEngine`]: crate::query::QueryEngine
/// [`DataStore`]: crate::datastore::DataStore
#[macro_export]
macro_rules! query_engine {
    (
        engine $engine:ident {
            $(
                meta {
                    $($pool_key:ident : $pool_size:literal),+ $(,)?
                }
            )?
            $(component: {
                $($component:ident : $component_ty:ty),* $(,)?
            } $(,)?)?
        }
        resources {
            $($resource:ident),+ $(,)?
        }
        $(reports {
            $($report:ident),+ $(,)?
        })?
    ) => {
        #[derive(Clone)]
        pub struct $engine {
            ctx: $crate::base::EngineContext,
            engine: $crate::query::QueryEngine,
            $($($component: $component_ty,)*)?
        }

        impl $engine {
            /// Concurrent query lanes for this domain engine (from engine meta, default 4).
            /// Prefer meta key `query_pool_size`; `actor_pool_size` remains a compatibility alias.
            pub const ACTOR_POOL_SIZE: u32 = $crate::query_engine!(@pool_size $($($pool_key = $pool_size),+)?);
            pub const QUERY_POOL_SIZE: u32 = Self::ACTOR_POOL_SIZE;

            /// Spawn the query engine, registering every declared query and report resource from `store`.
            pub async fn spawn<M: $crate::datastore::DataStore + 'static>(
                ctx: $crate::base::EngineContext,
                store: ::std::sync::Arc<M>,
                logstore: $crate::logstore::DomainLogStore,
                $($($component: $component_ty,)*)?
            ) -> $crate::base::RiverbaseResult<Self> {
                let mut args = $crate::query::QueryEngineArgs::new(logstore);
                $(
                    args.register(::std::sync::Arc::new($resource::new(store.clone())));
                )+
                $(
                    $(
                        args.register(::std::sync::Arc::new($report::new(store.clone())));
                    )+
                )?
                args.apply_engine_context(&ctx);
                let engine =
                    $crate::query::QueryEngine::spawn_with_size(Self::ACTOR_POOL_SIZE as usize, args).await?;
                ::std::result::Result::Ok(Self { ctx, engine, $($($component,)*)? })
            }

            pub fn context(&self) -> &$crate::base::EngineContext {
                &self.ctx
            }

            pub fn inner(&self) -> &$crate::query::QueryEngine {
                &self.engine
            }
        }

        #[::async_trait::async_trait]
        impl $crate::domain::DomainQueryEngine for $engine {
            fn context(&self) -> &$crate::base::EngineContext {
                &self.ctx
            }

            async fn queries(
                &self,
            ) -> $crate::base::RiverbaseResult<::std::vec::Vec<::std::string::String>> {
                $crate::base::Engine::items(&self.engine).await
            }

            async fn query_scope_metas(
                &self,
            ) -> $crate::base::RiverbaseResult<::std::vec::Vec<$crate::query::QueryRouteMeta>> {
                self.engine.scope_metas().await
            }

            async fn execute(
                &self,
                ctx: &$crate::base::EngineContext,
                query_resource: &str,
                access: $crate::query::QueryAccess,
                request: $crate::query::QueryRequest,
                item_id: ::std::option::Option<&str>,
            ) -> $crate::base::RiverbaseResult<::serde_json::Value> {
                self.engine
                    .execute(ctx, query_resource, access, request, item_id)
                    .await
            }
        }
    };

    (@pool_size) => {
        $crate::pool::clamp_actor_pool_size($crate::pool::DEFAULT_ACTOR_POOL_SIZE)
    };
    (@pool_size query_pool_size = $size:literal $(, $($rest:tt)*)?) => {
        $crate::pool::clamp_actor_pool_size($size)
    };
    (@pool_size actor_pool_size = $size:literal $(, $($rest:tt)*)?) => {
        $crate::pool::clamp_actor_pool_size($size)
    };
    (@pool_size $bad:ident = $size:literal $(, $($rest:tt)*)?) => {
        compile_error!(concat!(
            "query_engine!: unknown pool meta key `",
            stringify!($bad),
            "` (expected `query_pool_size` or `actor_pool_size`)"
        ))
    };
}
