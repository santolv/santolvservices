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

const MESSAGES: &str = "/api/v1/mail/messages";

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
         VALUES ($1, $2, 'test', now() + interval '5 minutes')",
    )
    .bind(auth::hash(&token))
    .bind(id)
    .execute(db)
    .await
    .unwrap();
    (id, token)
}

async fn call(
    router: &Router,
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
    let response = router
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

fn app_state(db: PgPool, root: &std::path::Path) -> App {
    App {
        journal: App::journal_pool(&db),
        db,
        root: root.into(),
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
async fn local_drafts_respect_permissions_and_survive_router_recreation(db: PgPool) {
    let temporary = tempfile::tempdir().unwrap();
    let initial = router(app_state(db.clone(), temporary.path()));
    let (alice_id, alice) = user(&db, "mail-alice").await;
    let (_, bob) = user(&db, "mail-bob").await;
    let alice = Some(alice.as_str());
    let bob = Some(bob.as_str());
    let draft_list = format!("{MESSAGES}?folder=drafts");

    call(
        &initial,
        "GET",
        &draft_list,
        None,
        Value::Null,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    call(
        &initial,
        "GET",
        &draft_list,
        alice,
        Value::Null,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    manager::set_enabled(&db, "mail", true).await.unwrap();
    let status = call(
        &initial,
        "GET",
        "/api/v1/mail/status",
        alice,
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["transport"], "not_configured");

    let draft = json!({
        "subject":"Informe %_",
        "content":"El detalle del mensaje.",
        "to_addresses":["ana@example.test"],
        "cc_addresses":["bea@example.test"],
        "bcc_addresses":["cris@example.test"]
    });
    let created = call(
        &initial,
        "POST",
        MESSAGES,
        alice,
        draft.clone(),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["kind"], "draft");
    assert_eq!(created["revision"], 1);
    assert!(created.get("owner_id").is_none());
    let path = format!("{MESSAGES}/{}", created["id"].as_str().unwrap());

    drop(initial);
    let api = router(app_state(db.clone(), temporary.path()));
    let persisted = call(&api, "GET", &path, alice, Value::Null, StatusCode::OK).await;
    assert_eq!(persisted["bcc_addresses"], draft["bcc_addresses"]);
    assert_eq!(persisted["content"], draft["content"]);
    call(&api, "GET", &path, bob, Value::Null, StatusCode::NOT_FOUND).await;
    call(
        &api,
        "PATCH",
        &path,
        bob,
        json!({"revision":1,"action":"trash"}),
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "DELETE",
        &format!("{path}?revision=1"),
        bob,
        Value::Null,
        StatusCode::NOT_FOUND,
    )
    .await;
    let bobs_list = call(&api, "GET", &draft_list, bob, Value::Null, StatusCode::OK).await;
    assert!(bobs_list["items"].as_array().unwrap().is_empty());

    let literal = call(
        &api,
        "GET",
        &format!("{draft_list}&q=%25_"),
        alice,
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(literal["items"].as_array().unwrap().len(), 1);
    assert_eq!(literal["items"][0]["id"], created["id"]);
    let inbox = call(&api, "GET", MESSAGES, alice, Value::Null, StatusCode::OK).await;
    assert!(inbox["items"].as_array().unwrap().is_empty());

    let mut edited = draft.clone();
    edited["revision"] = json!(1);
    edited["subject"] = json!("Asunto actualizado");
    call(&api, "PUT", &path, alice, edited.clone(), StatusCode::OK).await;
    call(&api, "PUT", &path, alice, edited, StatusCode::CONFLICT).await;
    call(
        &api,
        "DELETE",
        &format!("{path}?revision=2"),
        alice,
        Value::Null,
        StatusCode::CONFLICT,
    )
    .await;

    for (revision, action) in [(2, "star"), (3, "archive"), (4, "trash"), (5, "restore")] {
        call(
            &api,
            "PATCH",
            &path,
            alice,
            json!({"revision":revision,"action":action}),
            StatusCode::OK,
        )
        .await;
    }
    for folder in ["archive", "starred"] {
        let result = call(
            &api,
            "GET",
            &format!("{MESSAGES}?folder={folder}"),
            alice,
            Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(result["items"][0]["id"], created["id"]);
    }
    let active = call(&api, "GET", &draft_list, alice, Value::Null, StatusCode::OK).await;
    assert!(active["items"].as_array().unwrap().is_empty());

    sqlx::query("INSERT INTO public.service_user_denials(user_id,service_id) VALUES ($1,'mail')")
        .bind(alice_id)
        .execute(&db)
        .await
        .unwrap();
    call(
        &api,
        "GET",
        &path,
        alice,
        Value::Null,
        StatusCode::FORBIDDEN,
    )
    .await;
    sqlx::query("DELETE FROM public.service_user_denials WHERE user_id=$1 AND service_id='mail'")
        .bind(alice_id)
        .execute(&db)
        .await
        .unwrap();

    call(
        &api,
        "PATCH",
        &path,
        alice,
        json!({"revision":6,"action":"trash"}),
        StatusCode::OK,
    )
    .await;
    call(
        &api,
        "DELETE",
        &format!("{path}?revision=7"),
        alice,
        Value::Null,
        StatusCode::NO_CONTENT,
    )
    .await;
    call(
        &api,
        "GET",
        &path,
        alice,
        Value::Null,
        StatusCode::NOT_FOUND,
    )
    .await;

    let mut invalid = draft;
    invalid["to_addresses"] = json!(["a@example.test\r\nBcc: outsider@example.test"]);
    call(
        &api,
        "POST",
        MESSAGES,
        alice,
        invalid,
        StatusCode::BAD_REQUEST,
    )
    .await;
}
