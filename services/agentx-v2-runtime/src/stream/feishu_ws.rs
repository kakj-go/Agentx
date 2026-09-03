//! Feishu long-connection client (protocol as implemented by the official Go
//! SDK `ws` package, "pbbp2" protobuf frames).
//!
//! Handshake: POST {domain}/callback/ws/endpoint with {"AppID","AppSecret"} ->
//! {"code":0,"data":{"URL": wss endpoint carrying device_id/service_id query,
//! "ClientConfig":{"PingInterval": seconds}}}; dial the URL directly, then
//! exchange protobuf binary frames: the client pings (control frame) every
//! PingInterval and must ACK data frames with a Response payload.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio_tungstenite::tungstenite::Message;

use prost::Message as ProstMessage;

use super::{ProviderWebSocket, StreamClaim};

#[derive(Clone, PartialEq, prost::Message)]
pub struct Frame {
    #[prost(uint64, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, tag = "2")]
    pub log_id: u64,
    #[prost(int32, tag = "3")]
    pub service: i32,
    #[prost(int32, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    #[prost(string, tag = "6")]
    pub payload_encoding: String,
    #[prost(string, tag = "7")]
    pub payload_type: String,
    #[prost(bytes = "vec", tag = "8")]
    pub payload: Vec<u8>,
    #[prost(string, tag = "9")]
    pub log_id_new: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Header {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(string, tag = "2")]
    pub value: String,
}

const FRAME_METHOD_CONTROL: i32 = 0;
const FRAME_METHOD_DATA: i32 = 1;

fn domain_base() -> String {
    std::env::var("AGENTX_STREAM_FEISHU_DOMAIN").unwrap_or_else(|_| "https://open.feishu.cn".into())
}

fn string_field(credentials: &Map<String, Value>, key: &str) -> Option<String> {
    credentials
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn header_value<'a>(frame: &'a Frame, key: &str) -> Option<&'a str> {
    frame
        .headers
        .iter()
        .find(|header| header.key == key)
        .map(|header| header.value.as_str())
}

fn header_int(frame: &Frame, key: &str) -> Option<i64> {
    header_value(frame, key).and_then(|value| value.parse().ok())
}

struct Endpoint {
    url: String,
    ping_interval: Duration,
}

async fn fetch_endpoint(
    app_id: &str,
    app_secret: &str,
    tenant_id: uuid::Uuid,
    request_id: uuid::Uuid,
) -> Result<Endpoint> {
    let base = domain_base().trim_end_matches('/').to_owned();
    let response = super::provider_post_json(
        &format!("{base}/callback/ws/endpoint"),
        &json!({"AppID": app_id, "AppSecret": app_secret}),
        tenant_id,
        request_id,
    )
    .await?;
    let code = response.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        bail!(
            "Feishu endpoint bootstrap failed: {} {}",
            code,
            response
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or_default()
        );
    }
    let data = response
        .get("data")
        .context("Feishu endpoint bootstrap response missing data")?;
    let url = data
        .get("URL")
        .and_then(Value::as_str)
        .context("Feishu endpoint bootstrap response missing URL")?
        .to_owned();
    let ping_seconds = data
        .pointer("/ClientConfig/PingInterval")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .unwrap_or(30);
    Ok(Endpoint {
        url,
        ping_interval: Duration::from_secs(ping_seconds as u64),
    })
}

/// Reassembles multi-part data frames (headers `sum`/`seq`), mirroring the
/// official SDK's five-second partial cache.
type PartialFrames = (Instant, Vec<Option<Vec<u8>>>);

pub(crate) struct FrameAssembler {
    parts: HashMap<String, PartialFrames>,
}

impl FrameAssembler {
    pub(crate) fn new() -> Self {
        Self {
            parts: HashMap::new(),
        }
    }

    pub(crate) fn combine(
        &mut self,
        message_id: &str,
        sum: i64,
        seq: i64,
        payload: Vec<u8>,
    ) -> Option<Vec<u8>> {
        let entry = self
            .parts
            .entry(message_id.to_owned())
            .or_insert_with(|| (Instant::now(), vec![None; sum as usize]));
        if seq >= 0 && (seq as usize) < entry.1.len() {
            entry.1[seq as usize] = Some(payload);
        }
        if let Some(complete) = entry
            .1
            .iter()
            .all(Option::is_some)
            .then(|| entry.1.iter().flatten().flatten().copied().collect())
        {
            self.parts.remove(message_id);
            return Some(complete);
        }
        None
    }

    fn evict(&mut self) {
        self.parts
            .retain(|_, (seen_at, _)| seen_at.elapsed() < Duration::from_secs(5));
    }
}

