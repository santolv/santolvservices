//! End-to-end tests of the collaboration engine over real WebSockets: the API router is served on
//! a loopback port and clients are real Yrs documents. Run with a test PostgreSQL in DATABASE_URL.

use futures_util::{SinkExt, StreamExt};
use santolvdrive::{App, auth, router};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async,
    tungstenite::{Message, client::IntoClientRequest, handshake::client::generate_key},
};
use tower::ServiceExt;
use uuid::Uuid;
use yrs::{
    Doc, GetString, ReadTxn, Text, Transact, Update,
    updates::{decoder::Decode, encoder::Encode},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Server {
    app: App,
    addr: SocketAddr,
    _root: tempfile::TempDir,
    admin_id: Uuid,
}

struct Account {
    id: Uuid,
    space: Uuid,
    token: String,
    username: String,
}

impl Server {
    async fn start(db: PgPool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let app = App {
            journal: App::journal_pool(&db),
            db,
            root: root.path().into(),
            origin: "http://localhost:8080".into(),
            frontend_origin: santolvdrive::web::default_frontend_origin(),
            secure: false,
            hashes: Arc::new(tokio::sync::Semaphore::new(2)),
            transfers: Arc::new(tokio::sync::Semaphore::new(8)),
            user_transfers: Default::default(),
            quota: 10 * 1024 * 1024,
            requests: Arc::new(tokio::sync::Semaphore::new(256)),
            retention_days: 30,
        };
        santolvdrive::admin::bootstrap(
            &app,
            santolvdrive::admin::Credentials {
                username: "director".into(),
                password: "administrator test password".into(),
            },
        )
        .await
        .unwrap();
        let admin_id = sqlx::query_scalar("SELECT id FROM users WHERE account_type='admin'")
            .fetch_one(&app.db)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        Self {
            app,
            addr,
            _root: root,
            admin_id,
        }
    }

    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "localhost:8080")
            .header("content-type", "application/json")
            .extension(axum::extract::ConnectInfo(self.addr));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router(self.app.clone())
            .oneshot(
                request
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn account(&self, username: &str) -> Account {
        santolvdrive::admin::create(
            &self.app,
            self.admin_id,
            santolvdrive::admin::Credentials {
                username: username.into(),
                password: "temporary test password".into(),
            },
        )
        .await
        .unwrap();
        let login = |password: &'static str| json!({"username": username, "password": password, "mobile": true});
        let (_, temporary) = self
            .post("/api/v1/auth/login", None, login("temporary test password"))
            .await;
        let (status, _) = self
            .post(
                "/api/v1/auth/password",
                temporary["token"].as_str(),
                json!({"current_password":"temporary test password","new_password":"very strong test password"}),
            )
            .await;
        assert_eq!(status, 204);
        let (status, session) = self
            .post(
                "/api/v1/auth/login",
                None,
                login("very strong test password"),
            )
            .await;
        assert_eq!(status, 200, "{session}");
        let id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username=$1")
            .bind(username)
            .fetch_one(&self.app.db)
            .await
            .unwrap();
        let space = sqlx::query_scalar("SELECT id FROM spaces WHERE owner_id=$1")
            .bind(id)
            .fetch_one(&self.app.db)
            .await
            .unwrap();
        Account {
            id,
            space,
            token: session["token"].as_str().unwrap().into(),
            username: username.into(),
        }
    }

    async fn file(&self, owner: &Account, kind: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO nodes(id,space_id,parent_id,name,kind) VALUES($1,$2,NULL,$3,$4)")
            .bind(id)
            .bind(owner.space)
            .bind(format!("{id}.docx"))
            .bind(kind)
            .execute(&self.app.db)
            .await
            .unwrap();
        id
    }

    async fn grant(&self, node: Uuid, to: &Account, role: i16) {
        sqlx::query("INSERT INTO grants(node_id,user_id,role) VALUES($1,$2,$3)")
            .bind(node)
            .bind(to.id)
            .bind(role)
            .execute(&self.app.db)
            .await
            .unwrap();
    }

    fn request(&self, node: Uuid) -> axum::http::Request<()> {
        let mut request = format!("ws://{}/api/v1/collab/{node}", self.addr)
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        headers.insert("host", "localhost:8080".parse().unwrap());
        headers.insert("sec-websocket-key", generate_key().parse().unwrap());
        request
    }

    /// Connect as a browser on the suite's frontend would; `Err` carries the HTTP status.
    async fn connect(
        &self,
        node: Uuid,
        token: Option<&str>,
        origin: Option<&str>,
    ) -> Result<Socket, u16> {
        let mut request = self.request(node);
        let origin = origin
            .map(str::to_owned)
            .unwrap_or_else(santolvdrive::web::default_frontend_origin);
        if !origin.is_empty() {
            request
                .headers_mut()
                .insert("origin", origin.parse().unwrap());
        }
        if let Some(token) = token {
            request
                .headers_mut()
                .insert("cookie", format!("drive_session={token}").parse().unwrap());
        }
        let stream = TcpStream::connect(self.addr).await.unwrap();
        match client_async(request, MaybeTlsStream::Plain(stream)).await {
            Ok((socket, _)) => Ok(socket),
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                Err(response.status().as_u16())
            }
            Err(error) => panic!("unexpected handshake error: {error}"),
        }
    }
}

