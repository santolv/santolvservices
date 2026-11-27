use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{App, auth, core::manager, router};

const CONTACTS: &str = "/api/v1/contacts";

async fn user(db: &PgPool, name: &str) -> (Uuid, String) {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO public.users (id, username, password_hash) VALUES ($1, $2, 'test')")
        .bind(id)
        .bind(name)
        .execute(db)
        .await
        .unwrap();
    let token = auth::token();
    sqlx::query(
        "INSERT INTO public.sessions (token_hash, user_id, csrf_hash, expires_at)
         VALUES ($1, $2, 'test', now() + interval '1 minute')",
    )
    .bind(auth::hash(&token))
    .bind(id)
    .execute(db)
    .await
    .unwrap();
    (id, token)
}

async fn call(
    api: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
    expected: StatusCode,
) -> Value {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:8443");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        request = request.header("content-type", "application/json");
        Body::from(body.to_string())
    };
    let response = api
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(
        status,
        expected,
        "{method} {path}: {}",
        String::from_utf8_lossy(&bytes)
    );
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

fn app_state(db: PgPool, root: &tempfile::TempDir) -> App {
    App {
        journal: App::journal_pool(&db),
        db,
        root: root.path().into(),
        origin: "http://localhost:8443".into(),
        frontend_origin: "http://localhost:5500".into(),
        secure: false,
        hashes: Arc::new(Semaphore::new(2)),
        transfers: Arc::new(Semaphore::new(8)),
        user_transfers: Default::default(),
        quota: 10 * 1024 * 1024,
        requests: Arc::new(Semaphore::new(128)),
        retention_days: 30,
    }
}

#[sqlx::test]
async fn contacts_are_private_and_changes_require_current_revision(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let api = router(app_state(db.clone(), &root));
    let (_, alice) = user(&db, "contact-alice").await;
    let (bob_id, bob) = user(&db, "contact-bob").await;
    let a = Some(alice.as_str());
    let b = Some(bob.as_str());
    let none = Value::Null;

    call(
        &api,
        "GET",
        CONTACTS,
        a,
        none.clone(),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    manager::set_enabled(&db, "contacts", true).await.unwrap();
    call(
        &api,
        "GET",
        CONTACTS,
        None,
        none.clone(),
        StatusCode::UNAUTHORIZED,
    )
    .await;

    let ana = call(
        &api, "POST", CONTACTS, a,
        json!({"given_name":" Ana ","family_name":" Alba","email":"ana+team@example.com","company":"A_B"}),
        StatusCode::CREATED,
    ).await;
    let path = format!("{CONTACTS}/{}", ana["id"].as_str().unwrap());
    assert_eq!(ana["display_name"], "Ana Alba");
    assert_eq!(ana["revision"], 1);
    assert!(ana.get("owner_id").is_none());
    let other = call(&api, "GET", CONTACTS, b, none.clone(), StatusCode::OK).await;
    assert_eq!(other["items"], json!([]));
    call(&api, "GET", &path, b, none.clone(), StatusCode::NOT_FOUND).await;
    call(
        &api,
        "PUT",
        &path,
        b,
        json!({"revision":1,"fields":{"given_name":"Otra"}}),
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "DELETE",
        &format!("{path}?revision=1"),
        b,
        none.clone(),
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "POST",
        CONTACTS,
        a,
        json!({"given_name":"Mal","website":"javascript:alert(1)"}),
        StatusCode::BAD_REQUEST,
    )
    .await;
    call(
        &api,
        "POST",
        CONTACTS,
        a,
        json!({"email":"sin-nombre@example.com"}),
        StatusCode::BAD_REQUEST,
    )
    .await;
    call(
        &api,
        "POST",
        CONTACTS,
        a,
        json!({"given_name":"Nombre","email":"no es una dirección"}),
        StatusCode::BAD_REQUEST,
    )
    .await;
    call(
        &api,
        "POST",
        CONTACTS,
        a,
        json!({"company":"Zeta","notes":"Texto\ncon acentos"}),
        StatusCode::CREATED,
    )
    .await;

    let first = call(
        &api,
        "GET",
        &format!("{CONTACTS}?limit=1"),
        a,
        none.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["items"][0]["id"], ana["id"]);
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", first["next_cursor"].as_str().unwrap())
        .finish();
    let second = call(
        &api,
        "GET",
        &format!("{CONTACTS}?{query}"),
        a,
        none.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(second["items"][0]["display_name"], "Zeta");
    let search = call(
        &api,
        "GET",
        &format!("{CONTACTS}?q=%5F"),
        a,
        none.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(search["items"].as_array().unwrap().len(), 1);

    let marked = call(
        &api,
        "PATCH",
        &format!("{path}/favorite"),
        a,
        json!({"revision":1,"favorite":true}),
        StatusCode::OK,
    )
    .await;
    assert_eq!(marked["favorite"], true);
    assert_eq!(marked["revision"], 2);
    let favorites = call(
        &api,
        "GET",
        &format!("{CONTACTS}?favorite=true"),
        a,
        none.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(favorites["items"].as_array().unwrap().len(), 1);
    call(
        &api,
        "PUT",
        &path,
        a,
        json!({"revision":1,"fields":{"given_name":"Otro"}}),
        StatusCode::CONFLICT,
    )
    .await;
    let changed = call(
        &api,
        "PUT",
        &path,
        a,
        json!({"revision":2,"fields":{"given_name":"Ana","company":"Equipo"}}),
        StatusCode::OK,
    )
    .await;
    assert_eq!(changed["revision"], 3);
    assert_eq!(changed["favorite"], true);
    call(
        &api,
        "DELETE",
        &format!("{path}?revision=2"),
        a,
        none.clone(),
        StatusCode::CONFLICT,
    )
    .await;
    call(
        &api,
        "DELETE",
        &format!("{path}?revision=3"),
        a,
        none.clone(),
        StatusCode::NO_CONTENT,
    )
    .await;

    sqlx::query(
        "INSERT INTO public.service_user_denials(user_id, service_id) VALUES ($1, 'contacts')",
    )
    .bind(bob_id)
    .execute(&db)
    .await
    .unwrap();
    call(&api, "GET", CONTACTS, b, none, StatusCode::FORBIDDEN).await;
}
