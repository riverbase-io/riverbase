//! Architecture conformance checks ([TST-01]).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn exp_backend_root() -> Option<PathBuf> {
    for ancestor in workspace_root().ancestors() {
        let in_tree = ancestor.join("api/exp-api/Cargo.toml");
        if in_tree.is_file() {
            return Some(ancestor.to_path_buf());
        }
        let sibling = ancestor.join("exp-backend/api/exp-api/Cargo.toml");
        if sibling.is_file() {
            return Some(ancestor.join("exp-backend"));
        }
    }
    None
}

fn http_crate_root() -> PathBuf {
    workspace_root().join("crates/riverbase_http")
}

fn read_http(rel: &str) -> String {
    fs::read_to_string(http_crate_root().join(rel))
        .unwrap_or_else(|err| panic!("read riverbase_http/{rel}: {err}"))
}

#[test]
fn error_codes_are_unique_across_scanned_roots() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest_dir.join("../../scripts/validate-error-codes.py");
    let output = Command::new("python3")
        .arg(&script)
        .current_dir(manifest_dir.join("../.."))
        .output()
        .expect("run validate-error-codes.py");
    assert!(
        output.status.success(),
        "duplicate error codes detected:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn policy_requirement_defaults_to_required() {
    let resource = include_str!("../src/query/resource.rs");
    assert!(resource.contains("PolicyRequirement::Required"));
    assert!(resource.contains("Public"));
}

#[test]
fn roles_required_is_declared_on_command_meta_and_macro() {
    let meta = include_str!("../src/command/meta.rs");
    let command_macro = include_str!("../src/macro/command.rs");
    assert!(meta.contains("roles_required"));
    assert!(meta.contains("authorize_command_roles"));
    assert!(command_macro.contains("roles_required:"));
    assert!(command_macro.contains("with_roles_required"));
}

#[test]
fn roles_required_is_declared_on_query_interface_and_macro() {
    let iface = include_str!("../src/query/interface.rs");
    let query_macro = include_str!("../src/macro/query.rs");
    let engine = include_str!("../src/query/engine.rs");
    assert!(iface.contains("fn roles_required"));
    assert!(query_macro.contains("roles_required:"));
    assert!(engine.contains("iface.roles_required()"));
}

#[test]
fn auth_matrix_unmapped_mounted_namespace_is_denied() {
    let casbin = read_http("src/web/casbin_layer.rs");
    let route_auth = read_http("src/web/route_auth.rs");
    assert!(!casbin.contains("fn should_skip"));
    assert!(casbin.contains("forbidden_unmapped"));
    assert!(route_auth.contains("is_mounted_wire_namespace"));
}

#[test]
fn migrations_run_off_async_startup_path() {
    let dbpool = include_str!("../src/datastore/postgres/dbpool.rs");
    assert!(dbpool.contains("spawn_blocking"));
    assert!(dbpool.contains("run_startup_migrations"));
}

#[test]
fn query_max_limit_is_configured() {
    let cfg = include_str!("../src/config/mod.rs");
    assert!(cfg.contains("query_max_limit"));
    assert!(cfg.contains("query_count_max_rows"));
    assert!(include_str!("../src/query/engine.rs").contains("QRY-130"));
}

#[test]
fn remove_and_invalidate_are_separate_store_primitives() {
    let store = include_str!("../src/datastore/store.rs");
    assert!(!store.contains("enum DeleteMode"));
    assert!(store.contains("async fn remove"));
    assert!(store.contains("async fn invalidate"));
    let entity = include_str!("../src/datastore/postgres/entity.rs");
    assert!(entity.contains("async fn remove"));
    assert!(entity.contains("async fn invalidate"));
    assert!(!entity.contains("supported_delete_modes"));
}

#[test]
fn ordered_domain_migrations_exist() {
    let migrations = include_str!("../src/domain/migrations.rs");
    assert!(migrations.contains("run_ordered_migrations"));
    assert!(include_str!("../src/domain/store.rs").contains("migration_name"));
}

#[test]
fn readiness_gates_on_migrations() {
    let health = read_http("src/web/health.rs");
    assert!(health.contains("migrations_ready"));
}