/// A collaborator: a WebSocket plus its local Yrs document.
struct Client {
    socket: Socket,
    doc: Doc,
    role: u8,
}

impl Client {
    async fn join(server: &Server, node: Uuid, account: &Account) -> Self {
        let mut socket = server
            .connect(node, Some(&account.token), None)
            .await
            .unwrap();
        let doc = Doc::new();
        let mut hello = vec![0x01, 1];
        hello.extend(doc.transact().state_vector().encode_v1());
        socket.send(Message::Binary(hello.into())).await.unwrap();
        let welcome = next_frame(&mut socket).await;
        assert_eq!(
            welcome[0],
            0x81,
            "welcome expected, got {:?}",
            String::from_utf8_lossy(&welcome)
        );
        let mut client = Self {
            socket,
            doc,
            role: welcome[1],
        };
        client.integrate(&welcome[18..]);
        client
    }
    fn integrate(&mut self, update: &[u8]) {
        let update = Update::decode_v1(update).unwrap();
        self.doc.transact_mut().apply_update(update).unwrap();
    }
    /// Edit locally and return the encoded change (what an editor adapter would send).
    fn insert(&self, index: u32, text: &str) -> Vec<u8> {
        let body = self.doc.get_or_insert_text("body");
        let before = self.doc.transact().state_vector();
        {
            let mut txn = self.doc.transact_mut();
            body.insert(&mut txn, index, text);
        }
        self.doc.transact().encode_diff_v1(&before)
    }
    async fn send_update(&mut self, op: Uuid, update: &[u8]) {
        let mut frame = vec![0x02];
        frame.extend_from_slice(op.as_bytes());
        frame.extend_from_slice(update);
        self.socket
            .send(Message::Binary(frame.into()))
            .await
            .unwrap();
    }
    fn text(&self) -> String {
        let body = self.doc.get_or_insert_text("body");
        body.get_string(&self.doc.transact())
    }
    /// Read frames until every tag in `tags` was seen, in any order (acks and other members'
    /// updates interleave freely). Returns the first frame of each tag, in the order of `tags`.
    async fn expect_all(&mut self, tags: &[u8]) -> Vec<Vec<u8>> {
        let mut found: Vec<Option<Vec<u8>>> = vec![None; tags.len()];
        while found.iter().any(Option::is_none) {
            let frame = next_frame(&mut self.socket).await;
            if frame[0] == 0x82 {
                self.integrate(updates_payload(&frame));
            }
            assert_ne!(
                frame[0],
                0x86,
                "server error: {}",
                String::from_utf8_lossy(&frame[1..])
            );
            if let Some(slot) = tags
                .iter()
                .zip(found.iter_mut())
                .find_map(|(tag, slot)| (*tag == frame[0] && slot.is_none()).then_some(slot))
            {
                *slot = Some(frame);
            }
        }
        found.into_iter().map(Option::unwrap).collect()
    }
    /// Read frames until one with `tag`, integrating remote updates seen on the way.
    async fn expect(&mut self, tag: u8) -> Vec<u8> {
        loop {
            let frame = next_frame(&mut self.socket).await;
            if frame[0] == 0x82 {
                self.integrate(updates_payload(&frame));
            }
            if frame[0] == tag {
                return frame;
            }
            assert_ne!(
                frame[0],
                0x86,
                "server error: {}",
                String::from_utf8_lossy(&frame[1..])
            );
        }
    }
}

