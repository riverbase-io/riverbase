//! Connected client state and channel ACL.
//!
//! Mirrors `RTCBridgeClient` and `authorize_transport_channel` in Python.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::auth::Principal;
use crate::base::RiverbaseResult;

use super::datadef::channel_permission_error;

/// Client Prefix constant.
pub const CLIENT_PREFIX: &str = "ws.client.";
/// User Prefix constant.
pub const USER_PREFIX: &str = "ws.user.";
/// Profile Prefix constant.
pub const PROFILE_PREFIX: &str = "ws.profile.";
/// Channel Prefix constant.
pub const CHANNEL_PREFIX: &str = "ws.channel.";
/// Request Prefix constant.
pub const REQUEST_PREFIX: &str = "ws.request.";

/// Subscribe or publish on a transport channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportAction {
    /// Publish.
    Publish,
    /// Subscribe.
    Subscribe,
}

impl TransportAction {
    /// Borrow as r.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Subscribe => "subscribe",
        }
    }
}

/// ACL entry name: exact channel or honest prefix match ([SUR-04]).
#[derive(Debug, Clone)]
pub enum AclName {
    /// Exact.
    Exact(String),
    /// Channel names that start with this prefix.
    Prefix(String),
}

/// Channel permission entry (mirrors `TransportChannelAclEntry`).
#[derive(Debug, Clone)]
pub struct ChannelAclEntry {
    /// Name.
    pub name: AclName,
    /// Publish.
    pub publish: bool,
    /// Subscribe.
    pub subscribe: bool,
    /// Auto subscribe.
    pub auto_subscribe: bool,
}

/// Connected RTC client (mirrors `RTCBridgeClient`).
pub struct RtcBridgeClient {
    /// Client id.
    pub client_id: String,
    /// User id.
    pub user_id: String,
    /// Principal.
    pub principal: Principal,
    /// Connected at.
    pub connected_at: DateTime<Utc>,
    /// Subscribed channels.
    pub subscribed_channels: Vec<String>,
    /// Auto subscribes.
    pub auto_subscribes: Vec<String>,
    /// Channel permissions.
    pub channel_permissions: Vec<ChannelAclEntry>,
}

impl RtcBridgeClient {
    /// Construct a new value.
    pub fn new(principal: Principal, channel_permissions: Vec<ChannelAclEntry>) -> Self {
        let user_id = principal.subject().to_string();
        let auto_subscribes = channel_permissions
            .iter()
            .filter(|p| p.auto_subscribe)
            .filter_map(|p| match &p.name {
                AclName::Exact(ch) => Some(ch.clone()),
                AclName::Prefix(_) => None,
            })
            .collect();

        Self {
            client_id: Uuid::new_v4().to_string(),
            user_id,
            principal,
            connected_at: Utc::now(),
            subscribed_channels: Vec::new(),
            auto_subscribes,
            channel_permissions,
        }
    }

    /// Transport channel for this client's user inbox (`ws.user.{tenant}.{sub}`).
    pub fn user_transport_channel(&self) -> String {
        format!(
            "{USER_PREFIX}{}.{}",
            tenant_segment(&self.principal),
            self.user_id
        )
    }
}

