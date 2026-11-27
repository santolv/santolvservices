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
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use super::record_support::{ListQuery, Page, body, id_from_path, page};

#[cfg(test)]
mod tests;

pub const SERVICE: Service = Service {
    id: "mail",
    schema: "mail",
    page: "/mail",
    html: include_str!("../../../../frontend/mail/index.html"),
    uses_file_store: false,
    requires: None,
    worker: |_, _| Box::pin(async {}),
    routes,
};

fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/mail/status", get(status))
        .route("/api/v1/mail/messages", get(list).post(create))
        .route(
            "/api/v1/mail/messages/{id}",
            get(get_message).put(update).patch(change).delete(remove),
        )
        .layer(DefaultBodyLimit::max(650 * 1024))
}

#[derive(Serialize)]
struct MailStatus {
    transport: &'static str,
}

async fn status(_user: User) -> Json<MailStatus> {
    Json(MailStatus {
        transport: "not_configured",
    })
}

#[derive(FromRow, Serialize)]
struct Message {
    id: Uuid,
    kind: String,
    sender: Option<String>,
    to_addresses: Vec<String>,
    cc_addresses: Vec<String>,
    bcc_addresses: Vec<String>,
    subject: String,
    content: String,
    starred: bool,
    unread: bool,
    archived: bool,
    trashed: bool,
    revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Folder {
    #[default]
    Inbox,
    Drafts,
    Sent,
    Starred,
    Archive,
    Trash,
}

impl Folder {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Drafts => "drafts",
            Self::Sent => "sent",
            Self::Starred => "starred",
            Self::Archive => "archive",
            Self::Trash => "trash",
        }
    }
}

#[derive(Deserialize)]
struct MessageQuery {
    #[serde(default)]
    folder: Folder,
    #[serde(flatten)]
    list: ListQuery,
}

async fn list(
    State(app): State<App>,
    user: User,
    query: std::result::Result<Query<MessageQuery>, QueryRejection>,
) -> Result<Json<Page<Message>>> {
    let Query(query) = query.map_err(|_| Error::bad("invalid_query"))?;
    let options = query.list.options()?;
    let items = sqlx::query_as(
        "SELECT id, kind, sender, to_addresses[1:3] AS to_addresses, cc_addresses[1:3] AS cc_addresses,
                bcc_addresses[1:3] AS bcc_addresses, subject, left(content, 180) AS content,
                starred, unread, archived, trashed, revision, updated_at
         FROM mail.messages
         WHERE owner_id = $1
           AND CASE $2::text
                 WHEN 'inbox' THEN kind='received' AND NOT archived AND NOT trashed
                 WHEN 'drafts' THEN kind='draft' AND NOT archived AND NOT trashed
                 WHEN 'sent' THEN kind='sent' AND NOT archived AND NOT trashed
                 WHEN 'starred' THEN starred AND NOT trashed
                 WHEN 'archive' THEN archived AND NOT trashed
                 WHEN 'trash' THEN trashed
               END
           AND ($3::text IS NULL OR subject ILIKE $3 ESCAPE '!' OR content ILIKE $3 ESCAPE '!'
                OR sender ILIKE $3 ESCAPE '!' OR array_to_string(to_addresses, ',') ILIKE $3 ESCAPE '!'
                OR array_to_string(cc_addresses, ',') ILIKE $3 ESCAPE '!'
                OR array_to_string(bcc_addresses, ',') ILIKE $3 ESCAPE '!')
           AND ($4::timestamptz IS NULL OR (updated_at,id) < ($4,$5))
         ORDER BY updated_at DESC, id DESC LIMIT $6",
    )
    .bind(user.id)
    .bind(query.folder.as_str())
    .bind(options.pattern)
    .bind(options.before_time)
    .bind(options.before_id)
    .bind(options.limit + 1)
    .fetch_all(&app.db)
    .await?;
    page(items, options.limit, |message: &Message| {
        (message.updated_at, message.id)
    })
}

async fn get_message(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
) -> Result<Json<Message>> {
    let message = sqlx::query_as(
        "SELECT id, kind, sender, to_addresses, cc_addresses, bcc_addresses, subject, content,
                starred, unread, archived, trashed, revision, updated_at
         FROM mail.messages WHERE owner_id=$1 AND id=$2",
    )
    .bind(user.id)
    .bind(id_from_path(&id)?)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Error::denied)?;
    Ok(Json(message))
}

#[derive(Deserialize)]
struct Draft {
    to_addresses: Vec<String>,
    cc_addresses: Vec<String>,
    bcc_addresses: Vec<String>,
    subject: String,
    content: String,
}

#[derive(Deserialize)]
struct EditDraft {
    revision: i64,
    #[serde(flatten)]
    draft: Draft,
}

fn validate_draft(mut draft: Draft) -> Result<Draft> {
    let addresses = [
        &mut draft.to_addresses,
        &mut draft.cc_addresses,
        &mut draft.bcc_addresses,
    ];
    if addresses.iter().map(|group| group.len()).sum::<usize>() > 50 {
        return Err(Error::bad("too_many_recipients"));
    }
    for group in addresses {
        for address in group {
            *address = address.trim().to_owned();
            let valid = address.split_once('@').is_some_and(|(local, domain)| {
                !local.is_empty()
                    && !domain.is_empty()
                    && !domain.contains('@')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
                    && !domain.contains("..")
            });
            if !valid
                || address.len() > 320
                || address.chars().any(|c| {
                    c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';')
                })
            {
                return Err(Error::bad("invalid_recipient"));
            }
        }
    }
    draft.subject = draft.subject.trim().to_owned();
    if draft.subject.chars().count() > 250 || draft.subject.chars().any(char::is_control) {
        return Err(Error::bad("invalid_subject"));
    }
    if draft.content.chars().count() > 30_000 || draft.content.contains('\0') {
        return Err(Error::bad("invalid_mail_body"));
    }
    Ok(draft)
}

