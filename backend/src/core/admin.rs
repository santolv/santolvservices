use crate::{
    App, auth,
    error::{Error, Result},
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
/// Only called by the local console; never registered as an HTTP handler.
pub async fn console(
    s: &App,
    args: &[String],
) -> std::result::Result<Value, Box<dyn std::error::Error>> {
    let actor: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE account_type='admin'")
        .fetch_one(&s.db)
        .await?;
    match args {
        [action] if action == "list" => Ok(list(s, None).await?),
        [action, after] if action == "list" => Ok(list(s, Some(after.parse()?)).await?),
        [action] if action == "audit" => Ok(audit_log(s, None).await?),
        [action, after] if action == "audit" => Ok(audit_log(s, Some(after.parse()?)).await?),
        [action] | [action, _] if (action == "create" || action == "reset-password") &&
            (args.len() == 1 || args[1] == "--stdin") => {
            let c = if args.len() == 2 { crate::installer::stdin_credentials()? }
                else { crate::installer::credentials_for("Usuario de Drive: ")? };
            if action == "create" { return Ok(create(s, actor, c).await?); }
            let id = user_id(s, &c.username).await?;
            reset_password(s, actor, id, c.password).await?;
            Ok(json!({"updated":true,"must_change_password":true}))
        }
        [action, username] if action == "disable" || action == "enable" => {
            let id = user_id(s, username).await?;
            set_disabled(s, actor, id, action == "disable").await?;
            Ok(json!({"updated":true,"disabled":action == "disable"}))
        }
        [action, username] if action == "modules" => {
            let id = user_id(s, username).await?;
            let denied = sqlx::query_scalar::<_, String>("SELECT service_id FROM public.service_user_denials WHERE user_id=$1 ORDER BY service_id")
                .bind(id).fetch_all(&s.db).await?;
            Ok(json!({"username":auth::username(username)?,"denied":denied,
                "available":crate::core::manager::accessible(&s.db, id).await?}))
        }
        [action, username, module] if action == "deny-module" || action == "allow-module" => {
            let id = user_id(s, username).await?;
            if !crate::modules::REGISTERED.iter().any(|service| service.id == module) {
                return Err(Error::denied().into());
            }
            let mut tx = s.db.begin().await?;
            let installed = sqlx::query_scalar::<_, String>("SELECT id FROM public.service_settings WHERE id=$1 FOR KEY SHARE")
                .bind(module).fetch_optional(&mut *tx).await?.is_some();
            if !installed { return Err(Error::denied().into()); }
            let denied = action == "deny-module";
            let query = if denied {
                "INSERT INTO public.service_user_denials(user_id, service_id) VALUES($1,$2) ON CONFLICT DO NOTHING"
            } else {
                "DELETE FROM public.service_user_denials WHERE user_id=$1 AND service_id=$2"
            };
            let result = sqlx::query(query).bind(id).bind(module).execute(&mut *tx).await?;
            if result.rows_affected() != 0 {
                audit(&mut tx, actor, id, &format!("{action}:{module}")).await?;
            }
            tx.commit().await?;
            Ok(json!({"module":module,"denied":denied}))
        }
        _ => Err("usage: users [list | create | disable USERNAME | enable USERNAME | reset-password | audit | modules USERNAME | deny-module USERNAME ID | allow-module USERNAME ID]".into()),
    }
}
async fn user_id(s: &App, username: &str) -> Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM users WHERE username=$1 AND account_type='user'")
        .bind(auth::username(username)?)
        .fetch_optional(&s.db)
        .await?
        .ok_or_else(Error::denied)
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
pub async fn list(s: &App, after: Option<Uuid>) -> Result<Value> {
    let mut v:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'username',username,'disabled',disabled,'must_change_password',must_change_password,'created_at',created_at) FROM users WHERE account_type='user' AND id>$1 ORDER BY id LIMIT 101").bind(after.unwrap_or(Uuid::nil())).fetch_all(&s.db).await?;
    let next = if v.len() > 100 {
        v[99]["id"].clone()
    } else {
        Value::Null
    };
    v.truncate(100);
    Ok(json!({"items":v,"next_cursor":next}))
}
pub async fn create(s: &App, actor: Uuid, c: Credentials) -> Result<Value> {
    auth::rate(s, format!("create-user:{actor}"), 100).await?;
    let username = auth::username(&c.username)?;
    let ph = auth::password_hash(s, c.password).await?;
    let mut tx = s.db.begin().await?;
    let id = Uuid::new_v4();
    let space = Uuid::new_v4();
    let directory = crate::storage::user_directory(&username, id);
    sqlx::query(
        "INSERT INTO users(id,username,password_hash,must_change_password) VALUES($1,$2,$3,true)",
    )
    .bind(id)
    .bind(username)
    .bind(ph)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO spaces(id,name,kind,owner_id,quota,storage_directory) VALUES($1,'Personal','personal',$2,$3,$4)",
    )
    .bind(space)
    .bind(id)
    .bind(s.quota)
    .bind(&directory)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO members VALUES($1,$2,3)")
        .bind(space)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, actor, id, "user_created").await?;
    crate::storage::prepare_space(&s.root, &directory).await?;
    tx.commit().await?;
    Ok(json!({"id":id,"space_id":space,"must_change_password":true}))
}
pub async fn set_disabled(s: &App, actor: Uuid, id: Uuid, disabled: bool) -> Result<()> {
    let mut tx = s.db.begin().await?;
    let r = sqlx::query("UPDATE users SET disabled=$2 WHERE id=$1 AND account_type='user'")
        .bind(id)
        .bind(disabled)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(Error::denied());
    }
    if disabled {
        sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut tx,
        actor,
        id,
        if disabled {
            "user_disabled"
        } else {
            "user_enabled"
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn reset_password(s: &App, actor: Uuid, id: Uuid, password: String) -> Result<()> {
    auth::rate(s, format!("reset-user:{actor}"), 100).await?;
    let ph = auth::password_hash(s, password).await?;
    let mut tx = s.db.begin().await?;
    let r=sqlx::query("UPDATE users SET password_hash=$1,must_change_password=true WHERE id=$2 AND account_type='user'").bind(ph).bind(id).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(Error::denied());
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, actor, id, "password_reset").await?;
    tx.commit().await?;
    Ok(())
}
pub async fn audit_log(s: &App, after: Option<i64>) -> Result<Value> {
    let mut v: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM admin_audit a WHERE id>$1 ORDER BY id LIMIT 101",
    )
    .bind(after.unwrap_or(0))
    .fetch_all(&s.db)
    .await?;
    let next = if v.len() > 100 {
        v[99]["id"].clone()
    } else {
        Value::Null
    };
    v.truncate(100);
    Ok(json!({"items":v,"next_cursor":next}))
}