#[test]
fn success_envelope_and_problem_json() {
    let response = include_str!("../src/base/response.rs");
    assert!(response.contains("success_envelope"));
    assert!(include_str!("../src/query/engine.rs").contains("list_envelope"));

    let http_response = read_http("src/http_response.rs");
    assert!(http_response.contains("application/problem+json"));
}

#[test]
fn api_contract_wave4_conformance() {
    let response = include_str!("../src/base/response.rs");
    assert!(response.contains("API_CONTRACT_VERSION"));
    assert!(response.contains("\"2.0.0\""));
    assert!(response.contains("success_envelope"));
    assert!(response.contains("command_success_meta"));
    assert!(response.contains("struct ProblemDetails"));

    let openapi = read_http("src/web/openapi.rs");
    assert!(openapi.contains("default_response_with::<Json<ProblemDetails>"));
    assert!(!openapi.contains("default_response_with::<String"));
    assert!(openapi.contains("API_CONTRACT_VERSION"));

    let validator = read_http("src/auth/validator.rs");
    assert!(validator.contains("AUT-146"));
    assert!(validator.contains("Access token has expired."));

    let router = read_http("src/web/router.rs");
    assert!(router.contains("If-Match") || router.contains("IF_MATCH"));
    assert!(router.contains("ETAG") || router.contains("ETag"));
    assert!(router.contains("CREATED") || router.contains("201"));
    assert!(router.contains("command_success_meta"));

    let dbpool = include_str!("../src/datastore/postgres/dbpool.rs");
    assert!(dbpool.contains("DAT-012"));

    let middleware = read_http("src/auth/middleware.rs");
    assert!(middleware.contains("IDM-010"));
    assert!(middleware.contains("128"));
}

#[test]
fn memory_backends_are_gone() {
    let roots = [
        include_str!("../src/datastore/mod.rs"),
        include_str!("../src/datastore/store.rs"),
        include_str!("../src/domain/mod.rs"),
        include_str!("../src/domain/runtime.rs"),
        include_str!("../src/domain/store.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/logstore/mod.rs"),
        include_str!("../src/transport/bus.rs"),
        include_str!("../src/transport/mod.rs"),
        include_str!("../src/config/bus.rs"),
    ];
    let joined = roots.join("\n");
    for forbidden in [
        "MemoryDataStore",
        "MemoryMessageBus",
        "MemoryDomainLogStore",
        "MemoryIdempotencyStore",
        "MemoryProcessManagerStore",
        "DomainRuntime::in_memory",
        "fn in_memory(",
        "BusKind::Memory",
        "DbConnection::Memory",
        "DbConnection::Sqlite",
    ] {
        assert!(
            !joined.contains(forbidden),
            "in-memory removal: `{forbidden}` must be gone from riverbase_core surfaces"
        );
    }
    // Negative test may still mention the rejected scheme.
    assert!(
        include_str!("../src/config/bus.rs").contains("kind_from_url(\"memory://\").is_err()"),
        "memory:// must remain rejected by bus URL parsing"
    );
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!manifest.join("src/datastore/memory").exists());
    assert!(!manifest.join("src/transport/memory.rs").exists());
    assert!(!manifest.join("tests/datastore_memory.rs").exists());
}

#[test]
fn arc01_dead_abstractions_removed() {
    let roots = [
        include_str!("../src/command/mod.rs"),
        include_str!("../src/datastore/mod.rs"),
        include_str!("../src/datastore/store.rs"),
        include_str!("../src/domain/mod.rs"),
        include_str!("../src/domain/repository.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/transport/bus.rs"),
        include_str!("../src/transport/mod.rs"),
    ];
    let joined = roots.join("\n");
    for forbidden in [
        "CommandHandler<",
        "AggregateRepository",
        "VersionedAggregate",
        "DeleteSemantics",
        "LegacyPgDataStore",
        "ActorMessageBus",
        "trait StateStore",
        "pub use state::StateStore",
    ] {
        assert!(
            !joined.contains(forbidden),
            "ARC-01: `{forbidden}` must be gone from riverbase_core surfaces"
        );
    }
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/command/command.rs")
        .exists());
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/datastore/state.rs")
        .exists());
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/datastore/postgres/datastore.rs")
        .exists());
}