async fn create(
    State(app): State<App>,
    user: User,
    input: std::result::Result<Json<Draft>, JsonRejection>,
) -> Result<(StatusCode, Json<Message>)> {
    let input = validate_draft(body(input)?)?;
    let message = sqlx::query_as(
        "INSERT INTO mail.messages (id,owner_id,kind,to_addresses,cc_addresses,bcc_addresses,subject,content)
         VALUES ($1,$2,'draft',$3,$4,$5,$6,$7)
         RETURNING id, kind, sender, to_addresses, cc_addresses, bcc_addresses, subject, content,
                   starred, unread, archived, trashed, revision, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(user.id)
    .bind(input.to_addresses)
    .bind(input.cc_addresses)
    .bind(input.bcc_addresses)
    .bind(input.subject)
    .bind(input.content)
    .fetch_one(&app.db)
    .await?;
    Ok((StatusCode::CREATED, Json(message)))
}

#[derive(FromRow)]
struct MessageState {
    kind: String,
    trashed: bool,
    revision: i64,
}

async fn lock_message(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: Uuid,
    id: Uuid,
    revision: i64,
) -> Result<MessageState> {
    let state: MessageState = sqlx::query_as(
        "SELECT kind, trashed, revision FROM mail.messages WHERE owner_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(user)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(Error::denied)?;
    if state.revision != revision {
        return Err(Error::conflict("mail_changed"));
    }
    Ok(state)
}

async fn update(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    input: std::result::Result<Json<EditDraft>, JsonRejection>,
) -> Result<Json<Message>> {
    let id = id_from_path(&id)?;
    let input = body(input)?;
    let draft = validate_draft(input.draft)?;
    let mut tx = app.db.begin().await?;
    let state = lock_message(&mut tx, user.id, id, input.revision).await?;
    if state.kind != "draft" || state.trashed {
        return Err(Error::conflict("draft_not_editable"));
    }
    let message = sqlx::query_as(
        "UPDATE mail.messages
         SET to_addresses=$3, cc_addresses=$4, bcc_addresses=$5, subject=$6, content=$7,
             revision=revision+1, updated_at=statement_timestamp()
         WHERE owner_id=$1 AND id=$2
         RETURNING id, kind, sender, to_addresses, cc_addresses, bcc_addresses, subject, content,
                   starred, unread, archived, trashed, revision, updated_at",
    )
    .bind(user.id)
    .bind(id)
    .bind(draft.to_addresses)
    .bind(draft.cc_addresses)
    .bind(draft.bcc_addresses)
    .bind(draft.subject)
    .bind(draft.content)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(message))
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Star,
    Unstar,
    Archive,
    Unarchive,
    Trash,
    Restore,
    Read,
    Unread,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    revision: i64,
    action: Action,
}

async fn change(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    input: std::result::Result<Json<Change>, JsonRejection>,
) -> Result<Json<Message>> {
    let id = id_from_path(&id)?;
    let input = body(input)?;
    let mut tx = app.db.begin().await?;
    let state = lock_message(&mut tx, user.id, id, input.revision).await?;
    if (state.trashed && !matches!(input.action, Action::Restore))
        || (!state.trashed && matches!(input.action, Action::Restore))
        || (state.kind != "received" && matches!(input.action, Action::Read | Action::Unread))
    {
        return Err(Error::conflict("invalid_mail_action"));
    }
    let mut starred = None;
    let mut archived = None;
    let mut trashed = None;
    let mut unread = None;
    match input.action {
        Action::Star => starred = Some(true),
        Action::Unstar => starred = Some(false),
        Action::Archive => archived = Some(true),
        Action::Unarchive => archived = Some(false),
        Action::Trash => trashed = Some(true),
        Action::Restore => trashed = Some(false),
        Action::Read => unread = Some(false),
        Action::Unread => unread = Some(true),
    }
    let message = sqlx::query_as(
        "UPDATE mail.messages
         SET starred=coalesce($3,starred), archived=coalesce($4,archived),
             trashed=coalesce($5,trashed), unread=coalesce($6,unread), revision=revision+1
         WHERE owner_id=$1 AND id=$2
         RETURNING id, kind, sender, to_addresses, cc_addresses, bcc_addresses, subject, content,
                   starred, unread, archived, trashed, revision, updated_at",
    )
    .bind(user.id)
    .bind(id)
    .bind(starred)
    .bind(archived)
    .bind(trashed)
    .bind(unread)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(message))
}

#[derive(Deserialize)]
struct Revision {
    revision: i64,
}

async fn remove(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    Query(query): Query<Revision>,
) -> Result<StatusCode> {
    let id = id_from_path(&id)?;
    let mut tx = app.db.begin().await?;
    let state = lock_message(&mut tx, user.id, id, query.revision).await?;
    if !state.trashed {
        return Err(Error::conflict("mail_must_be_trashed"));
    }
    sqlx::query("DELETE FROM mail.messages WHERE owner_id=$1 AND id=$2")
        .bind(user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
