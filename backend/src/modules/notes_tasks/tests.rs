use std::{borrow::Cow, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, migrate::Migrator};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{App, auth, core::manager, router};

const NOTES: &str = "/api/v1/notes-tasks/notes";
const TASKS: &str = "/api/v1/notes-tasks/tasks";

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

#[sqlx::test]
async fn routes_use_core_permissions_and_keep_records_private(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let app = App {
        journal: App::journal_pool(&db),
        db: db.clone(),
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
    };
    let api = router(app);
    let (alice_id, alice) = user(&db, "notes-alice").await;
    let (bob_id, bob) = user(&db, "notes-bob").await;
    let alice = Some(alice.as_str());
    let bob = Some(bob.as_str());
    let empty = Value::Null;

    call(
        &api,
        "GET",
        NOTES,
        None,
        empty.clone(),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let missing = call(
        &api,
        "GET",
        NOTES,
        alice,
        empty.clone(),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(missing["error"]["code"], "service_not_installed");
    call(
        &api,
        "POST",
        "/api/v1/services",
        alice,
        json!({"id":"notes-tasks","enabled":true}),
        StatusCode::METHOD_NOT_ALLOWED,
    )
    .await;

    sqlx::query("INSERT INTO public.service_settings (id, enabled) VALUES ('notes-tasks', false)")
        .execute(&db)
        .await
        .unwrap();
    call(
        &api,
        "GET",
        NOTES,
        alice,
        empty.clone(),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    manager::set_enabled(&db, "notes-tasks", true)
        .await
        .unwrap();

    let first = call(
        &api,
        "POST",
        NOTES,
        alice,
        json!({"title":"  Idea  ","content":"100%_ útil!"}),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(first["title"], "Idea");
    assert!(first.get("owner_id").is_none());
    let id = first["id"].as_str().unwrap();
    let note_path = format!("{NOTES}/{id}");
    let second = call(
        &api,
        "POST",
        NOTES,
        alice,
        json!({"title":"Segunda","content":"100abc útil!"}),
        StatusCode::CREATED,
    )
    .await;
    let private = call(
        &api,
        "GET",
        &format!("{NOTES}?q=100%25_"),
        bob,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(private["items"], json!([]));
    let literal = call(
        &api,
        "GET",
        &format!("{NOTES}?q=100%25_"),
        alice,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(literal["items"][0]["id"], id);
    assert_eq!(literal["items"].as_array().unwrap().len(), 1);
    call(
        &api,
        "GET",
        &note_path,
        bob,
        empty.clone(),
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "PUT",
        &note_path,
        bob,
        json!({"title":"Ajena", "content":"", "archived":false}),
        StatusCode::NOT_FOUND,
    )
    .await;

    let page = call(
        &api,
        "GET",
        &format!("{NOTES}?limit=1"),
        alice,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["items"][0]["id"], second["id"]);
    let next = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("cursor", page["next_cursor"].as_str().unwrap())
        .append_pair("limit", "1")
        .finish();
    let page = call(
        &api,
        "GET",
        &format!("{NOTES}?{next}"),
        alice,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["items"][0]["id"], id);
    assert!(page["next_cursor"].is_null());

    let linked = call(
        &api,
        "POST",
        TASKS,
        alice,
        json!({"title":"Pendiente", "note_id":id, "due_at":"2026-11-27T10:00:00+01:00"}),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(linked["status"], "pending");
    assert_eq!(linked["priority"], "normal");
    assert_eq!(linked["due_at"], "2026-11-27T09:00:00Z");
    let task_path = format!("{TASKS}/{}", linked["id"].as_str().unwrap());
    call(
        &api,
        "POST",
        TASKS,
        bob,
        json!({"title":"Ajena","note_id":id}),
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "GET",
        &task_path,
        bob,
        empty.clone(),
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "DELETE",
        &task_path,
        bob,
        empty.clone(),
        StatusCode::NOT_FOUND,
    )
    .await;
    let list = call(
        &api,
        "GET",
        &format!("{TASKS}?status=pending&priority=normal&note_id={id}&q=PENDIENTE"),
        alice,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(list["items"], json!([linked]));

    let error = sqlx::query("UPDATE notes_tasks.tasks SET owner_id=$1 WHERE owner_id=$2")
        .bind(bob_id)
        .bind(alice_id)
        .execute(&db)
        .await
        .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    let archived = call(
        &api,
        "PUT",
        &note_path,
        alice,
        json!({"title":"Editada","content":"Nuevo texto","archived":true}),
        StatusCode::OK,
    )
    .await;
    assert!(archived["archived_at"].is_string());
    let archived_list = call(
        &api,
        "GET",
        &format!("{NOTES}?archived=true"),
        alice,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(archived_list["items"], json!([archived]));
    let stored = call(
        &api,
        "GET",
        &task_path,
        alice,
        empty.clone(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(stored["note_id"], id);
    let restored = call(
        &api,
        "PUT",
        &note_path,
        alice,
        json!({"title":"Editada","content":"Nuevo texto","archived":false}),
        StatusCode::OK,
    )
    .await;
    assert!(restored["archived_at"].is_null());
    call(
        &api,
        "PUT",
        &task_path,
        alice,
        json!({"title":"Tarea","status":"done","priority":"high","due_at":null}),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let updated = call(
        &api,
        "PUT",
        &task_path,
        alice,
        json!({"title":"Tarea","status":"done","priority":"high","due_at":null,"note_id":null}),
        StatusCode::OK,
    )
    .await;
    assert!(updated["note_id"].is_null() && updated["due_at"].is_null());
    assert_eq!(updated["status"], "done");
    call(
        &api,
        "DELETE",
        &task_path,
        alice,
        empty.clone(),
        StatusCode::NO_CONTENT,
    )
    .await;
    call(
        &api,
        "GET",
        &task_path,
        alice,
        empty.clone(),
        StatusCode::NOT_FOUND,
    )
    .await;

    let large = call(
        &api,
        "POST",
        NOTES,
        alice,
        json!({"title":"Unicode","content":"🦀".repeat(50_000)}),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(large["content"].as_str().unwrap().chars().count(), 50_000);
    call(
        &api,
        "POST",
        NOTES,
        alice,
        json!({"title":"x","content":"a".repeat(50_001)}),
        StatusCode::BAD_REQUEST,
    )
    .await;

    sqlx::query(
        "INSERT INTO public.service_user_denials (user_id, service_id) VALUES ($1, 'notes-tasks')",
    )
    .bind(alice_id)
    .execute(&db)
    .await
    .unwrap();
    call(&api, "GET", NOTES, alice, empty, StatusCode::FORBIDDEN).await;
}

#[sqlx::test(migrations = false)]
async fn migration_preserves_existing_accounts_and_service_settings(db: PgPool) {
    let full = sqlx::migrate!("./migrations");
    let previous = Migrator {
        migrations: Cow::Owned(
            full.iter()
                .filter(|migration| migration.version < 19)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    previous.run(&db).await.unwrap();
    let (owner, _) = user(&db, "notes-existing").await;
    full.run(&db).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM public.users WHERE username='notes-existing'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        owner
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT id FROM public.service_settings ORDER BY id")
            .fetch_all(&db)
            .await
            .unwrap(),
        ["drive", "photos"]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notes_tasks.notes")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
}
