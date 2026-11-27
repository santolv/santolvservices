use crate::{
    App,
    auth::User,
    core::manager::Service,
    error::{Error, Result},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use super::record_support::{body, id_from_path, search_pattern};

#[cfg(test)]
mod tests;

pub const SERVICE: Service = Service {
    id: "contacts",
    schema: "contacts",
    page: "/contacts",
    html: include_str!("../../../../frontend/contacts/index.html"),
    uses_file_store: false,
    requires: None,
    worker: |_, _| Box::pin(async {}),
    routes,
};

fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/contacts", get(list).post(create))
        .route(
            "/api/v1/contacts/{id}",
            get(read).put(update).delete(remove),
        )
        .route("/api/v1/contacts/{id}/favorite", patch(favorite))
        .layer(DefaultBodyLimit::max(64 * 1024))
}

#[derive(Debug, FromRow, Serialize)]
struct Contact {
    id: Uuid,
    given_name: String,
    family_name: String,
    display_name: String,
    email: String,
    phone: String,
    company: String,
    job_title: String,
    address: String,
    website: String,
    notes: String,
    favorite: bool,
    revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Fields {
    given_name: String,
    family_name: String,
    email: String,
    phone: String,
    company: String,
    job_title: String,
    address: String,
    website: String,
    notes: String,
}

#[derive(Deserialize)]
struct Update {
    revision: i64,
    fields: Fields,
}

#[derive(Deserialize)]
struct Favorite {
    revision: i64,
    favorite: bool,
}

#[derive(Deserialize)]
struct Revision {
    revision: i64,
}

#[derive(Deserialize)]
struct List {
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
    #[serde(default)]
    favorite: bool,
}

#[derive(FromRow)]
struct ListRow {
    #[sqlx(flatten)]
    contact: Contact,
    sort_name: String,
}

#[derive(Serialize)]
struct Page {
    items: Vec<Contact>,
    next_cursor: Option<String>,
}

impl Fields {
    fn clean(mut self) -> Result<Self> {
        for (value, max) in [
            (&mut self.given_name, 100),
            (&mut self.family_name, 100),
            (&mut self.email, 254),
            (&mut self.phone, 80),
            (&mut self.company, 160),
            (&mut self.job_title, 120),
            (&mut self.address, 500),
            (&mut self.website, 500),
            (&mut self.notes, 4000),
        ] {
            *value = value.trim().to_owned();
            if value.chars().count() > max
                || value
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
            {
                return Err(Error::bad("invalid_contact_field"));
            }
        }
        for value in [
            &self.given_name,
            &self.family_name,
            &self.phone,
            &self.company,
            &self.job_title,
            &self.website,
        ] {
            if value.chars().any(char::is_control) {
                return Err(Error::bad("invalid_contact_field"));
            }
        }
        if self.given_name.is_empty() && self.family_name.is_empty() && self.company.is_empty() {
            return Err(Error::bad("contact_name_required"));
        }
        if !self.email.is_empty() && !valid_email(&self.email) {
            return Err(Error::bad("invalid_contact_email"));
        }
        if !self.website.is_empty()
            && !Url::parse(&self.website).is_ok_and(|url| {
                matches!(url.scheme(), "https" | "http")
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none()
            })
        {
            return Err(Error::bad("invalid_contact_website"));
        }
        Ok(self)
    }
}

fn valid_email(email: &str) -> bool {
    let Some((local, host)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&byte))
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

async fn list(
    State(app): State<App>,
    user: User,
    query: std::result::Result<Query<List>, QueryRejection>,
) -> Result<Json<Page>> {
    let Query(query) = query.map_err(|_| Error::bad("invalid_query"))?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(Error::bad("invalid_limit"));
    }
    let pattern = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(search_pattern)
        .transpose()?;
    let (after_name, after_id) = match query.cursor {
        Some(cursor) if cursor.len() <= 2048 => {
            let (name, id): (String, Uuid) =
                serde_json::from_str(&cursor).map_err(|_| Error::bad("invalid_cursor"))?;
            if name.contains('\0') {
                return Err(Error::bad("invalid_cursor"));
            }
            (Some(name), Some(id))
        }
        Some(_) => return Err(Error::bad("invalid_cursor")),
        None => (None, None),
    };
    let mut rows = sqlx::query_as::<_, ListRow>(
        "SELECT *, lower(display_name) AS sort_name FROM contacts.people
         WHERE owner_id=$1 AND (NOT $2 OR favorite)
           AND ($3::text IS NULL OR display_name ILIKE $3 ESCAPE '!'
            OR email ILIKE $3 ESCAPE '!' OR phone ILIKE $3 ESCAPE '!'
            OR company ILIKE $3 ESCAPE '!')
           AND ($4::text IS NULL OR (lower(display_name), id) > ($4, $5))
         ORDER BY lower(display_name), id LIMIT $6",
    )
    .bind(user.id)
    .bind(query.favorite)
    .bind(pattern)
    .bind(after_name)
    .bind(after_id)
    .bind(limit + 1)
    .fetch_all(&app.db)
    .await?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next_cursor = rows
        .last()
        .filter(|_| has_more)
        .map(|row| serde_json::to_string(&(&row.sort_name, row.contact.id)))
        .transpose()
        .map_err(|_| Error::unavailable())?;
    Ok(Json(Page {
        items: rows.into_iter().map(|row| row.contact).collect(),
        next_cursor,
    }))
}

