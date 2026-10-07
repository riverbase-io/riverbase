# riverbase

Command/query library for domain-driven applications. It provides the kernel for aggregates, commands, and queries, plus HTTP, media, worker, and form crates on top of **axum**, **Diesel**, and **Postgres**.

This repository is the library workspace. It contains the crates you depend on. Application examples and design notes are not included.

## Crates

| Crate | Role |
| --- | --- |
| `riverbase_core` | Domain kernel: commands, queries, aggregates, Postgres datastore, audit log, configuration, and transport |
| `riverbase_proc` | Proc-macros that register commands and domain actions for `riverbase_core` |
| `riverbase_http` | HTTP portal: axum routes, OIDC auth, Casbin, OpenAPI, websocket, and RxDB replication |
| `riverbase_file` | Media files: OpenDAL storage plus Postgres metadata |
| `riverbase_task` | Worker runtime and job tracker |
| `riverbase_form` | Declarative form and document templates, with validation and codegen |

`riverbase_core` is the crate most domain code imports. The HTTP, file, task, and form crates sit on that kernel and are optional for a given application.

## Concepts

A domain owns commands and queries.

- **Commands** change aggregate state. Collection commands use `:post`. Object commands use `:exec` and carry an identifier.
- **Queries** read projected state. List, item, and metadata reads use `.list`, `.item`, and `.meta`.
- **Aggregates** apply commands and emit events. Persistence goes through a Postgres `DataStore`.
- **Audit** records commands, events, messages, and activities when the corresponding `audit_log` flags are on.

Errors are structured values (`RiverbaseError`) with a user-facing message, a stable code such as `QRY-123`, and developer data. Problem responses use the type base `https://riverbase.io/~/rs/error/`.

## Configuration

Load configuration with `riverbase_core::config::RiverbaseConfig`. The loader checks, in order:

1. `RIVERBASE_CONFIG`, an explicit path to a TOML file
2. `./riverbase.toml`
3. `./config/riverbase.toml`

A file may use a `[riverbase]` table or place the same keys at the root. Environment variables with the `RIVERBASE_` prefix override the file. `DB_URL` is accepted as a fallback for the database URL.

| Variable | Default | Purpose |
| --- | --- | --- |
| `RIVERBASE_DB_URL` | — | Postgres connection string |
| `RIVERBASE_BIND_ADDR` | `0.0.0.0:8080` | Coupled HTTP listen address |
| `RIVERBASE_COMMAND_BIND_ADDR` | `0.0.0.0:8081` | Split command service |
| `RIVERBASE_QUERY_BIND_ADDR` | `0.0.0.0:8082` | Split query service |
| `RIVERBASE_LOG_LEVEL` | `info` | Tracing filter, unless `RUST_LOG` is set |
| `RIVERBASE_LOG_FORMAT` | `compact` | `compact`, `pretty`, or `json` |
| `RIVERBASE_DBPOOL_MAX_SIZE` | `32` | Postgres pool size |
| `RIVERBASE_BUS_URL` | `nats://127.0.0.1:4222` | NATS or Redis bus URL |
| `RIVERBASE_AUTH_ENABLED` | `false` | Require a JWT on HTTP routes |
| `RIVERBASE_CASBIN_ENABLED` | `false` | Turn on activity authorization |

Schema migrations for the kernel live in `crates/riverbase_core/migrations/` and run when the pool is established.

## HTTP paths

Routes hang off a configurable API base, `/api` by default.

| Kind | Method and path |
| --- | --- |
| Collection command | `POST /{namespace}/{command}:post/{resource}` |
| Object command | `POST /{namespace}/{command}:exec/{resource}/{id}` |
| Query list | `GET /{namespace}/{resource}.list` |
| Query item | `GET /{namespace}/{resource}.item/{id}` |
| Metadata | `GET /{namespace}/.meta` and `GET /{namespace}/{resource}.meta` |

A coupled process serves commands and queries on one address. Split mode runs the command service and the query service as separate binaries.

## Build

Requires Rust 1.78 or newer and PostgreSQL 14 or newer for the database tests.

```bash
cargo build --workspace
cargo test --workspace
```

Database tests read `RIVERBASE_DB_URL`. They skip when that variable is unset.

```bash
export RIVERBASE_DB_URL=postgres://USER@127.0.0.1:5432/riverbase
cargo test --workspace
```

## Layout

```text
crates/riverbase_core/     kernel, migrations, proc-macro helpers
crates/riverbase_proc/     command and domain-action macros
crates/riverbase_http/     portal, auth, Casbin, OpenAPI, websocket
crates/riverbase_file/     media storage
crates/riverbase_task/     worker and tracker
crates/riverbase_form/     forms, templates, validation
```

## License

RFX JSC Proprietary License. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT).
