/// Default Casbin model for domain activity authorization.
///
/// Request: subject, namespace (domain), activity_type, resource, object_id.
/// Policy: same fields plus effect (`allow` / `deny`).
pub const DEFAULT_ACTIVITY_MODEL: &str = r#"
[request_definition]
r = sub, dom, act, res, obj

[policy_definition]
p = sub, dom, act, res, obj, eft

[role_definition]
g = _, _

[policy_effect]
e = some(where (p.eft == allow)) && !some(where (p.eft == deny))

[matchers]
m = (g(r.sub, p.sub) || r.sub == p.sub) && r.dom == p.dom && (p.act == "*" || r.act == p.act) && (p.res == "*" || r.res == p.res) && (p.obj == "*" || r.obj == p.obj)
"#;

/// Default activity model.
pub fn default_activity_model() -> &'static str {
    DEFAULT_ACTIVITY_MODEL
}
