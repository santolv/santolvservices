use crate::{
    App,
    error::{Error, Result},
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{ConnectInfo, FromRequestParts, State},
    http::{HeaderMap, StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

pub fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
pub fn token() -> String {
    let mut b = [0; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}
pub fn username(value: &str) -> Result<String> {
    let v = value.trim().to_ascii_lowercase();
    if !(3..=64).contains(&v.len())
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(Error::bad(
            "username_3_to_64_ascii_letters_digits_dot_dash_underscore",
        ));
    }
    Ok(v)
}
pub fn validate_password(p: &str) -> Result<()> {
    if !(12..=1024).contains(&p.len()) {
        return Err(Error::bad("password_length_12_to_1024_bytes"));
    }
    Ok(())
}
pub async fn password_hash(s: &App, password: String) -> Result<String> {
    validate_password(&password)?;
    let permit = s
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "password_capacity"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|p| p.to_string())
            .map_err(|_| Error::unavailable())
    })
    .await
    .map_err(|_| Error::unavailable())?
}
#[derive(Clone)]
pub struct Principal {
    pub id: Uuid,
    pub session: String,
    pub account_type: String,
    pub must_change_password: bool,
}
pub struct User {
    pub id: Uuid,
}
pub struct Admin {
    pub id: Uuid,
}
impl FromRequestParts<App> for Principal {
    type Rejection = Error;
    async fn from_request_parts(p: &mut Parts, s: &App) -> Result<Self> {
        let bearer = p
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let cookie = p
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.split(';')
                    .find_map(|c| c.trim().strip_prefix("drive_session="))
            });
        let raw = cookie
            .or(bearer)
            .filter(|v| v.len() == 64)
            .ok_or_else(Error::auth)?;
        let session = hash(raw);
        let r=sqlx::query("SELECT s.user_id,s.csrf_hash,u.account_type,u.must_change_password FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.expires_at>now() AND NOT u.disabled").bind(&session).fetch_optional(&s.db).await?.ok_or_else(Error::auth)?;
        if cookie.is_some()
            && !matches!(
                p.method,
                axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
            )
        {
            let csrf = p
                .headers
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if csrf.len() != 64 || hash(csrf) != r.get::<String, _>("csrf_hash") {
                return Err(Error(StatusCode::FORBIDDEN, "csrf_required"));
            }
        }
        Ok(Self {
            id: r.get("user_id"),
            session,
            account_type: r.get("account_type"),
            must_change_password: r.get("must_change_password"),
        })
    }
}
impl FromRequestParts<App> for User {
    type Rejection = Error;
    async fn from_request_parts(p: &mut Parts, s: &App) -> Result<Self> {
        let i = Principal::from_request_parts(p, s).await?;
        if i.account_type != "user" {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "administrator_has_no_drive_access",
            ));
        }
        if i.must_change_password {
            return Err(Error(StatusCode::FORBIDDEN, "password_change_required"));
        }
        Ok(Self { id: i.id })
    }
}
impl FromRequestParts<App> for Admin {
    type Rejection = Error;
    async fn from_request_parts(p: &mut Parts, s: &App) -> Result<Self> {
        let i = Principal::from_request_parts(p, s).await?;
        if i.account_type != "admin" || i.must_change_password {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "server_administrator_required",
            ));
        }
        Ok(Self { id: i.id })
    }
}
pub async fn security(State(s): State<App>, req: axum::extract::Request, next: Next) -> Response {
    let _permit = match s.requests.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return Error(StatusCode::TOO_MANY_REQUESTS, "request_capacity").into_response(),
    };
    if let Some(origin) = req.headers().get("origin")
        && origin.to_str().ok() != Some(&s.origin)
    {
        return Error(StatusCode::FORBIDDEN, "origin_not_allowed").into_response();
    }
    if req
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return Error(StatusCode::FORBIDDEN, "cross_site_denied").into_response();
    }
    let timeout = if req.uri().path().starts_with("/api/v1/uploads/") {
        150
    } else {
        30
    };
    let mut response =
        match tokio::time::timeout(std::time::Duration::from_secs(timeout), next.run(req)).await {
            Ok(r) => r,
            Err(_) => Error(StatusCode::REQUEST_TIMEOUT, "request_timeout").into_response(),
        };
    if (response.status().is_client_error() || response.status().is_server_error())
        && !response
            .headers()
            .get("content-type")
            .is_some_and(|v| v == "application/json")
    {
        response = Error(response.status(), "invalid_request").into_response();
    }
    for (k, v) in [
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
    ] {
        response.headers_mut().insert(k, v.parse().unwrap());
    }
    if s.secure {
        response.headers_mut().insert(
            "strict-transport-security",
            "max-age=31536000".parse().unwrap(),
        );
    }
    response
}

