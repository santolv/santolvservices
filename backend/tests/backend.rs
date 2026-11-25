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

#[sqlx::test(migrations = false)]
async fn modular_migration_preserves_existing_photo_metadata_and_file_links(pool: PgPool) {
    let mut connection = pool.acquire().await.unwrap();
    let migrations = sqlx::migrate!("./migrations");
    for migration in migrations.iter().filter(|migration| migration.version < 13) {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    let (owner, drive, photos, node, object, version) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    sqlx::query("INSERT INTO users(id,username,password_hash) VALUES($1,'legacy-photo','hash')")
        .bind(owner)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO spaces(id,owner_id,name,kind,quota,storage_directory,service) VALUES($1,$3,'Drive','personal',100,$4,'drive'),($2,$3,'Fotos','personal',100,$5,'photos')").bind(drive).bind(photos).bind(owner).bind(format!("usuarios/{owner}/drive")).bind(format!("usuarios/{owner}/photos")).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO members VALUES($1,$2,3)")
        .bind(photos)
        .bind(owner)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO nodes(id,space_id,name,kind) VALUES($1,$2,'muestra.jpg','file')")
        .bind(node)
        .bind(photos)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO objects(id,space_id,size,sha256,original_name) VALUES($1,$2,3,$3,'muestra.jpg')").bind(object).bind(photos).bind("a".repeat(64)).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO versions(id,node_id,object_id,created_by) VALUES($1,$2,$3,$4)")
        .bind(version)
        .bind(node)
        .bind(object)
        .bind(owner)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("UPDATE nodes SET current_version=$1 WHERE id=$2")
        .bind(version)
        .bind(node)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO photo_metadata VALUES($1,true)")
        .bind(node)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO recent_nodes VALUES($1,$2,100)")
        .bind(owner)
        .bind(node)
        .execute(&mut *connection)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0013_modules.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let preserved:(Uuid,Uuid,bool,i64)=sqlx::query_as("SELECT n.current_version,v.object_id,m.favorite,r.used_at FROM photos.nodes n JOIN photos.versions v ON v.id=n.current_version JOIN photos.photo_metadata m ON m.node_id=n.id JOIN photos.recent_nodes r ON r.node_id=n.id WHERE n.id=$1").bind(node).fetch_one(&pool).await.unwrap();
    assert_eq!(preserved, (version, object, true, 100));
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM public.spaces")
            .fetch_one(&pool)
            .await
            .unwrap(),
        drive
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM public.nodes")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    for migration in migrations.iter().filter(|migration| migration.version > 13) {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    drop(connection);
    let h = Harness::new(pool).await;
    let old = h.app.root.join(format!("usuarios/{owner}/photos"));
    let new = h.app.root.join(format!("usuarios/{owner}/drive/Photos"));
    let original = std::path::Path::new("archivos/muestra.jpg/muestra.jpg");
    tokio::fs::create_dir_all(old.join(original).parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(old.join(original), b"old").await.unwrap();
    sqlx::query("INSERT INTO photos.workspace_publications VALUES($1,$2)")
        .bind(node)
        .bind(old.join(original).to_string_lossy().as_ref())
        .execute(&h.app.db)
        .await
        .unwrap();
    tokio::fs::create_dir_all(&new).await.unwrap();
    assert!(
        santolvdrive::services::migrate_storage(&h.app)
            .await
            .is_err()
    );
    assert_eq!(tokio::fs::read(old.join(original)).await.unwrap(), b"old");
    tokio::fs::remove_dir(&new).await.unwrap();
    santolvdrive::services::migrate_storage(&h.app)
        .await
        .unwrap();
    santolvdrive::services::migrate_storage(&h.app)
        .await
        .unwrap();
    assert!(!old.exists());
    assert_eq!(tokio::fs::read(new.join(original)).await.unwrap(), b"old");
    let journal: String =
        sqlx::query_scalar("SELECT target FROM photos.workspace_publications WHERE node_id=$1")
            .bind(node)
            .fetch_one(&h.app.db)
            .await
            .unwrap();
    assert_eq!(std::path::Path::new(&journal), new.join(original));
}