/// The operation ids of a server `0x82` frame (count, ids, merged update).
fn updates_ops(frame: &[u8]) -> Vec<Uuid> {
    let count = u16::from_be_bytes([frame[1], frame[2]]) as usize;
    frame[3..3 + 16 * count]
        .chunks(16)
        .map(|id| Uuid::from_slice(id).unwrap())
        .collect()
}

/// The merged Yrs update of a server `0x82` frame.
fn updates_payload(frame: &[u8]) -> &[u8] {
    let count = u16::from_be_bytes([frame[1], frame[2]]) as usize;
    &frame[3 + 16 * count..]
}

async fn next_frame(socket: &mut Socket) -> Vec<u8> {
    loop {
        match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => return bytes.to_vec(),
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            other => panic!("expected a frame, got {other:?}"),
        }
    }
}

/// The next server error code, skipping other frames.
async fn next_error(socket: &mut Socket) -> String {
    loop {
        match tokio::time::timeout(Duration::from_secs(12), socket.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) if bytes[0] == 0x86 => {
                return String::from_utf8_lossy(&bytes[1..]).into();
            }
            Ok(Some(Ok(_))) => continue,
            other => panic!("expected an error frame, got {other:?}"),
        }
    }
}

#[sqlx::test]
async fn handshake_requires_suite_origin_cookie_session_and_drive_access(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-owner").await;
    let stranger = server.account("collab-stranger").await;
    let node = server.file(&owner, "file").await;
    let folder = server.file(&owner, "folder").await;
    let token = Some(owner.token.as_str());
    assert_eq!(
        server.connect(node, token, Some("")).await.err(),
        Some(403),
        "missing Origin"
    );
    assert_eq!(
        server
            .connect(node, token, Some("http://evil.example"))
            .await
            .err(),
        Some(403),
        "foreign Origin"
    );
    assert_eq!(
        server.connect(node, None, None).await.err(),
        Some(401),
        "no session"
    );
    let mut bearer = server.request(node);
    bearer.headers_mut().insert(
        "origin",
        santolvdrive::web::default_frontend_origin()
            .parse()
            .unwrap(),
    );
    bearer.headers_mut().insert(
        "authorization",
        format!("Bearer {}", owner.token).parse().unwrap(),
    );
    let stream = TcpStream::connect(server.addr).await.unwrap();
    assert!(
        matches!(
            client_async(bearer, MaybeTlsStream::Plain(stream)).await,
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) if r.status() == 403
        ),
        "bearer tokens are not accepted from pages"
    );
    assert_eq!(
        server
            .connect(node, Some(&stranger.token), None)
            .await
            .err(),
        Some(404),
        "no permission"
    );
    assert_eq!(
        server.connect(folder, token, None).await.err(),
        Some(404),
        "folders are not documents"
    );
    assert_eq!(
        server.connect(Uuid::new_v4(), token, None).await.err(),
        Some(404),
        "unknown file"
    );
    sqlx::query("UPDATE nodes SET trashed_at=now() WHERE id=$1")
        .bind(node)
        .execute(&server.app.db)
        .await
        .unwrap();
    assert_eq!(
        server.connect(node, token, None).await.err(),
        Some(404),
        "trashed file"
    );
}