pub async fn rate(s: &App, key: String, max: i32) -> Result<()> {
    let n:i32=sqlx::query_scalar("INSERT INTO rate_limits(key) VALUES($1) ON CONFLICT(key) DO UPDATE SET count=CASE WHEN rate_limits.window_start<now()-interval '15 minutes' THEN 1 ELSE LEAST(rate_limits.count+1,100000) END,window_start=CASE WHEN rate_limits.window_start<now()-interval '15 minutes' THEN now() ELSE rate_limits.window_start END RETURNING count").bind(key).fetch_one(&s.db).await?;
    if n > max {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    Ok(())
}
#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
    #[serde(default)]
    mobile: bool,
}
pub async fn login(
    State(s): State<App>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(c): Json<Login>,
) -> Result<Response> {
    let username = c.username.trim().to_lowercase();
    if username.is_empty() || username.len() > 254 || c.password.len() > 1024 {
        return Err(Error::auth());
    }
    rate(&s, format!("login-ip:{}", addr.ip()), 50).await?;
    rate(&s, format!("login-account:{}", hash(&username)), 15).await?;
    if c.mobile && (headers.contains_key("origin") || headers.contains_key("sec-fetch-site")) {
        return Err(Error::bad("browser_requires_cookie"));
    }
    let row = sqlx::query("SELECT id,password_hash FROM users WHERE username=$1 AND NOT disabled")
        .bind(username)
        .fetch_optional(&s.db)
        .await?;
    let ph = row.as_ref().map(|r| r.get::<String, _>("password_hash"));
    let permit = s
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "password_capacity"))?;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if let Some(ph) = ph {
            PasswordHash::new(&ph).ok().is_some_and(|h| {
                Argon2::default()
                    .verify_password(c.password.as_bytes(), &h)
                    .is_ok()
            })
        } else {
            let _ = Argon2::default()
                .hash_password(c.password.as_bytes(), &SaltString::generate(&mut OsRng));
            false
        }
    })
    .await
    .map_err(|_| Error::unavailable())?;
    if !verified {
        return Err(Error::auth());
    }
    let row = row.ok_or_else(Error::auth)?;
    let id: Uuid = row.get("id");
    let mut tx = s.db.begin().await?;
    let current=sqlx::query("SELECT password_hash,account_type,must_change_password,disabled FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
    if current.get::<String, _>("password_hash") != row.get::<String, _>("password_hash")
        || current.get::<bool, _>("disabled")
    {
        return Err(Error::auth());
    }
    let kind: String = current.get("account_type");
    let expires: i64 = if kind == "admin" { 1800 } else { 604800 };
    let raw = token();
    let csrf = token();
    sqlx::query("INSERT INTO sessions VALUES($1,$2,$3,now()+$4*interval '1 second')")
        .bind(hash(&raw))
        .bind(id)
        .bind(hash(&csrf))
        .bind(expires)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut body = json!({"account_type":kind,"must_change_password":current.get::<bool,_>("must_change_password"),"expires_in":expires});
    if c.mobile {
        body["token"] = json!(raw);
    } else {
        body["csrf_token"] = json!(csrf);
    }
    let mut r = Json(body).into_response();
    if !c.mobile {
        r.headers_mut().insert(
            "set-cookie",
            format!(
                "drive_session={raw}; Path=/; HttpOnly; SameSite=Strict; Max-Age={expires}{}",
                if s.secure { "; Secure" } else { "" }
            )
            .parse()
            .unwrap(),
        );
    }
    Ok(r)
}
pub async fn logout(State(s): State<App>, u: Principal) -> Result<Response> {
    sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
        .bind(u.session)
        .execute(&s.db)
        .await?;
    let mut r = StatusCode::NO_CONTENT.into_response();
    r.headers_mut().insert(
        "set-cookie",
        format!(
            "drive_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
            if s.secure { "; Secure" } else { "" }
        )
        .parse()
        .unwrap(),
    );
    Ok(r)
}
pub async fn me(State(s): State<App>, u: Principal) -> Result<Json<Value>> {
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id=$1")
        .bind(u.id)
        .fetch_one(&s.db)
        .await?;
    Ok(Json(
        json!({"id":u.id,"username":username,"account_type":u.account_type,"must_change_password":u.must_change_password}),
    ))
}
#[derive(Deserialize)]
pub struct PasswordChange {
    current_password: String,
    new_password: String,
}
pub async fn change_password(
    State(s): State<App>,
    u: Principal,
    Json(c): Json<PasswordChange>,
) -> Result<StatusCode> {
    rate(&s, format!("change-password:{}", u.id), 10).await?;
    if c.current_password.len() > 1024 || c.current_password == c.new_password {
        return Err(Error::bad("invalid_password_change"));
    }
    let new = password_hash(&s, c.new_password).await?;
    let mut tx = s.db.begin().await?;
    let old: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1 FOR UPDATE")
        .bind(u.id)
        .fetch_one(&mut *tx)
        .await?;
    let permit = s
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::unavailable())?;
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&old).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(c.current_password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(|_| Error::unavailable())?;
    if !valid {
        return Err(Error::auth());
    }
    sqlx::query("UPDATE users SET password_hash=$1,must_change_password=false WHERE id=$2")
        .bind(new)
        .bind(u.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(u.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[cfg(test)]
mod tests {
    #[test]
    fn account_names() {
        assert_eq!(super::username(" Alice ").unwrap(), "alice");
        for name in ["a", "a@b", "a/b", "a b", ""] {
            assert!(super::username(name).is_err());
        }
    }
}
