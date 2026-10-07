//! Expression evaluation for constraint rules.

use std::collections::BTreeMap;

use evalexpr::{
    eval_with_context, ContextWithMutableFunctions, ContextWithMutableVariables, HashMapContext,
    Value,
};
use regex::Regex;
use serde_json::Value as JsonValue;

use riverbase_core::base::RiverbaseResult;

/// Map dotted path to a valid evalexpr identifier.
pub fn path_var(path: &str) -> String {
    path.replace('.', "__")
}

/// Rewrite dotted paths in an expression to use `__` separators.
pub fn rewrite_expr_paths(expr: &str, paths: &[String]) -> String {
    let mut out = expr.to_string();
    let mut sorted: Vec<_> = paths.iter().collect();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for path in sorted {
        out = out.replace(path.as_str(), &path_var(path));
    }
    out
}

fn json_to_eval(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Empty,
        JsonValue::Bool(b) => Value::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Empty
            }
        }
        JsonValue::String(s) => Value::String(s.clone()),
        _ => Value::String(value.to_string()),
    }
}

fn register_builtin_functions(ctx: &mut HashMapContext) -> RiverbaseResult<()> {
    ctx.set_function(
        "trim".to_string(),
        evalexpr::Function::new(|argument| {
            let s = argument.as_string()?;
            Ok(Value::String(s.trim().to_string()))
        }),
    )
    .map_err(|e| crate::errors::FRM_082.with_data(e.to_string()))?;

    ctx.set_function(
        "len".to_string(),
        evalexpr::Function::new(|argument| {
            let s = argument.as_string()?;
            Ok(Value::Int(s.chars().count() as i64))
        }),
    )
    .map_err(|e| crate::errors::FRM_083.with_data(e.to_string()))?;

    ctx.set_function(
        "empty".to_string(),
        evalexpr::Function::new(|argument| match argument {
            Value::Empty => Ok(Value::Boolean(true)),
            Value::String(s) => Ok(Value::Boolean(s.is_empty())),
            _ => Ok(Value::Boolean(false)),
        }),
    )
    .map_err(|e| crate::errors::FRM_062.with_data(e.to_string()))?;

    ctx.set_function(
        "coalesce".to_string(),
        evalexpr::Function::new(|argument| {
            let args = argument.as_tuple()?;
            for arg in args {
                match arg.as_string() {
                    Ok(s) if !s.is_empty() => return Ok(Value::String(s)),
                    Ok(_) => continue,
                    Err(_) => {
                        if !matches!(arg, Value::Empty) {
                            return Ok(arg.clone());
                        }
                    }
                }
            }
            Ok(Value::Empty)
        }),
    )
    .map_err(|e| crate::errors::FRM_063.with_data(e.to_string()))?;

    ctx.set_function(
        "matches".to_string(),
        evalexpr::Function::new(|argument| {
            let args = argument.as_tuple()?;
            if args.len() != 2 {
                return Err(evalexpr::EvalexprError::WrongFunctionArgumentAmount {
                    expected: 2..=2,
                    actual: args.len(),
                });
            }
            let text = args[0].as_string()?;
            let pattern = args[1].as_string()?;
            let re = Regex::new(&pattern).map_err(|e| {
                evalexpr::EvalexprError::CustomMessage(format!("invalid regex: {e}"))
            })?;
            Ok(Value::Boolean(re.is_match(&text)))
        }),
    )
    .map_err(|e| crate::errors::FRM_064.with_data(e.to_string()))?;

    Ok(())
}

fn build_context(vars: &BTreeMap<String, JsonValue>) -> RiverbaseResult<HashMapContext> {
    let mut ctx = HashMapContext::new();
    register_builtin_functions(&mut ctx)?;
    for (name, value) in vars {
        ctx.set_value(name.clone(), json_to_eval(value))
            .map_err(|e| crate::errors::FRM_065.with_data(format!("{name}: {e}")))?;
    }
    Ok(ctx)
}

/// Evaluate a boolean constraint expression against a variable map.
pub fn eval_constraint_expr(expr: &str, vars: &BTreeMap<String, JsonValue>) -> RiverbaseResult<bool> {
    let ctx = build_context(vars)?;
    let result = eval_with_context(expr, &ctx)
        .map_err(|e| crate::errors::FRM_066.with_data(e.to_string()))?;
    result
        .as_boolean()
        .map_err(|e| crate::errors::FRM_067.with_data(e.to_string()))
}

/// Build field-scope variables: `value` plus sibling field values from element data.
pub fn field_scope_vars(
    field_name: &str,
    data: &JsonValue,
    all_fields: &[String],
) -> BTreeMap<String, JsonValue> {
    let mut vars = BTreeMap::new();
    if let Some(obj) = data.as_object() {
        for name in all_fields {
            let v = obj.get(name).cloned().unwrap_or(JsonValue::Null);
            vars.insert(name.clone(), v);
        }
        if let Some(v) = obj.get(field_name) {
            vars.insert("value".to_string(), v.clone());
        } else {
            vars.insert("value".to_string(), JsonValue::Null);
        }
    } else {
        vars.insert("value".to_string(), JsonValue::Null);
    }
    vars
}

/// Build element-scope variables from field data object.
pub fn element_scope_vars(data: &JsonValue, fields: &[String]) -> BTreeMap<String, JsonValue> {
    let mut vars = BTreeMap::new();
    let obj = data.as_object();
    for name in fields {
        let v = obj
            .and_then(|m| m.get(name))
            .cloned()
            .unwrap_or(JsonValue::Null);
        vars.insert(name.clone(), v);
    }
    vars
}

/// Build form-scope variables from `{ elem: { field: value } }` payload.
pub fn form_scope_vars(data: &JsonValue, paths: &[String]) -> BTreeMap<String, JsonValue> {
    let mut vars = BTreeMap::new();
    for path in paths {
        let var = path_var(path);
        let value = resolve_form_path(data, path).unwrap_or(JsonValue::Null);
        vars.insert(var, value);
    }
    vars
}

fn resolve_form_path(data: &JsonValue, path: &str) -> Option<JsonValue> {
    let mut parts = path.split('.');
    let elem = parts.next()?;
    let mut current = data.get(elem)?;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current.clone())
}

/// Collect all element.field paths from a form payload shape.
pub fn collect_form_paths(data: &JsonValue) -> Vec<String> {
    let mut paths = Vec::new();
    let Some(obj) = data.as_object() else {
        return paths;
    };
    for (elem_name, elem_data) in obj {
        if let Some(fields) = elem_data.as_object() {
            for field_name in fields.keys() {
                paths.push(format!("{elem_name}.{field_name}"));
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eval_trim_not_blank() {
        let mut vars = BTreeMap::new();
        vars.insert("value".to_string(), json!("  hi "));
        assert!(eval_constraint_expr("trim(value) != \"\"", &vars).unwrap());
    }

    #[test]
    fn empty_is_false_for_numbers() {
        let mut vars = BTreeMap::new();
        vars.insert("amount".to_string(), json!(120000));
        assert!(!eval_constraint_expr("empty(amount)", &vars).unwrap());
    }

    #[test]
    fn form_path_vars() {
        let data = json!({
            "full_name": { "value": "Jane" },
            "amount": { "value": 42 }
        });
        let paths = collect_form_paths(&data);
        let vars = form_scope_vars(&data, &paths);
        assert_eq!(vars["full_name__value"], json!("Jane"));
    }
}
