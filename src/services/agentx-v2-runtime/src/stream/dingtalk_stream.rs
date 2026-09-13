//! DingTalk Stream protocol client (open protocol, JSON frames over WebSocket).
//!
//! Handshake: POST /v1.0/gateway/connections/open with clientId/clientSecret and
//! subscriptions -> { endpoint, ticket }; connect to `{endpoint}?ticket={ticket}`.
//! Frames: `{specVersion, type: SYSTEM|EVENT|CALLBACK, headers{topic,messageId}, data}`
//! where `data` is a JSON string; every non-system frame must be ACKed.

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio_tungstenite::tungstenite::Message;

use agentx_runtime_contracts::WebhookProviderV1;

use super::{ProviderWebSocket, StreamClaim};

const BOT_MESSAGE_TOPIC: &str = "/v1.0/im/bot/messages/get";

fn gateway_base() -> String {
    std::env::var("AGENTX_STREAM_DINGTALK_GATEWAY_URL")
        .unwrap_or_else(|_| "https://api.dingtalk.com".into())
}

fn string_field(credentials: &Map<String, Value>, key: &str) -> Option<String> {
    credentials
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn frame_pointer<'a>(frame: &'a Value, pointer: &str) -> Option<&'a str> {
    frame.pointer(pointer).and_then(Value::as_str)
}

fn ack(message_id: &str, data: Value) -> String {
    json!({"code":200,"headers":{"messageId":message_id,"contentType":"application/json"},"message":"OK","data":data}).to_string()
}

pub(crate) enum FrameAction {
    Ping {
        message_id: String,
        data: Value,
    },
    Disconnect,
    BotMessage {
        message_id: String,
        payload: Option<Value>,
    },
    Ignore {
        message_id: String,
    },
}

pub(crate) fn classify_frame(frame: &Value) -> FrameAction {
    let frame_type = frame_pointer(frame, "/type");
    let topic = frame_pointer(frame, "/headers/topic");
    let message_id = frame_pointer(frame, "/headers/messageId")
        .unwrap_or_default()
        .to_owned();
    if frame_type == Some("SYSTEM") {
        return if topic == Some("ping") {
            FrameAction::Ping {
                message_id,
                data: frame.get("data").cloned().unwrap_or(Value::Null),
            }
        } else if topic == Some("disconnect") {
            FrameAction::Disconnect
        } else {
            FrameAction::Ignore { message_id }
        };
    }
    if topic == Some(BOT_MESSAGE_TOPIC) {
        let data_text = frame
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return FrameAction::BotMessage {
            message_id,
            payload: serde_json::from_str(data_text).ok(),
        };
    }
    FrameAction::Ignore { message_id }
}

pub(crate) async fn run(pool: &sqlx::MySqlPool, claim: &StreamClaim, secret: &[u8]) -> Result<()> {
    let credentials = crate::webhook::credentials(secret);
    let client_id = string_field(&credentials, "clientId")
        .context("DingTalk stream config is missing clientId")?;
    let client_secret = string_field(&credentials, "clientSecret")
        .context("DingTalk stream config is missing clientSecret")?;
    let base = gateway_base().trim_end_matches('/').to_owned();
    let open: Value = super::provider_post_json(&format!("{base}/v1.0/gateway/connections/open"), &json!({"clientId":client_id,"clientSecret":client_secret,"subscriptions":[{"type":"CALLBACK","topic":BOT_MESSAGE_TOPIC}],"ua":"agentx-stream/0.1"}), claim.tenant_id, claim.binding_id)
        .await.context("DingTalk gateway connection open failed")?;
    let endpoint = open
        .get("endpoint")
        .and_then(Value::as_str)
        .context("DingTalk gateway response missing endpoint")?
        .to_owned();
    let ticket = open
        .get("ticket")
        .and_then(Value::as_str)
        .context("DingTalk gateway response missing ticket")?
        .to_owned();
    // Tickets are single-use and valid for 90 seconds; never cache them.
    let socket = super::dial_websocket(
        &format!("{endpoint}?ticket={ticket}"),
        claim.tenant_id,
        claim.binding_id,
    )
    .await
    .context("DingTalk stream WebSocket connect failed")?;
    super::update_status(pool, claim, "connected", None)
        .await
        .ok();
    let dispatch_state = crate::webhook::DispatchState {
        pool: pool.clone(),
        tenant_id: claim.tenant_id,
        application_id: claim.application_id,
        binding_id: claim.binding_id,
        connection_id: claim.binding_id.to_string(),
        trigger_name: claim.trigger_name.clone(),
        configuration_revision: claim.configuration_revision,
        input_mappings: claim.input_mappings.clone(),
        fixed_inputs: claim.fixed_inputs.clone(),
        provider: WebhookProviderV1::Dingtalk,
    };
    session(socket, dispatch_state).await
}