async fn read(State(app): State<App>, user: User, Path(id): Path<String>) -> Result<Json<Contact>> {
    let contact = sqlx::query_as("SELECT * FROM contacts.people WHERE owner_id=$1 AND id=$2")
        .bind(user.id)
        .bind(id_from_path(&id)?)
        .fetch_optional(&app.db)
        .await?
        .ok_or_else(Error::denied)?;
    Ok(Json(contact))
}

async fn create(
    State(app): State<App>,
    user: User,
    input: std::result::Result<Json<Fields>, JsonRejection>,
) -> Result<(StatusCode, Json<Contact>)> {
    let input = body(input)?.clean()?;
    let contact = sqlx::query_as(
        "INSERT INTO contacts.people
         (owner_id,id,given_name,family_name,email,phone,company,job_title,address,website,notes)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING *",
    )
    .bind(user.id)
    .bind(Uuid::new_v4())
    .bind(input.given_name)
    .bind(input.family_name)
    .bind(input.email)
    .bind(input.phone)
    .bind(input.company)
    .bind(input.job_title)
    .bind(input.address)
    .bind(input.website)
    .bind(input.notes)
    .fetch_one(&app.db)
    .await?;
    Ok((StatusCode::CREATED, Json(contact)))
}

fn valid_revision(revision: i64) -> Result<i64> {
    if revision < 1 {
        return Err(Error::bad("invalid_revision"));
    }
    Ok(revision)
}

async fn missing_change(db: &PgPool, owner: Uuid, id: Uuid) -> Error {
    match sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM contacts.people WHERE owner_id=$1 AND id=$2)",
    )
    .bind(owner)
    .bind(id)
    .fetch_one(db)
    .await
    {
        Ok(true) => Error::conflict("contact_changed"),
        Ok(false) => Error::denied(),
        Err(error) => error.into(),
    }
}

async fn update(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    input: std::result::Result<Json<Update>, JsonRejection>,
) -> Result<Json<Contact>> {
    let id = id_from_path(&id)?;
    let update = body(input)?;
    let revision = valid_revision(update.revision)?;
    let input = update.fields.clean()?;
    let contact = sqlx::query_as(
        "UPDATE contacts.people SET given_name=$4, family_name=$5, email=$6, phone=$7,
         company=$8, job_title=$9, address=$10, website=$11, notes=$12,
         revision=revision+1, updated_at=statement_timestamp()
         WHERE owner_id=$1 AND id=$2 AND revision=$3 RETURNING *",
    )
    .bind(user.id)
    .bind(id)
    .bind(revision)
    .bind(input.given_name)
    .bind(input.family_name)
    .bind(input.email)
    .bind(input.phone)
    .bind(input.company)
    .bind(input.job_title)
    .bind(input.address)
    .bind(input.website)
    .bind(input.notes)
    .fetch_optional(&app.db)
    .await?;
    match contact {
        Some(contact) => Ok(Json(contact)),
        None => Err(missing_change(&app.db, user.id, id).await),
    }
}

async fn favorite(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    input: std::result::Result<Json<Favorite>, JsonRejection>,
) -> Result<Json<Contact>> {
    let id = id_from_path(&id)?;
    let input = body(input)?;
    let contact = sqlx::query_as(
        "UPDATE contacts.people SET favorite=$4, revision=revision+1, updated_at=statement_timestamp()
         WHERE owner_id=$1 AND id=$2 AND revision=$3 RETURNING *",
    )
    .bind(user.id)
    .bind(id)
    .bind(valid_revision(input.revision)?)
    .bind(input.favorite)
    .fetch_optional(&app.db)
    .await?;
    match contact {
        Some(contact) => Ok(Json(contact)),
        None => Err(missing_change(&app.db, user.id, id).await),
    }
}

async fn remove(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    query: std::result::Result<Query<Revision>, QueryRejection>,
) -> Result<StatusCode> {
    let id = id_from_path(&id)?;
    let Query(query) = query.map_err(|_| Error::bad("invalid_query"))?;
    let removed =
        sqlx::query("DELETE FROM contacts.people WHERE owner_id=$1 AND id=$2 AND revision=$3")
            .bind(user.id)
            .bind(id)
            .bind(valid_revision(query.revision)?)
            .execute(&app.db)
            .await?;
    if removed.rows_affected() == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(missing_change(&app.db, user.id, id).await)
    }
}
