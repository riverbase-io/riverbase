//! RTC bridge: WebSocket endpoint, connection lifecycle, and diagnostic routes.
//!
//! Mirrors Python `RTCBridge` in `rtc_bridge.py`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use crate::openapi_meta::OpenApiMeta;
use crate::web::openapi::{apply_riverbase_operation, RiverbaseOperationKind, RiverbaseOperationMeta};
use aide::axum::{routing::get_with, ApiRouter};
use aide::transform::TransformOperation;
use axum::{
    body::{Body, Bytes},
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Extension,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Redirect, Response},
    Json,
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::auth::Principal;
use crate::base::RiverbaseResult;
use crate::http_response::http_json_response;
use crate::transport::StreamBus;

use super::client::{
    authorize_channel, default_channel_permissions, RtcBridgeClient, TransportAction,
    CHANNEL_PREFIX,
};
use super::codec::WireCodec;
use super::datadef::{to_client_message, ClientMessage};
use super::handler::{
    handle_client_message, register_builtin_handlers, BridgeProxy, MessageRegistry,
};
use super::transport::{RtcTransport, StreamRtcTransport};

/// Metadata about a connected client (for `GET …/info`).
#[derive(Debug, Clone)]
pub struct ClientInfo {
    /// Client id.
    pub client_id: String,
    /// User id.
    pub user_id: String,
    /// Connected at.
    pub connected_at: DateTime<Utc>,
    /// Subscriptions.
    pub subscriptions: Vec<String>,
}

struct SseClientGuard {
    clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
    client_id: String,
}

impl Drop for SseClientGuard {
    fn drop(&mut self) {
        let clients = self.clients.clone();
        let client_id = self.client_id.clone();
        tokio::spawn(async move {
            clients.lock().await.remove(&client_id);
        });
    }
}

/// RTC bridge builder — mirrors `RTCBridge` / `configure_rtc_bridge`.
pub struct RtcBridge {
    name: String,
    prefix: String,
    transport: Arc<dyn RtcTransport>,
    registry: MessageRegistry,
    clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
    websocket_uri: String,
    default_wire_codec: WireCodec,
}

impl RtcBridge {
    /// Construct a new value.
    pub fn new(
        name: impl Into<String>,
        prefix: impl Into<String>,
        bus: Arc<dyn StreamBus>,
    ) -> Self {
        let name = name.into();
        let mut prefix = prefix.into();
        if !prefix.starts_with('/') {
            prefix.insert(0, '/');
        }
        while prefix.len() > 1 && prefix.ends_with('/') {
            prefix.pop();
        }
        let websocket_uri = format!("{prefix}/{name}~websocket");
        Self {
            name,
            prefix,
            transport: Arc::new(StreamRtcTransport::new(bus)),
            registry: MessageRegistry::new(),
            clients: Arc::new(Mutex::new(HashMap::new())),
            websocket_uri,
            default_wire_codec: WireCodec::default(),
        }
    }

    /// Default wire codec when the client omits `Sec-WebSocket-Protocol`
    /// (mirrors Python `RTC_WIRE_ENCODING`).
    pub fn with_default_wire_codec(mut self, codec: WireCodec) -> Self {
        self.default_wire_codec = codec;
        self
    }

    /// Set transport and return self.
    pub fn with_transport(mut self, transport: Arc<dyn RtcTransport>) -> Self {
        self.transport = transport;
        self
    }

    /// Register builtins.
    pub fn register_builtins(mut self) -> RiverbaseResult<Self> {
        register_builtin_handlers(&mut self.registry)?;
        Ok(self)
    }

    /// Message registry mut.
    pub fn message_registry_mut(&mut self) -> &mut MessageRegistry {
        &mut self.registry
    }

    /// Websocket uri.
    pub fn websocket_uri(&self) -> &str {
        &self.websocket_uri
    }

