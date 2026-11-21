use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use santolvdrive::{App, auth, router, storage, worker};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Harness {
    app: App,
    api: Router,
    _root: tempfile::TempDir,
    admin: String,
}
struct Account {
    id: Uuid,
    space: Uuid,
    token: String,
    username: String,
}
impl Harness {
    async fn new(db: PgPool) -> Self {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(root.path().join("uploads"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.path().join("objects"))
            .await
            .unwrap();
        let app = App {
            db,
            root: root.path().into(),
            origin: "http://localhost:8080".into(),
            secure: false,
            hashes: Arc::new(tokio::sync::Semaphore::new(2)),
            transfers: Arc::new(tokio::sync::Semaphore::new(8)),
            quota: 10 * 1024 * 1024,
            requests: Arc::new(tokio::sync::Semaphore::new(128)),
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
        let admin = auth::token();
        let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE account_type='admin'")
            .fetch_one(&app.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sessions VALUES($1,$2,'unused',now()+interval '1 hour')")
            .bind(auth::hash(&admin))
            .bind(admin_id)
            .execute(&app.db)
            .await
            .unwrap();
        Self {
            api: router(app.clone()),
            app,
            _root: root,
            admin,
        }
    }
    async fn req(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
        body: Body,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .extension(ConnectInfo(
                "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
            ));
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let r = self
            .api
            .clone()
            .oneshot(b.body(body).unwrap())
            .await
            .unwrap();
        let status = r.status();
        let h = r.headers().clone();
        let bytes = to_bytes(r.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        (status, h, bytes)
    }
    async fn json(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
        v: Value,
    ) -> (StatusCode, Value) {
        let mut h = headers.to_vec();
        h.push(("content-type", "application/json"));
        let (s, _, b) = self
            .req(method, path, token, &h, Body::from(v.to_string()))
            .await;
        (s, serde_json::from_slice(&b).unwrap_or(json!(null)))
    }
    async fn account(&self, name: &str) -> Account {
        let username = name.to_owned();
        let (s, v) = self
            .json(
                "POST",
                "/api/v1/admin/users",
                Some(&self.admin),
                &[],
                json!({"username":username,"password":"temporary test password"}),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        let (s, v) = self
            .json(
                "POST",
                "/api/v1/auth/login",
                None,
                &[],
                json!({"username":username,"password":"temporary test password","mobile":true}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let temporary = v["token"].as_str().unwrap();
        let (s,v)=self.json("POST","/api/v1/auth/password",Some(temporary),&[],json!({"current_password":"temporary test password","new_password":"very strong test password"})).await;
        assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
        let (s, v) = self
            .json(
                "POST",
                "/api/v1/auth/login",
                None,
                &[],
                json!({"username":username,"password":"very strong test password","mobile":true}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username=$1")
            .bind(&username)
            .fetch_one(&self.app.db)
            .await
            .unwrap();
        let space: Uuid = sqlx::query_scalar("SELECT id FROM spaces WHERE owner_id=$1")
            .bind(id)
            .fetch_one(&self.app.db)
            .await
            .unwrap();
        Account {
            id,
            space,
            token: v["token"].as_str().unwrap().to_owned(),
            username,
        }
    }
    async fn folder(&self, a: &Account, parent: Option<Uuid>, name: &str, key: &str) -> Uuid {
        let (s, v) = self
            .json(
                "POST",
                &format!("/api/v1/spaces/{}/nodes", a.space),
                Some(&a.token),
                &[("idempotency-key", key)],
                json!({"name":name,"parent_id":parent}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        id(&v, "id")
    }
    #[allow(clippy::too_many_arguments)]
    async fn start(
        &self,
        a: &Account,
        name: &str,
        bytes: &[u8],
        key: &str,
        parent: Option<Uuid>,
        node: Option<Uuid>,
        revision: &str,
    ) -> Uuid {
        use sha2::{Digest, Sha256};
        let(s,v)=self.json("POST","/api/v1/uploads",Some(&a.token),&[("idempotency-key",key),("if-match",revision)],json!({"space_id":a.space,"parent_id":parent,"node_id":node,"name":name,"size":bytes.len(),"sha256":hex::encode(Sha256::digest(bytes))})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        id(&v, "id")
    }
    async fn chunk(
        &self,
        a: &Account,
        upload: Uuid,
        offset: i64,
        bytes: &[u8],
        good: bool,
    ) -> StatusCode {
        use sha2::{Digest, Sha256};
        let hash = if good {
            hex::encode(Sha256::digest(bytes))
        } else {
            "0".repeat(64)
        };
        self.req(
            "PATCH",
            &format!("/api/v1/uploads/{upload}/content"),
            Some(&a.token),
            &[
                ("upload-offset", &offset.to_string()),
                ("content-length", &bytes.len().to_string()),
                ("x-chunk-sha256", &hash),
            ],
            Body::from(bytes.to_vec()),
        )
        .await
        .0
    }
    async fn complete(&self, a: &Account, upload: Uuid) -> (StatusCode, Value) {
        let result = self
            .json(
                "POST",
                &format!("/api/v1/uploads/{upload}/complete"),
                Some(&a.token),
                &[],
                json!({}),
            )
            .await;
        if result.0 == StatusCode::ACCEPTED {
            worker::finalize_requested(&self.app).await.unwrap();
            return self
                .json(
                    "POST",
                    &format!("/api/v1/uploads/{upload}/complete"),
                    Some(&a.token),
                    &[],
                    json!({}),
                )
                .await;
        }
        result
    }
}
fn id(v: &Value, key: &str) -> Uuid {
    v[key].as_str().unwrap().parse().unwrap()
}

#[sqlx::test]
async fn native_https_login_uses_real_connection_ip(pool: PgPool) {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    use tokio_rustls::rustls;
    let h = Harness::new(pool).await;
    let user = h.account("native-tls").await;
    let material = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cert = temp.path().join("cert.pem");
    let key = temp.path().join("key.pem");
    std::fs::write(&cert, material.cert.pem()).unwrap();
    std::fs::write(&key, material.signing_key.serialize_pem()).unwrap();
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let tls = santolvdrive::tls::config(&cert, &key).unwrap();
    let mut app = h.app.clone();
    app.secure = true;
    app.origin = format!("https://localhost:{}", address.port());
    let server = tokio::spawn(async move {
        santolvdrive::tls::serve(socket, tls, router(app), std::future::pending())
            .await
            .unwrap();
    });
    let mut roots = rustls::RootCertStore::empty();
    roots.add(material.cert.der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let client = tokio_rustls::TlsConnector::from(Arc::new(config));
    let response=tokio::time::timeout(std::time::Duration::from_secs(10),async {
        let mut stream=client.connect("localhost".try_into().unwrap(),TcpStream::connect(address).await.unwrap()).await.unwrap();
        let body=json!({"username":user.username,"password":"very strong test password","mobile":true}).to_string();
        let request=format!("POST /api/v1/auth/login HTTP/1.1\r\nHost: localhost\r\nX-Forwarded-For: 203.0.113.50\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        stream.write_all(request.as_bytes()).await.unwrap();stream.flush().await.unwrap();let mut bytes=Vec::new();stream.read_to_end(&mut bytes).await.unwrap();String::from_utf8(bytes).unwrap()
    }).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        response
            .to_ascii_lowercase()
            .contains("strict-transport-security")
    );
    let spoofed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM rate_limits WHERE key='login-ip:203.0.113.50'")
            .fetch_one(&h.app.db)
            .await
            .unwrap();
    assert_eq!(spoofed, 0);
    server.abort();
}

#[sqlx::test]
async fn empty_files_and_competing_version_publications(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("versions").await;
    let empty = h
        .start(&a, "empty", b"", "empty", None, None, "\"1\"")
        .await;
    let (s, v) = h.complete(&a, empty).await;
    assert_eq!(s, StatusCode::OK);
    let empty_node = id(&v, "id");
    let (s, headers, bytes) = h
        .req(
            "GET",
            &format!("/api/v1/nodes/{empty_node}/content"),
            Some(&a.token),
            &[],
            Body::empty(),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(bytes.is_empty());
    assert_eq!(headers["content-length"], "0");
    let original = h
        .start(&a, "same-name", b"abc", "original", None, None, "\"1\"")
        .await;
    assert_eq!(h.chunk(&a, original, 0, b"abc", true).await, StatusCode::OK);
    let (s, v) = h.complete(&a, original).await;
    assert_eq!(s, StatusCode::OK);
    let node = id(&v, "id");
    let first = h
        .start(&a, "same-name", b"111", "first", None, Some(node), "\"1\"")
        .await;
    let second = h
        .start(&a, "same-name", b"222", "second", None, Some(node), "\"1\"")
        .await;
    assert_eq!(h.chunk(&a, first, 0, b"1", true).await, StatusCode::OK);
    // Bytes written before a lost confirmation must be discarded, not counted twice.
    use tokio::io::AsyncWriteExt;
    let mut temp = tokio::fs::OpenOptions::new()
        .append(true)
        .open(storage::temp(&h.app.root, first))
        .await
        .unwrap();
    temp.write_all(b"unconfirmed bytes").await.unwrap();
    temp.flush().await.unwrap();
    drop(temp);
    assert_eq!(h.chunk(&a, first, 1, b"11", true).await, StatusCode::OK);
    assert_eq!(h.chunk(&a, second, 0, b"222", true).await, StatusCode::OK);
    for upload in [first, second] {
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/uploads/{upload}/complete"),
                Some(&a.token),
                &[],
                json!({})
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
    }
    worker::finalize_requested(&h.app).await.unwrap();
    worker::finalize_requested(&h.app).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM uploads WHERE id=$1")
        .bind(first)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(state, "ready");
    let error: String = sqlx::query_scalar("SELECT completion_error FROM uploads WHERE id=$1")
        .bind(second)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(error, "revision_changed");
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{node}/content"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .2,
        b"111"
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM versions WHERE node_id=$1")
        .bind(node)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(
        h.req(
            "DELETE",
            &format!("/api/v1/uploads/{second}"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn organizations_expiration_missing_objects_and_pagination(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("org-owner").await;
    let b = h.account("org-member").await;
    let (s, v) = h
        .json(
            "POST",
            "/api/v1/spaces",
            Some(&a.token),
            &[("idempotency-key", "org")],
            json!({"name":"Organization"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let space = id(&v, "id");
    let org = Account {
        id: a.id,
        space,
        token: a.token.clone(),
        username: a.username.clone(),
    };
    let path = format!("/api/v1/spaces/{space}/members");
    assert_eq!(
        h.json(
            "PUT",
            &path,
            Some(&a.token),
            &[],
            json!({"username":b.username,"role":2})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.json(
            "PUT",
            &path,
            Some(&b.token),
            &[],
            json!({"username":a.username,"role":1})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.req(
            "DELETE",
            &format!("{path}/{}", a.id),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let folder = h.folder(&org, None, "retained", "org-folder").await;
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{folder}"),
            Some(&b.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        h.req(
            "DELETE",
            &format!("{path}/{}", b.id),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{folder}"),
            Some(&b.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{folder}"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
    for n in 0..5 {
        h.folder(&org, None, "same-name", &format!("duplicate-{n}"))
            .await;
    }
    let mut seen = std::collections::HashSet::new();
    let mut after = String::new();
    loop {
        let (s, v) = h
            .json(
                "GET",
                &format!("/api/v1/spaces/{space}/nodes?limit=2{after}"),
                Some(&a.token),
                &[],
                json!(null),
            )
            .await;
        assert_eq!(s, StatusCode::OK);
        for node in v["items"].as_array().unwrap() {
            assert!(seen.insert(id(node, "id")));
        }
        if let Some(next) = v["next_cursor"].as_str() {
            after = format!("&after={next}");
        } else {
            break;
        }
    }
    assert_eq!(seen.len(), 6);
    let expired = h
        .start(&org, "expired", b"abc", "expire", None, None, "\"1\"")
        .await;
    assert_eq!(h.chunk(&org, expired, 0, b"a", true).await, StatusCode::OK);
    sqlx::query("UPDATE uploads SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(expired)
        .execute(&h.app.db)
        .await
        .unwrap();
    let mut cursor = Uuid::nil();
    let mut cancelled = Uuid::nil();
    worker::tick(&h.app, &mut cursor, &mut cancelled)
        .await
        .unwrap();
    assert!(
        !tokio::fs::try_exists(storage::temp(&h.app.root, expired))
            .await
            .unwrap()
    );
    let reserved: i64 = sqlx::query_scalar("SELECT reserved_bytes FROM spaces WHERE id=$1")
        .bind(space)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(reserved, 0);
    let up = h
        .start(&org, "missing", b"abc", "missing", None, None, "\"1\"")
        .await;
    assert_eq!(h.chunk(&org, up, 0, b"abc", true).await, StatusCode::OK);
    let (s, v) = h.complete(&org, up).await;
    assert_eq!(s, StatusCode::OK);
    let file = id(&v, "id");
    storage::remove(&storage::object(&h.app.root, up))
        .await
        .unwrap();
    cursor = Uuid::nil();
    worker::tick(&h.app, &mut cursor, &mut cancelled)
        .await
        .unwrap();
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    tokio::fs::write(storage::object(&h.app.root, up), b"abc")
        .await
        .unwrap();
    cursor = Uuid::nil();
    worker::tick(&h.app, &mut cursor, &mut cancelled)
        .await
        .unwrap();
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .2,
        b"abc"
    );
    // Losing a confirmed temporary block must never extend zeros and pretend it survived.
    let lost = h
        .start(&org, "lost", b"abc", "lost", None, None, "\"1\"")
        .await;
    assert_eq!(h.chunk(&org, lost, 0, b"a", true).await, StatusCode::OK);
    storage::remove(&storage::temp(&h.app.root, lost))
        .await
        .unwrap();
    assert_eq!(
        h.chunk(&org, lost, 1, b"bc", true).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[sqlx::test]
async fn files_permissions_concurrency_and_recovery(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("alice").await;
    let b = h.account("bob").await;
    let folder = h.folder(&a, None, "carpeta", "folder-a").await;
    assert_eq!(folder, h.folder(&a, None, "carpeta", "folder-a").await);
    let (_, v) = h
        .json(
            "POST",
            &format!("/api/v1/spaces/{}/nodes", a.space),
            Some(&a.token),
            &[("idempotency-key", "folder-a")],
            json!({"name":"different"}),
        )
        .await;
    assert_eq!(v["error"]["code"], "idempotency_key_reused");
    let child = h.folder(&a, Some(folder), "child", "child").await;
    let (s, _) = h
        .json(
            "PATCH",
            &format!("/api/v1/nodes/{folder}"),
            Some(&a.token),
            &[("idempotency-key", "cycle"), ("if-match", "\"1\"")],
            json!({"parent_id":child}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let path = format!("/api/v1/nodes/{folder}");
    let (left, right) = tokio::join!(
        h.json(
            "PATCH",
            &path,
            Some(&a.token),
            &[("idempotency-key", "edit-left"), ("if-match", "\"1\"")],
            json!({"name":"left"})
        ),
        h.json(
            "PATCH",
            &path,
            Some(&a.token),
            &[("idempotency-key", "edit-right"), ("if-match", "\"1\"")],
            json!({"name":"right"})
        )
    );
    assert!(
        (left.0 == StatusCode::OK && right.0 == StatusCode::PRECONDITION_FAILED)
            || (right.0 == StatusCode::OK && left.0 == StatusCode::PRECONDITION_FAILED)
    );
    let upload = h
        .start(
            &a,
            "file.txt",
            b"abcdef",
            "upload-1",
            Some(folder),
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&a, upload, 0, b"abc", false).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(h.chunk(&a, upload, 0, b"abc", true).await, StatusCode::OK);
    assert_eq!(
        h.chunk(&a, upload, 0, b"abc", true).await,
        StatusCode::CONFLICT
    );
    assert_eq!(h.complete(&a, upload).await.0, StatusCode::CONFLICT);
    assert_eq!(h.chunk(&a, upload, 3, b"def", true).await, StatusCode::OK);
    // Simulate shutdown after durable rename but before PostgreSQL commit.
    tokio::fs::rename(
        storage::temp(&h.app.root, upload),
        storage::object(&h.app.root, upload),
    )
    .await
    .unwrap();
    let mut cursor = Uuid::nil();
    let mut cancelled = Uuid::nil();
    worker::tick(&h.app, &mut cursor, &mut cancelled)
        .await
        .unwrap();
    let (s, v) = h.complete(&a, upload).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let file = id(&v, "id");
    assert_eq!(h.complete(&a, upload).await.1["id"], v["id"]);
    let (s, _, bytes) = h
        .req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&a.token),
            &[("range", "bytes=1-3")],
            Body::empty(),
        )
        .await;
    assert_eq!(s, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes, b"bcd");
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&b.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (s, v) = h
        .json(
            "PUT",
            &format!("/api/v1/nodes/{folder}/grants"),
            Some(&a.token),
            &[],
            json!({"user_id":b.id,"role":1}),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&b.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
    let (s, v) = h
        .json("GET", "/api/v1/shared", Some(&b.token), &[], json!(null))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    let (s, _) = h
        .json(
            "POST",
            &format!("/api/v1/nodes/{folder}/grants/revoke"),
            Some(&a.token),
            &[],
            json!({"user_id":b.id}),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&b.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, old) = h
        .json(
            "GET",
            &format!("/api/v1/nodes/{file}/versions"),
            Some(&a.token),
            &[],
            json!(null),
        )
        .await;
    let old_version = id(&old["items"][0], "id");
    let upload2 = h
        .start(
            &a,
            "file.txt",
            b"new",
            "upload-2",
            None,
            Some(file),
            "\"1\"",
        )
        .await;
    assert_eq!(h.chunk(&a, upload2, 0, b"new", true).await, StatusCode::OK);
    assert_eq!(h.complete(&a, upload2).await.0, StatusCode::OK);
    let (s, _) = h
        .json(
            "POST",
            &format!("/api/v1/nodes/{file}/versions/{old_version}/restore"),
            Some(&a.token),
            &[("idempotency-key", "restore-old"), ("if-match", "\"2\"")],
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .2,
        b"abcdef"
    );
    let (s, _) = h
        .json(
            "POST",
            &format!("/api/v1/nodes/{folder}/trash"),
            Some(&a.token),
            &[("idempotency-key", "trash"), ("if-match", "\"2\"")],
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (s, _) = h
        .json(
            "POST",
            &format!("/api/v1/nodes/{folder}/restore"),
            Some(&a.token),
            &[("idempotency-key", "restore"), ("if-match", "\"3\"")],
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{file}/content"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
    let (s, _) = h
        .json(
            "POST",
            &format!("/api/v1/nodes/{folder}/trash"),
            Some(&a.token),
            &[("idempotency-key", "trash-again"), ("if-match", "\"4\"")],
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    sqlx::query("UPDATE nodes SET trashed_at=now()-interval '31 days' WHERE id=$1")
        .bind(folder)
        .execute(&h.app.db)
        .await
        .unwrap();
    let (s, v) = h
        .json(
            "POST",
            &format!("/api/v1/nodes/{folder}/purge"),
            Some(&a.token),
            &[("if-match", "\"5\"")],
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{folder}/restore"),
            Some(&a.token),
            &[
                ("idempotency-key", "restore-deleting"),
                ("if-match", "\"5\"")
            ],
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for _ in 0..4 {
        worker::tick(&h.app, &mut cursor, &mut cancelled)
            .await
            .unwrap();
    }
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{folder}"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE objects SET created_at=now()-interval '2 days'")
        .execute(&h.app.db)
        .await
        .unwrap();
    worker::tick(&h.app, &mut cursor, &mut cancelled)
        .await
        .unwrap();
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM spaces WHERE id=$1")
        .bind(a.space)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

#[sqlx::test]
async fn authentication_csrf_recovery_and_quota(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("security").await;
    let (s, headers, bytes) = h
        .req(
            "POST",
            "/api/v1/auth/login",
            None,
            &[
                ("content-type", "application/json"),
                ("origin", "http://localhost:8080"),
            ],
            Body::from(
                json!({"username":a.username,"password":"very strong test password"}).to_string(),
            ),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let csrf = body["csrf_token"].as_str().unwrap();
    assert_eq!(
        h.req(
            "POST",
            "/api/v1/auth/logout",
            None,
            &[("cookie", cookie)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.req(
            "POST",
            "/api/v1/auth/logout",
            None,
            &[("cookie", cookie), ("x-csrf-token", csrf)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            Some(&a.token),
            &[("origin", "https://attacker.example")],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let up = h
        .start(&a, "cancel", b"abc", "cancel", None, None, "\"1\"")
        .await;
    assert_eq!(
        h.req(
            "DELETE",
            &format!("/api/v1/uploads/{up}"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.req(
            "DELETE",
            &format!("/api/v1/uploads/{up}"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let reserved: i64 = sqlx::query_scalar("SELECT reserved_bytes FROM spaces WHERE id=$1")
        .bind(a.space)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(reserved, 0);
    sqlx::query("UPDATE spaces SET quota=1 WHERE id=$1")
        .bind(a.space)
        .execute(&h.app.db)
        .await
        .unwrap();
    let (s, v) = h
        .json(
            "POST",
            "/api/v1/uploads",
            Some(&a.token),
            &[("idempotency-key", "too-large")],
            json!({"space_id":a.space,"name":"big","size":3,"sha256":"0".repeat(64)}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "quota_exceeded");
    // Only the server administrator can reset credentials; no public recovery exists.
    let path = format!("/api/v1/admin/users/{}/password", a.id);
    assert_eq!(
        h.json(
            "POST",
            &path,
            Some(&a.token),
            &[],
            json!({"password":"another strong password"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.json(
            "POST",
            &path,
            Some(&h.admin),
            &[],
            json!({"password":"another strong password"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.req("GET", "/api/v1/me", Some(&a.token), &[], Body::empty())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (s, v) = h
        .json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            json!({"username":a.username,"password":"another strong password","mobile":true}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["must_change_password"], true);
    let temporary = v["token"].as_str().unwrap();
    assert_eq!(
        h.req("GET", "/api/v1/spaces", Some(temporary), &[], Body::empty())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(h.json("POST","/api/v1/auth/password",Some(temporary),&[],json!({"current_password":"another strong password","new_password":"new permanent strong password"})).await.0,StatusCode::NO_CONTENT);
    for path in [
        "/api/v1/auth/register",
        "/api/v1/auth/recovery",
        "/api/v1/auth/reset",
        "/api/v1/auth/verify",
    ] {
        assert_eq!(
            h.json("POST", path, None, &[], json!({})).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[sqlx::test]
async fn server_admin_is_management_only(pool: PgPool) {
    let h = Harness::new(pool).await;
    let user = h.account("ordinary").await;
    let no_space:i64=sqlx::query_scalar("SELECT count(*) FROM spaces s JOIN users u ON u.id=s.owner_id WHERE u.account_type='admin'").fetch_one(&h.app.db).await.unwrap();
    assert_eq!(no_space, 0);
    for path in ["/api/v1/spaces", "/api/v1/shared"] {
        assert_eq!(
            h.req("GET", path, Some(&h.admin), &[], Body::empty())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let folder = h.folder(&user, None, "private", "private").await;
    let pending = h
        .start(
            &user,
            "pending",
            b"abc",
            "disable-pending",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&user, pending, 0, b"abc", true).await,
        StatusCode::OK
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/uploads/{pending}/complete"),
            Some(&user.token),
            &[],
            json!({})
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{folder}"),
            Some(&h.admin),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/admin/users",
            Some(&user.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (s, v) = h
        .json(
            "GET",
            "/api/v1/admin/users",
            Some(&h.admin),
            &[],
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert!(v["items"][0].get("password_hash").is_none());
    assert_eq!(
        h.json(
            "PATCH",
            &format!("/api/v1/admin/users/{}", user.id),
            Some(&h.admin),
            &[],
            json!({"disabled":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.req("GET", "/api/v1/me", Some(&user.token), &[], Body::empty())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    worker::finalize_requested(&h.app).await.unwrap();
    let error: String =
        sqlx::query_scalar("SELECT completion_error FROM uploads WHERE id=$1 AND state='pending'")
            .bind(pending)
            .fetch_one(&h.app.db)
            .await
            .unwrap();
    assert_eq!(error, "not_found");
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            json!({"username":user.username,"password":"very strong test password","mobile":true})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.json(
            "PATCH",
            &format!("/api/v1/admin/users/{}", user.id),
            Some(&h.admin),
            &[],
            json!({"disabled":false})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.req("GET", "/api/v1/me", Some(&user.token), &[], Body::empty())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE account_type='admin'")
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(
        h.json(
            "PATCH",
            &format!("/api/v1/admin/users/{admin_id}"),
            Some(&h.admin),
            &[],
            json!({"disabled":true})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        santolvdrive::admin::bootstrap(
            &h.app,
            santolvdrive::admin::Credentials {
                username: "second-admin".into(),
                password: "second admin password".into()
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/admin/audit",
            Some(&h.admin),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = false)]
async fn migration_keeps_existing_accounts_and_content(pool: PgPool) {
    sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let user = Uuid::new_v4();
    let space = Uuid::new_v4();
    let node = Uuid::new_v4();
    let object = Uuid::new_v4();
    let version = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,'legacy@example.com','original-password-hash',true)").bind(user).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO spaces(id,name,kind,owner_id,quota,used_bytes) VALUES($1,'Personal','personal',$2,1024,3)").bind(space).bind(user).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO members VALUES($1,$2,3)")
        .bind(space)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO nodes(id,space_id,name,kind) VALUES($1,$2,'existing','file')")
        .bind(node)
        .bind(space)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO objects(id,space_id,size,sha256) VALUES($1,$2,3,$3)")
        .bind(object)
        .bind(space)
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO versions(id,node_id,object_id,created_by) VALUES($1,$2,$3,$4)")
        .bind(version)
        .bind(node)
        .bind(object)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE nodes SET current_version=$1 WHERE id=$2")
        .bind(version)
        .bind(node)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions VALUES('old-session',$1,'old-csrf',now()+interval '1 day')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0002_server_administration.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let actual: (Uuid, String, String, String) =
        sqlx::query_as("SELECT id,username,password_hash,account_type FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        actual,
        (
            user,
            "legacy@example.com".into(),
            "original-password-hash".into(),
            "user".into()
        )
    );
    let retained:(Uuid,Uuid,Uuid,i64)=sqlx::query_as("SELECT n.id,n.current_version,v.object_id,o.size FROM nodes n JOIN versions v ON v.id=n.current_version JOIN objects o ON o.id=v.object_id WHERE n.id=$1").bind(node).fetch_one(&pool).await.unwrap();
    assert_eq!(retained, (node, version, object, 3));
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    let old_tables: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.mail_jobs')::text")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(old_tables.is_none());
}