#[sqlx::test]
async fn concurrent_edits_converge_and_are_acknowledged_after_persisting(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-alice").await;
    let editor = server.account("collab-bob").await;
    let node = server.file(&owner, "file").await;
    server.grant(node, &editor, 2).await;
    let mut alice = Client::join(&server, node, &owner).await;
    let mut bob = Client::join(&server, node, &editor).await;
    assert_eq!((alice.role, bob.role), (3, 2));
    // Both type at the same position at the same time.
    let (a_op, b_op) = (Uuid::new_v4(), Uuid::new_v4());
    let a_update = alice.insert(0, "Hola ");
    let b_update = bob.insert(0, "Mundo ");
    alice.send_update(a_op, &a_update).await;
    bob.send_update(b_op, &b_update).await;
    let a_ack = alice.expect_all(&[0x83, 0x82]).await.remove(0);
    let b_ack = bob.expect_all(&[0x83, 0x82]).await.remove(0);
    assert_eq!(&a_ack[1..17], a_op.as_bytes());
    assert_eq!(&b_ack[1..17], b_op.as_bytes());
    // Acknowledged means stored.
    for op in [a_op, b_op] {
        let stored: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM collaboration.updates WHERE node_id=$1 AND op_id=$2)",
        )
        .bind(node)
        .bind(op)
        .fetch_one(&server.app.db)
        .await
        .unwrap();
        assert!(stored);
    }
    assert_eq!(alice.text(), bob.text(), "both replicas converge");
    assert!(alice.text().contains("Hola ") && alice.text().contains("Mundo "));
    // A delete racing an insert inside the deleted range converges too.
    let delete = {
        let body = alice.doc.get_or_insert_text("body");
        let before = alice.doc.transact().state_vector();
        body.remove_range(&mut alice.doc.transact_mut(), 0, 5);
        alice.doc.transact().encode_diff_v1(&before)
    };
    let insert = bob.insert(2, "XYZ");
    alice.send_update(Uuid::new_v4(), &delete).await;
    bob.send_update(Uuid::new_v4(), &insert).await;
    alice.expect_all(&[0x83, 0x82]).await;
    bob.expect_all(&[0x83, 0x82]).await;
    assert_eq!(alice.text(), bob.text());
}

#[sqlx::test]
async fn readers_cannot_write_retries_apply_once_and_state_survives_reconnection(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-owner2").await;
    let reader = server.account("collab-reader").await;
    let node = server.file(&owner, "file").await;
    server.grant(node, &reader, 1).await;
    let mut writer = Client::join(&server, node, &owner).await;
    let mut viewer = Client::join(&server, node, &reader).await;
    assert_eq!(viewer.role, 1);
    // A reader's update is refused and changes nothing.
    let sneaky = viewer.insert(0, "no permitido");
    viewer.send_update(Uuid::new_v4(), &sneaky).await;
    assert_eq!(next_error(&mut viewer.socket).await, "read_only");
    // The same operation sent twice (a retry after a lost ack) is stored and applied once.
    let op = Uuid::new_v4();
    let update = writer.insert(0, "Texto");
    writer.send_update(op, &update).await;
    let first = writer.expect(0x83).await;
    writer.send_update(op, &update).await;
    let second = writer.expect(0x83).await;
    assert_eq!(first, second, "a retry gets the original sequence");
    viewer.expect(0x82).await;
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM collaboration.updates WHERE node_id=$1")
            .bind(node)
            .fetch_one(&server.app.db)
            .await
            .unwrap();
    assert_eq!(rows, 1);
    // Out-of-order delivery: an update whose dependency arrives later still converges.
    let base = writer.insert(5, " uno");
    let dependent = writer.insert(9, " dos");
    writer.send_update(Uuid::new_v4(), &dependent).await;
    writer.expect(0x83).await;
    writer.send_update(Uuid::new_v4(), &base).await;
    writer.expect(0x83).await;
    drop(viewer);
    let expected = writer.text();
    drop(writer);
    // Everyone left: the room is compacted and closed; a new session rebuilds it from storage.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let fresh = Client::join(&server, node, &owner).await;
    assert_eq!(fresh.text(), expected);
    assert_eq!(fresh.text(), "Texto uno dos");
}

