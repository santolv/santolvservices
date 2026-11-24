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

pub const SESSION_SECONDS: i64 = 2 * 60 * 60;
fn cookie_session(headers: &HeaderMap) -> Result<Option<&str>> {
    if headers.get_all("cookie").iter().count() > 1 {
        return Err(Error::auth());
    }
    let mut values = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .into_iter()
        .flat_map(|v| v.split(';'))
        .filter_map(|c| c.trim().strip_prefix("drive_session="));
    let first = values.next();
    if values.next().is_some() {
        return Err(Error::auth());
    }
    Ok(first)
}
fn csrf_for(raw: &str) -> String {
    hash(&format!("SantolvDrive CSRF v1:{raw}"))
}

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
        || !crate::storage::portable_username(&v)
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
impl FromRequestParts<App> for Principal {
    type Rejection = Error;
    async fn from_request_parts(p: &mut Parts, s: &App) -> Result<Self> {
        let bearer = p
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let cookie = cookie_session(&p.headers)?;
        if (cookie.is_some() && bearer.is_some())
            || p.headers.get_all("authorization").iter().count() > 1
        {
            return Err(Error::auth());
        }
        let raw = cookie
            .or(bearer)
            .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(Error::auth)?;
        let session = hash(raw);
        let r=sqlx::query("SELECT s.user_id,s.csrf_hash,u.account_type,u.must_change_password FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.expires_at>now() AND NOT u.disabled AND u.account_type='user'").bind(&session).fetch_optional(&s.db).await?.ok_or_else(Error::auth)?;
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
pub async fn security(State(s): State<App>, req: axum::extract::Request, next: Next) -> Response {
    let _permit = match s.requests.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return Error(StatusCode::TOO_MANY_REQUESTS, "request_capacity").into_response(),
    };
    if req.headers().get_all("host").iter().count() != 1
        || !req
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|host| valid_host(&s, host))
    {
        return Error(StatusCode::FORBIDDEN, "host_not_allowed").into_response();
    }
    // The local deployment has no trusted proxy. Forwarded metadata must not change its trust boundary.
    if [
        "forwarded",
        "x-forwarded-for",
        "x-forwarded-host",
        "x-forwarded-proto",
        "x-real-ip",
    ]
    .iter()
    .any(|h| req.headers().contains_key(*h))
    {
        return Error(StatusCode::FORBIDDEN, "proxy_not_configured").into_response();
    }
    // Opaque origins cannot identify a trusted frontend.
    let local_file = req.headers().get("origin").is_some_and(|v| v == "null");
    if local_file {
        return Error(StatusCode::FORBIDDEN, "opaque_origin_disabled").into_response();
    }
    if let Some(origin) = req.headers().get("origin")
        && !local_file
        && origin.to_str().ok() != Some(&s.origin)
        && origin.to_str().ok() != Some(&s.frontend_origin)
    {
        return Error(StatusCode::FORBIDDEN, "origin_not_allowed").into_response();
    }
    if !local_file
        && req
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
    ] {
        response.headers_mut().insert(k, v.parse().unwrap());
    }
    response
        .headers_mut()
        .entry("content-security-policy")
        .or_insert(
            "default-src 'none'; frame-ancestors 'none'"
                .parse()
                .unwrap(),
        );
    if s.secure {
        response.headers_mut().insert(
            "strict-transport-security",
            "max-age=31536000".parse().unwrap(),
        );
    }
    response
}
fn valid_host(s: &App, host: &str) -> bool {
    let Ok(origin) = url::Url::parse(&s.origin) else {
        return false;
    };
    let Some(name) = origin.host_str() else {
        return false;
    };
    let Some(port) = origin.port_or_known_default() else {
        return false;
    };
    let configured = origin.as_str().split_once("://").unwrap().1;
    let configured = configured.trim_end_matches('/');
    if host.eq_ignore_ascii_case(configured) {
        return true;
    }
    if matches!(name, "localhost" | "127.0.0.1" | "[::1]") {
        return [
            format!("localhost:{port}"),
            format!("127.0.0.1:{port}"),
            format!("[::1]:{port}"),
        ]
        .iter()
        .any(|v| host.eq_ignore_ascii_case(v));
    }
    false
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
    #[serde(default)]
    local_file: bool,
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
    if c.local_file {
        return Err(Error::bad("opaque_origin_disabled"));
    }
    let failure_key = format!("{}:{}", addr.ip(), hash(&username));
    let attempt_key = format!("login-pair:{failure_key}");
    rate(&s, attempt_key.clone(), 50).await?;
    let delayed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM login_failures WHERE key=$1 AND next_attempt>now())",
    )
    .bind(&failure_key)
    .fetch_one(&s.db)
    .await?;
    if delayed {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "login_retry_later"));
    }
    if c.mobile && (headers.contains_key("origin") || headers.contains_key("sec-fetch-site")) {
        return Err(Error::bad("browser_requires_cookie"));
    }
    let token_mode = c.mobile;
    let row = sqlx::query("SELECT id,password_hash FROM users WHERE username=$1 AND NOT disabled AND account_type='user'")
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
        sqlx::query("INSERT INTO login_failures(key,failures,next_attempt) VALUES($1,1,now()+interval '1 second') ON CONFLICT(key) DO UPDATE SET failures=CASE WHEN login_failures.updated_at<now()-interval '15 minutes' THEN 1 ELSE LEAST(login_failures.failures+1,5) END,next_attempt=now()+LEAST(power(2,LEAST(login_failures.failures,3)),8)*interval '1 second',updated_at=now()")
            .bind(&failure_key).execute(&s.db).await?;
        tracing::warn!(client_ip=%addr.ip(),"login rejected");
        return Err(Error::auth());
    }
    let row = row.ok_or_else(Error::auth)?;
    let id: Uuid = row.get("id");
    let mut tx = s.db.begin().await?;
    let current=sqlx::query("SELECT password_hash,account_type,must_change_password,disabled FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
    if current.get::<String, _>("password_hash") != row.get::<String, _>("password_hash")
        || current.get::<bool, _>("disabled")
        || current.get::<String, _>("account_type") != "user"
    {
        return Err(Error::auth());
    }
    let kind: String = current.get("account_type");
    let expires: i64 = if current.get::<bool, _>("must_change_password") {
        900
    } else {
        SESSION_SECONDS
    };
    let raw = token();
    let csrf = csrf_for(&raw);
    sqlx::query("DELETE FROM login_failures WHERE key=$1")
        .bind(&failure_key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rate_limits WHERE key=$1")
        .bind(attempt_key)
        .execute(&mut *tx)
        .await?;
    // Bound active credentials per user; preserve at most nine prior sessions.
    sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND (expires_at<=now() OR token_hash IN(SELECT token_hash FROM sessions WHERE user_id=$1 ORDER BY expires_at DESC,token_hash DESC OFFSET 9))").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO sessions VALUES($1,$2,$3,now()+$4*interval '1 second')")
        .bind(hash(&raw))
        .bind(id)
        .bind(hash(&csrf))
        .bind(expires)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut body = json!({"account_type":kind,"must_change_password":current.get::<bool,_>("must_change_password"),"expires_in":expires});
    if token_mode {
        body["token"] = json!(raw);
    } else {
        body["csrf_token"] = json!(csrf);
    }
    let mut r = Json(body).into_response();
    if !token_mode {
        r.headers_mut().insert(
            "set-cookie",
            format!(
                "drive_session={raw}; Path=/api/v1; HttpOnly; SameSite=Strict; Max-Age={expires}{}",
                if s.secure { "; Secure" } else { "" }
            )
            .parse()
            .unwrap(),
        );
        // Clear the old, broader cookie so two credentials cannot be sent together.
        r.headers_mut().append(
            "set-cookie",
            format!(
                "drive_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
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
            "drive_session=; Path=/api/v1; HttpOnly; SameSite=Strict; Max-Age=0{}",
            if s.secure { "; Secure" } else { "" }
        )
        .parse()
        .unwrap(),
    );
    r.headers_mut().append(
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
pub async fn me(State(s): State<App>, u: Principal, headers: HeaderMap) -> Result<Json<Value>> {
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id=$1")
        .bind(u.id)
        .fetch_one(&s.db)
        .await?;
    let session=sqlx::query("SELECT csrf_hash,extract(epoch FROM expires_at)::bigint AS expires_at FROM sessions WHERE token_hash=$1 AND expires_at>now()")
        .bind(&u.session).fetch_optional(&s.db).await?.ok_or_else(Error::auth)?;
    let mut body = json!({"id":u.id,"username":username,"account_type":u.account_type,"must_change_password":u.must_change_password,"expires_at":session.get::<i64,_>("expires_at")});
    if let Some(raw) = cookie_session(&headers)? {
        let csrf = csrf_for(raw);
        if hash(&csrf) != session.get::<String, _>("csrf_hash") {
            return Err(Error::auth());
        }
        // The cookie stays HttpOnly. This non-authenticating token restores CSRF protection after reload.
        body["csrf_token"] = csrf.into();
    }
    Ok(Json(body))
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
        for name in [
            "a", "a@b", "a/b", "a b", "", "con", "aux.txt", "COM1", "santi.",
        ] {
            assert!(super::username(name).is_err());
        }
    }
}