pub(crate) async fn session(
    socket: ProviderWebSocket,
    state: crate::webhook::DispatchState,
) -> Result<()> {
    let mut socket = socket;
    while let Some(message) = socket.next().await {
        let message = message.context("DingTalk stream WebSocket error")?;
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => bail!("DingTalk stream WebSocket closed"),
            _ => continue,
        };
        let frame: Value = match serde_json::from_str(&text) {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "invalid DingTalk stream frame");
                continue;
            }
        };
        match classify_frame(&frame) {
            FrameAction::Ping { message_id, data } => {
                socket
                    .send(Message::Text(ack(&message_id, data).into()))
                    .await?;
            }
            FrameAction::Disconnect => bail!("DingTalk gateway requested disconnect"),
            FrameAction::BotMessage {
                message_id,
                payload,
            } => {
                if let Some(payload) = payload {
                    state.dispatch(payload).await;
                }
                socket
                    .send(Message::Text(
                        ack(&message_id, Value::String("{\"response\":null}".into())).into(),
                    ))
                    .await?;
            }
            FrameAction::Ignore { message_id } => {
                socket
                    .send(Message::Text(
                        ack(&message_id, Value::String("{\"response\":null}".into())).into(),
                    ))
                    .await?;
            }
        }
    }
    bail!("DingTalk stream WebSocket ended")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: &str, topic: &str, data: &str) -> Value {
        json!({"specVersion":"1.0","type":kind,"headers":{"contentType":"application/json","messageId":"m-1","topic":topic},"data":data})
    }

    #[test]
    fn ping_and_disconnect_frames_are_classified() {
        assert!(matches!(
            classify_frame(&frame("SYSTEM", "ping", "{\"opaque\":\"o-1\"}")),
            FrameAction::Ping { .. }
        ));
        assert!(matches!(
            classify_frame(&frame("SYSTEM", "disconnect", "")),
            FrameAction::Disconnect
        ));
    }

    #[test]
    fn bot_message_frame_carries_parsed_payload() {
        let action = classify_frame(&frame(
            "CALLBACK",
            BOT_MESSAGE_TOPIC,
            r#"{"msgId":"e-1","conversationId":"c-1","senderStaffId":"u-1","msgtype":"text","text":{"content":"hello"}}"#,
        ));
        let FrameAction::BotMessage { payload, .. } = action else {
            panic!("expected bot message")
        };
        let payload = payload.expect("payload parses");
        assert_eq!(
            payload.pointer("/text/content").and_then(Value::as_str),
            Some("hello")
        );
        assert_eq!(payload.get("sessionWebhook"), None);
    }

    /// Real-platform handshake probe. Runs only when the caller provides
    /// credentials: AGENTX_TEST_DINGTALK_CLIENT_ID / _CLIENT_SECRET.
    #[tokio::test]
    #[ignore = "requires AGENTX_TEST_DINGTALK_* env credentials"]
    async fn dingtalk_real_gateway_handshake() {
        let client_id = std::env::var("AGENTX_TEST_DINGTALK_CLIENT_ID").unwrap();
        let client_secret = std::env::var("AGENTX_TEST_DINGTALK_CLIENT_SECRET").unwrap();
        let tenant_id = uuid::Uuid::now_v7();
        let request_id = uuid::Uuid::now_v7();
        let base = gateway_base().trim_end_matches('/').to_owned();
        let open: Value = super::super::provider_post_json(
            &format!("{base}/v1.0/gateway/connections/open"),
            &json!({"clientId":client_id,"clientSecret":client_secret,"subscriptions":[{"type":"CALLBACK","topic":BOT_MESSAGE_TOPIC}],"ua":"agentx-stream/0.1"}),
            tenant_id, request_id,
        ).await.expect("bootstrap failed");
        println!("bootstrap: {}", open);
        let endpoint = open
            .get("endpoint")
            .and_then(Value::as_str)
            .expect("endpoint")
            .to_owned();
        let ticket = open
            .get("ticket")
            .and_then(Value::as_str)
            .expect("ticket")
            .to_owned();
        let socket = super::super::dial_websocket(
            &format!("{endpoint}?ticket={ticket}"),
            tenant_id,
            request_id,
        )
        .await;
        match socket {
            Ok(mut socket) => {
                use futures_util::StreamExt;
                println!("connected to {endpoint}");
                let frame =
                    tokio::time::timeout(std::time::Duration::from_secs(15), socket.next()).await;
                println!("first frame: {frame:?}");
            }
            Err(error) => panic!("dial failed: {error:#}"),
        }
    }

    #[tokio::test]
    async fn dingtalk_session_replies_ping_acks_messages_and_ends_on_close() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut server = tokio_tungstenite::accept_async(stream).await.unwrap();
            server.send(Message::Text(r#"{"specVersion":"1.0","type":"SYSTEM","headers":{"messageId":"m-ping","topic":"ping"},"data":"{\"opaque\":\"o-9\"}"}"#.into())).await.unwrap();
            let reply = server.next().await.unwrap().unwrap();
            let Message::Text(reply) = reply else {
                panic!("expected text reply")
            };
            let reply: Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(reply["code"], 200);
            assert_eq!(reply["headers"]["messageId"], "m-ping");
            assert_eq!(reply["data"], "{\"opaque\":\"o-9\"}");
            server.send(Message::Text(r#"{"specVersion":"1.0","type":"CALLBACK","headers":{"messageId":"m-2","topic":"/v1.0/im/bot/messages/get"},"data":"{\"msgId\":\"e-9\",\"conversationId\":\"c-9\",\"senderStaffId\":\"u-9\",\"msgtype\":\"text\",\"text\":{\"content\":\"hi\"}}"}"#.into())).await.unwrap();
            let reply = server.next().await.unwrap().unwrap();
            assert!(reply.to_string().contains("\"code\":200"));
            server.close(None).await.unwrap();
        });
        let (client, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
            .await
            .unwrap();
        let state = crate::webhook::DispatchState::for_test(WebhookProviderV1::Dingtalk);
        let result = session(client, state).await;
        assert!(result.is_err());
        server.await.unwrap();
    }
}