#[test]
fn arc02_statestore_collapsed_into_datastore() {
    let store = include_str!("../src/datastore/store.rs");
    assert!(store.contains("async fn state_fetch"));
    assert!(store.contains("async fn state_upsert"));
    assert!(store.contains("async fn state_create"));
    assert!(store.contains("fn supports_command_transactions"));
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/datastore/state.rs")
        .exists());
    let lib = include_str!("../src/lib.rs");
    assert!(!lib.contains("StateStore,"));
    assert!(lib.contains("DataStoreInit"));
}

#[test]
fn arc03_kernel_and_http_crates_split() {
    let root = workspace_root();
    assert!(
        root.join("crates/riverbase_http/Cargo.toml").exists(),
        "riverbase_http crate missing"
    );
    assert!(
        !root.join("crates/riverbase_kernel/Cargo.toml").exists(),
        "riverbase_kernel shim must be removed"
    );
    assert!(
        !root.join("crates/riverbase_web/Cargo.toml").exists(),
        "riverbase_web shim must be removed"
    );
    assert!(
        !root.join("crates/riverbase_base/Cargo.toml").exists(),
        "riverbase_base renamed to riverbase_http"
    );

    let core_lib = include_str!("../src/lib.rs");
    assert!(!core_lib.contains("pub mod web;"));
    assert!(!core_lib.contains("pub mod auth;"));
    assert!(!core_lib.contains("pub mod casbin;"));

    let http_lib = read_http("src/lib.rs");
    assert!(http_lib.contains("pub mod web;"));
    assert!(http_lib.contains("PortalComposer"));
}

#[test]
fn arc04_misconfigured_command_transactions_error() {
    let engine = include_str!("../src/command/engine.rs");
    assert!(engine.contains("CMD-027"));
    assert!(engine.contains("supports_command_transactions"));
    let tx = include_str!("../src/datastore/transaction.rs");
    assert!(tx.contains("enum CommandUnitOfWork"));
    assert!(!tx.contains("task_local!"));
    let pg_tx = include_str!("../src/datastore/postgres/transaction.rs");
    assert!(!pg_tx.contains("task_local!"));
    assert!(pg_tx.contains("pub async fn pool_connection"));
}

#[test]
fn dx03_readonly_entity_macro_and_hardened_defaults() {
    let readonly = include_str!("../src/macro/pg_readonly_entity.rs");
    assert!(readonly.contains("macro_rules! pg_readonly_entity"));
    let entity = include_str!("../src/datastore/postgres/entity.rs");
    assert!(entity.contains("fn enforces_policy_filter(&self) -> bool;"));
    assert!(entity.contains("async fn remove(&self, conn: &mut AsyncPgConnection, id: &str)"));
    assert!(!entity.contains("fn enforces_policy_filter(&self) -> bool {\n        false"));
}

#[test]
fn cmd_018_used_for_unregistered_command_not_cmd_002() {
    let registry = include_str!("../src/command/registry.rs");
    let router = read_http("src/web/router.rs");
    let payload = include_str!("../src/command/payload.rs");
    assert!(registry.contains("CMD-018"));
    assert!(router.contains("CMD-018"));
    assert!(payload.contains("CMD-002"));
    assert!(!registry.contains("\"CMD-002\""));
}

#[test]
fn dx05_startup_wiring_checks_exist() {
    let checks = read_http("src/web/startup_checks.rs");
    assert!(checks.contains("validate_mounted_query_engine"));
    assert!(checks.contains("APP-020"));
    let app = read_http("src/web/app_base.rs");
    assert!(app.contains("validate_mounted_query_engine"));
    let casbin = read_http("src/web/casbin_layer.rs");
    assert!(casbin.contains("CAS-010"));
    assert!(casbin.contains("policy_rule_count"));
    assert!(casbin.contains("mounted_namespace_count"));
}

#[test]
fn dx01_command_engine_supports_domain_trait_manual() {
    let command = include_str!("../src/macro/command.rs");
    assert!(command.contains("domain_trait"));
    assert!(command.contains("impl_domain_command_engine"));
    assert!(command.contains("@domain_trait_impl"));
}

