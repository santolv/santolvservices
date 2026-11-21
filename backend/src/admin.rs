use crate::{
    App,
    auth::{self, Admin},
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}
pub async fn bootstrap(s: &App, c: Credentials) -> Result<Uuid> {
    let username = auth::username(&c.username)?;
    let ph = auth::password_hash(s, c.password).await?;
    let mut tx = s.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('bootstrap-admin',0))")
        .execute(&mut *tx)
        .await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE account_type='admin')")
            .fetch_one(&mut *tx)
            .await?;
    if exists {
        return Err(Error::conflict("administrator_already_initialized"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,username,password_hash,account_type) VALUES($1,$2,$3,'admin')",
    )
    .bind(id)
    .bind(username)
    .bind(ph)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, id, id, "administrator_initialized").await?;
    tx.commit().await?;
    Ok(id)
}
pub async fn local_reset(s: &App, c: Credentials) -> Result<()> {
    let ph = auth::password_hash(s, c.password).await?;
    let mut tx = s.db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM users WHERE username=$1 AND account_type='admin' FOR UPDATE",
    )
    .bind(c.username.trim().to_ascii_lowercase())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::denied)?;
    sqlx::query(
        "UPDATE users SET password_hash=$1,disabled=false,must_change_password=false WHERE id=$2",
    )
    .bind(ph)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, id, id, "administrator_reset_from_console").await?;
    tx.commit().await?;
    Ok(())
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    target: Uuid,
    action: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO admin_audit(actor,target,action) VALUES($1,$2,$3)")
        .bind(actor)
        .bind(target)
        .bind(action)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
#[derive(Deserialize)]
pub struct Page {
    after: Option<Uuid>,
}
pub async fn list(State(s): State<App>, _: Admin, Query(q): Query<Page>) -> Result<Json<Value>> {
    let mut v:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'username',username,'disabled',disabled,'must_change_password',must_change_password,'created_at',created_at) FROM users WHERE account_type='user' AND id>$1 ORDER BY id LIMIT 101").bind(q.after.unwrap_or(Uuid::nil())).fetch_all(&s.db).await?;
    let next = if v.len() > 100 {
        v[99]["id"].clone()
    } else {
        Value::Null
    };
    v.truncate(100);
    Ok(Json(json!({"items":v,"next_cursor":next})))
}
pub async fn create(
    State(s): State<App>,
    a: Admin,
    Json(c): Json<Credentials>,
) -> Result<(StatusCode, Json<Value>)> {
    auth::rate(&s, format!("create-user:{}", a.id), 100).await?;
    let username = auth::username(&c.username)?;
    let ph = auth::password_hash(&s, c.password).await?;
    let mut tx = s.db.begin().await?;
    let id = Uuid::new_v4();
    let space = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,username,password_hash,must_change_password) VALUES($1,$2,$3,true)",
    )
    .bind(id)
    .bind(username)
    .bind(ph)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO spaces(id,name,kind,owner_id,quota) VALUES($1,'Personal','personal',$2,$3)",
    )
    .bind(space)
    .bind(id)
    .bind(s.quota)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO members VALUES($1,$2,3)")
        .bind(space)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, a.id, id, "user_created").await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id":id,"space_id":space,"must_change_password":true})),
    ))
}
#[derive(Deserialize)]
pub struct Disable {
    disabled: bool,
}
pub async fn set_disabled(
    State(s): State<App>,
    a: Admin,
    Path(id): Path<Uuid>,
    Json(c): Json<Disable>,
) -> Result<StatusCode> {
    let mut tx = s.db.begin().await?;
    let r = sqlx::query("UPDATE users SET disabled=$2 WHERE id=$1 AND account_type='user'")
        .bind(id)
        .bind(c.disabled)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(Error::denied());
    }
    if c.disabled {
        sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut tx,
        a.id,
        id,
        if c.disabled {
            "user_disabled"
        } else {
            "user_enabled"
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct Reset {
    password: String,
}
pub async fn reset_password(
    State(s): State<App>,
    a: Admin,
    Path(id): Path<Uuid>,
    Json(c): Json<Reset>,
) -> Result<StatusCode> {
    auth::rate(&s, format!("reset-user:{}", a.id), 100).await?;
    let ph = auth::password_hash(&s, c.password).await?;
    let mut tx = s.db.begin().await?;
    let r=sqlx::query("UPDATE users SET password_hash=$1,must_change_password=true WHERE id=$2 AND account_type='user'").bind(ph).bind(id).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(Error::denied());
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, a.id, id, "password_reset").await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct AuditPage {
    after: Option<i64>,
}
pub async fn audit_log(
    State(s): State<App>,
    _: Admin,
    Query(q): Query<AuditPage>,
) -> Result<Json<Value>> {
    let mut v: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM admin_audit a WHERE id>$1 ORDER BY id LIMIT 101",
    )
    .bind(q.after.unwrap_or(0))
    .fetch_all(&s.db)
    .await?;
    let next = if v.len() > 100 {
        v[99]["id"].clone()
    } else {
        Value::Null
    };
    v.truncate(100);
    Ok(Json(json!({"items":v,"next_cursor":next})))
}