#[sqlx::test]
async fn reused_operation_ids_stay_consistent_and_reconnections_are_limited(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-honest").await;
    let intruder = server.account("collab-reuser").await;
    let node = server.file(&owner, "file").await;
    server.grant(node, &intruder, 2).await;
    let mut honest = Client::join(&server, node, &owner).await;
    let mut other = Client::join(&server, node, &intruder).await;
    let op = Uuid::new_v4();
    let update = honest.insert(0, "Original");
    honest.send_update(op, &update).await;
    honest.expect(0x83).await;
    // The intruder saw the id in the broadcast and reuses it with other content. The server
    // already integrated that content, so it stores and broadcasts it under a new id: memory,
    // storage and every member stay identical (an exact retry would be skipped instead).
    let frame = other.expect(0x82).await;
    assert!(updates_ops(&frame).contains(&op));
    let forged = other.insert(0, "Otro ");
    other.send_update(op, &forged).await;
    other.expect(0x83).await;
    let relayed = honest.expect(0x82).await;
    assert!(
        !updates_ops(&relayed).contains(&op),
        "stored under a new id"
    );
    assert_eq!(honest.text(), other.text());
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM collaboration.updates WHERE node_id=$1")
            .bind(node)
            .fetch_one(&server.app.db)
            .await
            .unwrap();
    assert_eq!(rows, 2);
    let expected = honest.text();
    drop((honest, other));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let fresh = Client::join(&server, node, &owner).await;
    assert_eq!(fresh.text(), expected);
    assert_eq!(fresh.text(), "Otro Original");
    drop(fresh);
    // Reconnection loops are bounded per account (a burst, then about one per second).
    let mut refused = None;
    for attempt in 0..40 {
        if let Err(status) = server.connect(node, Some(&intruder.token), None).await {
            refused = Some((attempt, status));
            break;
        }
    }
    let (attempt, status) = refused.expect("reconnections must be limited");
    assert_eq!(status, 429);
    assert!(
        attempt >= 20,
        "the burst allows normal reconnections ({attempt})"
    );
}

#[sqlx::test]
async fn offline_edits_sync_on_reconnection(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-online").await;
    let other = server.account("collab-offline").await;
    let node = server.file(&owner, "file").await;
    server.grant(node, &other, 2).await;
    let mut online = Client::join(&server, node, &owner).await;
    let mut offline = Client::join(&server, node, &other).await;
    let start = online.insert(0, "Base.");
    online.send_update(Uuid::new_v4(), &start).await;
    online.expect(0x83).await;
    offline.expect(0x82).await;
    // The second user loses the connection and keeps editing.
    let local_doc = std::mem::replace(&mut offline.doc, Doc::new());
    drop(offline);
    let pending = {
        let body = local_doc.get_or_insert_text("body");
        let before = local_doc.transact().state_vector();
        body.insert(&mut local_doc.transact_mut(), 5, " Sin conexión.");
        local_doc.transact().encode_diff_v1(&before)
    };
    let meanwhile = online.insert(0, "Online. ");
    online.send_update(Uuid::new_v4(), &meanwhile).await;
    online.expect(0x83).await;
    // Reconnecting with its state vector, it receives only what it missed, then sends its edits.
    let mut socket = server
        .connect(node, Some(&other.token), None)
        .await
        .unwrap();
    let mut hello = vec![0x01, 1];
    hello.extend(local_doc.transact().state_vector().encode_v1());
    socket.send(Message::Binary(hello.into())).await.unwrap();
    let welcome = next_frame(&mut socket).await;
    let mut back = Client {
        socket,
        doc: local_doc,
        role: welcome[1],
    };
    back.integrate(&welcome[18..]);
    back.send_update(Uuid::new_v4(), &pending).await;
    back.expect(0x83).await;
    online.expect(0x82).await;
    assert_eq!(online.text(), back.text());
    assert!(back.text().contains("Online.") && back.text().contains("Sin conexión."));
}

