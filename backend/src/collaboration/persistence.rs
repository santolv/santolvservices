//! Durable state of a collaborative document: the latest snapshot plus the log of accepted
//! updates after it. An update is acknowledged to its author only after its row is committed,
//! so an acknowledged edit survives a crash. Compaction folds the log into a new snapshot in one
//! transaction, so a reader never sees a snapshot without the updates it replaced, or the reverse.

use crate::{App, error::Result};
use std::collections::HashMap;
use uuid::Uuid;

/// What a room needs to rebuild its document after a restart.
pub struct Stored {
    pub snapshot: Option<Vec<u8>>,
    pub snapshot_seq: i64,
    pub updates: Vec<(i64, Vec<u8>)>,
}

pub async fn load(app: &App, node: Uuid) -> Result<Stored> {
    let snapshot: Option<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT seq, state FROM collaboration.snapshots WHERE node_id=$1")
            .bind(node)
            .fetch_optional(&app.db)
            .await?;
    let snapshot_seq = snapshot.as_ref().map_or(0, |(seq, _)| *seq);
    let updates = sqlx::query_as(
        "SELECT seq, payload FROM collaboration.updates WHERE node_id=$1 AND seq>$2 ORDER BY seq",
    )
    .bind(node)
    .bind(snapshot_seq)
    .fetch_all(&app.db)
    .await?;
    Ok(Stored {
        snapshot: snapshot.map(|(_, state)| state),
        snapshot_seq,
        updates,
    })
}

/// One update waiting to be stored.
pub struct Pending<'a> {
    pub op: Uuid,
    pub user: Uuid,
    pub payload: &'a [u8],
}

/// The outcome of storing one update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appended {
    pub seq: i64,
    /// The operation id was already stored (a retry): nothing new was written.
    pub duplicate: bool,
    /// For a duplicate: whether the stored payload is the same. A different payload under a known
    /// id (or one already folded into a snapshot, which cannot be compared) is a conflict.
    pub same: bool,
}

/// Store a batch of accepted updates in one statement (group commit), in order. A retried
/// operation (same id for the same document, earlier or within the batch) is not stored twice.
pub async fn append_batch(app: &App, node: Uuid, batch: &[Pending<'_>]) -> Result<Vec<Appended>> {
    let ops: Vec<Uuid> = batch.iter().map(|p| p.op).collect();
    let users: Vec<Uuid> = batch.iter().map(|p| p.user).collect();
    let payloads: Vec<&[u8]> = batch.iter().map(|p| p.payload).collect();
    let inserted: Vec<(Uuid, i64)> = sqlx::query_as(
        "INSERT INTO collaboration.updates (node_id, op_id, user_id, payload)
         SELECT $1, t.op, t.usr, t.payload
         FROM unnest($2::uuid[], $3::uuid[], $4::bytea[]) WITH ORDINALITY AS t(op, usr, payload, n)
         ORDER BY t.n
         ON CONFLICT (node_id, op_id) DO NOTHING RETURNING op_id, seq",
    )
    .bind(node)
    .bind(&ops)
    .bind(&users)
    .bind(&payloads)
    .fetch_all(&app.db)
    .await?;
    let mut fresh: HashMap<Uuid, i64> = inserted.into_iter().collect();
    // Earlier copies of each retried id: stored before this batch, or first seen in it.
    let mut known: HashMap<Uuid, (i64, Vec<u8>)> = HashMap::new();
    let retried: Vec<Uuid> = ops
        .iter()
        .filter(|op| !fresh.contains_key(op))
        .copied()
        .collect();
    if !retried.is_empty() {
        let rows: Vec<(Uuid, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT op_id, seq, payload FROM collaboration.updates WHERE node_id=$1 AND op_id=ANY($2)",
        )
        .bind(node)
        .bind(&retried)
        .fetch_all(&app.db)
        .await?;
        known.extend(
            rows.into_iter()
                .map(|(op, seq, payload)| (op, (seq, payload))),
        );
    }
    Ok(batch
        .iter()
        .map(|pending| match fresh.remove(&pending.op) {
            Some(seq) => {
                known.insert(pending.op, (seq, pending.payload.to_vec()));
                Appended {
                    seq,
                    duplicate: false,
                    same: true,
                }
            }
            None => match known.get(&pending.op) {
                Some((seq, payload)) => Appended {
                    seq: *seq,
                    duplicate: true,
                    same: payload.as_slice() == pending.payload,
                },
                // Folded into a snapshot: it cannot be compared any more.
                None => Appended {
                    seq: 0,
                    duplicate: true,
                    same: false,
                },
            },
        })
        .collect())
}

/// Fold every update up to `seq` into `state` and drop them, atomically.
pub async fn compact(app: &App, node: Uuid, seq: i64, state: &[u8]) -> Result<()> {
    let mut tx = app.db.begin().await?;
    sqlx::query(
        "INSERT INTO collaboration.snapshots (node_id, seq, state) VALUES ($1,$2,$3)
         ON CONFLICT (node_id) DO UPDATE SET seq=excluded.seq, state=excluded.state, created_at=now()
         WHERE collaboration.snapshots.seq < excluded.seq",
    )
    .bind(node)
    .bind(seq)
    .bind(state)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM collaboration.updates WHERE node_id=$1 AND seq<=$2")
        .bind(node)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