#[test]
fn arc03_domain_crates_do_not_depend_on_riverbase_http() {
    let exp_backend = exp_backend_root().expect(
        "exp-backend checkout not found (expected api/exp-api next to or under an ancestor of riverbase)",
    );
    let domain_manifests = [
        exp_backend.join("api/exp-api/crates/exp_catalog/Cargo.toml"),
        exp_backend.join("api/exp-api/crates/exp_order/Cargo.toml"),
        exp_backend.join("api/exp-api/crates/exp_pricing/Cargo.toml"),
        exp_backend.join("api/rfx-bootstrap/crates/rfx_audit/Cargo.toml"),
        exp_backend.join("api/rfx-bootstrap/crates/rfx_flow/Cargo.toml"),
        exp_backend.join("api/rfx-bootstrap/crates/rfx_rule/Cargo.toml"),
    ];
    for manifest in domain_manifests {
        let text = fs::read_to_string(&manifest)
            .unwrap_or_else(|err| panic!("read {}: {err}", manifest.display()));
        assert!(
            !text.contains("riverbase_http"),
            "{} must not depend on riverbase_http",
            manifest.display()
        );
        if text.contains("riverbase_core") {
            assert!(
                !text.contains(r#"features = ["web"#) && !text.contains(r#"features = ["auth"#),
                "{} must not enable riverbase_core web/auth features",
                manifest.display()
            );
        }
    }
}

#[test]
fn arc03_kernel_crate_has_no_http_stack_in_tree() {
    let output = Command::new("cargo")
        .args([
            "tree",
            "-p",
            "riverbase_core",
            "--no-default-features",
            "--features",
            "kernel",
            "--edges",
            "normal",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("cargo tree riverbase_core");
    assert!(
        output.status.success(),
        "cargo tree failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8_lossy(&output.stdout);
    for forbidden in ["axum", "casbin", "jsonwebtoken"] {
        assert!(
            !tree.contains(forbidden),
            "riverbase_core kernel tree must not include {forbidden}:\n{tree}"
        );
    }
}

#[test]
fn arc03_sample_domain_has_no_http_stack_in_tree() {
    let rfx_bootstrap = exp_backend_root()
        .expect("exp-backend checkout not found")
        .join("api/rfx-bootstrap");
    for pkg in ["axum", "casbin", "jsonwebtoken"] {
        let output = Command::new("cargo")
            .args(["tree", "-p", "rfx_audit", "-i", pkg, "--edges", "normal"])
            .current_dir(&rfx_bootstrap)
            .output()
            .expect("cargo tree rfx_audit");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let absent = stdout.trim().is_empty() || stderr.contains("did not match any packages");
        assert!(absent, "rfx_audit must not pull {pkg}:\n{stdout}\n{stderr}");
    }
}

#[test]
fn todo_domain_default_tree_has_no_axum() {
    for pkg in ["axum", "casbin", "jsonwebtoken"] {
        let output = Command::new("cargo")
            .args(["tree", "-p", "todo-domain", "-i", pkg, "--edges", "normal"])
            .current_dir(workspace_root())
            .output()
            .expect("cargo tree todo-domain");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let absent = stdout.trim().is_empty() || stderr.contains("did not match any packages");
        assert!(
            absent,
            "todo-domain default features must not pull {pkg}:\n{stdout}\n{stderr}"
        );
    }
}

fn riverbase_conformance_app_rs() -> Option<PathBuf> {
    for ancestor in workspace_root().ancestors() {
        let toml = ancestor.join("lib/river-levee/app_rs/riverbase.toml");
        if toml.is_file() {
            return Some(ancestor.join("lib/river-levee/app_rs"));
        }
    }
    None
}

#[test]
fn app_rs_has_casbin_enabled() {
    let Some(app_rs) = riverbase_conformance_app_rs() else {
        return;
    };
    let toml = fs::read_to_string(app_rs.join("riverbase.toml")).expect("read app_rs riverbase.toml");
    let enabled = toml.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "enabled = true" && toml.contains("[riverbase.casbin]")
    });
    let casbin_section = toml.split("[riverbase.casbin]").nth(1).unwrap_or("");
    let casbin_enabled = casbin_section
        .lines()
        .take_while(|line| !line.starts_with('['))
        .any(|line| line.trim() == "enabled = true");
    assert!(
        casbin_enabled,
        "app_rs riverbase.toml must enable Casbin (REF-02); found:\n{toml}"
    );
    let _ = enabled;
    assert!(
        app_rs.join("configs/policies/conform.csv").is_file(),
        "app_rs must ship configs/policies/conform.csv"
    );
}