#[sqlx::test]
async fn revoked_access_and_ended_sessions_close_connections(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-owner3").await;
    let guest = server.account("collab-guest").await;
    let node = server.file(&owner, "file").await;
    server.grant(node, &guest, 2).await;
    let mut shared = Client::join(&server, node, &guest).await;
    let mut own = Client::join(&server, node, &owner).await;
    sqlx::query("DELETE FROM grants WHERE node_id=$1 AND user_id=$2")
        .bind(node)
        .bind(guest.id)
        .execute(&server.app.db)
        .await
        .unwrap();
    assert_eq!(next_error(&mut shared.socket).await, "access_revoked");
    sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
        .bind(auth::hash(&owner.token))
        .execute(&server.app.db)
        .await
        .unwrap();
    assert_eq!(next_error(&mut own.socket).await, "session_ended");
    let _ = (owner.username, guest.username);
}

#[sqlx::test]
async fn malformed_oversized_and_text_frames_are_rejected(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-fuzz").await;
    let node = server.file(&owner, "file").await;
    // Hello is mandatory first.
    let mut socket = server
        .connect(node, Some(&owner.token), None)
        .await
        .unwrap();
    socket
        .send(Message::Binary(vec![0x04].into()))
        .await
        .unwrap();
    assert_eq!(next_error(&mut socket).await, "hello_required");
    // Garbage frames are answered with errors and, repeated, close the connection.
    let mut client = Client::join(&server, node, &owner).await;
    for frame in [
        vec![0x7f],
        vec![0x02, 1],
        b"\x03[]".to_vec(),
        vec![0x01, 1],
        {
            let mut bad = vec![0x02];
            bad.extend_from_slice(Uuid::new_v4().as_bytes());
            bad.extend_from_slice(&[0xff; 32]);
            bad
        },
    ] {
        client
            .socket
            .send(Message::Binary(frame.into()))
            .await
            .unwrap();
    }
    let mut codes = Vec::new();
    while let Ok(Some(Ok(message))) =
        tokio::time::timeout(Duration::from_secs(5), client.socket.next()).await
    {
        if let Message::Binary(bytes) = message
            && bytes[0] == 0x86
        {
            codes.push(String::from_utf8_lossy(&bytes[1..]).to_string());
        }
    }
    assert!(codes.contains(&"unknown_frame".into()), "{codes:?}");
    assert!(codes.contains(&"invalid_update".into()), "{codes:?}");
    assert!(codes.contains(&"protocol_errors".into()), "{codes:?}");
    // Text frames close the connection.
    let mut text = Client::join(&server, node, &owner).await;
    text.socket
        .send(Message::Text("hola".into()))
        .await
        .unwrap();
    assert_eq!(next_error(&mut text.socket).await, "text_not_allowed");
    // A frame over the limit is refused by the server before it is processed.
    let mut big = Client::join(&server, node, &owner).await;
    let _ = big
        .socket
        .send(Message::Binary(vec![0x03; 300 * 1024].into()))
        .await;
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match big.socket.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return true,
                _ => continue,
            }
        }
    })
    .await;
    assert_eq!(closed, Ok(true));
    // Nothing invalid reached storage.
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM collaboration.updates WHERE node_id=$1")
            .bind(node)
            .fetch_one(&server.app.db)
            .await
            .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test]