#[sqlx::test]
async fn account_profile_is_private_mutable_and_protected_by_csrf(pool: PgPool) {
    let h = Harness::new(pool).await;
    let alice = h.account("account-alice").await;
    let bob = h.account("account-bob").await;
    let path = "/api/v1/me/profile";
    let profile = json!({"display_name":"  Ana Gómez  ","avatar_color":"plum"});
    assert_eq!(
        h.json("PUT", path, None, &[], profile.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (login, headers, body) = h
        .req(
            "POST",
            "/api/v1/auth/login",
            None,
            &[
                ("origin", "http://localhost:5500"),
                ("content-type", "application/json"),
            ],
            Body::from(
                json!({"username":alice.username,"password":"very strong test password"})
                    .to_string(),
            ),
        )
        .await;
    assert_eq!(login, StatusCode::OK);
    let cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let csrf = body["csrf_token"].as_str().unwrap();
    assert_eq!(
        h.json("PUT", path, None, &[("cookie", cookie)], profile.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    let (updated, saved) = h
        .json(
            "PUT",
            path,
            None,
            &[("cookie", cookie), ("x-csrf-token", csrf)],
            profile,
        )
        .await;
    assert_eq!(updated, StatusCode::OK);
    assert_eq!(
        saved,
        json!({"display_name":"Ana Gómez","avatar_color":"plum"})
    );
    let (_, me) = h
        .json("GET", "/api/v1/me", Some(&alice.token), &[], json!(null))
        .await;
    assert_eq!(me["display_name"], "Ana Gómez");
    assert_eq!(me["username"], alice.username);
    let (_, me) = h
        .json("GET", "/api/v1/me", Some(&bob.token), &[], json!(null))
        .await;
    assert_eq!(me["display_name"], Value::Null);
    assert_eq!(me["avatar_color"], "blue");
    assert_eq!(
        h.json(
            "PUT",
            path,
            Some(&alice.token),
            &[],
            json!({"display_name":"Ana","avatar_color":"url(http://example.com)"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.json(
            "PUT",
            path,
            Some(&alice.token),
            &[],
            json!({"id":bob.id,"display_name":"Ana","avatar_color":"plum"})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (reset, saved) = h
        .json(
            "PUT",
            path,
            Some(&alice.token),
            &[],
            json!({"display_name":"  ","avatar_color":"blue"}),
        )
        .await;
    assert_eq!(reset, StatusCode::OK);
    assert_eq!(saved["display_name"], Value::Null);

    let web = santolvdrive::frontend::router(
        santolvdrive::frontend::Frontend::bundled(
            "http://localhost:5500",
            "http://127.0.0.1:8080",
            h.app,
        )
        .unwrap(),
    );
    let page = |cookie: &str| {
        web.clone().oneshot(
            Request::builder()
                .uri("/account/")
                .header("host", "localhost:5500")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
    };
    let anonymous = page("").await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
    assert_eq!(anonymous.headers()["location"], "/");
    let signed_in = page(cookie).await.unwrap();
    assert_eq!(signed_in.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&to_bytes(signed_in.into_body(), 1024 * 1024).await.unwrap())
            .contains("id=\"profile-form\"")
    );
}

#[sqlx::test]
async fn only_authenticated_users_get_pages_and_only_allowed_modules_can_be_used(pool: PgPool) {
    use santolvdrive::{admin, core::manager, frontend};
    let h = Harness::new(pool).await;
    let web = frontend::router(
        frontend::Frontend::bundled(
            "http://localhost:5500",
            "http://127.0.0.1:8080",
            h.app.clone(),
        )
        .unwrap(),
    );
    let page = |path: &str, cookie: &str| {
        let request = Request::builder()
            .uri(path)
            .header("host", "localhost:5500")
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap();
        web.clone().oneshot(request)
    };
    let html = |response: axum::response::Response| async move {
        String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    };
    let login = page("/", "").await.unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    assert_eq!(login.headers()["cache-control"], "no-store");
    let csp = login.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(csp.contains("form-action 'none'") && csp.contains("style-src-attr 'none'"));
    let login = html(login).await;
    let nonce = login
        .split("nonce=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(csp.contains(&format!("script-src 'nonce-{nonce}'")));
    assert!(login.contains("id=\"login\""));
    assert!(!login.contains("data-shell-theme") && !login.contains("data-shell-account"));
    for route in [
        "/index.html",
        "/drive",
        "/drive/",
        "/photos/",
        "/photos/index.html",
    ] {
        let response = page(route, "drive_session=invalid").await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{route}");
        assert_eq!(response.headers()["location"], "/");
    }
    for route in [
        "/server.json",
        "/photos/AGENTS.md",
        "/drive/../../server.json",
        "/api/v1/me",
    ] {
        assert_eq!(
            page(route, "").await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
    for route in [
        "/api/v1/services",
        "/api/v1/photos/library",
        "/api/v1/activity/recent",
        "/api/v1/openapi.json",
    ] {
        assert_eq!(
            h.req("GET", route, None, &[], Body::empty()).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let alice = h.account("modules-alice").await;
    let bob = h.account("modules-bob").await;
    let browser_login = |username: String| {
        let data = json!({"username":username,"password":"very strong test password"}).to_string();
        let harness = &h;
        async move {
            let (status, headers, _) = harness
                .req(
                    "POST",
                    "/api/v1/auth/login",
                    None,
                    &[
                        ("origin", "http://localhost:5500"),
                        ("content-type", "application/json"),
                    ],
                    Body::from(data),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            headers["set-cookie"]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        }
    };
    let cookie = browser_login(alice.username.clone()).await;
    let bob_cookie = browser_login(bob.username.clone()).await;
    let (created, space) = h
        .json(
            "POST",
            "/api/v1/photos/space",
            Some(&alice.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(created, StatusCode::OK);
    let shared_path = format!("/api/v1/spaces/{}/nodes", space["id"].as_str().unwrap());
    assert_eq!(
        h.req("GET", &shared_path, Some(&alice.token), &[], Body::empty())
            .await
            .0,
        StatusCode::OK
    );
    assert!(
        html(page("/photos/", &cookie).await.unwrap())
            .await
            .contains("SantolvPhotos")
    );
    let command = |action: &str| vec![action.to_owned(), alice.username.clone(), "photos".into()];
    admin::console(&h.app, &command("deny-module"))
        .await
        .unwrap();
    let home = html(page("/", &cookie).await.unwrap()).await;
    assert!(home.contains("data-shell-service=\"drive\""));
    assert!(
        !home.contains("data-shell-service=\"photos\"")
            && !home.contains("class=\"service\" href=\"/photos/\"")
    );
    assert!(!home.contains("id=\"login\""));
    assert_eq!(
        page("/photos/", &cookie).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        page("/photos/index.html", &bob_cookie)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for route in ["/api/v1/photos/library", &shared_path] {
        assert_eq!(
            h.req("GET", route, Some(&alice.token), &[], Body::empty())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let (_, _, catalog) = h
        .req(
            "GET",
            "/api/v1/services",
            Some(&alice.token),
            &[],
            Body::empty(),
        )
        .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&catalog).unwrap()["items"],
        json!([{"id":"drive","enabled":true,"path":"/drive"}])
    );
    assert_eq!(
        h.req(
            "POST",
            "/api/v1/services/install",
            Some(&alice.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    admin::console(&h.app, &command("allow-module"))
        .await
        .unwrap();
    assert_eq!(
        page("/photos/", &cookie).await.unwrap().status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM public.service_settings WHERE id='photos'")
        .execute(&h.app.db)
        .await
        .unwrap();
    assert!(
        manager::set_enabled(&h.app.db, "photos", true)
            .await
            .is_err()
    );
    assert!(manager::lease(&h.app.db, "photos").await.unwrap().is_none());
    assert_eq!(
        page("/photos/", &cookie).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/photos/library",
            Some(&alice.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        page("/drive/", &cookie).await.unwrap().status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn concurrent_users_and_hot_service_disable(pool: PgPool) {
    use santolvdrive::core::manager;
    let h = Harness::new(pool).await;
    let (alice, bob) = tokio::join!(h.account("concurrent-alice"), h.account("concurrent-bob"));
    let (a, b) = tokio::join!(
        h.start(
            &alice,
            "mismo.txt",
            b"Alice",
            "same-key",
            None,
            None,
            "\"1\""
        ),
        h.start(&bob, "mismo.txt", b"Bob", "same-key", None, None, "\"1\"")
    );
    let (ra, rb) = tokio::join!(
        h.chunk(&alice, a, 0, b"Alice", true),
        h.chunk(&bob, b, 0, b"Bob", true)
    );
    assert_eq!((ra, rb), (StatusCode::OK, StatusCode::OK));
    let (_, space) = h
        .json(
            "POST",
            "/api/v1/photos/space",
            Some(&alice.token),
            &[],
            json!({}),
        )
        .await;
    let photos = Account {
        id: alice.id,
        space: id(&space, "id"),
        token: alice.token.clone(),
        username: alice.username.clone(),
    };
    let photo = h
        .start(
            &photos,
            "foto.jpg",
            b"Foto",
            "photo-pending",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&photos, photo, 0, b"Foto", true).await,
        StatusCode::OK
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/uploads/{photo}/complete"),
            Some(&alice.token),
            &[],
            json!({})
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    manager::set_enabled(&h.app.db, "photos", false)
        .await
        .unwrap();
    let workers = tokio::spawn(manager::run(h.app.clone()));
    let result = async {
        for path in [
            "/api/v1/photos/library".into(),
            format!("/api/v1/uploads/{photo}"),
            format!("/api/v1/spaces/{}/nodes", photos.space),
        ] {
            let (status, body) = h
                .json("GET", &path, Some(&alice.token), &[], json!({}))
                .await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"]["code"], "service_disabled");
        }
        let complete = |account: &Account, upload| {
            let token = account.token.clone();
            let h = &h;
            async move {
                h.json(
                    "POST",
                    &format!("/api/v1/uploads/{upload}/complete"),
                    Some(&token),
                    &[],
                    json!({}),
                )
                .await
                .0
            }
        };
        let (ra, rb) = tokio::join!(complete(&alice, a), complete(&bob, b));
        assert_eq!((ra, rb), (StatusCode::ACCEPTED, StatusCode::ACCEPTED));
        let ready = |upload| {
            let db = h.app.db.clone();
            async move {
                tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    loop {
                        if let Some(node) = sqlx::query_scalar::<_, Uuid>(
                            "SELECT result_node FROM uploads WHERE id=$1 AND state='ready'",
                        )
                        .bind(upload)
                        .fetch_optional(&db)
                        .await
                        .unwrap()
                        {
                            break node;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                })
                .await
                .unwrap()
            }
        };
        let (na, nb) = tokio::join!(ready(a), ready(b));
        let ap = format!("/api/v1/nodes/{na}/content");
        let bp = format!("/api/v1/nodes/{nb}/content");
        let (ar, br) = tokio::join!(
            h.req("GET", &ap, Some(&alice.token), &[], Body::empty()),
            h.req("GET", &bp, Some(&bob.token), &[], Body::empty())
        );
        assert_eq!(ar.2, b"Alice");
        assert_eq!(br.2, b"Bob");
        assert_eq!(
            h.req("GET", &ap, Some(&bob.token), &[], Body::empty())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        let state: String = sqlx::query_scalar("SELECT state FROM photos.uploads WHERE id=$1")
            .bind(photo)
            .fetch_one(&h.app.db)
            .await
            .unwrap();
        assert_eq!(state, "pending");
        manager::set_enabled(&h.app.db, "photos", true)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let (_, library) = h
                    .json(
                        "GET",
                        "/api/v1/photos/library",
                        Some(&alice.token),
                        &[],
                        json!({}),
                    )
                    .await;
                if library["items"][0]["name"] == "foto.jpg" {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        manager::set_enabled(&h.app.db, "drive", false)
            .await
            .unwrap();
        assert_eq!(
            manager::set_enabled(&h.app.db, "photos", true)
                .await
                .unwrap_err()
                .1,
            "required_service_disabled"
        );
        assert!(manager::lease(&h.app.db, "photos").await.unwrap().is_none());
        assert_eq!(
            h.req("GET", &ap, Some(&alice.token), &[], Body::empty())
                .await
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            h.json(
                "GET",
                "/api/v1/photos/library",
                Some(&alice.token),
                &[],
                json!({})
            )
            .await
            .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        manager::set_enabled(&h.app.db, "drive", true)
            .await
            .unwrap();
        assert!(manager::enabled(&h.app.db, "photos").await.unwrap());
        assert_eq!(
            h.json("GET", "/api/v1/me", Some(&alice.token), &[], json!({}))
                .await
                .0,
            StatusCode::OK
        );
    };
    result.await;
    workers.abort();
}

#[sqlx::test]
async fn photos_share_sessions_but_isolate_storage_and_persist_favorites(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("photo-owner").await;
    let b = h.account("photo-other").await;
    let (status, created) = h
        .json(
            "POST",
            "/api/v1/photos/space",
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let space = id(&created, "id");
    let (_, again) = h
        .json(
            "POST",
            "/api/v1/photos/space",
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(again, created);
    let directory: String =
        sqlx::query_scalar("SELECT storage_directory FROM photos.spaces WHERE id=$1")
            .bind(space)
            .fetch_one(&h.app.db)
            .await
            .unwrap();
    assert_eq!(directory, format!("usuarios/{}/drive/Photos", a.id));
    assert!(h.app.root.join(&directory).join("archivos").is_dir());
    let photo_account = Account {
        id: a.id,
        space,
        token: a.token.clone(),
        username: a.username.clone(),
    };
    let upload = h
        .start(
            &photo_account,
            "original.jpg",
            b"abc",
            "photo-upload",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&photo_account, upload, 0, b"abc", true).await,
        StatusCode::OK
    );
    let (status, done) = h.complete(&photo_account, upload).await;
    assert_eq!(status, StatusCode::OK);
    let node = id(&done, "id");
    let favorite = format!("/api/v1/photos/{node}/favorite");
    assert_eq!(
        h.json(
            "PUT",
            &favorite,
            Some(&b.token),
            &[("if-match", "\"1\"")],
            json!({"favorite":true})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.json(
            "PUT",
            &favorite,
            Some(&a.token),
            &[("if-match", "\"1\"")],
            json!({"favorite":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        h.json(
            "PUT",
            &favorite,
            Some(&a.token),
            &[("if-match", "\"1\"")],
            json!({"favorite":false})
        )
        .await
        .0,
        StatusCode::PRECONDITION_FAILED
    );
    let (_, library) = h
        .json(
            "GET",
            "/api/v1/photos/library",
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(library["items"][0]["id"], node.to_string());
    assert_eq!(library["items"][0]["favorite"], true);
    let root = format!("/api/v1/spaces/{}/nodes", a.space);
    let (_, drive_root) = h.json("GET", &root, Some(&a.token), &[], json!(null)).await;
    assert_eq!(
        drive_root["photos_folder"],
        json!({"id":"photos","name":"Photos","kind":"folder","protected":true})
    );
    let mount = "/api/v1/photos/drive-folder";
    let (status, album) = h
        .json(
            "POST",
            &format!("/api/v1/spaces/{space}/nodes"),
            Some(&a.token),
            &[("idempotency-key", "mount-album")],
            json!({"name":"Viaje"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let album = id(&album, "id");
    let (_, mounted) = h.json("GET", mount, Some(&a.token), &[], json!(null)).await;
    assert_eq!(mounted["items"][0]["name"], "Cámara");
    assert_eq!(mounted["items"][1]["id"], album.to_string());
    let camera = format!("{mount}?camera=true");
    let (_, camera_items) = h
        .json("GET", &camera, Some(&a.token), &[], json!(null))
        .await;
    assert_eq!(camera_items["items"][0]["id"], node.to_string());
    let (_, other_camera) = h
        .json("GET", &camera, Some(&b.token), &[], json!(null))
        .await;
    assert_eq!(other_camera["items"], json!([]));
    let foreign_album = format!("{mount}?parent={album}");
    assert_eq!(
        h.json("GET", &foreign_album, Some(&b.token), &[], json!(null))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.json(
            "PATCH",
            "/api/v1/nodes/photos",
            Some(&a.token),
            &[],
            json!({"name":"Cambio"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/nodes/photos/trash",
            Some(&a.token),
            &[],
            json!(null)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, other) = h
        .json(
            "GET",
            "/api/v1/photos/library",
            Some(&b.token),
            &[],
            json!({}),
        )
        .await;
    assert!(other["items"].as_array().unwrap().is_empty());
    let (_, drive) = h
        .json("GET", "/api/v1/spaces", Some(&a.token), &[], json!({}))
        .await;
    assert_eq!(drive["items"].as_array().unwrap().len(), 1);
    assert!(drive["items"][0].get("legacy_storage_directory").is_none());
    let (_, recent) = h
        .json("GET", "/api/v1/recent", Some(&a.token), &[], json!({}))
        .await;
    assert!(recent["items"].as_array().unwrap().is_empty());
    let (_, activity) = h
        .json(
            "GET",
            "/api/v1/activity/recent",
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert!(
        activity["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == node.to_string() && item["service"] == "photos")
    );
    let (_, other_activity) = h
        .json(
            "GET",
            "/api/v1/activity/recent",
            Some(&b.token),
            &[],
            json!({}),
        )
        .await;
    assert!(other_activity["items"].as_array().unwrap().is_empty());
    let (_, requested) = h
        .json(
            "GET",
            &format!("/api/v1/photos/library?id={node}"),
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(requested["items"][0]["id"], node.to_string());
    let (_, other_requested) = h
        .json(
            "GET",
            &format!("/api/v1/photos/library?id={node}"),
            Some(&b.token),
            &[],
            json!({}),
        )
        .await;
    assert!(other_requested["items"].as_array().unwrap().is_empty());
    let (status, folder) = h
        .json(
            "POST",
            &format!("/api/v1/spaces/{}/nodes", a.space),
            Some(&a.token),
            &[("idempotency-key", "activity-folder")],
            json!({"name":"Trabajo","parent_id":null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, activity) = h
        .json(
            "GET",
            "/api/v1/activity/recent",
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(activity["items"][0]["id"], folder["id"]);
    assert_eq!(activity["items"][0]["service"], "drive");
    assert_eq!(activity["items"][1]["service"], "photos");
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{node}/trash"),
            Some(&a.token),
            &[("if-match", "\"2\""), ("idempotency-key", "photo-trash")],
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, trash) = h
        .json(
            "GET",
            "/api/v1/photos/library",
            Some(&a.token),
            &[],
            json!({}),
        )
        .await;
    assert_eq!(
        trash["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == node.to_string())
            .unwrap()["deleted"],
        true
    );
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{node}/restore"),
            Some(&a.token),
            &[("if-match", "\"3\""), ("idempotency-key", "photo-restore")],
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, _, bytes) = h
        .req(
            "GET",
            &format!("/api/v1/nodes/{node}/content"),
            Some(&a.token),
            &[],
            Body::empty(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"abc");
}

#[sqlx::test]
async fn service_storage_migration_preserves_names_resumes_and_refuses_collisions(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("migrate-user").await;
    let new = storage::user_directory(&a.username, a.id);
    let old = "usuarios/migrate-user";
    let relative = "archivos/Recuerdos";
    tokio::fs::create_dir(h.app.root.join(&new).join(relative))
        .await
        .unwrap();
    tokio::fs::write(
        h.app.root.join(&new).join(relative).join("foto.jpg"),
        b"original",
    )
    .await
    .unwrap();
    tokio::fs::rename(h.app.root.join(&new), h.app.root.join(old))
        .await
        .unwrap();
    sqlx::query("UPDATE spaces SET legacy_storage_directory=$1 WHERE id=$2")
        .bind(old)
        .bind(a.space)
        .execute(&h.app.db)
        .await
        .unwrap();
    santolvdrive::services::migrate_storage(&h.app)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(h.app.root.join(&new).join(relative).join("foto.jpg"))
            .await
            .unwrap(),
        b"original"
    );
    assert!(!h.app.root.join(old).exists());
    // Simulate interruption after the filesystem rename and before the SQL commit.
    sqlx::query("UPDATE spaces SET legacy_storage_directory=$1 WHERE id=$2")
        .bind(old)
        .bind(a.space)
        .execute(&h.app.db)
        .await
        .unwrap();
    santolvdrive::services::migrate_storage(&h.app)
        .await
        .unwrap();
    storage::prepare_space(&h.app.root, old).await.unwrap();
    tokio::fs::write(h.app.root.join(old).join("keep.txt"), b"keep")
        .await
        .unwrap();
    sqlx::query("UPDATE spaces SET legacy_storage_directory=$1 WHERE id=$2")
        .bind(old)
        .bind(a.space)
        .execute(&h.app.db)
        .await
        .unwrap();
    assert!(
        santolvdrive::services::migrate_storage(&h.app)
            .await
            .is_err()
    );
    assert_eq!(
        tokio::fs::read(h.app.root.join(old).join("keep.txt"))
            .await
            .unwrap(),
        b"keep"
    );
}

#[sqlx::test]
async fn reserved_journal_survives_exhausted_application_pool(pool: PgPool) {
    let journal = App::journal_pool(&pool);
    let mut held = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        held.push(pool.acquire().await.unwrap());
    }
    assert!(pool.try_acquire().is_none());
    let result: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&journal)
        .await
        .unwrap();
    assert_eq!(result, 1);
    drop(held);
    journal.close().await;
}

#[sqlx::test]
async fn login_limit_of_one_account_does_not_lock_other_users_on_same_ip(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("blocked-user").await;
    let b = h.account("other-user").await;
    let key = format!("login-pair:127.0.0.1:{}", auth::hash(&a.username));
    sqlx::query("INSERT INTO rate_limits(key,count) VALUES($1,50)")
        .bind(&key)
        .execute(&h.app.db)
        .await
        .unwrap();
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            json!({"username":a.username,"password":"very strong test password","mobile":true})
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            json!({"username":b.username,"password":"very strong test password","mobile":true})
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn host_proxy_failures_and_session_limits_are_enforced(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("hardened-user").await;
    for headers in [
        [("host", "attacker.example")],
        [("x-forwarded-for", "203.0.113.7")],
        [("x-forwarded-proto", "https")],
        [("forwarded", "for=203.0.113.7")],
    ] {
        assert_eq!(
            h.req("GET", "/api/v1/me", Some(&a.token), &headers, Body::empty())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let login = |password: &str| json!({"username":a.username,"password":password,"mobile":true});
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            login("incorrect-password")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let failures: (i32, bool) =
        sqlx::query_as("SELECT failures,next_attempt>now() FROM login_failures WHERE key=$1")
            .bind(format!("127.0.0.1:{}", auth::hash(&a.username)))
            .fetch_one(&h.app.db)
            .await
            .unwrap();
    assert_eq!(failures, (1, true));
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            login("very strong test password")
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("UPDATE login_failures SET next_attempt=now()-interval '1 second'")
        .execute(&h.app.db)
        .await
        .unwrap();
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            login("very strong test password")
        )
        .await
        .0,
        StatusCode::OK
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM login_failures")
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    for _ in 0..11 {
        assert_eq!(
            h.json(
                "POST",
                "/api/v1/auth/login",
                None,
                &[],
                login("very strong test password")
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
        .bind(a.id)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(count, 10);
    let (_, latest) = h
        .json(
            "POST",
            "/api/v1/auth/login",
            None,
            &[],
            login("very strong test password"),
        )
        .await;
    let raw = latest["token"].as_str().unwrap();
    let (_, team) = h
        .json(
            "POST",
            "/api/v1/spaces",
            Some(raw),
            &[("idempotency-key", "limits-team")],
            json!({"name":"Otro espacio"}),
        )
        .await;
    sqlx::query("INSERT INTO uploads(id,user_id,space_id,name,size,sha256) SELECT gen_random_uuid(),$1,CASE WHEN n<=16 THEN $2 ELSE $3 END,'pending-'||n,0,$4 FROM generate_series(1,32)n")
        .bind(a.id).bind(a.space).bind(id(&team,"id")).bind("0".repeat(64)).execute(&h.app.db).await.unwrap();
    assert_eq!(
        h.json(
            "POST",
            "/api/v1/uploads",
            Some(raw),
            &[("idempotency-key", "max-pending")],
            json!({"space_id":a.space,"name":"extra","size":0,"sha256":"0".repeat(64)})
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        h.req("GET", "/api/v1/me", Some(&a.token), &[], Body::empty())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn first_publication_recovers_after_bytes_moved_before_metadata_commit(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("publication-user").await;
    let upload = h
        .start(
            &a,
            "original.bin",
            b"precious content",
            "publication-upload",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&a, upload, 0, b"precious content", true).await,
        StatusCode::OK
    );
    let (_, created) = h.complete(&a, upload).await;
    let node = id(&created, "id");
    let path = h
        .app
        .root
        .join(storage::user_directory("", a.id))
        .join("archivos/original.bin");
    // The disk already contains the final bytes, but the main transaction never recorded its entry.
    sqlx::query("DELETE FROM workspace_entries WHERE node_id=$1")
        .bind(node)
        .execute(&h.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspace_publications(node_id,target) VALUES($1,$2)")
        .bind(node)
        .bind(path.to_string_lossy().as_ref())
        .execute(&h.app.db)
        .await
        .unwrap();
    let mut tx = h.app.db.begin().await.unwrap();
    santolvdrive::workspace::schedule(&mut tx, node, a.space)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut cursor = Uuid::nil();
    let mut cancelled = Uuid::nil();
    worker::tick(&h.app, &mut cursor, &mut cancelled)
        .await
        .unwrap();
    santolvdrive::workspace::sync_node(&h.app, node)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(path.join("original.bin")).await.unwrap(),
        b"precious content"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_publications WHERE node_id=$1")
            .bind(node)
            .fetch_one(&h.app.db)
            .await
            .unwrap();
    assert_eq!(count, 0);
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
        b"precious content"
    );
}

#[sqlx::test]
async fn short_cookie_sessions_restore_csrf_after_reload_and_expire(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("cookie-reload").await;
    let (status, headers, body) = h
        .req(
            "POST",
            "/api/v1/auth/login",
            None,
            &[
                ("origin", "http://localhost:5500"),
                ("content-type", "application/json"),
            ],
            Body::from(
                json!({"username":a.username,"password":"very strong test password"}).to_string(),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["expires_in"], 7200);
    assert!(value.get("token").is_none());
    let set_cookie = headers["set-cookie"].to_str().unwrap();
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Strict"));
    assert!(set_cookie.contains("Max-Age=7200"));
    assert!(set_cookie.contains("; Path=/;"));
    assert!(headers.get_all("set-cookie").iter().any(|value| {
        let value = value.to_str().unwrap();
        value.starts_with("drive_session=; Path=/api/v1;") && value.contains("Max-Age=0")
    }));
    let cookie = set_cookie.split(';').next().unwrap();
    let raw = cookie.strip_prefix("drive_session=").unwrap();
    let stored: String = sqlx::query_scalar("SELECT token_hash FROM sessions WHERE token_hash=$1")
        .bind(auth::hash(raw))
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_ne!(stored, raw);
    for _ in 0..2 {
        let (status, _, body) = h
            .req(
                "GET",
                "/api/v1/me",
                None,
                &[("origin", "http://localhost:5500"), ("cookie", cookie)],
                Body::empty(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let restored: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(restored["csrf_token"], value["csrf_token"]);
        assert!(restored.get("token").is_none());
    }
    let (_, body) = h
        .json(
            "POST",
            "/api/v1/spaces",
            None,
            &[
                ("origin", "http://localhost:5500"),
                ("cookie", cookie),
                ("x-csrf-token", value["csrf_token"].as_str().unwrap()),
                ("idempotency-key", "restored-cookie"),
            ],
            json!({"name":"Después de recargar"}),
        )
        .await;
    assert!(body.get("id").is_some());
    let duplicate = format!("{cookie}; {cookie}");
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            None,
            &[("cookie", &duplicate)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            Some(raw),
            &[("cookie", cookie)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            None,
            &[("origin", "https://attacker.example"), ("cookie", cookie)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second' WHERE token_hash=$1")
        .bind(auth::hash(raw))
        .execute(&h.app.db)
        .await
        .unwrap();
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            None,
            &[("cookie", cookie)],
            Body::empty()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn recent_history_is_private_ordered_bounded_and_rechecks_permissions(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("recent-owner").await;
    let b = h.account("recent-reader").await;
    let alpha = h.folder(&a, None, "Alpha", "recent-alpha").await;
    let zeta = h.folder(&a, None, "Zeta", "recent-zeta").await;
    let (_, list) = h
        .json("GET", "/api/v1/recent", Some(&a.token), &[], json!(null))
        .await;
    assert_eq!(list["items"][0]["id"], zeta.to_string());
    for _ in 0..2 {
        assert_eq!(
            h.json(
                "POST",
                &format!("/api/v1/nodes/{alpha}/touch"),
                Some(&a.token),
                &[],
                json!(null)
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
    }
    let (_, list) = h
        .json("GET", "/api/v1/recent", Some(&a.token), &[], json!(null))
        .await;
    assert_eq!(list["items"][0]["id"], alpha.to_string());
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{alpha}/touch"),
            Some(&b.token),
            &[],
            json!(null)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    h.json(
        "PUT",
        &format!("/api/v1/nodes/{alpha}/grants"),
        Some(&a.token),
        &[],
        json!({"username":b.username,"role":1}),
    )
    .await;
    assert_eq!(
        h.json(
            "POST",
            &format!("/api/v1/nodes/{alpha}/touch"),
            Some(&b.token),
            &[],
            json!(null)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, list) = h
        .json("GET", "/api/v1/recent", Some(&b.token), &[], json!(null))
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    h.json(
        "POST",
        &format!("/api/v1/nodes/{alpha}/grants/revoke"),
        Some(&a.token),
        &[],
        json!({"user_id":b.id}),
    )
    .await;
    let (_, list) = h
        .json("GET", "/api/v1/recent", Some(&b.token), &[], json!(null))
        .await;
    assert!(list["items"].as_array().unwrap().is_empty());
    sqlx::query("INSERT INTO nodes(id,space_id,name,kind) SELECT gen_random_uuid(),$1,'recent-'||n,'folder' FROM generate_series(1,205) n").bind(a.space).execute(&h.app.db).await.unwrap();
    sqlx::query("INSERT INTO recent_nodes(user_id,node_id,used_at) SELECT $1,id,123 FROM nodes WHERE space_id=$2 AND name LIKE 'recent-%'").bind(a.id).bind(a.space).execute(&h.app.db).await.unwrap();
    h.json(
        "POST",
        &format!("/api/v1/nodes/{alpha}/touch"),
        Some(&a.token),
        &[],
        json!(null),
    )
    .await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM recent_nodes WHERE user_id=$1")
        .bind(a.id)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(count, 200);
    let (_, first) = h
        .json("GET", "/api/v1/recent", Some(&a.token), &[], json!(null))
        .await;
    let (_, second) = h
        .json(
            "GET",
            &format!(
                "/api/v1/recent?after={}",
                first["next_cursor"].as_str().unwrap()
            ),
            Some(&a.token),
            &[],
            json!(null),
        )
        .await;
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    assert_eq!(second["items"].as_array().unwrap().len(), 100);
    let ids: std::collections::HashSet<_> = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["items"].as_array().unwrap())
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 200);
    assert!(second["next_cursor"].is_null());
    assert_eq!(
        h.json(
            "GET",
            "/api/v1/recent?after=invalid",
            Some(&a.token),
            &[],
            json!(null)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test]
async fn workspace_owner_can_delete_contents_and_pending_uploads_durably(pool: PgPool) {
    let h = Harness::new(pool).await;
    let owner = h.account("space-owner").await;
    let other = h.account("space-manager").await;
    let outsider = h.account("space-outsider").await;
    let personal = h
        .folder(&owner, None, "Conservar", "personal-preserved")
        .await;
    let (_, created) = h
        .json(
            "POST",
            "/api/v1/spaces",
            Some(&owner.token),
            &[("idempotency-key", "delete-space-create")],
            json!({"name":"Eliminar equipo"}),
        )
        .await;
    let space = id(&created, "id");
    h.json(
        "PUT",
        &format!("/api/v1/spaces/{space}/members"),
        Some(&owner.token),
        &[],
        json!({"username":other.username,"role":3}),
    )
    .await;
    let org = Account {
        id: owner.id,
        space,
        token: owner.token.clone(),
        username: owner.username.clone(),
    };
    let folder = h.folder(&org, None, "Datos", "org-folder").await;
    let upload = h
        .start(
            &org,
            "data.bin",
            b"content",
            "org-content",
            Some(folder),
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&org, upload, 0, b"content", true).await,
        StatusCode::OK
    );
    let (_, file) = h.complete(&org, upload).await;
    let file = id(&file, "id");
    h.json(
        "PUT",
        &format!("/api/v1/nodes/{file}/grants"),
        Some(&owner.token),
        &[],
        json!({"user_id":outsider.id,"role":1}),
    )
    .await;
    let pending = h
        .start(
            &org,
            "partial.bin",
            b"partial",
            "org-pending",
            Some(folder),
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(
        h.chunk(&org, pending, 0, b"part", true).await,
        StatusCode::OK
    );
    let url = format!("/api/v1/spaces/{space}");
    assert_eq!(
        h.req("DELETE", &url, Some(&other.token), &[], Body::empty())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.req("DELETE", &url, Some(&outsider.token), &[], Body::empty())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.req(
            "DELETE",
            &format!("/api/v1/spaces/{}", owner.space),
            Some(&owner.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for _ in 0..2 {
        assert_eq!(
            h.req("DELETE", &url, Some(&owner.token), &[], Body::empty())
                .await
                .0,
            StatusCode::ACCEPTED
        );
    }
    for account in [&owner, &other, &outsider] {
        let (_, spaces) = h
            .json(
                "GET",
                "/api/v1/spaces",
                Some(&account.token),
                &[],
                json!(null),
            )
            .await;
        assert!(
            !spaces["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["id"] == space.to_string())
        );
        assert_eq!(
            h.req(
                "GET",
                &format!("/api/v1/nodes/{file}/content"),
                Some(&account.token),
                &[],
                Body::empty()
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let mut cursor = Uuid::nil();
    let mut cancelled = Uuid::nil();
    for _ in 0..4 {
        worker::tick(&h.app, &mut cursor, &mut cancelled)
            .await
            .unwrap();
    }
    let removed: bool = sqlx::query_scalar("SELECT storage_removed FROM spaces WHERE id=$1")
        .bind(space)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert!(removed);
    assert!(!h.app.root.join(format!("organizaciones/{space}")).exists());
    assert!(!storage::temp(&h.app.root, &format!("organizaciones/{space}"), pending).exists());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE space_id=$1")
        .bind(space)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        h.app
            .root
            .join(storage::user_directory("", owner.id))
            .join("archivos/Conservar")
            .is_dir()
    );
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{personal}"),
            Some(&owner.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn sharing_and_memberships_accept_separate_usernames_and_ids(pool: PgPool) {
    let h = Harness::new(pool).await;
    let owner = h.account("sharing-owner").await;
    let recipient = h.account("sharing-recipient").await;
    let outsider = h.account("sharing-outsider").await;
    let folder = h.folder(&owner, None, "Compartir", "username-folder").await;
    let path = format!("/api/v1/nodes/{folder}/grants");
    assert_eq!(
        h.json(
            "PUT",
            &path,
            Some(&owner.token),
            &[],
            json!({"username":" SHARING-RECIPIENT ","role":1})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, grants) = h
        .json("GET", &path, Some(&owner.token), &[], json!(null))
        .await;
    assert_eq!(grants["items"][0]["username"], recipient.username);
    assert_eq!(grants["items"][0]["user_id"], recipient.id.to_string());
    assert_eq!(
        h.req(
            "GET",
            &format!("/api/v1/nodes/{folder}"),
            Some(&recipient.token),
            &[],
            Body::empty()
        )
        .await
        .0,
        StatusCode::OK
    );
    for payload in [
        json!({"username":recipient.username,"user_id":recipient.id,"role":1}),
        json!({"username":recipient.username,"subject_space_id":owner.space,"role":1}),
        json!({"role":1}),
        json!({"username":" ","role":1}),
    ] {
        assert_eq!(
            h.json("PUT", &path, Some(&owner.token), &[], payload)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    for payload in [
        json!({"username":"missing-user","role":1}),
        json!({"username":"director","role":1}),
        json!({"user_id":h.admin_id,"role":1}),
    ] {
        assert_eq!(
            h.json("PUT", &path, Some(&owner.token), &[], payload)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        h.json(
            "PUT",
            &path,
            Some(&outsider.token),
            &[],
            json!({"username":recipient.username,"role":3})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.json("GET", &path, Some(&recipient.token), &[], json!(null))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.json(
            "PUT",
            &path,
            Some(&owner.token),
            &[],
            json!({"user_id":recipient.id,"role":2})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, grants) = h
        .json("GET", &path, Some(&owner.token), &[], json!(null))
        .await;
    assert_eq!(grants["items"].as_array().unwrap().len(), 1);
    assert_eq!(grants["items"][0]["role"], 2);
    let (_, space) = h
        .json(
            "POST",
            "/api/v1/spaces",
            Some(&owner.token),
            &[("idempotency-key", "username-space")],
            json!({"name":"Equipo"}),
        )
        .await;
    let members = format!("/api/v1/spaces/{}/members", id(&space, "id"));
    assert_eq!(
        h.json(
            "PUT",
            &members,
            Some(&owner.token),
            &[],
            json!({"user_id":recipient.id,"role":1})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.json(
            "PUT",
            &members,
            Some(&owner.token),
            &[],
            json!({"username":recipient.username,"role":2})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.json(
            "PUT",
            &members,
            Some(&owner.token),
            &[],
            json!({"username":recipient.username,"user_id":recipient.id,"role":2})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.json(
            "PUT",
            &members,
            Some(&owner.token),
            &[],
            json!({"user_id":owner.id,"role":1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.json(
            "PUT",
            &members,
            Some(&owner.token),
            &[],
            json!({"user_id":h.admin_id,"role":1})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    santolvdrive::admin::set_disabled(&h.app, h.admin_id, recipient.id, true)
        .await
        .unwrap();
    assert_eq!(
        h.json(
            "PUT",
            &path,
            Some(&owner.token),
            &[],
            json!({"username":recipient.username,"role":1})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.json(
            "POST",
            &(path.clone() + "/revoke"),
            Some(&owner.token),
            &[],
            json!({"username":recipient.username})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, grants) = h
        .json("GET", &path, Some(&owner.token), &[], json!(null))
        .await;
    assert!(grants["items"].as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn photo_derivatives_are_persistent_private_and_versioned(pool: PgPool) {
    let h = Harness::new(pool).await;
    let alice = h.account("derived-alice").await;
    let bob = h.account("derived-bob").await;
    let (_, space) = h
        .json(
            "POST",
            "/api/v1/photos/space",
            Some(&alice.token),
            &[],
            json!({}),
        )
        .await;
    let a = Account {
        space: id(&space, "id"),
        ..alice
    };
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1600, 800)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let source = encoded.into_inner();
    let upload = h
        .start(
            &a,
            "test.png",
            &source,
            "derivative-upload",
            None,
            None,
            "\"1\"",
        )
        .await;
    assert_eq!(h.chunk(&a, upload, 0, &source, true).await, StatusCode::OK);
    let (status, done) = h.complete(&a, upload).await;
    assert_eq!(status, StatusCode::OK);
    let node = id(&done, "id");
    let app = santolvdrive::modules::photos::SERVICE.app(&h.app);
    let worker = tokio::spawn((santolvdrive::modules::photos::SERVICE.worker)(
        app.clone(),
        "photos",
    ));
    let path = format!("/api/v1/photos/{node}/image/256");
    let result = async {
        let mut ready = None;
        for _ in 0..40 {
            let response = h
                .req("GET", &path, Some(&a.token), &[], Body::empty())
                .await;
            if response.0 == StatusCode::OK {
                ready = Some(response);
                break;
            }
            assert_eq!(response.0, StatusCode::ACCEPTED);
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let (_, headers, bytes) = ready.expect("thumbnail generated");
        assert_eq!(headers["content-type"], "image/webp");
        assert_eq!(headers["cache-control"], "private, no-cache");
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 256);
        assert_eq!(
            h.req("GET", &path, Some(&bob.token), &[], Body::empty())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            h.req(
                "GET",
                &path,
                Some(&a.token),
                &[("if-none-match", headers["etag"].to_str().unwrap())],
                Body::empty()
            )
            .await
            .0,
            StatusCode::NOT_MODIFIED
        );
        let preview = h
            .req(
                "GET",
                &format!("/api/v1/photos/{node}/image/1280"),
                Some(&a.token),
                &[],
                Body::empty(),
            )
            .await;
        assert_eq!(image::load_from_memory(&preview.2).unwrap().width(), 1280);
        let original = h
            .req(
                "GET",
                &format!("/api/v1/nodes/{node}/content"),
                Some(&a.token),
                &[],
                Body::empty(),
            )
            .await;
        assert_eq!(original.2, source);
        let trashed = h
            .json(
                "POST",
                &format!("/api/v1/nodes/{node}/trash"),
                Some(&a.token),
                &[("if-match", "\"1\""), ("idempotency-key", "derived-trash")],
                json!({}),
            )
            .await;
        assert!(
            matches!(trashed.0, StatusCode::OK | StatusCode::ACCEPTED),
            "{trashed:?}"
        );
        assert_eq!(
            h.req("GET", &path, Some(&a.token), &[], Body::empty())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    .await;
    worker.abort();
    let _ = result;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM photos.derivatives WHERE node_id=$1")
            .bind(node)
            .fetch_one(&h.app.db)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE photos.nodes SET trashed_at=NULL WHERE id=$1")
        .bind(node)
        .execute(&h.app.db)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM photos.derivative_jobs WHERE node_id=$1"
        )
        .bind(node)
        .fetch_one(&h.app.db)
        .await
        .unwrap(),
        1
    );
    let version: Uuid = sqlx::query_scalar("SELECT current_version FROM photos.nodes WHERE id=$1")
        .bind(node)
        .fetch_one(&h.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO photos.derivatives(node_id,version_id) VALUES($1,$2)")
        .bind(node)
        .bind(version)
        .execute(&h.app.db)
        .await
        .unwrap();
    sqlx::query("UPDATE photos.nodes SET current_version=NULL WHERE id=$1")
        .bind(node)
        .execute(&h.app.db)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM photos.derivatives WHERE node_id=$1")
            .bind(node)
            .fetch_one(&h.app.db)
            .await
            .unwrap(),
        0
    );
}

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
            .header("host", self.app.origin.split("://").nth(1).unwrap())
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
            let in_photos: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos.uploads WHERE id=$1)")
                    .bind(upload)
                    .fetch_one(&self.app.db)
                    .await
                    .unwrap();
            let app = if in_photos {
                santolvdrive::modules::photos::SERVICE.app(&self.app)
            } else {
                self.app.clone()
            };
            worker::finalize_requested(&app).await.unwrap();
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
        let request=format!("POST /api/v1/auth/login HTTP/1.1\r\nHost: localhost:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",address.port(),body.len());
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
        assert_eq!(
            body["error"]["code"],
            if headers.is_empty() {
                "authentication_required"
            } else {
                "opaque_origin_disabled"
            }
        );
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
    assert_eq!(directory, format!("usuarios_compat/{user}"));
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
async fn opaque_html_cannot_authenticate_or_reuse_tokens(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("file-client").await;
    let (status,headers,_)=h.req("POST","/api/v1/auth/login",None,&[("origin","null"),("content-type","application/json")],Body::from(json!({"username":a.username,"password":"very strong test password","local_file":true}).to_string())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!headers.contains_key("set-cookie"));
    assert!(!headers.contains_key("access-control-allow-credentials"));
    assert_eq!(
        h.req(
            "GET",
            "/api/v1/me",
            Some(&a.token),
            &[("origin", "null")],
            Body::empty()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
async fn user_storage_isolated_shared_edits_and_legacy_content_survive(pool: PgPool) {
    let h = Harness::new(pool).await;
    let a = h.account("alice").await;
    let b = h.account("bob").await;
    let directory = storage::user_directory(&a.username, a.id);
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
        // Physical reconciliation is asynchronous; purge must wait for an acknowledged move.
        for _ in 0..10 {
            santolvdrive::workspace::process(&h.app, 100).await.unwrap();
            let pending: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workspace_jobs WHERE space_id=$1 AND NOT audit)",
            )
            .bind(a.space)
            .fetch_one(&h.app.db)
            .await
            .unwrap();
            if !pending {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
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
    let root = h
        .app
        .root
        .join(storage::user_directory("", a.id))
        .join("archivos");
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
            .join(storage::user_directory("", a.id))
            .join("papelera/Trabajo/Proyectos 2026")
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
    assert!(
        !h.app
            .root
            .join(storage::user_directory("", a.id))
            .join(".versions")
            .exists()
    );
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
    let root = h
        .app
        .root
        .join(storage::user_directory("", a.id))
        .join("archivos");
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
    let home = h.app.root.join(storage::user_directory("", a.id));
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
    storage::prepare_space(&h.app.root, &storage::user_directory("", a.id))
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
