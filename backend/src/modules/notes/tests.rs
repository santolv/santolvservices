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

use crate::{App, auth, core::manager, frontend, router};

const NOTES: &str = "/api/v1/notes";
const TASKS: &str = "/api/v1/tasks";

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
async fn routes_use_core_permissions_and_keep_records_private(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let api = router(app_state(db.clone(), &root));
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
    call(
        &api,
        "POST",
        "/api/v1/services",
        alice,
        json!({"id":"notes","enabled":true}),
        StatusCode::METHOD_NOT_ALLOWED,
    )
    .await;

    let disabled = call(
        &api,
        "GET",
        NOTES,
        alice,
        empty.clone(),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(disabled["error"]["code"], "service_disabled");
    manager::set_enabled(&db, "tasks", true).await.unwrap();
    manager::set_enabled(&db, "notes", true).await.unwrap();

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

    let error = sqlx::query("UPDATE tasks.tasks SET owner_id=$1 WHERE owner_id=$2")
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
        "INSERT INTO public.service_user_denials (user_id, service_id) VALUES ($1, 'notes')",
    )
    .bind(alice_id)
    .execute(&db)
    .await
    .unwrap();
    call(&api, "GET", NOTES, alice, empty, StatusCode::FORBIDDEN).await;
}

#[sqlx::test]
async fn navigation_and_catalog_follow_core_activation_and_denials(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let app = app_state(db.clone(), &root);
    let api = router(app.clone());
    let pages = frontend::router(
        frontend::Frontend::bundled(&app.frontend_origin, &app.origin, app.clone()).unwrap(),
    );
    let (owner, token) = user(&db, "notes-navigation").await;
    let (_, other) = user(&db, "notes-other").await;
    let card = "class=\"service\" href=\"/notes/\"";

    let page = |path: &'static str, token: Option<&str>| {
        let pages = pages.clone();
        let mut request = Request::builder()
            .uri(path)
            .header("host", "localhost:5500");
        if let Some(token) = token {
            request = request.header("cookie", format!("drive_session={token}"));
        }
        async move {
            let response = pages
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            (status, String::from_utf8(body.to_vec()).unwrap())
        }
    };

    assert_eq!(page("/notes/", None).await.0, StatusCode::SEE_OTHER);
    assert_eq!(
        page("/notes/", Some(&token)).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let home = page("/", Some(&token)).await.1;
    assert!(!home.contains(card));

    let args = ["enable".to_owned(), "notes".to_owned()];
    manager::console(&app, &args).await.unwrap();
    let catalog = call(
        &api,
        "GET",
        "/api/v1/services",
        Some(&token),
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert!(
        catalog["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["id"] == "notes" && entry["enabled"] == true })
    );
    let home = page("/", Some(&token)).await.1;
    assert!(home.contains(card));
    let (status, body) = page("/notes/", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<h1>Notas</h1>"));

    sqlx::query(
        "INSERT INTO public.service_user_denials (user_id, service_id) VALUES ($1, 'notes')",
    )
    .bind(owner)
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(page("/notes/", Some(&token)).await.0, StatusCode::FORBIDDEN);
    let home = page("/", Some(&token)).await.1;
    assert!(!home.contains(card));
    assert_eq!(page("/notes/", Some(&other)).await.0, StatusCode::OK);
}

#[sqlx::test(migrations = false)]
async fn split_preserves_records_links_activation_and_denials(db: PgPool) {
    let full = sqlx::migrate!("./migrations");
    let previous = Migrator {
        migrations: Cow::Owned(full.iter().filter(|m| m.version < 21).cloned().collect()),
        ..Migrator::DEFAULT
    };
    previous.run(&db).await.unwrap();
    let (owner, _) = user(&db, "existing-records").await;
    let note = Uuid::new_v4();
    let task = Uuid::new_v4();
    sqlx::query("UPDATE public.service_settings SET enabled=true WHERE id='notes-tasks'")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO public.service_user_denials VALUES ($1,'notes-tasks')")
        .bind(owner)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO notes_tasks.notes(id,owner_id,title,content) VALUES($1,$2,'Idea','Contenido')",
    )
    .bind(note)
    .bind(owner)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO notes_tasks.tasks(id,owner_id,title,status,priority,note_id) VALUES($1,$2,'Hacer','pending','normal',$3)").bind(task).bind(owner).bind(note).execute(&db).await.unwrap();
    full.run(&db).await.unwrap();
    let row: (Uuid, Uuid) = sqlx::query_as("SELECT owner_id,note_id FROM tasks.tasks WHERE id=$1")
        .bind(task)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(row, (owner, note));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT content FROM notes.notes WHERE id=$1")
            .bind(note)
            .fetch_one(&db)
            .await
            .unwrap(),
        "Contenido"
    );
    for service in ["notes", "tasks"] {
        assert!(manager::enabled(&db, service).await.unwrap());
        assert_eq!(
            manager::require_access(&db, service, owner)
                .await
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM public.service_settings WHERE id='notes-tasks'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        0
    );
}

#[sqlx::test]
async fn services_switch_independently_and_preserve_links(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let api = router(app_state(db.clone(), &root));
    let (owner, token) = user(&db, "independent").await;
    let token = Some(token.as_str());
    manager::set_enabled(&db, "notes", true).await.unwrap();
    manager::set_enabled(&db, "tasks", true).await.unwrap();
    let note = call(
        &api,
        "POST",
        NOTES,
        token,
        json!({"title":"Nota","content":"Privada"}),
        StatusCode::CREATED,
    )
    .await;
    let task = call(
        &api,
        "POST",
        TASKS,
        token,
        json!({"title":"Tarea","note_id":note["id"]}),
        StatusCode::CREATED,
    )
    .await;
    manager::set_enabled(&db, "notes", false).await.unwrap();
    call(
        &api,
        "GET",
        NOTES,
        token,
        Value::Null,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    call(&api, "GET", TASKS, token, Value::Null, StatusCode::OK).await;
    call(
        &api,
        "POST",
        TASKS,
        token,
        json!({"title":"Independiente"}),
        StatusCode::CREATED,
    )
    .await;
    call(&api,"PUT",&format!("{}/{}",TASKS,task["id"].as_str().unwrap()),token,json!({"title":"Editada","status":"done","priority":"normal","due_at":null,"note_id":note["id"]}),StatusCode::OK).await;
    call(
        &api,
        "POST",
        TASKS,
        token,
        json!({"title":"Nuevo vínculo","note_id":note["id"]}),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    manager::set_enabled(&db, "notes", true).await.unwrap();
    manager::set_enabled(&db, "tasks", false).await.unwrap();
    call(&api, "GET", NOTES, token, Value::Null, StatusCode::OK).await;
    call(
        &api,
        "GET",
        TASKS,
        token,
        Value::Null,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    manager::set_enabled(&db, "tasks", true).await.unwrap();
    sqlx::query("INSERT INTO public.service_user_denials VALUES ($1,'tasks')")
        .bind(owner)
        .execute(&db)
        .await
        .unwrap();
    call(&api, "GET", NOTES, token, Value::Null, StatusCode::OK).await;
    call(
        &api,
        "GET",
        TASKS,
        token,
        Value::Null,
        StatusCode::FORBIDDEN,
    )
    .await;
    call(
        &api,
        "GET",
        "/api/v1/notes-tasks/tasks",
        token,
        Value::Null,
        StatusCode::FORBIDDEN,
    )
    .await;
}
#[sqlx::test]
async fn calendar_links_are_personal_and_recheck_both_services(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let api = router(app_state(db.clone(), &root));
    let (alice_id, a) = user(&db, "link-alice").await;
    let (bob_id, b) = user(&db, "link-bob").await;
    let a = Some(a.as_str());
    let b = Some(b.as_str());
    for service in ["notes", "tasks", "calendar"] {
        manager::set_enabled(&db, service, true).await.unwrap();
    }
    let note = call(
        &api,
        "POST",
        NOTES,
        a,
        json!({"title":"Privada","content":"Secreto"}),
        StatusCode::CREATED,
    )
    .await;
    let task = call(
        &api,
        "POST",
        TASKS,
        a,
        json!({"title":"Vinculada","note_id":note["id"]}),
        StatusCode::CREATED,
    )
    .await;
    let event=call(&api,"POST","/api/v1/calendar/events",a,json!({"title":"Agenda","description":"","local_start":"2026-12-03T09:00","local_end":"2026-12-03T10:00","zone":"UTC"}),StatusCode::CREATED).await;
    let event_id = event["id"].as_str().unwrap();
    let notes = format!("/api/v1/calendar/events/{event_id}/links/notes");
    let tasks = format!("/api/v1/calendar/events/{event_id}/links/tasks");
    call(
        &api,
        "PUT",
        &notes,
        a,
        json!({"id":note["id"]}),
        StatusCode::OK,
    )
    .await;
    call(
        &api,
        "PUT",
        &tasks,
        a,
        json!({"id":task["id"]}),
        StatusCode::OK,
    )
    .await;
    call(
        &api,
        "GET",
        &notes,
        None,
        Value::Null,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    call(&api, "GET", &notes, b, Value::Null, StatusCode::NOT_FOUND).await;
    sqlx::query("INSERT INTO calendar.members(calendar_id,user_id,role) VALUES($1,$2,'reader')")
        .bind(Uuid::parse_str(event["calendar_id"].as_str().unwrap()).unwrap())
        .bind(bob_id)
        .execute(&db)
        .await
        .unwrap();
    assert!(call(&api, "GET", &notes, b, Value::Null, StatusCode::OK).await["id"].is_null());
    call(
        &api,
        "PUT",
        &notes,
        b,
        json!({"id":note["id"]}),
        StatusCode::NOT_FOUND,
    )
    .await;
    let own = call(
        &api,
        "POST",
        NOTES,
        b,
        json!({"title":"De Bob","content":"Otra"}),
        StatusCode::CREATED,
    )
    .await;
    call(
        &api,
        "PUT",
        &notes,
        b,
        json!({"id":own["id"]}),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        call(&api, "GET", &notes, a, Value::Null, StatusCode::OK).await["id"],
        note["id"]
    );
    manager::set_enabled(&db, "notes", false).await.unwrap();
    call(
        &api,
        "GET",
        &notes,
        a,
        Value::Null,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(
        call(&api, "GET", &tasks, a, Value::Null, StatusCode::OK).await["id"],
        task["id"]
    );
    manager::set_enabled(&db, "notes", true).await.unwrap();
    sqlx::query("INSERT INTO public.service_user_denials VALUES($1,'notes')")
        .bind(alice_id)
        .execute(&db)
        .await
        .unwrap();
    call(&api, "GET", &notes, a, Value::Null, StatusCode::FORBIDDEN).await;
    sqlx::query("DELETE FROM public.service_user_denials WHERE user_id=$1")
        .bind(alice_id)
        .execute(&db)
        .await
        .unwrap();
    call(
        &api,
        "DELETE",
        &format!("{NOTES}/{}", note["id"].as_str().unwrap()),
        b,
        Value::Null,
        StatusCode::NOT_FOUND,
    )
    .await;
    call(
        &api,
        "DELETE",
        &format!("{NOTES}/{}", note["id"].as_str().unwrap()),
        a,
        Value::Null,
        StatusCode::NO_CONTENT,
    )
    .await;
    assert!(call(&api, "GET", &notes, a, Value::Null, StatusCode::OK).await["id"].is_null());
    assert!(
        call(
            &api,
            "GET",
            &format!("{TASKS}/{}", task["id"].as_str().unwrap()),
            a,
            Value::Null,
            StatusCode::OK
        )
        .await["note_id"]
            .is_null()
    );
    manager::set_enabled(&db, "calendar", false).await.unwrap();
    call(
        &api,
        "GET",
        &tasks,
        a,
        Value::Null,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
}

#[sqlx::test]
async fn task_archive_is_persistent_private_and_independent_of_completion(db: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let api = router(app_state(db.clone(), &root));
    let (_, a) = user(&db, "archive-alice").await;
    let (_, b) = user(&db, "archive-bob").await;
    let a = Some(a.as_str());
    let b = Some(b.as_str());
    manager::set_enabled(&db, "tasks", true).await.unwrap();
    let task = call(
        &api,
        "POST",
        TASKS,
        a,
        json!({"title":"Para archivar"}),
        StatusCode::CREATED,
    )
    .await;
    let path = format!("{TASKS}/{}", task["id"].as_str().unwrap());
    let mut input = json!({"title":"Para archivar","status":"done","priority":"normal","due_at":null,"note_id":null,"archived":true});
    call(&api, "PUT", &path, b, input.clone(), StatusCode::NOT_FOUND).await;
    let archived = call(&api, "PUT", &path, a, input.clone(), StatusCode::OK).await;
    assert!(archived["archived_at"].is_string());
    assert!(
        call(&api, "GET", TASKS, a, Value::Null, StatusCode::OK).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        call(
            &api,
            "GET",
            &format!("{TASKS}?archived=true"),
            a,
            Value::Null,
            StatusCode::OK
        )
        .await["items"][0]["id"],
        task["id"]
    );
    input["archived"] = json!(false);
    call(&api, "PUT", &path, a, input, StatusCode::OK).await;
    assert_eq!(
        call(&api, "GET", TASKS, a, Value::Null, StatusCode::OK).await["items"][0]["status"],
        "done"
    );
}