pub(crate) async fn run(pool: &sqlx::MySqlPool, claim: &StreamClaim, secret: &[u8]) -> Result<()> {
    let credentials = crate::webhook::credentials(secret);
    let app_id =
        string_field(&credentials, "appId").context("Feishu stream config is missing appId")?;
    let app_secret = string_field(&credentials, "appSecret")
        .context("Feishu stream config is missing appSecret")?;
    let endpoint = fetch_endpoint(&app_id, &app_secret, claim.tenant_id, claim.binding_id).await?;
    let parsed = url::Url::parse(&endpoint.url)?;
    let service_id = parsed
        .query_pairs()
        .find(|(key, _)| key == "service_id")
        .and_then(|(_, value)| value.parse::<i32>().ok())
        .unwrap_or(0);
    let socket = super::dial_websocket(&endpoint.url, claim.tenant_id, claim.binding_id)
        .await
        .context("Feishu long connection WebSocket dial failed")?;
    super::update_status(pool, claim, "connected", None)
        .await
        .ok();
    let state = crate::webhook::DispatchState {
        pool: pool.clone(),
        tenant_id: claim.tenant_id,
        application_id: claim.application_id,
        binding_id: claim.binding_id,
        connection_id: claim.binding_id.to_string(),
        trigger_name: claim.trigger_name.clone(),
        configuration_revision: claim.configuration_revision,
        input_mappings: claim.input_mappings.clone(),
        fixed_inputs: claim.fixed_inputs.clone(),
        provider: agentx_runtime_contracts::WebhookProviderV1::Feishu,
    };
    session(socket, state, service_id, endpoint.ping_interval).await
}