    /// Build routes: WebSocket upgrade, `.meta`, `.info`.
    pub fn into_router(self) -> ApiRouter {
        let bridge = Arc::new(self);
        let ws_bridge = bridge.clone();
        let sse_bridge = bridge.clone();
        let meta_bridge = bridge.clone();
        let info_bridge = bridge.clone();
        let root_bridge = bridge.clone();

        let prefix = bridge.prefix.clone();
        let name = bridge.name.clone();
        let ws_path = format!("{prefix}/{name}~websocket/{{*channel}}");
        let sse_path = format!("{prefix}/{name}~ssestream/{{*channel}}");
        let meta_path = format!("{prefix}/{name}.meta");
        let info_path = format!("{prefix}/{name}.info");
        let root_path = format!("{prefix}/{name}");

        let mut router = ApiRouter::new();

        router = router.route(
            ws_path.as_str(),
            axum::routing::get({
                move |ws: WebSocketUpgrade,
                      principal: Option<Extension<Principal>>,
                      headers: HeaderMap,
                      axum::extract::Path(channel): axum::extract::Path<String>| {
                    let bridge = ws_bridge.clone();
                    let default_codec = bridge.default_wire_codec;
                    async move {
                        match principal {
                            Some(Extension(p)) => match select_wire_codec(
                                parse_sec_websocket_protocol(&headers).as_deref(),
                                default_codec,
                            ) {
                                Ok(codec) => {
                                    ws.protocols([codec.encoding()]).on_upgrade(move |socket| {
                                        bridge.handle_socket(socket, p, codec, channel)
                                    })
                                }
                                Err(reason) => {
                                    http_json_response(crate::errors::RTC_010.with_data(reason))
                                }
                            },
                            None => http_json_response(crate::errors::AUT_001.with_data(json!({}))),
                        }
                    }
                }
            }),
        );

        let openapi_ns = rtc_openapi_namespace(&prefix);
        router = router.api_route(
            sse_path.as_str(),
            get_with(
                move |principal: Option<Extension<Principal>>,
                      axum::extract::Path(channel): axum::extract::Path<String>| {
                    let bridge = sse_bridge.clone();
                    async move {
                        match principal {
                            Some(Extension(principal)) => {
                                bridge.sse_response(principal, channel).await
                            }
                            None => http_json_response(crate::errors::AUT_001.with_data(json!({}))),
                        }
                    }
                },
                rtc_sse_operation(&openapi_ns, &name),
            ),
        );

        router = router.api_route(
            meta_path.as_str(),
            get_with(
                move || {
                    let uri = meta_bridge.websocket_uri.clone();
                    async move { Json(json!({ "websocket": uri })) }
                },
                rtc_meta_operation(&openapi_ns, &name, &bridge.websocket_uri),
            ),
        );

        router = router.api_route(
            info_path.as_str(),
            get_with(
                move || {
                    let bridge = info_bridge.clone();
                    async move { Json(bridge.bridge_info().await) }
                },
                rtc_info_operation(&openapi_ns, &name),
            ),
        );

        router = router.route(
            root_path.as_str(),
            axum::routing::get({
                let target = format!("{name}.meta");
                move || async move { Redirect::temporary(&target) }
            }),
        );

        let _ = root_bridge;
        router
    }

