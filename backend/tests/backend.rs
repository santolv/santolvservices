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
    admin_id: Uuid,
}
struct Account {
    id: Uuid,
    space: Uuid,
    token: String,
    username: String,
}
impl Harness {
    async fn directory(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT s.storage_directory FROM spaces s WHERE s.id=(SELECT space_id FROM uploads WHERE id=$1 UNION SELECT space_id FROM objects WHERE id=$1 LIMIT 1)")
            .bind(id).fetch_one(&self.app.db).await.unwrap()
    }
    async fn original_name(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT name FROM uploads WHERE id=$1 UNION ALL SELECT original_name FROM objects WHERE id=$1 LIMIT 1")
            .bind(id).fetch_one(&self.app.db).await.unwrap()
    }
    async fn new(db: PgPool) -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
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
            frontend_origin: santolvdrive::web::default_frontend_origin(),
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
            admin_id,
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
        santolvdrive::admin::create(
            &self.app,
            self.admin_id,
            santolvdrive::admin::Credentials {
                username: username.clone(),
                password: "temporary test password".into(),
            },
        )
        .await
        .unwrap();
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

async fn current_object(
    h: &Harness,
    _root: &std::path::Path,
    directory: &str,
    object: Uuid,
    name: &str,
) -> std::path::PathBuf {
    santolvdrive::workspace::content_path(&h.app, directory, object, name)
        .await
        .unwrap()
}

#[sqlx::test]
async fn independent_frontend_cors_keeps_cookie_authentication_and_csrf(pool: PgPool) {
    let h = Harness::new(pool).await;
    let user = h.account("independent-client").await;
    let frontend = "http://localhost:5500";
    let (status, headers, _) = h
        .req(
            "OPTIONS",
            "/api/v1/auth/login",
            None,
            &[
                ("origin", frontend),
                ("access-control-request-method", "POST"),
                (
                    "access-control-request-headers",
                    "content-type,x-csrf-token,idempotency-key",
                ),
            ],
            Body::empty(),
        )
        .await;
    assert!(status.is_success());
    assert_eq!(headers["access-control-allow-origin"], frontend);
    assert_eq!(headers["access-control-allow-credentials"], "true");
    let (status, headers, bytes) = h
        .req(
            "POST",
            "/api/v1/auth/login",
            None,
            &[
                ("origin", frontend),
                ("sec-fetch-site", "same-site"),
                ("content-type", "application/json"),
            ],
            Body::from(
                json!({"username":user.username,"password":"very strong test password"})
                    .to_string(),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["access-control-allow-origin"], frontend);
    let cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    let csrf = result["csrf_token"].as_str().unwrap();
    let (status, _, _) = h
        .req(
            "GET",
            "/api/v1/me",
            None,
            &[("origin", frontend), ("cookie", &cookie)],
            Body::empty(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let key = Uuid::new_v4().to_string();
    let (status, _) = h
        .json(
            "POST",
            "/api/v1/spaces",
            None,
            &[
                ("origin", frontend),
                ("cookie", &cookie),
                ("idempotency-key", &key),
            ],
            json!({"name":"must-not-create"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = h
        .json(
            "POST",
            "/api/v1/spaces",
            None,
            &[
                ("origin", frontend),
                ("cookie", &cookie),
                ("x-csrf-token", csrf),
                ("idempotency-key", &key),
                ("sec-fetch-site", "same-site"),
            ],
            json!({"name":"Frontend separado"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    for origin in [
        "http://localhost:5501",
        "http://evil.example",
        "http://localhost.evil.example:5500",
    ] {
        let (_, headers, _) = h
            .req(
                "OPTIONS",
                "/api/v1/auth/login",
                None,
                &[
                    ("origin", origin),
                    ("access-control-request-method", "POST"),
                ],
                Body::empty(),
            )
            .await;
        assert!(!headers.contains_key("access-control-allow-origin"));
        let (status, headers, _) = h
            .req(
                "GET",
                "/api/v1/me",
                None,
                &[("origin", origin), ("cookie", &cookie)],
                Body::empty(),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!headers.contains_key("access-control-allow-origin"));
    }
}

#[sqlx::test]
async fn local_http_login_needs_only_drive_credentials(pool: PgPool) {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    let h = Harness::new(pool).await;
    h.account("local-http").await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut app = h.app.clone();
    app.origin = format!("http://{addr}");
    let server = tokio::spawn(santolvdrive::tls::serve_http(
        listener,
        router(app),
        std::future::pending(),
    ));
    for (method, path, origin, body, expected) in [
        ("GET", "/", format!("http://{addr}"), "", "HTTP/1.1 200"),
        (
            "GET",
            "/api/v1/me",
            format!("http://{addr}"),
            "",
            "HTTP/1.1 401",
        ),
        (
            "POST",
            "/api/v1/auth/login",
            format!("http://{addr}"),
            r#"{"username":"local-http","password":"very strong test password"}"#,
            "HTTP/1.1 200",
        ),
        (
            "POST",
            "/api/v1/auth/login",
            "http://attacker.example".into(),
            r#"{"username":"local-http","password":"very strong test password"}"#,
            "HTTP/1.1 403",
        ),
    ] {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            stream.read_to_end(&mut bytes),
        )
        .await
        .unwrap()
        .unwrap();
        let response = String::from_utf8(bytes).unwrap();
        assert!(response.starts_with(expected), "{response}");
        if path == "/api/v1/auth/login" && expected == "HTTP/1.1 200" {
            assert!(response.contains("drive_session="));
            assert!(response.contains("HttpOnly"));
            assert!(response.contains("csrf_token"));
            assert!(!response.contains("; Secure"));
        }
    }
    server.abort();
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
        .open(storage::temp(&h.app.root, &h.directory(first).await, first))
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
    assert_eq!(n, 1);
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
        h.folder(
            &org,
            None,
            &format!("same-name-{n}"),
            &format!("duplicate-{n}"),
        )
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
        !tokio::fs::try_exists(storage::temp(
            &h.app.root,
            &h.directory(expired).await,
            expired
        ))
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
    storage::remove(
        &current_object(
            &h,
            &h.app.root,
            &h.directory(up).await,
            up,
            &h.original_name(up).await,
        )
        .await,
    )
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
    tokio::fs::write(
        current_object(
            &h,
            &h.app.root,
            &h.directory(up).await,
            up,
            &h.original_name(up).await,
        )
        .await,
        b"abc",
    )
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
    storage::remove(&storage::temp(&h.app.root, &h.directory(lost).await, lost))
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
    storage::prepare_object(
        &h.app.root,
        &h.directory(upload).await,
        upload,
        &h.original_name(upload).await,
    )
    .await
    .unwrap();
    tokio::fs::rename(
        storage::temp(&h.app.root, &h.directory(upload).await, upload),
        current_object(
            &h,
            &h.app.root,
            &h.directory(upload).await,
            upload,
            &h.original_name(upload).await,
        )
        .await,
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
    let old_version: Uuid = sqlx::query_scalar("SELECT current_version FROM nodes WHERE id=$1")
        .bind(file)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
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
    assert_eq!(s, StatusCode::NOT_FOUND);
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
        b"new"
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
    // Password resets are available only through the server console.
    santolvdrive::admin::reset_password(&h.app, h.admin_id, a.id, "another strong password".into())
        .await
        .unwrap();
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
async fn server_admin_is_console_only(pool: PgPool) {
    let h = Harness::new(pool).await;
    let user = h.account("ordinary").await;
    let no_space:i64=sqlx::query_scalar("SELECT count(*) FROM spaces s JOIN users u ON u.id=s.owner_id WHERE u.account_type='admin'").fetch_one(&h.app.db).await.unwrap();
    assert_eq!(no_space, 0);
    for headers in [vec![], vec![("origin", "null")]] {
        let (_, body)=h.json("POST","/api/v1/auth/login",None,&headers,
            json!({"username":"director","password":"administrator test password","mobile":headers.is_empty(),"local_file":!headers.is_empty()})).await;
        assert_eq!(body["error"]["code"], "authentication_required");
        assert!(body.get("token").is_none());
    }
    for path in ["/api/v1/me", "/api/v1/spaces", "/api/v1/shared"] {
        assert_eq!(
            h.req("GET", path, Some(&h.admin), &[], Body::empty())
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    for (method, path) in [
        ("GET", "/api/v1/admin/users"),
        ("POST", "/api/v1/admin/users"),
        (
            "PATCH",
            "/api/v1/admin/users/00000000-0000-0000-0000-000000000000",
        ),
        (
            "POST",
            "/api/v1/admin/users/00000000-0000-0000-0000-000000000000/password",
        ),
        ("GET", "/api/v1/admin/audit"),
    ] {
        assert_eq!(
            h.json(method, path, Some(&h.admin), &[], json!({})).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            h.json(method, path, Some(&user.token), &[], json!({}))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let listed = santolvdrive::admin::list(&h.app, None).await.unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert!(listed["items"][0].get("password_hash").is_none());
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
    santolvdrive::admin::console(&h.app, &["disable".into(), user.username.clone()])
        .await
        .unwrap();
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
    santolvdrive::admin::console(&h.app, &["enable".into(), user.username.clone()])
        .await
        .unwrap();
    assert_eq!(
        h.req("GET", "/api/v1/me", Some(&user.token), &[], Body::empty())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        santolvdrive::admin::set_disabled(&h.app, h.admin_id, h.admin_id, true)
            .await
            .is_err()
    );
    assert!(
        santolvdrive::admin::bootstrap(
            &h.app,
            santolvdrive::admin::Credentials {
                username: "second-admin".into(),
                password: "second administrator password".into()
            }
        )
        .await
        .is_err()
    );
    assert!(
        santolvdrive::admin::console(&h.app, &["disable".into(), "director".into()])
            .await
            .is_err()
    );
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE id=$1")
        .bind(folder)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(kept, 1);
    let audit = santolvdrive::admin::audit_log(&h.app, None).await.unwrap();
    assert!(audit["items"].as_array().unwrap().len() >= 4);
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
    let admin = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,password_hash,account_type) VALUES($1,'server-migration','kept-admin-hash','admin')")
        .bind(admin).execute(&pool).await.unwrap();
    for (token, account) in [
        ("existing-user-session", user),
        ("existing-admin-session", admin),
    ] {
        sqlx::query("INSERT INTO sessions VALUES($1,$2,'csrf',now()+interval '1 hour')")
            .bind(token)
            .bind(account)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::raw_sql(include_str!(
        "../migrations/0003_server_only_administrator.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let retained_sessions: Vec<String> =
        sqlx::query_scalar("SELECT token_hash FROM sessions ORDER BY token_hash")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(retained_sessions, vec!["existing-user-session"]);
    let kept: (Uuid, Uuid) = sqlx::query_as("SELECT id,current_version FROM nodes WHERE id=$1")
        .bind(node)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, (node, version));
    assert!(sqlx::query("INSERT INTO users(id,username,password_hash,account_type) VALUES($1,'second-admin','hash','admin')").bind(Uuid::new_v4()).execute(&pool).await.is_err());
    sqlx::raw_sql(include_str!(
        "../migrations/0004_user_storage_directories.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let directory: String = sqlx::query_scalar("SELECT storage_directory FROM spaces WHERE id=$1")
        .bind(space)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        directory,
        storage::user_directory("legacy@example.com", user)
    );
    assert!(
        sqlx::query("UPDATE spaces SET storage_directory='../outside' WHERE id=$1")
            .bind(space)
            .execute(&pool)
            .await
            .is_err()
    );
    let unchanged: (Uuid, Uuid) =
        sqlx::query_as("SELECT id,current_version FROM nodes WHERE id=$1")
            .bind(node)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unchanged, (node, version));
    sqlx::raw_sql(include_str!(
        "../migrations/0005_original_content_names.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let original_name: String = sqlx::query_scalar("SELECT original_name FROM objects WHERE id=$1")
        .bind(object)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(original_name, "existing");
}

#[sqlx::test]
async fn standalone_file_uses_tokens_and_never_cookie_sessions(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("file-client").await;
    let file_headers = [("origin", "null"), ("sec-fetch-site", "cross-site")];
    let (status, headers, bytes) = h.req("POST", "/api/v1/auth/login", None,
        &[("origin", "null"), ("sec-fetch-site", "cross-site"), ("content-type", "application/json")],
        Body::from(json!({"username":a.username,"password":"very strong test password","local_file":true}).to_string())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!headers.contains_key("set-cookie"));
    assert!(!headers.contains_key("access-control-allow-credentials"));
    let login: Value = serde_json::from_slice(&bytes).unwrap();
    let token = login["token"].as_str().unwrap();
    assert_eq!(login["expires_in"], 1800);
    assert_eq!(
        h.req("GET", "/api/v1/me", None, &file_headers, Body::empty())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            Some(token),
            &file_headers,
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
    let cookie = format!("drive_session={token}");
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            Some(token),
            &[("origin", "null"), ("cookie", &cookie)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, _) = h
        .json(
            "POST",
            &format!("/api/v1/spaces/{}/nodes", a.space),
            Some(token),
            &[("origin", "null"), ("idempotency-key", "file-folder")],
            json!({"name":"Local folder"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        h.req(
            "POST",
            "/api/v1/auth/logout",
            Some(token),
            &file_headers,
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
            Some(token),
            &file_headers,
            Body::empty()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn user_storage_isolated_shared_edits_and_legacy_content_survive(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("alice").await;
    let b = h.account("bob").await;
    let directory = format!("usuarios/{}", a.username);
    assert!(h.app.root.join(&directory).join("archivos").is_dir());
    assert!(!h.app.root.join("usuarios/bob/.uploads").exists());
    assert!(!h.app.root.join("usuarios/director").exists());
    let data = b"custom\0binary\xff\r\n";
    let upload = h
        .start(
            &a,
            "custom.data",
            data,
            "custom-content",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(h.chunk(&a, upload, 0, data, true).await, StatusCode::OK);
    let (status, node) = h.complete(&a, upload).await;
    assert_eq!(status, StatusCode::OK);
    let node = id(&node, "id");
    let path = current_object(
        &h,
        &h.app.root,
        &directory,
        upload,
        &h.original_name(upload).await,
    )
    .await;
    assert_eq!(tokio::fs::read(&path).await.unwrap(), data);
    assert!(
        !current_object(
            &h,
            &h.app.root,
            "usuarios/bob",
            upload,
            &h.original_name(upload).await
        )
        .await
        .exists()
    );
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{node}/content"),
            Some(&b.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    tokio::fs::rename(&path, h.app.root.join("objects").join(upload.to_string()))
        .await
        .unwrap();
    let downloaded = h
        .req(
            "GET",
            &format!("/api/v1/nodes/{node}/content"),
            Some(&a.token),
            &[],
            Body::empty(),
        )
        .await;
    assert_eq!(downloaded.0, StatusCode::OK);
    assert_eq!(downloaded.2, data);
    santolvdrive::workspace::sync_node(&h.app, node)
        .await
        .unwrap();
    assert!(path.exists());
    assert!(!h.app.root.join("objects").join(upload.to_string()).exists());
    assert_eq!(
        h.json(
            "PUT",
            &format!("/api/v1/nodes/{node}/grants"),
            Some(&a.token),
            &[],
            json!({"user_id":b.id,"role":2})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let editor = Account {
        id: b.id,
        space: a.space,
        token: b.token.clone(),
        username: b.username.clone(),
    };
    let revision = h
        .start(
            &editor,
            "custom.data",
            b"updated",
            "shared-version",
            None,
            Some(node),
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&editor, revision, 0, b"updated", true).await,
        StatusCode::OK
    );
    assert_eq!(h.complete(&editor, revision).await.0, StatusCode::OK);
    assert!(
        current_object(
            &h,
            &h.app.root,
            &directory,
            revision,
            &h.original_name(revision).await
        )
        .await
        .exists()
    );
    assert!(
        !current_object(
            &h,
            &h.app.root,
            "usuarios/bob",
            revision,
            &h.original_name(revision).await
        )
        .await
        .exists()
    );
    let pending = h
        .start(
            &a,
            "resume.bin",
            b"abc",
            "legacy-pending",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(h.chunk(&a, pending, 0, b"a", true).await, StatusCode::OK);
    tokio::fs::rename(
        storage::temp(&h.app.root, &directory, pending),
        h.app.root.join("uploads").join(pending.to_string()),
    )
    .await
    .unwrap();
    assert_eq!(h.chunk(&a, pending, 1, b"bc", true).await, StatusCode::OK);
    assert_eq!(h.complete(&a, pending).await.0, StatusCode::OK);
    assert_eq!(
        tokio::fs::read(
            current_object(
                &h,
                &h.app.root,
                &directory,
                pending,
                &h.original_name(pending).await
            )
            .await
        )
        .await
        .unwrap(),
        b"abc"
    );
    let (_, spaces) = h
        .json("GET", "/api/v1/spaces", Some(&a.token), &[], json!(null))
        .await;
    assert!(spaces["items"][0].get("storage_directory").is_none());
}

#[sqlx::test]
async fn permanent_delete_removes_named_content_only_after_last_reference(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("delete-owner").await;
    let upload = h
        .start(
            &a,
            "original.txt",
            b"original content",
            "named-delete",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&a, upload, 0, b"original content", true).await,
        StatusCode::OK
    );
    let (_, created) = h.complete(&a, upload).await;
    let first = id(&created, "id");
    let second = Uuid::new_v4();
    let version = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes(id,space_id,name,kind) VALUES($1,$2,'second.txt','file')")
        .bind(second)
        .bind(a.space)
        .execute(&h.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO versions(id,node_id,object_id,created_by) VALUES($1,$2,$3,$4)")
        .bind(version)
        .bind(second)
        .bind(upload)
        .bind(a.id)
        .execute(&h.app.db)
        .await
        .unwrap();
    sqlx::query("UPDATE nodes SET current_version=$1 WHERE id=$2")
        .bind(version)
        .bind(second)
        .execute(&h.app.db)
        .await
        .unwrap();
    let path = current_object(
        &h,
        &h.app.root,
        &h.directory(upload).await,
        upload,
        "original.txt",
    )
    .await;
    let mut cursor = Uuid::nil();
    let mut cancelled = Uuid::nil();
    for node in [first, second] {
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/nodes/{node}/purge"),
                Some(&a.token),
                &[("if-match", "\"1\"")],
                json!({})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/nodes/{node}/trash"),
                Some(&a.token),
                &[
                    ("if-match", "\"1\""),
                    ("idempotency-key", &node.to_string())
                ],
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/nodes/{node}/purge"),
                Some(&a.token),
                &[("if-match", "\"2\"")],
                json!({})
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
        worker::tick(&h.app, &mut cursor, &mut cancelled)
            .await
            .unwrap();
        if node == first {
            assert!(
                current_object(
                    &h,
                    &h.app.root,
                    &h.directory(upload).await,
                    upload,
                    "original.txt"
                )
                .await
                .exists()
            );
            let download = h
                .req(
                    "GET",
                    &format!("/api/v1/nodes/{second}/content"),
                    Some(&a.token),
                    &[],
                    Body::empty(),
                )
                .await;
            assert_eq!(download.0, StatusCode::OK);
            assert_eq!(download.2, b"original content");
        }
    }
    assert!(!path.exists());
    assert!(!path.parent().unwrap().exists());
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM spaces WHERE id=$1")
        .bind(a.space)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

#[sqlx::test]
async fn named_workspace_folders_files_rename_move_and_restore(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("named-owner").await;
    let root = h.app.root.join("usuarios/named-owner/archivos");
    let work = h.folder(&a, None, "Trabajo", "work").await;
    let child = h.folder(&a, Some(work), "Proyectos", "child").await;
    assert!(root.join("Trabajo/Proyectos").is_dir());
    let upload = h
        .start(
            &a,
            "informe.pdf",
            b"original",
            "wrapped-file",
            Some(child),
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&a, upload, 0, b"original", true).await,
        StatusCode::OK
    );
    let (status, created) = h.complete(&a, upload).await;
    assert_eq!(status, StatusCode::OK, "{created}");

    let file = id(&created, "id");
    let path = root.join("Trabajo/Proyectos/informe.pdf/informe.pdf");
    assert_eq!(tokio::fs::read(&path).await.unwrap(), b"original");
    tokio::fs::write(
        path.parent().unwrap().join("analisis.json"),
        b"{\"keep\":true}",
    )
    .await
    .unwrap();
    let edit = |name: &str| json!({"name":name});
    let (status, result) = h
        .json(
            "PATCH",
            &format!("/api/v1/nodes/{child}"),
            Some(&a.token),
            &[("if-match", "\"1\""), ("idempotency-key", "rename-child")],
            edit("Proyectos 2026"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["storage_pending"], false);
    assert!(
        root.join("Trabajo/Proyectos 2026/informe.pdf/analisis.json")
            .exists()
    );
    assert!(!path.exists());
    let (status, result) = h
        .json(
            "PATCH",
            &format!("/api/v1/nodes/{file}"),
            Some(&a.token),
            &[("if-match", "\"1\""), ("idempotency-key", "rename-file")],
            edit("informe final.pdf"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["storage_pending"], false);
    let renamed = root.join("Trabajo/Proyectos 2026/informe final.pdf/informe final.pdf");
    assert_eq!(tokio::fs::read(&renamed).await.unwrap(), b"original");
    assert!(!renamed.parent().unwrap().join("informe.pdf").exists());
    let destination = h.folder(&a, None, "Destino", "destination").await;
    let (status, result) = h
        .json(
            "PATCH",
            &format!("/api/v1/nodes/{file}"),
            Some(&a.token),
            &[("if-match", "\"2\""), ("idempotency-key", "move-file")],
            json!({"parent_id":destination}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["storage_pending"], false);
    let moved = root.join("Destino/informe final.pdf/informe final.pdf");
    assert!(moved.exists());
    assert!(moved.parent().unwrap().join("analisis.json").exists());
    assert!(!renamed.exists());
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{work}/trash"),
            Some(&a.token),
            &[("if-match", "\"1\""), ("idempotency-key", "trash-work")],
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(!root.join("Trabajo").exists());
    assert!(
        h.app
            .root
            .join("usuarios/named-owner/papelera/Trabajo/Proyectos 2026")
            .exists()
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{work}/restore"),
            Some(&a.token),
            &[("if-match", "\"2\""), ("idempotency-key", "restore-work")],
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(root.join("Trabajo/Proyectos 2026").exists());
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/spaces/{}/nodes", a.space),
            Some(&a.token),
            &[("idempotency-key", "duplicate-name")],
            json!({"name":"destino"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{file}/trash"),
            Some(&a.token),
            &[("if-match", "\"3\""), ("idempotency-key", "trash-file")],
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    let blocker = h
        .folder(&a, Some(destination), "informe final.pdf", "blocker")
        .await;
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{file}/restore"),
            Some(&a.token),
            &[
                ("if-match", "\"4\""),
                ("idempotency-key", "restore-blocked")
            ],
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.json(
            "PATCH",
            &format!("/api/v1/nodes/{blocker}"),
            Some(&a.token),
            &[("if-match", "\"1\""), ("idempotency-key", "move-blocker")],
            edit("Otra carpeta")
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{file}/restore"),
            Some(&a.token),
            &[("if-match", "\"4\""), ("idempotency-key", "restore-file")],
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(tokio::fs::read(&moved).await.unwrap(), b"original");
    // Simulate a crash after moving the wrapper, before its entry and content name commit.
    let recovered = root.join("Destino/recuperado.pdf");
    sqlx::query("INSERT INTO workspace_moves(node_id,source,target) VALUES($1,$2,$3)")
        .bind(file)
        .bind(moved.parent().unwrap().to_string_lossy().to_string())
        .bind(recovered.to_string_lossy().to_string())
        .execute(&h.app.db)
        .await
        .unwrap();
    tokio::fs::rename(moved.parent().unwrap(), &recovered)
        .await
        .unwrap();
    let mut tx = h.app.db.begin().await.unwrap();
    sqlx::query("UPDATE nodes SET name='recuperado.pdf',revision=revision+1 WHERE id=$1")
        .bind(file)
        .execute(&mut *tx)
        .await
        .unwrap();
    santolvdrive::workspace::schedule(&mut tx, file, a.space)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    santolvdrive::workspace::sync_node(&h.app, file)
        .await
        .unwrap();
    santolvdrive::workspace::sync_node(&h.app, file)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(recovered.join("recuperado.pdf"))
            .await
            .unwrap(),
        b"original"
    );
    assert!(recovered.join("analisis.json").exists());
    assert!(!recovered.join("informe final.pdf").exists());
    let (status, result) = h
        .json(
            "PATCH",
            &format!("/api/v1/nodes/{file}"),
            Some(&a.token),
            &[("if-match", "\"6\""), ("idempotency-key", "case-rename")],
            edit("RECUPERADO.PDF"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["storage_pending"], false);
    let uppercase = root.join("Destino/RECUPERADO.PDF");
    let mut names = tokio::fs::read_dir(&uppercase).await.unwrap();
    let mut exact = false;
    while let Some(entry) = names.next_entry().await.unwrap() {
        if entry.file_name() == "RECUPERADO.PDF" {
            exact = true
        }
    }
    assert!(exact);
    let staging = h.app.root.join(format!(".temporal/movimientos/{file}"));
    sqlx::query("INSERT INTO workspace_moves(node_id,source,target) VALUES($1,$2,$3)")
        .bind(file)
        .bind(uppercase.to_string_lossy().to_string())
        .bind(
            root.join("Destino/reanudado.pdf")
                .to_string_lossy()
                .to_string(),
        )
        .execute(&h.app.db)
        .await
        .unwrap();
    tokio::fs::rename(&uppercase, &staging).await.unwrap();
    let mut tx = h.app.db.begin().await.unwrap();
    sqlx::query("UPDATE nodes SET name='reanudado.pdf',revision=revision+1 WHERE id=$1")
        .bind(file)
        .execute(&mut *tx)
        .await
        .unwrap();
    santolvdrive::workspace::schedule(&mut tx, file, a.space)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    santolvdrive::workspace::sync_node(&h.app, file)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(root.join("Destino/reanudado.pdf/reanudado.pdf"))
            .await
            .unwrap(),
        b"original"
    );
    assert!(root.join("Destino/reanudado.pdf/analisis.json").exists());
    let next = h
        .start(
            &a,
            "reanudado.pdf",
            b"new version",
            "named-new-version",
            Some(destination),
            Some(file),
            "\"8\"",
        )
        .await;
    assert_eq!(
        h.chunk(&a, next, 0, b"new version", true).await,
        StatusCode::OK
    );
    assert_eq!(h.complete(&a, next).await.0, StatusCode::OK);
    let current = root.join("Destino/reanudado.pdf/reanudado.pdf");
    assert_eq!(tokio::fs::read(&current).await.unwrap(), b"new version");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM versions WHERE node_id=$1")
        .bind(file)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(!h.app.root.join("usuarios/named-owner/.versions").exists());
    assert!(!current.parent().unwrap().join(".drive-node").exists());
    assert!(current.parent().unwrap().join("analisis.json").exists());
    let long = "Carpeta".repeat(13);
    let one = h.folder(&a, None, &long, "long-one").await;
    let two = h.folder(&a, Some(one), &long, "long-two").await;
    h.folder(&a, Some(two), &long, "long-three").await;
    assert!(root.join(&long).join(&long).join(&long).is_dir());
}

#[sqlx::test]
async fn named_workspace_does_not_overwrite_unmanaged_directories(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("collision-owner").await;
    let root = h.app.root.join("usuarios/collision-owner/archivos");
    tokio::fs::create_dir(root.join("Ocupada")).await.unwrap();
    tokio::fs::write(root.join("Ocupada/keep.txt"), b"untouched")
        .await
        .unwrap();
    let (status, result) = h
        .json(
            "POST",
            &format!("/api/v1/spaces/{}/nodes", a.space),
            Some(&a.token),
            &[("idempotency-key", "occupied")],
            json!({"name":"Ocupada"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["storage_pending"], true);
    assert_eq!(
        tokio::fs::read(root.join("Ocupada/keep.txt"))
            .await
            .unwrap(),
        b"untouched"
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/spaces/{}/nodes", a.space),
            Some(&a.token),
            &[("idempotency-key", "wait-for-recovery")],
            json!({"name":"Otra"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    tokio::fs::rename(root.join("Ocupada"), root.join("Contenido externo"))
        .await
        .unwrap();
    santolvdrive::workspace::process(&h.app, 100).await.unwrap();
    assert!(root.join("Ocupada").is_dir());
    assert!(!root.join("Ocupada/.drive-node").exists());
    assert_eq!(
        tokio::fs::read(root.join("Contenido externo/keep.txt"))
            .await
            .unwrap(),
        b"untouched"
    );
    h.folder(&a, None, "Después", "after-recovery").await;
    assert!(root.join("Después").exists());
}

#[sqlx::test]
async fn simple_storage_adopts_legacy_copy_and_trash_names_are_distinct(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("simple-owner").await;
    let upload = h
        .start(
            &a,
            "original.pdf",
            b"pdf content",
            "simple-upload",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&a, upload, 0, b"pdf content", true).await,
        StatusCode::OK
    );
    let (_, created) = h.complete(&a, upload).await;
    let node = id(&created, "id");
    let home = h.app.root.join("usuarios/simple-owner");
    let live = home.join("archivos/original.pdf/original.pdf");
    // Simulate the preceding release's archive plus independent working copy.
    let legacy = home.join(".versions").join(upload.to_string());
    tokio::fs::create_dir_all(&legacy).await.unwrap();
    tokio::fs::copy(&live, legacy.join("original.pdf"))
        .await
        .unwrap();
    tokio::fs::write(live.parent().unwrap().join(".drive-node"), node.to_string())
        .await
        .unwrap();
    santolvdrive::workspace::sync_node(&h.app, node)
        .await
        .unwrap();
    storage::prepare_space(&h.app.root, "usuarios/simple-owner")
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&live).await.unwrap(), b"pdf content");
    assert!(!home.join(".versions").exists());
    assert!(!live.parent().unwrap().join(".drive-node").exists());
    assert!(
        !storage::object(&h.app.root, "usuarios/simple-owner", upload, "original.pdf").exists()
    );
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{node}/versions"),
            Some(&a.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let one = h.folder(&a, None, "Uno", "one").await;
    let two = h.folder(&a, None, "Dos", "two").await;
    let first = h
        .folder(&a, Some(one), "Documentos", "first-documents")
        .await;
    let second = h
        .folder(&a, Some(two), "Documentos", "second-documents")
        .await;
    for node in [first, second] {
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/nodes/{node}/trash"),
                Some(&a.token),
                &[
                    ("if-match", "\"1\""),
                    ("idempotency-key", &node.to_string())
                ],
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert!(home.join("papelera/Documentos").is_dir());
    assert!(home.join("papelera/Documentos (2)").is_dir());
    for node in [first, second] {
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/nodes/{node}/restore"),
                Some(&a.token),
                &[
                    ("if-match", "\"2\""),
                    ("idempotency-key", &format!("restore-{node}"))
                ],
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert!(home.join("archivos/Uno/Documentos").is_dir());
    assert!(home.join("archivos/Dos/Documentos").is_dir());
}