async fn presence_is_stamped_by_the_server(pool: PgPool) {
    let server = Server::start(pool).await;
    let owner = server.account("collab-presence").await;
    let other = server.account("collab-watcher").await;
    let node = server.file(&owner, "file").await;
    server.grant(node, &other, 1).await;
    let mut speaker = Client::join(&server, node, &owner).await;
    let mut listener = Client::join(&server, node, &other).await;
    // A client claiming another identity is ignored: the server stamps its own.
    let forged = json!({"cursor": 3, "user": other.id, "name": "suplantado"});
    let mut frame = vec![0x03];
    frame.extend(forged.to_string().as_bytes());
    speaker
        .socket
        .send(Message::Binary(frame.into()))
        .await
        .unwrap();
    let presence = listener.expect(0x84).await;
    let presence: Value = serde_json::from_slice(&presence[1..]).unwrap();
    assert_eq!(presence["user"], json!(owner.id));
    assert_eq!(presence["name"], json!("collab-presence"));
    assert_eq!(presence["state"]["cursor"], json!(3));
}

/// Load measurements (run explicitly):
/// `cargo test --release --test collaboration load -- --ignored --nocapture`.
/// Every member edits concurrently at about 20 operations per second. Reported per scenario:
/// acknowledgement latency (send → persisted ack), propagation latency (send → another member
/// receives it), accepted operations per second, bytes received, errors and disconnections.
#[sqlx::test]
#[ignore]
async fn load_same_document_and_separate_documents(pool: PgPool) {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;
    let server = Server::start(pool).await;
    // Account creation is rate limited (100 per administrator): reuse one pool of accounts.
    let owner = server.account("collab-load").await;
    let mut accounts = vec![];
    for i in 0..99 {
        accounts.push(server.account(&format!("load-{i}")).await);
    }
    let rounds = 40u32;
    let interval = Duration::from_millis(50);
    for clients in [2usize, 10, 25, 50, 100] {
        for shared in [true, false] {
            let members: Vec<&Account> = std::iter::once(&owner)
                .chain(accounts.iter())
                .take(clients)
                .collect();
            let common = server.file(&owner, "file").await;
            let sent: Arc<Mutex<HashMap<Uuid, Instant>>> = Arc::default();
            let codes: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
            // Everyone joins before anyone edits, so every member receives every remote update.
            let start = Arc::new(tokio::sync::Barrier::new(clients));
            let mut tasks = Vec::new();
            for account in &members {
                let node = if shared {
                    common
                } else {
                    server.file(&owner, "file").await
                };
                if account.id != owner.id {
                    server.grant(node, account, 2).await;
                }
                let client = Client::join(&server, node, account).await;
                let expected_remote = if shared {
                    rounds as usize * (clients - 1)
                } else {
                    0
                };
                let sent = sent.clone();
                let codes = codes.clone();
                let start = start.clone();
                // Operations this member sent: they may come back inside mixed batches.
                let mine: Arc<Mutex<std::collections::HashSet<Uuid>>> = Arc::default();
                tasks.push(tokio::spawn(async move {
                    let Client { socket, doc, .. } = client;
                    let (mut sink, mut stream) = socket.split();
                    let reader_sent = sent.clone();
                    let reader_codes = codes.clone();
                    let reader_mine = mine.clone();
                    let reader = tokio::spawn(async move {
                        let (mut acks, mut remote, mut bytes, mut errors) =
                            (Vec::new(), Vec::new(), 0usize, 0usize);
                        // 0 = finished, 1 = socket closed, 2 = waited 30 s for an expected frame.
                        let mut outcome = 0u8;
                        while acks.len() + errors < rounds as usize
                            || remote.len() < expected_remote
                        {
                            match tokio::time::timeout(Duration::from_secs(30), stream.next()).await
                            {
                                Ok(Some(Ok(Message::Binary(frame)))) => {
                                    bytes += frame.len();

                                    let elapsed =
                                        |op| {
                                            reader_sent.lock().unwrap().get(&op).map(
                                                |t: &Instant| t.elapsed().as_secs_f64() * 1000.0,
                                            )
                                        };
                                    match frame[0] {
                                        0x83 => acks.extend(elapsed(
                                            Uuid::from_slice(&frame[1..17]).unwrap(),
                                        )),
                                        0x82 => {
                                            let own = reader_mine.lock().unwrap();
                                            for op in updates_ops(&frame) {
                                                if !own.contains(&op) {
                                                    remote.extend(elapsed(op));
                                                }
                                            }
                                        }
                                        0x86 => {
                                            errors += 1;
                                            let code =
                                                String::from_utf8_lossy(&frame[1..]).into_owned();
                                            *reader_codes
                                                .lock()
                                                .unwrap()
                                                .entry(code)
                                                .or_default() += 1;
                                        }
                                        _ => {}
                                    }
                                }
                                Ok(Some(Ok(_))) => {}
                                Err(_) => {
                                    outcome = 2;
                                    break;
                                }
                                _ => {
                                    outcome = 1;
                                    break;
                                }
                            }
                        }
                        (acks, remote, bytes, errors, outcome)
                    });
                    let body = doc.get_or_insert_text("body");
                    start.wait().await;
                    let began = Instant::now();
                    for round in 0..rounds {
                        let before = doc.transact().state_vector();
                        body.insert(&mut doc.transact_mut(), 0, &format!("{round} "));
                        let update = doc.transact().encode_diff_v1(&before);
                        let op = Uuid::new_v4();
                        let mut frame = vec![0x02];
                        frame.extend_from_slice(op.as_bytes());
                        frame.extend_from_slice(&update);
                        mine.lock().unwrap().insert(op);
                        sent.lock().unwrap().insert(op, Instant::now());
                        if sink.send(Message::Binary(frame.into())).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(interval).await;
                    }
                    let result = reader.await.unwrap();
                    let editing = began.elapsed().as_secs_f64();
                    drop(sink);
                    (result, editing)
                }));
            }
            let (mut acks, mut remote, mut bytes, mut errors, mut closed, mut stalled) =
                (Vec::new(), Vec::new(), 0, 0, 0, 0);
            let mut elapsed = 0f64;
            for task in tasks {
                let ((a, r, b, e, outcome), editing) = task.await.unwrap();
                acks.extend(a);
                remote.extend(r);
                bytes += b;
                errors += e;
                closed += usize::from(outcome == 1);
                stalled += usize::from(outcome == 2);
                elapsed = elapsed.max(editing);
            }
            let pct = |v: &mut Vec<f64>, p: f64| {
                v.sort_by(f64::total_cmp);
                if v.is_empty() {
                    0.0
                } else {
                    v[((v.len() as f64 * p) as usize).min(v.len() - 1)]
                }
            };
            let expected_total = clients * rounds as usize;
            println!(
                "clients={clients:3} shared={shared:5} accepted={}/{expected_total} ops/s={:7.1} ack p50={:6.2} p95={:6.2} p99={:6.2} ms | propagation p50={:6.2} p95={:6.2} p99={:6.2} ms ({} deliveries) | received={:.1} KiB errors={errors} {:?} closed={closed} stalled={stalled} elapsed={elapsed:.1}s",
                acks.len(),
                acks.len() as f64 / elapsed,
                pct(&mut acks, 0.50),
                pct(&mut acks, 0.95),
                pct(&mut acks, 0.99),
                pct(&mut remote, 0.50),
                pct(&mut remote, 0.95),
                pct(&mut remote, 0.99),
                remote.len(),
                bytes as f64 / 1024.0,
                codes.lock().unwrap()
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}