    async fn sse_response(&self, principal: Principal, channel: String) -> Response {
        let topic = channel.trim_matches('/').to_string();
        let transport_channel = format!("{CHANNEL_PREFIX}{topic}");
        let permissions = default_channel_permissions(&principal);
        let client = RtcBridgeClient::new(principal, permissions);
        if let Err(err) = authorize_channel(&client, &transport_channel, TransportAction::Subscribe)
        {
            return http_json_response(err);
        }

        let subscription = match self.transport.subscribe(&transport_channel).await {
            Ok(subscription) => subscription,
            Err(err) => return http_json_response(err),
        };
        let client_id = client.client_id.clone();
        self.clients.lock().await.insert(
            client_id.clone(),
            ClientInfo {
                client_id: client_id.clone(),
                user_id: client.user_id.clone(),
                connected_at: client.connected_at,
                subscriptions: vec![transport_channel.clone()],
            },
        );

        let guard = SseClientGuard {
            clients: self.clients.clone(),
            client_id: client_id.clone(),
        };
        let stream = async_stream::stream! {
            let _guard = guard;
            yield Ok::<Bytes, Infallible>(Bytes::from_static(b": keepalive\n\n"));
            let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(30));
            keepalive.tick().await;
            loop {
                tokio::select! {
                    message = subscription.recv() => {
                        let Some(message) = message else {
                            break;
                        };
                        let client_message = to_client_message(message);
                        if let Ok(data) = serde_json::to_string(&client_message) {
                            yield Ok(Bytes::from(format!("data: {data}\n\n")));
                        }
                    }
                    _ = keepalive.tick() => {
                        yield Ok(Bytes::from_static(b": keepalive\n\n"));
                    }
                }
            }
        };

        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        response
            .headers_mut()
            .insert("x-accel-buffering", HeaderValue::from_static("no"));
        if let Ok(value) = HeaderValue::from_str(&client_id) {
            response.headers_mut().insert("x-rtc-client", value);
        }
        response
    }

    async fn bridge_info(&self) -> Value {
        let clients = self.clients.lock().await;
        let mut channel_map: HashMap<String, Vec<String>> = HashMap::new();
        for c in clients.values() {
            for ch in &c.subscriptions {
                channel_map
                    .entry(ch.clone())
                    .or_default()
                    .push(c.client_id.clone());
            }
        }

        let connections: Vec<Value> = clients
            .values()
            .map(|c| {
                json!({
                    "client_id": c.client_id,
                    "user_id": c.user_id,
                    "connected_at": c.connected_at.to_rfc3339(),
                    "subscriptions": c.subscriptions,
                })
            })
            .collect();

        let channels: Vec<Value> = channel_map
            .into_iter()
            .map(|(channel, subscribers)| json!({ "channel": channel, "subscribers": subscribers }))
            .collect();

        let messages: Value = self
            .registry
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    json!({
                        "name": v.name,
                        "description": v.description,
                        "handler": "handler",
                    }),
                )
            })
            .collect();

        json!({
            "connections": connections,
            "channels": channels,
            "clients": clients.len(),
            "transport": "RtcTransport",
            "messages": messages,
        })
    }

    async fn handle_socket(
        self: Arc<Self>,
        socket: WebSocket,
        principal: Principal,
        selected_codec: WireCodec,
        channel: String,
    ) {
        let protocol = socket.protocol().and_then(|v| v.to_str().ok());
        let codec = match negotiate_codec(protocol, selected_codec) {
            Ok(c) => c,
            Err(reason) => {
                let (mut sink, _) = socket.split();
                let _ = sink
                    .send(Message::Close(Some(CloseFrame {
                        code: 4401,
                        reason: reason.into(),
                    })))
                    .await;
                return;
            }
        };
        let (mut sink, mut stream) = socket.split();

        let perms = default_channel_permissions(&principal);
        let mut client = RtcBridgeClient::new(principal, perms);
        let client_id = client.client_id.clone();

        self.clients.lock().await.insert(
            client_id.clone(),
            ClientInfo {
                client_id: client_id.clone(),
                user_id: client.user_id.clone(),
                connected_at: client.connected_at,
                subscriptions: vec![],
            },
        );

        let topic = {
            let trimmed = channel.trim_matches('/');
            if trimmed.is_empty() {
                client_id.clone()
            } else {
                trimmed.to_string()
            }
        };
        let confirm =
            ClientMessage::new("ws.connected", &topic, json!({ "user_id": client.user_id }))
                .with_txid(&client_id);

        if let Ok(frame) = codec.encode(&confirm) {
            let _ = sink.send(frame).await;
        }

        let (fwd_tx, mut fwd_rx) = mpsc::unbounded_channel::<ClientMessage>();

        // Match Python: subscribe to the path channel (`ws.channel.{path}`) on connect.
        if !topic.is_empty() && topic != client_id {
            let path_channel = format!("{CHANNEL_PREFIX}{topic}");
            if let Err(e) = self
                .subscribe_client(&mut client, &path_channel, fwd_tx.clone())
                .await
            {
                tracing::warn!(
                    client_id = %client_id,
                    channel = %path_channel,
                    error = %e,
                    "path-channel subscribe failed"
                );
            }
        }

        for channel in client.auto_subscribes.clone() {
            if let Err(e) = self
                .subscribe_client(&mut client, &channel, fwd_tx.clone())
                .await
            {
                tracing::warn!(client_id = %client_id, channel = %channel, error = %e, "auto-subscribe failed");
            }
        }
        self.update_client_subscriptions(&client).await;

        let proxy = BridgeProxy::new(self.transport.clone());
        let registry = &self.registry;

        loop {
            tokio::select! {
                biased;
                Some(fwd_msg) = fwd_rx.recv() => {
                    if let Ok(frame) = codec.encode(&fwd_msg) {
                        if sink.send(frame).await.is_err() {
                            break;
                        }
                    }
                }
                maybe_frame = stream.next() => {
                    let Some(Ok(frame)) = maybe_frame else { break };
                    match frame {
                        Message::Close(_) => break,
                        Message::Ping(p) => {
                            let _ = sink.send(Message::Pong(p)).await;
                        }
                        other => {
                            match codec.decode(other) {
                                Ok(message) => {
                                    let replies = handle_client_message(
                                        registry,
                                        &proxy,
                                        &client,
                                        message,
                                    )
                                    .await;
                                    for reply in replies {
                                        if let Ok(out) = codec.encode(&reply) {
                                            if sink.send(out).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(client_id = %client_id, error = %e, "bad websocket message");
                                    let err = ClientMessage::new(
                                        "ws.error",
                                        "",
                                        json!({
                                            "errmsg": format!("Unable to handle message. Details: {e}"),
                                            "errcode": "WS100",
                                        }),
                                    );
                                    if let Ok(out) = codec.encode(&err) {
                                        let _ = sink.send(out).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        self.cleanup_client(&mut client).await;
        self.clients.lock().await.remove(&client_id);
    }

    async fn subscribe_client(
        &self,
        client: &mut RtcBridgeClient,
        channel: &str,
        fwd_tx: mpsc::UnboundedSender<ClientMessage>,
    ) -> RiverbaseResult<()> {
        if client.subscribed_channels.iter().any(|c| c == channel) {
            tracing::warn!(
                client_id = %client.client_id,
                channel = %channel,
                "already subscribed"
            );
            return Ok(());
        }

        authorize_channel(client, channel, TransportAction::Subscribe)?;
        let sub = self.transport.subscribe(channel).await?;

        tokio::spawn(async move {
            while let Some(tm) = sub.recv().await {
                let cm = to_client_message(tm);
                if fwd_tx.send(cm).is_err() {
                    break;
                }
            }
        });

        client.subscribed_channels.push(channel.to_string());
        Ok(())
    }

    async fn update_client_subscriptions(&self, client: &RtcBridgeClient) {
        let subs = client.subscribed_channels.clone();
        if let Some(info) = self.clients.lock().await.get_mut(&client.client_id) {
            info.subscriptions = subs;
        }
    }

    async fn cleanup_client(&self, client: &mut RtcBridgeClient) {
        client.subscribed_channels.clear();
    }
}

fn parse_sec_websocket_protocol(headers: &HeaderMap) -> Option<Vec<String>> {
    headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map(|header| {
            header
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
                .collect()
        })
}

/// Pick the first client-offered subprotocol the server supports.
pub fn select_wire_codec(
    client_protocols: Option<&[String]>,
    default: WireCodec,
) -> Result<WireCodec, String> {
    if let Some(protocols) = client_protocols {
        for token in protocols {
            if let Some(codec) = WireCodec::from_encoding(token) {
                return Ok(codec);
            }
        }
        return Err(format!(
            "Unsupported WebSocket protocol: {}",
            protocols.join(", ")
        ));
    }
    Ok(default)
}

fn negotiate_codec(protocol: Option<&str>, default: WireCodec) -> Result<WireCodec, String> {
    match protocol {
        None => Ok(default),
        Some(name) => WireCodec::from_encoding(name)
            .ok_or_else(|| format!("Unsupported WebSocket protocol: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_wire_codec_prefers_first_client_token() {
        let offered = vec!["cbor".into(), "json".into()];
        assert_eq!(
            select_wire_codec(Some(&offered), WireCodec::Json).unwrap(),
            WireCodec::Cbor
        );
        let offered = vec!["json".into(), "cbor".into()];
        assert_eq!(
            select_wire_codec(Some(&offered), WireCodec::Json).unwrap(),
            WireCodec::Json
        );
    }

    #[test]
    fn select_wire_codec_uses_default_without_header() {
        assert_eq!(
            select_wire_codec(None, WireCodec::Json).unwrap(),
            WireCodec::Json
        );
    }

    #[test]
    fn select_wire_codec_rejects_unknown_tokens() {
        let offered = vec!["msgpack".into()];
        assert!(select_wire_codec(Some(&offered), WireCodec::Json).is_err());
    }

    #[test]
    fn negotiate_codec_falls_back_to_connection_default() {
        assert_eq!(
            negotiate_codec(None, WireCodec::Cbor).unwrap(),
            WireCodec::Cbor
        );
    }
}

fn rtc_openapi_namespace(prefix: &str) -> String {
    let trimmed = prefix.trim_start_matches('/');
    trimmed.strip_prefix("api/").unwrap_or(trimmed).to_string()
}

fn rtc_sse_operation(
    namespace: &str,
    name: &str,
) -> impl FnOnce(TransformOperation) -> TransformOperation {
    let op_meta = RiverbaseOperationMeta::realtime(
        namespace,
        name,
        RiverbaseOperationKind::RealtimeSseStream,
        OpenApiMeta::default(),
    );
    move |op| {
        apply_riverbase_operation(op, &op_meta, "get")
            .summary("rtc SSE stream")
            .description("Subscribe to an RTC channel as a server-sent event stream.")
    }
}

fn rtc_meta_operation(
    namespace: &str,
    name: &str,
    websocket_uri: &str,
) -> impl FnOnce(TransformOperation) -> TransformOperation {
    let op_meta = RiverbaseOperationMeta::query(
        namespace,
        name,
        RiverbaseOperationKind::QueryMeta,
        false,
        OpenApiMeta::default().with_explorer(false),
    );
    let socket = websocket_uri.to_string();
    move |op| {
        apply_riverbase_operation(op, &op_meta, "get")
            .summary("rtc bridge meta")
            .description("WebSocket URI for this RTC bridge.")
            .with(|mut t| {
                t.inner_mut()
                    .extensions
                    .insert("x-socket".into(), json!(socket));
                t
            })
    }
}

fn rtc_info_operation(
    namespace: &str,
    name: &str,
) -> impl FnOnce(TransformOperation) -> TransformOperation {
    let op_meta = RiverbaseOperationMeta::realtime(
        namespace,
        name,
        RiverbaseOperationKind::GenericGet,
        OpenApiMeta::default().with_explorer(false),
    );
    move |op| {
        apply_riverbase_operation(op, &op_meta, "get")
            .summary("rtc bridge info")
            .description("Connected clients, channels, and registered message handlers.")
    }
}
