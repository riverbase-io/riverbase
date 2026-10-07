//! Deployment zone gating for HTTP route registration.

/// Whether an API should be registered for the configured deployment zones.
pub fn zone_allowed(allowed_zones: &[String], api_zone: &[String]) -> bool {
    if api_zone.is_empty() {
        return true;
    }
    if allowed_zones.is_empty() {
        return true;
    }
    allowed_zones
        .iter()
        .any(|zone| api_zone.iter().any(|configured| configured == zone))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_allowed_truth_table() {
        assert!(zone_allowed(&[], &[]));
        assert!(zone_allowed(&[], &["seller".into()]));
        assert!(zone_allowed(&["seller".into()], &[]));
        assert!(zone_allowed(&["seller".into()], &["seller".into()]));
        assert!(zone_allowed(
            &["seller".into()],
            &["seller".into(), "staff".into()]
        ));
        assert!(!zone_allowed(&["seller".into()], &["coordinator".into()]));
        assert!(!zone_allowed(&["coordinator".into()], &["seller".into()]));
    }
}
