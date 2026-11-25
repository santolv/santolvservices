use crate::{
    App,
    auth::User,
    error::{Error, Result},
    files, storage,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub const SERVICE: crate::core::manager::Service = crate::core::manager::Service {
    id: "photos",
    schema: "photos",
    page: "/photos",
    html: include_str!("../../../../frontend/photos/index.html"),
    uses_file_store: true,
    worker: |app, id| Box::pin(crate::worker::run(app, id)),
    routes,
};

fn routes() -> axum::Router<App> {
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route("/api/v1/photos/library", get(library))
        .route("/api/v1/photos/space", post(space))
        .route("/api/v1/photos/{id}/favorite", put(favorite))
}

pub async fn space(State(s): State<App>, u: User) -> Result<Json<Value>> {
    let mut tx = s.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("photos:{}", u.id))
        .execute(&mut *tx)
        .await?;
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM spaces WHERE owner_id=$1 AND service='photos' AND kind='personal'",
    )
    .bind(u.id)
    .fetch_optional(&mut *tx)
    .await?;
    let id = if let Some(id) = existing {
        id
    } else {
        let id = Uuid::new_v4();
        let directory = format!("usuarios/{}/photos", u.id);
        storage::prepare_space(&s.root, &directory).await?;
        sqlx::query("INSERT INTO spaces(id,name,kind,owner_id,quota,storage_directory,service) VALUES($1,'Fotos','personal',$2,$3,$4,'photos')")
            .bind(id).bind(u.id).bind(s.quota).bind(directory).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO members VALUES($1,$2,3)")
            .bind(id)
            .bind(u.id)
            .execute(&mut *tx)
            .await?;
        id
    };
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}

#[derive(Deserialize)]
pub struct LibraryQuery {
    after: Option<Uuid>,
    id: Option<Uuid>,
}
pub async fn library(
    State(s): State<App>,
    u: User,
    Query(q): Query<LibraryQuery>,
) -> Result<Json<Value>> {
    let mut items: Vec<Value>=sqlx::query_scalar(
        "WITH RECURSIVE library AS (
          SELECT n.* FROM nodes n JOIN spaces s ON s.id=n.space_id WHERE s.owner_id=$1 AND s.service='photos' AND s.kind='personal' AND s.deleted_at IS NULL AND n.parent_id IS NULL
          UNION ALL SELECT n.* FROM nodes n JOIN library p ON n.parent_id=p.id AND n.space_id=p.space_id WHERE p.trashed_at IS NULL
        ) SELECT jsonb_build_object('id',n.id,'name',n.name,'kind',n.kind,'album',n.parent_id,'revision',n.revision,'deleted',n.trashed_at IS NOT NULL,'date',to_char(n.created_at AT TIME ZONE 'UTC','YYYY-MM-DD'),'favorite',COALESCE(m.favorite,false))
        FROM library n LEFT JOIN photo_metadata m ON m.node_id=n.id
        WHERE ($2::uuid IS NULL OR n.id>$2) AND ($3::uuid IS NULL OR n.id=$3) AND NOT EXISTS(SELECT 1 FROM purge_jobs p WHERE p.node_id=n.id)
          AND (n.kind='folder' OR n.name ~* '\\.(jpe?g|png|webp|gif|avif)$') ORDER BY n.id LIMIT 101")
        .bind(u.id).bind(q.after).bind(q.id).fetch_all(&s.db).await?;
    let next = if items.len() > 100 {
        items[99]["id"].clone()
    } else {
        Value::Null
    };
    items.truncate(100);
    Ok(Json(json!({"items":items,"next_cursor":next})))
}

#[derive(Deserialize)]
pub struct Favorite {
    favorite: bool,
}
pub async fn favorite(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(c): Json<Favorite>,
) -> Result<Json<Value>> {
    let mut tx = s.db.begin().await?;
    let n = files::node(&mut tx, id).await?;
    files::lock(&mut tx, n.space_id).await?;
    let n = files::access(&mut tx, id, u.id, 2, false).await?;
    let own: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM spaces WHERE id=$1 AND owner_id=$2 AND service='photos' AND kind='personal')")
        .bind(n.space_id).bind(u.id).fetch_one(&mut *tx).await?;
    if !own || n.kind != "file" {
        return Err(Error::denied());
    }
    files::check_revision(&n, &h)?;
    sqlx::query("INSERT INTO photo_metadata(node_id,favorite) VALUES($1,$2) ON CONFLICT(node_id) DO UPDATE SET favorite=excluded.favorite")
        .bind(id).bind(c.favorite).execute(&mut *tx).await?;
    sqlx::query("UPDATE nodes SET revision=revision+1 WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    files::touch_recent(&mut tx, u.id, id).await?;
    tx.commit().await?;
    Ok(Json(json!({"favorite":c.favorite,"revision":n.revision+1})))
}
