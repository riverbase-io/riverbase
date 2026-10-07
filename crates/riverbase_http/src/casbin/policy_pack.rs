//! Casbin pack drift detection and command-row coverage ([SEC-08], [SEC-13]).

use std::path::Path;

use crate::base::RiverbaseResult;
use crate::command::CommandAuthz;
use crate::command::CommandMeta;

/// Normalize policy CSV for equality (comments and blank lines dropped, whitespace trimmed).
pub fn normalize_policy_csv(csv: &str) -> String {
    let mut lines: Vec<String> = csv
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split(',').map(str::trim).collect::<Vec<_>>().join(","))
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Fail when the embedded pack and on-disk `configs/policies/{pack}.csv` diverge.
pub fn detect_policy_drift(pack: &str, embedded: &str) -> RiverbaseResult<()> {
    if embedded.trim().is_empty() {
        return Ok(());
    }
    let path = Path::new("configs/policies").join(format!("{pack}.csv"));
    let Ok(on_disk) = std::fs::read_to_string(&path) else {
        tracing::debug!(path = %path.display(), "no on-disk policy pack to compare");
        return Ok(());
    };
    if normalize_policy_csv(embedded) != normalize_policy_csv(&on_disk) {
        return Err(
            crate::errors::CAS_012.with_data(format!("pack={pack} path={}", path.display()))
        );
    }
    Ok(())
}

/// True when the pack has a `*` activity or an exact `{cmdkey}.execute` row for `identity`.
pub fn policy_covers_command(csv: &str, identity: &str, cmdkey: &str) -> bool {
    let activity = format!("{cmdkey}.execute");
    for line in csv.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.first().copied() != Some("p") || parts.len() < 4 {
            continue;
        }
        let ns = parts[2];
        let act = parts[3];
        if ns == identity && (act == "*" || act == activity) {
            return true;
        }
    }
    false
}

/// Commands that are `Required` and have no covering Casbin row.
pub fn missing_command_policy_rows(
    csv: &str,
    catalog: &[(String, CommandMeta)],
    identity_of: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut missing = Vec::new();
    for (wire, meta) in catalog {
        if matches!(meta.authz, CommandAuthz::Public { .. }) {
            continue;
        }
        let Some(identity) = identity_of(wire) else {
            missing.push(format!("{wire}.{} (unregistered identity)", meta.key));
            continue;
        };
        if !policy_covers_command(csv, &identity, &meta.key) {
            missing.push(format!("{identity}.{}.execute", meta.key));
        }
    }
    missing
}

/// Assert commands have policy rows.
pub fn assert_commands_have_policy_rows(
    csv: &str,
    catalog: &[(String, CommandMeta)],
    identity_of: impl Fn(&str) -> Option<String>,
) -> RiverbaseResult<()> {
    let missing = missing_command_policy_rows(csv, catalog, identity_of);
    for row in &missing {
        tracing::warn!(command = %row, "command has no Casbin policy row");
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(crate::errors::CAS_013.with_data(missing.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandMeta;

    #[test]
    fn drift_detects_divergent_packs() {
        let err = detect_policy_drift_from_strings(
            "p, admin, ns, *, *, *, allow\n",
            "p, admin, ns, other.execute, *, *, allow\n",
        )
        .expect_err("drift");
        assert_eq!(err.errcode.as_str(), "CAS-012");
    }

    fn detect_policy_drift_from_strings(embedded: &str, on_disk: &str) -> RiverbaseResult<()> {
        if normalize_policy_csv(embedded) != normalize_policy_csv(on_disk) {
            return Err(crate::errors::CAS_012.with_data("test"));
        }
        Ok(())
    }

    #[test]
    fn wildcard_activity_covers_commands() {
        let csv = "p, admin, exp.order, *, *, *, allow\n";
        assert!(policy_covers_command(csv, "exp.order", "confirm"));
        let meta = CommandMeta::object("confirm", "Confirm");
        let missing = missing_command_policy_rows(csv, &[("exp.order".into(), meta)], |wire| {
            Some(wire.to_string())
        });
        assert!(missing.is_empty());
    }

    #[test]
    fn required_command_without_row_is_named() {
        let csv = "p, admin, exp.order, other.execute, *, *, allow\n";
        let meta = CommandMeta::object("confirm", "Confirm");
        let missing = missing_command_policy_rows(csv, &[("exp.order".into(), meta)], |wire| {
            Some(wire.to_string())
        });
        assert_eq!(missing, vec!["exp.order.confirm.execute".to_string()]);
    }
}