pub(crate) async fn session(
    socket: ProviderWebSocket,
    state: crate::webhook::DispatchState,
    service_id: i32,
    ping_interval: Duration,
) -> Result<()> {
    let (sink, mut stream) = socket.split();
    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let writer = tokio::spawn(async move {
        let mut sink = sink;
        while let Some(frame) = outbound_rx.recv().await {
            if sink.send(Message::Binary(frame.into())).await.is_err() {
                break;
            }
        }
    });
    let pinger = tokio::spawn({
        let outbound = outbound_tx.clone();
        async move {
            let mut ticker = tokio::time::interval(ping_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                let ping = Frame {
                    method: FRAME_METHOD_CONTROL,
                    service: service_id,
                    headers: vec![Header {
                        key: "type".into(),
                        value: "ping".into(),
                    }],
                    ..Default::default()
                };
                if outbound.send(ping.encode_to_vec()).await.is_err() {
                    break;
                }
                ticker.tick().await;
            }
        }
    });
    // A silent connection (2x ping interval + grace) means the peer vanished
    // without a close frame; treat it like a dropped socket.
    let read_timeout = ping_interval.saturating_mul(2) + Duration::from_secs(5);
    let mut assembler = FrameAssembler::new();
    let outcome = loop {
        let message = tokio::select! {
            message = tokio::time::timeout(read_timeout, stream.next()) => match message {
                Ok(message) => message,
                Err(_) => break Err(anyhow::anyhow!("Feishu long connection read timed out")),
            },
        };
        let message = match message {
            Some(Ok(message)) => message,
            Some(Err(error)) => {
                break Err(anyhow::Error::new(error).context("Feishu long connection error"));
            }
            None => break Err(anyhow::anyhow!("Feishu long connection closed")),
        };
        let bytes = match message {
            Message::Binary(bytes) => bytes,
            Message::Close(_) => break Err(anyhow::anyhow!("Feishu long connection closed")),
            _ => continue,
        };
        let frame = match Frame::decode(bytes.as_ref()) {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, binding = %state.binding_id, "Feishu frame decode failed");
                continue;
            }
        };
        if frame.method != FRAME_METHOD_DATA {
            // pong frames only refresh the read deadline implicitly; config
            // updates inside the payload keep the bootstrap interval.
            continue;
        }
        assembler.evict();
        let message_id = header_value(&frame, "message_id")
            .unwrap_or_default()
            .to_owned();
        let sum = header_int(&frame, "sum").unwrap_or(1);
        let payload = if sum > 1 {
            match header_int(&frame, "seq")
                .map(|seq| assembler.combine(&message_id, sum, seq, frame.payload.clone()))
            {
                Some(Some(payload)) => payload,
                _ => continue,
            }
        } else {
            frame.payload.clone()
        };
        let started = Instant::now();
        if let Ok(event) = serde_json::from_slice::<Value>(&payload) {
            state.dispatch(event).await;
        }
        let mut response = frame.clone();
        response.headers.push(Header {
            key: "biz_rt".into(),
            value: started.elapsed().as_millis().to_string(),
        });
        response.payload = json!({"code": 200}).to_string().into_bytes();
        if outbound_tx.send(response.encode_to_vec()).await.is_err() {
            break Err(anyhow::anyhow!("Feishu long connection writer stopped"));
        }
    };
    pinger.abort();
    writer.abort();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_protobuf_roundtrip_preserves_wire_layout() {
        let frame = Frame {
            seq_id: 7,
            log_id: 8,
            service: 3,
            method: FRAME_METHOD_DATA,
            headers: vec![Header {
                key: "message_id".into(),
                value: "m-1".into(),
            }],
            payload_encoding: "json".into(),
            payload_type: "event".into(),
            payload: br#"{"header":{"event_id":"e-1"}}"#.to_vec(),
            log_id_new: "log-1".into(),
        };
        let bytes = frame.encode_to_vec();
        // Field 1 varint tag 0x08 and field 2 tag 0x10 anchor the pbbp2 layout.
        assert_eq!(&bytes[..4], &[0x08, 0x07, 0x10, 0x08]);
        let decoded = Frame::decode(bytes.as_ref()).unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(header_value(&decoded, "message_id"), Some("m-1"));
    }

    #[test]
    fn assembler_joins_split_frames_in_any_order() {
        let mut assembler = FrameAssembler::new();
        let second = assembler.combine("m-2", 2, 1, b"world".to_vec());
        assert!(second.is_none());
        let joined = assembler.combine("m-2", 2, 0, b"hello ".to_vec());
        assert_eq!(joined, Some(b"hello world".to_vec()));
        assert!(assembler.parts.is_empty());
    }

    /// Real-platform probe. Runs only with AGENTX_TEST_FEISHU_APP_ID /
    /// AGENTX_TEST_FEISHU_APP_SECRET; prints every frame received for 20s.
    #[tokio::test]
    #[ignore = "requires AGENTX_TEST_FEISHU_* env credentials"]
    async fn feishu_real_endpoint_probe() {
        let app_id = std::env::var("AGENTX_TEST_FEISHU_APP_ID").unwrap();
        let app_secret = std::env::var("AGENTX_TEST_FEISHU_APP_SECRET").unwrap();
        let tenant_id = uuid::Uuid::now_v7();
        let request_id = uuid::Uuid::now_v7();
        let endpoint = fetch_endpoint(&app_id, &app_secret, tenant_id, request_id)
            .await
            .expect("bootstrap failed");
        println!("endpoint: {}", endpoint.url);
        let socket = super::super::dial_websocket(&endpoint.url, tenant_id, request_id).await;
        match socket {
            Ok(mut socket) => {
                use futures_util::StreamExt;
                let started = std::time::Instant::now();
                while started.elapsed() < std::time::Duration::from_secs(20) {
                    match tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
                        .await
                    {
                        Ok(Some(Ok(message))) => println!("frame: {message:?}"),
                        Ok(Some(Err(error))) => {
                            println!("stream error: {error}");
                            break;
                        }
                        Ok(None) => {
                            println!("stream closed");
                            break;
                        }
                        Err(_) => println!("(5s silence)"),
                    }
                }
            }
            Err(error) => panic!("dial failed: {error:#}"),
        }
    }

    #[tokio::test]
    async fn feishu_session_pings_events_and_acks() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut server = tokio_tungstenite::accept_async(stream).await.unwrap();
            let ping = server.next().await.unwrap().unwrap();
            let Message::Binary(ping) = ping else {
                panic!("expected binary ping")
            };
            let ping_frame = Frame::decode(ping.as_ref()).unwrap();
            assert_eq!(ping_frame.method, FRAME_METHOD_CONTROL);
            assert_eq!(header_value(&ping_frame, "type"), Some("ping"));
            let event = Frame {
                method: FRAME_METHOD_DATA,
                headers: vec![
                    Header { key: "type".into(), value: "event".into() },
                    Header { key: "message_id".into(), value: "m-9".into() },
                    Header { key: "sum".into(), value: "1".into() },
                ],
                payload: br#"{"header":{"event_id":"e-9","event_type":"im.message.receive_v1"},"event":{"sender":{"sender_id":{"open_id":"u-9"}},"message":{"chat_id":"c-9","message_type":"text","content":"{\"text\":\"hello\"}"}}}"#.to_vec(),
                ..Default::default()
            };
            server
                .send(Message::Binary(event.encode_to_vec().into()))
                .await
                .unwrap();
            let reply = server.next().await.unwrap().unwrap();
            let Message::Binary(reply) = reply else {
                panic!("expected binary reply")
            };
            let reply_frame = Frame::decode(reply.as_ref()).unwrap();
            assert_eq!(reply_frame.method, FRAME_METHOD_DATA);
            assert!(header_value(&reply_frame, "biz_rt").is_some());
            assert_eq!(
                serde_json::from_slice::<Value>(&reply_frame.payload).unwrap()["code"],
                200
            );
            assert_eq!(header_value(&reply_frame, "message_id"), Some("m-9"));
            server.close(None).await.unwrap();
        });
        let (client, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
            .await
            .unwrap();
        let state = crate::webhook::DispatchState::for_test(
            agentx_runtime_contracts::WebhookProviderV1::Feishu,
        );
        let result = session(client, state, 1, Duration::from_secs(30)).await;
        assert!(result.is_err());
        server.await.unwrap();
    }
}
