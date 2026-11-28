//! Authorization for collaborative sessions. Every decision comes from the server's own records:
//! Drive's permission on the file (owner, space membership or grant, trash, disabled accounts)
//! and the session row. Nothing the client sends (user ids, roles) is trusted.

use super::protocol::Role;
use crate::{App, error::Result, file_store::files};
use uuid::Uuid;

/// The caller's role on a Drive file, or `None` when they may not open it at all (missing, a
/// folder, in the trash, being purged, or no permission). Uses Drive's own access check.
pub async fn role(app: &App, node: Uuid, user: Uuid) -> Result<Option<Role>> {
    for (required, role) in [(3, Role::Admin), (2, Role::Editor), (1, Role::Reader)] {
        let mut tx = app.db.begin().await?;
        match files::access(&mut tx, node, user, required, false).await {
            Ok(found) if found.kind == "file" => return Ok(Some(role)),
            Ok(_) => return Ok(None),
            Err(error) if error.0 == axum::http::StatusCode::NOT_FOUND => continue,
            Err(error) if error.0 == axum::http::StatusCode::CONFLICT => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// Whether the session that opened the connection is still valid: not logged out, not expired,
/// and its account still active and not forced to change password.
pub async fn session_alive(app: &App, session_hash: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id
         WHERE s.token_hash=$1 AND s.expires_at>now() AND NOT u.disabled
           AND NOT u.must_change_password AND u.account_type='user')",
    )
    .bind(session_hash)
    .fetch_one(&app.db)
    .await?)
}

/// The name shown to collaborators, from the account (never from the client).
pub async fn display_name(app: &App, user: Uuid) -> Result<(String, String)> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT username, NULLIF(btrim(display_name),''), avatar_color FROM users WHERE id=$1",
    )
    .bind(user)
    .fetch_optional(&app.db)
    .await?;
    Ok(row.map_or_else(
        || ("".into(), "blue".into()),
        |(username, name, color)| {
            (
                name.unwrap_or(username),
                color.unwrap_or_else(|| "blue".into()),
            )
        },
    ))
}