/// Tenant / realm segment for channel names (`organization_id` or `realm` claim, else `_`).
pub fn tenant_segment(principal: &Principal) -> &str {
    principal
        .claims
        .get("organization_id")
        .or_else(|| principal.claims.get("realm"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("_")
}

/// Default channel ACL for an authenticated principal.
///
/// User channels are `{USER_PREFIX}{tenant}.{sub}`. Cross-user publish is denied; the former
/// `sendusr` capability was removed (SUR-01). User-channel subscribe is scoped to the caller's
/// tenant. Named `ws.channel.*` rooms remain globally subscribable (publish still denied except
/// broadcast).
pub fn default_channel_permissions(principal: &Principal) -> Vec<ChannelAclEntry> {
    let sub = principal.subject();
    let tenant = tenant_segment(principal);
    let user_channel = format!("{USER_PREFIX}{tenant}.{sub}");
    let subject_channel = format!("{CHANNEL_PREFIX}{tenant}.{sub}");
    let tenant_user_prefix = format!("{USER_PREFIX}{tenant}.");
    vec![
        ChannelAclEntry {
            name: AclName::Exact(user_channel),
            // Own inbox only — peer user channels stay publish-denied via the tenant regex below.
            publish: true,
            subscribe: true,
            auto_subscribe: true,
        },
        ChannelAclEntry {
            name: AclName::Exact(subject_channel),
            publish: false,
            subscribe: true,
            auto_subscribe: true,
        },
        ChannelAclEntry {
            name: AclName::Exact(format!("{CHANNEL_PREFIX}broadcast")),
            publish: true,
            subscribe: true,
            auto_subscribe: false,
        },
        ChannelAclEntry {
            name: AclName::Prefix(tenant_user_prefix),
            publish: false,
            subscribe: true,
            auto_subscribe: false,
        },
        ChannelAclEntry {
            name: AclName::Prefix(format!("{CHANNEL_PREFIX}")),
            publish: false,
            subscribe: true,
            auto_subscribe: false,
        },
        ChannelAclEntry {
            name: AclName::Prefix("rxdb.notify.".into()),
            publish: false,
            subscribe: true,
            auto_subscribe: false,
        },
    ]
}

fn acl_matches(name: &AclName, channel: &str) -> bool {
    match name {
        AclName::Exact(exact) => exact == channel,
        AclName::Prefix(prefix) => !prefix.is_empty() && channel.starts_with(prefix),
    }
}

/// Check whether the client may perform `action` on `transport_channel`.
pub fn authorize_channel(
    client: &RtcBridgeClient,
    transport_channel: &str,
    action: TransportAction,
) -> RiverbaseResult<()> {
    let mut allowed = false;
    for perm in &client.channel_permissions {
        if !acl_matches(&perm.name, transport_channel) {
            continue;
        }
        allowed = match action {
            TransportAction::Subscribe => perm.subscribe,
            TransportAction::Publish => perm.publish,
        };
        if allowed {
            break;
        }
    }

    if allowed {
        Ok(())
    } else {
        Err(channel_permission_error(action.as_str(), transport_channel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Principal;
    use serde_json::json;

    fn test_principal(sub: &str) -> Principal {
        Principal {
            sub: sub.into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({}),
        }
    }

    #[test]
    fn default_permissions_include_user_channel() {
        let p = test_principal("user-1");
        let perms = default_channel_permissions(&p);
        assert!(perms
            .iter()
            .any(|e| matches!(&e.name, AclName::Exact(ch) if ch == "ws.user._.user-1")));
        assert!(perms.iter().any(|e| e.auto_subscribe));
    }

    #[test]
    fn authorize_subscribe_user_channel() {
        let p = test_principal("u42");
        let perms = default_channel_permissions(&p);
        let client = RtcBridgeClient::new(p, perms);
        assert!(authorize_channel(&client, "ws.user._.u42", TransportAction::Subscribe).is_ok());
    }

    #[test]
    fn authorize_deny_cross_user_publish() {
        let p = test_principal("u42");
        let perms = default_channel_permissions(&p);
        let client = RtcBridgeClient::new(p, perms);
        assert!(authorize_channel(&client, "ws.user._.other", TransportAction::Publish).is_err());
        assert!(authorize_channel(&client, "ws.user._.u42", TransportAction::Publish).is_ok());
    }

    #[test]
    fn authorize_deny_publish_client_channel() {
        let p = test_principal("u42");
        let perms = default_channel_permissions(&p);
        let client = RtcBridgeClient::new(p, perms);
        assert!(authorize_channel(&client, "ws.client.abc", TransportAction::Publish).is_err());
    }

    #[test]
    fn authorize_broadcast_publish() {
        let p = test_principal("u1");
        let perms = default_channel_permissions(&p);
        let client = RtcBridgeClient::new(p, perms);
        assert!(
            authorize_channel(&client, "ws.channel.broadcast", TransportAction::Publish).is_ok()
        );
    }

    #[test]
    fn authorize_prefix_ws_channel() {
        let p = test_principal("u1");
        let perms = default_channel_permissions(&p);
        let client = RtcBridgeClient::new(p, perms);
        assert!(
            authorize_channel(&client, "ws.channel.room-a", TransportAction::Subscribe).is_ok()
        );
        assert!(authorize_channel(&client, "ws.channel.room-a", TransportAction::Publish).is_err());
        assert!(acl_matches(
            &AclName::Prefix("ws.channel.".into()),
            "ws.channel.room-a"
        ));
        assert!(!acl_matches(
            &AclName::Prefix("ws.channel.".into()),
            "ws.other.room-a"
        ));
        assert!(!acl_matches(
            &AclName::Prefix(String::new()),
            "ws.channel.x"
        ));
    }

    #[test]
    fn authorize_tenant_scopes_user_subscribe() {
        let p = Principal {
            sub: "u1".into(),
            preferred_username: None,
            email: None,
            roles: vec![],
            iam_roles: vec![],
            claims: json!({ "organization_id": "org-a" }),
        };
        let perms = default_channel_permissions(&p);
        let client = RtcBridgeClient::new(p, perms);
        assert!(
            authorize_channel(&client, "ws.user.org-a.peer", TransportAction::Subscribe).is_ok()
        );
        assert!(
            authorize_channel(&client, "ws.user.org-b.peer", TransportAction::Subscribe).is_err()
        );
    }
}
