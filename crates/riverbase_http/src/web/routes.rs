//! Riverbase-compatible HTTP path literals (method suffix in the same segment as the API name).

use super::api_path::join_api_path;

/// `POST {api_base}/{namespace}/{cmdkey}:exec/{resource}/{identifier}` (unscoped)
pub fn command_exec_path(api_base: &str, namespace: &str, cmdkey: &str, scoped: bool) -> String {
    if scoped {
        join_api_path(
            api_base,
            &format!("/{namespace}/{cmdkey}:exec/{{scope}}/{{resource}}/{{identifier}}"),
        )
    } else {
        join_api_path(
            api_base,
            &format!("/{namespace}/{cmdkey}:exec/{{resource}}/{{identifier}}"),
        )
    }
}

/// `POST {api_base}/{namespace}/{cmdkey}:post/{resource}` (unscoped)
pub fn command_post_path(api_base: &str, namespace: &str, cmdkey: &str, scoped: bool) -> String {
    if scoped {
        join_api_path(
            api_base,
            &format!("/{namespace}/{cmdkey}:post/{{scope}}/{{resource}}"),
        )
    } else {
        join_api_path(
            api_base,
            &format!("/{namespace}/{cmdkey}:post/{{resource}}"),
        )
    }
}

/// `GET {api_base}/{namespace}/{cmdkey}:link/{link_token}` — signed command link.
pub fn command_link_path(api_base: &str, namespace: &str, cmdkey: &str) -> String {
    join_api_path(
        api_base,
        &format!("/{namespace}/{cmdkey}:link/{{link_token}}"),
    )
}

/// `GET {api_base}/{namespace}/{cmdkey}:hook/{hook_token}` — JWT-signed hook trigger.
pub fn command_hook_path(api_base: &str, namespace: &str, cmdkey: &str) -> String {
    join_api_path(
        api_base,
        &format!("/{namespace}/{cmdkey}:hook/{{hook_token}}"),
    )
}

/// `POST {api_base}/{namespace}/{resource}.rept` (optional `/{scope}`)
pub fn query_rept_path(api_base: &str, namespace: &str, resource: &str, scoped: bool) -> String {
    if scoped {
        join_api_path(api_base, &format!("/{namespace}/{resource}.rept/{{scope}}"))
    } else {
        join_api_path(api_base, &format!("/{namespace}/{resource}.rept"))
    }
}

/// `GET {api_base}/{namespace}/domain.meta` — command discovery for a namespace.
pub fn command_meta_path(api_base: &str, namespace: &str) -> String {
    join_api_path(api_base, &format!("/{namespace}/domain.meta"))
}

/// `GET {api_base}/{namespace}/{cmdkey}.meta` — per-command metadata (`command:meta`).
pub fn command_key_meta_path(api_base: &str, namespace: &str, cmdkey: &str) -> String {
    join_api_path(api_base, &format!("/{namespace}/{cmdkey}.meta"))
}

/// `GET {api_base}/{namespace}/{resource}.list` (optional `/{scope}`)
pub fn query_list_path(api_base: &str, namespace: &str, resource: &str, scoped: bool) -> String {
    if scoped {
        join_api_path(api_base, &format!("/{namespace}/{resource}.list/{{scope}}"))
    } else {
        join_api_path(api_base, &format!("/{namespace}/{resource}.list"))
    }
}

/// `GET {api_base}/{namespace}/{resource}.item/{identifier}` or `.item/{scope}/{identifier}`
pub fn query_item_path(api_base: &str, namespace: &str, resource: &str, scoped: bool) -> String {
    if scoped {
        join_api_path(
            api_base,
            &format!("/{namespace}/{resource}.item/{{scope}}/{{identifier}}"),
        )
    } else {
        join_api_path(
            api_base,
            &format!("/{namespace}/{resource}.item/{{identifier}}"),
        )
    }
}

/// `GET {api_base}/{namespace}/{resource}.meta`
pub fn query_meta_path(api_base: &str, namespace: &str, resource: &str) -> String {
    join_api_path(api_base, &format!("/{namespace}/{resource}.meta"))
}

#[cfg(test)]
mod tests {
    use super::super::api_path::DEFAULT_API_BASE;
    use super::*;

    #[test]
    fn riverbase_context_paths() {
        let base = DEFAULT_API_BASE;
        assert_eq!(
            command_exec_path(base, "riverbase.todo", "update-todo", false),
            "/api/riverbase.todo/update-todo:exec/{resource}/{identifier}"
        );
        assert_eq!(
            command_post_path(base, "riverbase.todo", "create-todo", false),
            "/api/riverbase.todo/create-todo:post/{resource}"
        );
        assert_eq!(
            command_post_path(base, "riverbase.todo", "clear-done", false),
            "/api/riverbase.todo/clear-done:post/{resource}"
        );
        assert_eq!(
            query_item_path(base, "riverbase.todo", "todo", false),
            "/api/riverbase.todo/todo.item/{identifier}"
        );
        assert_eq!(
            query_item_path(base, "riverbase.todo", "todo", true),
            "/api/riverbase.todo/todo.item/{scope}/{identifier}"
        );
        assert_eq!(
            command_link_path(base, "riverbase.todo", "confirm-order"),
            "/api/riverbase.todo/confirm-order:link/{link_token}"
        );
        assert_eq!(
            command_hook_path(base, "exp.payment", "payment-webhook"),
            "/api/exp.payment/payment-webhook:hook/{hook_token}"
        );
        assert_eq!(
            query_rept_path(base, "riverbase.todo", "todo_summary", false),
            "/api/riverbase.todo/todo_summary.rept"
        );
        assert_eq!(
            command_key_meta_path(base, "exp.catalog", "create-product"),
            "/api/exp.catalog/create-product.meta"
        );
        assert_eq!(
            command_meta_path(base, "riverbase.todo"),
            "/api/riverbase.todo/domain.meta"
        );
    }
}
