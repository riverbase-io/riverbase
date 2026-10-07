use std::sync::Arc;

use async_trait::async_trait;
use casbin::prelude::*;
use casbin::{DefaultModel, Enforcer};
use riverbase_core::datastore::PgPool;
use serde_json::json;
use tokio::sync::RwLock;

use super::activity::ActivityRequest;
use super::model::default_activity_model;
use super::pg_adapter::PgCasbinAdapter;
use crate::RiverbaseResult;

/// Authorize domain activities (commands map to activity types before handler execution).
#[async_trait]
pub trait ActivityAuthorizer: Send + Sync {
    /// Enforce.
    async fn enforce(&self, request: &ActivityRequest) -> RiverbaseResult<()>;

    /// Authorize a domain activity for `subject`.
    async fn enforce_activity(
        &self,
        subject: &str,
        activity: &super::activity::DomainActivity,
    ) -> RiverbaseResult<()> {
        self.enforce(&ActivityRequest::new(subject, activity.clone()))
            .await
    }
}

/// Casbin-backed [`ActivityAuthorizer`] using the default activity model.
pub struct CasbinActivityAuthorizer {
    enforcer: Arc<RwLock<Enforcer>>,
}

impl CasbinActivityAuthorizer {
    /// Build from model and policy.
    pub async fn from_model_and_policy(
        pool: PgPool,
        model_conf: &str,
        policy_csv: &str,
    ) -> RiverbaseResult<Self> {
        let model = DefaultModel::from_str(model_conf)
            .await
            .map_err(|e| crate::errors::CAS_001.with_data(format!("casbin model: {e}")))?;
        let adapter = PgCasbinAdapter::new(pool);
        adapter
            .ensure_schema()
            .await
            .map_err(|e| crate::errors::CAS_008.with_data(format!("casbin schema: {e}")))?;
        let mut enforcer = Enforcer::new(model, adapter)
            .await
            .map_err(|e| crate::errors::CAS_002.with_data(format!("casbin enforcer: {e}")))?;
        load_policy_csv(&mut enforcer, policy_csv).await?;
        Ok(Self {
            enforcer: Arc::new(RwLock::new(enforcer)),
        })
    }

    /// Set defaults and return self.
    pub async fn with_defaults(pool: PgPool) -> RiverbaseResult<Self> {
        Self::from_model_and_policy(pool, default_activity_model(), "").await
    }

    /// Add allow policy.
    pub async fn add_allow_policy(
        &self,
        subject: &str,
        namespace: &str,
        activity_type: &str,
        resource: &str,
        object_id: &str,
    ) -> RiverbaseResult<()> {
        let mut e = self.enforcer.write().await;
        e.add_policy(vec![
            subject.into(),
            namespace.into(),
            activity_type.into(),
            resource.into(),
            object_id.into(),
            "allow".into(),
        ])
        .await
        .map_err(|err| crate::errors::CAS_003.with_data(format!("casbin add_policy: {err}")))?;
        Ok(())
    }

    /// Add role.
    pub async fn add_role(&self, user: &str, role: &str) -> RiverbaseResult<()> {
        let mut e = self.enforcer.write().await;
        e.add_grouping_policy(vec![user.into(), role.into()])
            .await
            .map_err(|err| crate::errors::CAS_004.with_data(format!("casbin add_role: {err}")))?;
        Ok(())
    }

    /// Number of `p` policy rules loaded into the enforcer ([DX-05]).
    pub async fn policy_rule_count(&self) -> usize {
        let e = self.enforcer.read().await;
        e.get_policy().len()
    }
}

#[async_trait]
impl ActivityAuthorizer for CasbinActivityAuthorizer {
    async fn enforce(&self, request: &ActivityRequest) -> RiverbaseResult<()> {
        let activity = &request.activity;
        let obj = if activity.object_id.is_empty() {
            "*".to_string()
        } else {
            activity.object_id.clone()
        };
        let res = if activity.resource.is_empty() {
            "*".to_string()
        } else {
            activity.resource.clone()
        };

        let allowed = {
            let e = self.enforcer.read().await;
            e.enforce((
                request.subject.as_str(),
                activity.namespace.as_str(),
                activity.activity_type.as_str(),
                res.as_str(),
                obj.as_str(),
            ))
            .map_err(|err| crate::errors::CAS_005.with_data(format!("casbin enforce: {err}")))?
        };

        if allowed {
            Ok(())
        } else {
            Err(riverbase_core::errors::AUT_003.with_data(json!({
                "subject": request.subject,
                "namespace": activity.namespace,
                "activity_type": activity.activity_type,
                "resource": res,
                "object_id": obj,
                "check": format!(
                    "enforce({}, {}, {}, {}, {})",
                    request.subject, activity.namespace, activity.activity_type, res, obj
                ),
            })))
        }
    }
}

async fn load_policy_csv(enforcer: &mut Enforcer, policy_csv: &str) -> RiverbaseResult<()> {
    for line in policy_csv.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            continue;
        }
        match parts[0] {
            "p" if parts.len() >= 7 => {
                enforcer
                    .add_policy(parts[1..7].iter().map(|s| (*s).to_string()).collect())
                    .await
                    .map_err(|e| crate::errors::CAS_006.with_data(format!("casbin policy: {e}")))?;
            }
            "g" if parts.len() >= 3 => {
                enforcer
                    .add_grouping_policy(vec![parts[1].into(), parts[2].into()])
                    .await
                    .map_err(|e| {
                        crate::errors::CAS_007.with_data(format!("casbin grouping: {e}"))
                    })?;
            }
            _ => {}
        }
    }
    Ok(())
}
