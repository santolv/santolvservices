//! The collaboration WebSocket: `GET /api/v1/collab/{node}` on the API listener (no new port).
//!
//! Before upgrading: the browser's `Origin` must be the suite's own frontend (Cross-Site WebSocket
//! Hijacking), the cookie session must be valid, and Drive must grant the user access to that file.
//! After upgrading, every frame is decoded with bounds, rate-limited, and checked against the
//! connection's current role; the session and the role are re-checked every few seconds, so a
//! logout, an expired session or a revoked share closes the connection.

use super::{
    permissions,
    protocol::{self, ClientFrame, Role},
    room::{self, Broadcast},
};
use crate::{App, auth::Principal, error::Error};
use axum::{
    Router,
    body::Bytes,
    extract::{
        FromRequestParts, Path, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast::error::RecvError, mpsc};
use uuid::Uuid;

/// A client that sends nothing (not even a ping) for this long is disconnected.
const IDLE: Duration = Duration::from_secs(60);
/// Time to send the opening Hello after connecting.
const HELLO: Duration = Duration::from_secs(10);
/// How often the session and the role are checked again.
const RECHECK: Duration = Duration::from_secs(5);
/// Frames queued for one client before it is treated as too slow and disconnected.
const OUTBOX: usize = 1024;
/// Malformed frames tolerated before closing the connection.
const MAX_PROTOCOL_ERRORS: u32 = 5;

pub fn routes() -> Router<App> {
    Router::new().route("/api/v1/collab/{node}", get(connect))
}

/// A token bucket: `rate` events per second, bursts up to `burst`.
struct Bucket {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
}
impl Bucket {
    fn new(rate: f64, burst: f64) -> Self {
        Self {
            tokens: burst,
            rate,
            burst,
            last: Instant::now(),
        }
    }
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens =
            (self.tokens + now.duration_since(self.last).as_secs_f64() * self.rate).min(self.burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

struct Peer {
    user: Uuid,
    session: String,
    name: String,
    color: String,
    role: Role,
}

async fn connect(
    State(app): State<App>,
    Path(node): Path<Uuid>,
    upgrade: WebSocketUpgrade,
    mut parts: Parts,
) -> Response {
    // Browsers always send Origin on WebSocket handshakes; only the suite's frontend may connect.
    let origin = parts.headers.get("origin").and_then(|v| v.to_str().ok());
    if origin != Some(app.frontend_origin.as_str()) {
        return Error(StatusCode::FORBIDDEN, "origin_not_allowed").into_response();
    }
    // Cookie sessions only: a bearer token is never accepted from a page.
    if parts.headers.contains_key("authorization") {
        return Error(StatusCode::FORBIDDEN, "cookie_session_required").into_response();
    }
    let principal = match Principal::from_request_parts(&mut parts, &app).await {
        Ok(principal) => principal,
        Err(error) => return error.into_response(),
    };
    if principal.account_type != "user" || principal.must_change_password {
        return Error(StatusCode::FORBIDDEN, "user_session_required").into_response();
    }
    if !room::attempt(principal.id) {
        return Error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts").into_response();
    }
    if let Err(error) = crate::core::manager::require_access(&app.db, "drive", principal.id).await {
        return error.into_response();
    }
    let role = match permissions::role(&app, node, principal.id).await {
        Ok(Some(role)) => role,
        Ok(None) => return Error::denied().into_response(),
        Err(error) => return error.into_response(),
    };
    let (name, color) = match permissions::display_name(&app, principal.id).await {
        Ok(found) => found,
        Err(error) => return error.into_response(),
    };
    let membership = match room::join(&app, node, principal.id).await {
        Ok(membership) => membership,
        Err(error) => return error.into_response(),
    };
    let peer = Peer {
        user: principal.id,
        session: principal.session,
        name,
        color,
        role,
    };
    upgrade
        .max_message_size(protocol::MAX_FRAME)
        .max_frame_size(protocol::MAX_FRAME)
        .on_upgrade(move |socket| session(app, node, peer, membership, socket))
}

async fn session(
    app: App,
    node: Uuid,
    mut peer: Peer,
    membership: room::Membership,
    socket: WebSocket,
) {
    let room = membership.room.clone();
    let connection = Uuid::new_v4();
    let (mut sink, mut stream) = socket.split();
    let (outbox, mut queue) = mpsc::channel::<Bytes>(OUTBOX);
    // A single writer owns the socket's sending half.
    let writer = tokio::spawn(async move {
        while let Some(frame) = queue.recv().await {
            if frame.is_empty() {
                break;
            }
            if sink.send(Message::Binary(frame)).await.is_err() {
                break;
            }
        }
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: 1000,
                reason: "".into(),
            })))
            .await;
    });
    let send = |frame: Vec<u8>| outbox.try_send(frame.into()).is_ok();
    // Other members' frames are forwarded by their own task, so a connection waiting for its
    // turn to persist an edit keeps receiving. A member that cannot keep up is disconnected.
    let mut members = room.channel.subscribe();
    let (slow_tx, mut slow) = tokio::sync::oneshot::channel::<&'static str>();
    let forward_outbox = outbox.clone();
    let forwarder = tokio::spawn(async move {
        let reason = loop {
            match members.recv().await {
                Ok(message) if message.from == connection => {}
                Ok(message) => {
                    if forward_outbox.try_send(message.frame).is_err() {
                        break "too_slow";
                    }
                }
                Err(RecvError::Lagged(_)) => break "too_slow",
                Err(RecvError::Closed) => break "room_closed",
            }
        };
        let _ = slow_tx.send(reason);
    });

    // The client starts with Hello and its state vector; it gets what it is missing.
    let hello = match tokio::time::timeout(HELLO, stream.next()).await {
        Ok(Some(Ok(Message::Binary(bytes)))) => protocol::decode(&bytes),
        _ => Err(protocol::ProtocolError::Truncated),
    };
    let welcome = match hello {
        Ok(ClientFrame::Hello { state_vector }) => room.diff(&state_vector).await,
        Ok(_) | Err(_) => Err(Error::bad("hello_required")),
    };
    match welcome {
        Ok(diff) => {
            send(protocol::welcome(peer.role, connection, &diff));
        }
        Err(error) => {
            forwarder.abort();
            send(protocol::error(error.1));
            send(Vec::new());
            let _ = writer.await;
            return;
        }
    }

    let mut updates = Bucket::new(100.0, 300.0);
    let mut presence = Bucket::new(20.0, 40.0);
    let mut pings = Bucket::new(2.0, 10.0);
    let mut errors = 0u32;
    let mut last_seen = Instant::now();
    let mut recheck = tokio::time::interval(RECHECK);
    recheck.tick().await;
    let reason = loop {
        tokio::select! {
            incoming = tokio::time::timeout(IDLE, stream.next()) => {
                let bytes = match incoming {
                    Err(_) => break "idle_timeout",
                    Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => break "closed",
                    Ok(Some(Ok(Message::Binary(bytes)))) => bytes,
                    Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
                    Ok(Some(Ok(Message::Text(_)))) => break "text_not_allowed",
                };
                last_seen = Instant::now();
                match protocol::decode(&bytes) {
                    Err(error) => {
                        errors += 1;
                        send(protocol::error(error.code()));
                        if errors >= MAX_PROTOCOL_ERRORS {
                            break "protocol_errors";
                        }
                    }
                    // Pings beyond the rate keep the connection alive but get no answer.
                    Ok(ClientFrame::Ping) => {
                        if pings.take() {
                            send(protocol::pong());
                        }
                    }
                    Ok(ClientFrame::Hello { .. }) => {
                        errors += 1;
                        send(protocol::error("unexpected_hello"));
                    }
                    Ok(ClientFrame::Update { op, update }) => {
                        if peer.role < Role::Editor {
                            send(protocol::error("read_only"));
                            continue;
                        }
                        if !updates.take() {
                            send(protocol::error("rate_limited"));
                            continue;
                        }
                        // The room broadcasts the update once its batch is stored.
                        match room.apply(peer.user, connection, op, &update).await {
                            Ok((seq, _duplicate)) => {
                                send(protocol::ack(op, seq));
                            }
                            Err(error) if room.poisoned.load(std::sync::atomic::Ordering::Acquire) => {
                                send(protocol::error(error.1));
                                break "room_reset";
                            }
                            Err(error) => {
                                send(protocol::error(error.1));
                                // A malformed CRDT update is a protocol violation like a malformed frame.
                                if error.1 == "invalid_update" {
                                    errors += 1;
                                    if errors >= MAX_PROTOCOL_ERRORS {
                                        break "protocol_errors";
                                    }
                                }
                            }
                        }
                    }
                    Ok(ClientFrame::Awareness(state)) => {
                        if !presence.take() {
                            continue;
                        }
                        // Identity is stamped by the server; the client only says where it is.
                        let stamped = json!({
                            "connection": connection, "user": peer.user, "name": peer.name,
                            "color": peer.color, "state": state,
                        });
                        let _ = room.channel.send(Broadcast { from: connection, frame: protocol::awareness(&stamped).into() });
                    }
                }
            }
            reason = &mut slow => break reason.unwrap_or("room_closed"),
            _ = recheck.tick() => {
                if last_seen.elapsed() > IDLE {
                    break "idle_timeout";
                }
                if room.poisoned.load(std::sync::atomic::Ordering::Acquire) {
                    break "room_reset";
                }
                match permissions::session_alive(&app, &peer.session).await {
                    Ok(true) => {}
                    Ok(false) => break "session_ended",
                    Err(_) => continue,
                }
                match permissions::role(&app, node, peer.user).await {
                    Ok(Some(role)) => peer.role = role,
                    Ok(None) => break "access_revoked",
                    Err(_) => {}
                }
            }
        }
    };
    forwarder.abort();
    let left = json!({"connection": connection, "user": peer.user, "left": true});
    let _ = room.channel.send(Broadcast {
        from: connection,
        frame: protocol::awareness(&left).into(),
    });
    if !matches!(reason, "closed") {
        let _ = outbox.try_send(protocol::error(reason).into());
    }
    let _ = outbox.try_send(Bytes::new());
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    drop(membership);
}
