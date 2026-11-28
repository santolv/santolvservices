//! One room per open document: its Yrs document, its members and their broadcast channel.
//! Updates are integrated in memory under the room's lock, in arrival order, then handed to the
//! room's single writer, which stores them in batches (group commit). An update is acknowledged
//! and broadcast only after its batch is committed. Yrs updates commute and are idempotent, so a
//! snapshot taken from memory always contains every stored update up to its sequence; independent
//! documents never wait for each other and each open document uses at most one database
//! connection for its writes.

use super::{persistence, protocol};
use crate::{
    App,
    error::{Error, Result},
};
use axum::http::StatusCode;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;
use yrs::{Doc, ReadTxn, StateVector, Transact, Update, updates::decoder::Decode};

/// Largest document a room keeps (snapshot plus log), in bytes.
pub const MAX_DOCUMENT: usize = 32 * 1024 * 1024;
/// Encoded bytes all open rooms of this process may hold together.
pub const MAX_TOTAL: usize = 512 * 1024 * 1024;
/// Compact the log into a snapshot after this many updates or bytes.
const COMPACT_UPDATES: u32 = 500;
const COMPACT_BYTES: usize = 4 * 1024 * 1024;
/// Updates with missing dependencies tolerated before the room is considered inconsistent.
const MAX_PENDING: u32 = 256;
/// Members per document and messages queued per member before a slow member is dropped.
pub const MAX_MEMBERS: usize = 500;
pub const BROADCAST_CAPACITY: usize = 1024;
/// Updates waiting to be stored per room; a full queue answers `busy` (the client retries).
const QUEUE: usize = 1024;
/// Updates stored per statement (each at most `protocol::MAX_FRAME`).
const BATCH: usize = 64;

/// An update integrated in memory and waiting for its batch to be committed.
struct Job {
    op: Uuid,
    user: Uuid,
    connection: Uuid,
    payload: Vec<u8>,
    done: oneshot::Sender<Result<(i64, bool)>>,
}

/// A frame for every member except its sender (`from`; nil when several members sent it, then
/// each of them also gets its own updates back, which Yrs ignores).
#[derive(Clone)]
pub struct Broadcast {
    pub from: Uuid,
    /// Shared by every member (reference-counted, never copied per member).
    pub frame: axum::body::Bytes,
}

struct State {
    doc: Doc,
    seq: i64,
    size: usize,
    since_compaction: u32,
    bytes_since_compaction: usize,
    pending: u32,
}

pub struct Room {
    pub node: Uuid,
    state: tokio::sync::Mutex<State>,
    pub channel: broadcast::Sender<Broadcast>,
    queue: mpsc::Sender<Job>,
    members: AtomicUsize,
    /// Set when memory and storage may have diverged; members must reconnect.
    pub poisoned: AtomicBool,
}

impl State {
    /// Change the document's size and the process-wide total with it.
    fn resize(&mut self, size: usize) {
        let total = &registry().bytes;
        if size >= self.size {
            total.fetch_add(size - self.size, Ordering::AcqRel);
        } else {
            total.fetch_sub(self.size - size, Ordering::AcqRel);
        }
        self.size = size;
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        self.state.get_mut().resize(0);
    }
}

impl Room {
    async fn open(app: &App, node: Uuid) -> Result<(Self, mpsc::Receiver<Job>)> {
        let stored = persistence::load(app, node).await?;
        let doc = Doc::new();
        let mut size = 0;
        let mut seq = stored.snapshot_seq;
        {
            let mut txn = doc.transact_mut();
            for payload in stored
                .snapshot
                .iter()
                .chain(stored.updates.iter().map(|(_, p)| p))
            {
                size += payload.len();
                let update = Update::decode_v1(payload).map_err(|_| corrupt())?;
                txn.apply_update(update).map_err(|_| corrupt())?;
            }
            if let Some((last, _)) = stored.updates.last() {
                seq = *last;
            }
        }
        if registry().bytes.load(Ordering::Acquire) + size > MAX_TOTAL {
            return Err(busy());
        }
        let (channel, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (queue, jobs) = mpsc::channel(QUEUE);
        let mut room = Self {
            node,
            state: tokio::sync::Mutex::new(State {
                doc,
                seq,
                size: 0,
                since_compaction: stored.updates.len() as u32,
                bytes_since_compaction: stored.updates.iter().map(|(_, p)| p.len()).sum(),
                pending: 0,
            }),
            channel,
            queue,
            members: AtomicUsize::new(0),
            poisoned: AtomicBool::new(false),
        };
        room.state.get_mut().resize(size);
        Ok((room, jobs))
    }

    /// The update a client with this state vector is missing (all of it for an empty vector).
    pub async fn diff(&self, state_vector: &[u8]) -> Result<Vec<u8>> {
        let vector = if state_vector.is_empty() {
            StateVector::default()
        } else {
            StateVector::decode_v1(state_vector).map_err(|_| Error::bad("invalid_state_vector"))?
        };
        let state = self.state.lock().await;
        Ok(state.doc.transact().encode_diff_v1(&vector))
    }

    /// Integrate an authorized update and wait until it is stored. Returns its sequence and
    /// whether it was a retry of an operation already stored (then it is not broadcast again).
    pub async fn apply(
        &self,
        user: Uuid,
        connection: Uuid,
        op: Uuid,
        payload: &[u8],
    ) -> Result<(i64, bool)> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(reset());
        }
        let update = Update::decode_v1(payload).map_err(|_| Error::bad("invalid_update"))?;
        let stored = {
            let mut state = self.state.lock().await;
            if state.size + payload.len() > MAX_DOCUMENT {
                return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "document_too_large"));
            }
            if registry().bytes.load(Ordering::Acquire) + payload.len() > MAX_TOTAL {
                return Err(busy());
            }
            // Reserve the queue slot first: a busy room rejects the update before it touches memory.
            let slot = self
                .queue
                .try_reserve()
                .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "busy"))?;
            let missing = {
                let mut txn = state.doc.transact_mut();
                txn.apply_update(update)
                    .map_err(|_| Error::bad("invalid_update"))?;
                txn.has_missing_updates()
            };
            state.pending = if missing { state.pending + 1 } else { 0 };
            if state.pending > MAX_PENDING {
                self.poison();
                return Err(Error(StatusCode::CONFLICT, "document_inconsistent"));
            }
            let size = state.size + payload.len();
            state.resize(size);
            let (done, stored) = oneshot::channel();
            slot.send(Job {
                op,
                user,
                connection,
                payload: payload.to_vec(),
                done,
            });
            stored
        };
        stored.await.unwrap_or_else(|_| Err(reset()))
    }

    /// Fold the stored log into a snapshot. The snapshot is encoded under the lock but written
    /// without it, so editors are not blocked by the database while it is stored.
    async fn compact(&self, app: &App) {
        let (seq, count, bytes, snapshot) = {
            let state = self.state.lock().await;
            // A reset room's memory may hold what storage does not: never fold it into a snapshot.
            if state.since_compaction == 0 || self.poisoned.load(Ordering::Acquire) {
                return;
            }
            let snapshot = state
                .doc
                .transact()
                .encode_state_as_update_v1(&StateVector::default());
            (
                state.seq,
                state.since_compaction,
                state.bytes_since_compaction,
                snapshot,
            )
        };
        match persistence::compact(app, self.node, seq, &snapshot).await {
            Ok(()) => {
                let mut state = self.state.lock().await;
                state.since_compaction = state.since_compaction.saturating_sub(count);
                state.bytes_since_compaction = state.bytes_since_compaction.saturating_sub(bytes);
                let size = snapshot.len() + state.bytes_since_compaction;
                state.resize(size);
            }
            // The log is intact; compaction is retried later.
            Err(error) => {
                tracing::warn!(node = %self.node, ?error, "collaboration compaction failed")
            }
        }
    }

    fn poison(&self) {
        self.poisoned.store(true, Ordering::Release);
        registry()
            .rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.node);
    }

    /// The text of a root text type, for tests and diagnostics.
    pub async fn text(&self, name: &str) -> String {
        use yrs::GetString;
        let state = self.state.lock().await;
        let text = state.doc.get_or_insert_text(name);
        text.get_string(&state.doc.transact())
    }
}

/// Store a batch. Every job's payload is already in memory, so storage must end up with all of
/// it: an exact retry is skipped, but different content under a known operation id (a client
/// reusing someone else's id, or a retry already folded into a snapshot) is stored again under a
/// new server-chosen id, so memory, the log and what members receive never diverge.
async fn store(app: &App, node: Uuid, jobs: &mut [Job]) -> Result<Vec<persistence::Appended>> {
    let pending = |index: &[usize], jobs: &[Job]| -> Vec<(Uuid, Uuid, usize)> {
        index
            .iter()
            .map(|&i| (jobs[i].op, jobs[i].user, i))
            .collect()
    };
    let batch: Vec<persistence::Pending> = jobs
        .iter()
        .map(|job| persistence::Pending {
            op: job.op,
            user: job.user,
            payload: &job.payload,
        })
        .collect();
    let mut stored = persistence::append_batch(app, node, &batch).await?;
    drop(batch);
    let reused: Vec<usize> = (0..jobs.len()).filter(|&i| !stored[i].same).collect();
    if !reused.is_empty() {
        tracing::warn!(%node, count = reused.len(), "collaboration operation id reused with different content");
        for &i in &reused {
            jobs[i].op = Uuid::new_v4();
        }
        let rows = pending(&reused, jobs);
        let batch: Vec<persistence::Pending> = rows
            .iter()
            .map(|&(op, user, i)| persistence::Pending {
                op,
                user,
                payload: &jobs[i].payload,
            })
            .collect();
        let again = persistence::append_batch(app, node, &batch).await?;
        for (&i, appended) in reused.iter().zip(again) {
            stored[i] = appended;
        }
    }
    Ok(stored)
}

/// The room's single writer: stores queued updates in batches, then acknowledges them.
async fn write(app: App, node: Uuid, room: Weak<Room>, mut queue: mpsc::Receiver<Job>) {
    let mut jobs = Vec::with_capacity(BATCH);
    // Ends when the room is dropped (its sender with it).
    while queue.recv_many(&mut jobs, BATCH).await > 0 {
        let result = store(&app, node, &mut jobs).await;
        let Some(room) = room.upgrade() else {
            return;
        };
        let stored = match result {
            Ok(stored) => stored,
            Err(error) => {
                // Memory now holds updates storage may not: members reload from storage.
                tracing::error!(%node, ?error, "collaboration log write failed");
                room.poison();
                queue.close();
                for job in jobs.drain(..) {
                    let _ = job.done.send(Err(reset()));
                }
                while let Some(job) = queue.recv().await {
                    let _ = job.done.send(Err(reset()));
                }
                return;
            }
        };
        let due = {
            let mut state = room.state.lock().await;
            for (appended, job) in stored.iter().zip(&jobs) {
                if !appended.duplicate {
                    state.seq = state.seq.max(appended.seq);
                    state.since_compaction += 1;
                    state.bytes_since_compaction += job.payload.len();
                }
            }
            state.since_compaction >= COMPACT_UPDATES
                || state.bytes_since_compaction >= COMPACT_BYTES
        };
        // One frame per committed batch (exact retries are not sent again), then the acks.
        let fresh: Vec<&Job> = jobs
            .iter()
            .zip(&stored)
            .filter(|(_, appended)| !appended.duplicate)
            .map(|(job, _)| job)
            .collect();
        for group in broadcast_groups(&fresh) {
            match yrs::merge_updates_v1(group.iter().map(|job| &job.payload)) {
                Ok(merged) => {
                    let ops: Vec<Uuid> = group.iter().map(|job| job.op).collect();
                    let first = group[0].connection;
                    let from = if group.iter().all(|job| job.connection == first) {
                        first
                    } else {
                        Uuid::nil()
                    };
                    let _ = room.channel.send(Broadcast {
                        from,
                        frame: protocol::updates(&ops, &merged).into(),
                    });
                }
                // Every update was already integrated, so this cannot happen; members that missed
                // it would resynchronise on reconnection, so the room is reset to be safe.
                Err(error) => {
                    tracing::error!(%node, ?error, "collaboration batch merge failed");
                    room.poison();
                }
            }
        }
        for (job, appended) in jobs.drain(..).zip(stored) {
            let _ = job.done.send(Ok((appended.seq, appended.duplicate)));
        }
        if due {
            room.compact(&app).await;
        }
    }
}

/// Split a batch into consecutive groups whose payloads fit in one client frame.
fn broadcast_groups<'a>(jobs: &[&'a Job]) -> Vec<Vec<&'a Job>> {
    let mut groups: Vec<Vec<&Job>> = Vec::new();
    let mut bytes = 0;
    for &job in jobs {
        let size = 16 + job.payload.len();
        match groups.last_mut() {
            Some(group) if bytes + size <= protocol::MAX_FRAME => group.push(job),
            _ => {
                groups.push(vec![job]);
                bytes = 0;
            }
        }
        bytes += size;
    }
    groups
}

fn busy() -> Error {
    Error(StatusCode::SERVICE_UNAVAILABLE, "server_busy")
}

fn reset() -> Error {
    Error(StatusCode::SERVICE_UNAVAILABLE, "room_reset")
}

fn corrupt() -> Error {
    Error(StatusCode::INTERNAL_SERVER_ERROR, "document_corrupt")
}

/// Open rooms and connection counts for this process.
pub struct Registry {
    rooms: Mutex<HashMap<Uuid, Arc<Room>>>,
    users: Mutex<HashMap<Uuid, usize>>,
    pub connections: AtomicUsize,
    /// Encoded bytes held by all open rooms (bounded by `MAX_TOTAL`).
    pub bytes: AtomicUsize,
    /// Recent connection attempts per account: (tokens, last refill).
    attempts: Mutex<HashMap<Uuid, (f64, std::time::Instant)>>,
}

pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry {
        rooms: Mutex::new(HashMap::new()),
        users: Mutex::new(HashMap::new()),
        connections: AtomicUsize::new(0),
        bytes: AtomicUsize::new(0),
        attempts: Mutex::new(HashMap::new()),
    })
}

/// A member's place in a room; leaving (drop) compacts and closes the room when it empties.
pub struct Membership {
    pub room: Arc<Room>,
    user: Uuid,
    app: App,
}

/// Connections allowed per account and in total, and members per room.
pub const MAX_USER_CONNECTIONS: usize = 16;
pub const MAX_CONNECTIONS: usize = 2_000;

/// Connection attempts per account: a burst of `ATTEMPT_BURST`, then `ATTEMPT_RATE` per second.
/// Each attempt costs several queries and a full document diff, so reconnection loops are bounded.
const ATTEMPT_RATE: f64 = 1.0;
const ATTEMPT_BURST: f64 = 30.0;

/// Count a connection attempt; false when the account is reconnecting too often.
pub fn attempt(user: Uuid) -> bool {
    let now = std::time::Instant::now();
    let mut attempts = registry()
        .attempts
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Forget accounts whose bucket is full again, so the map stays small.
    if attempts.len() > 10_000 {
        attempts.retain(|_, (tokens, last)| {
            *tokens + now.duration_since(*last).as_secs_f64() * ATTEMPT_RATE < ATTEMPT_BURST
        });
    }
    let (tokens, last) = attempts.entry(user).or_insert((ATTEMPT_BURST, now));
    *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * ATTEMPT_RATE).min(ATTEMPT_BURST);
    *last = now;
    if *tokens >= 1.0 {
        *tokens -= 1.0;
        true
    } else {
        false
    }
}

pub async fn join(app: &App, node: Uuid, user: Uuid) -> Result<Membership> {
    let registry = registry();
    {
        let mut users = registry.users.lock().unwrap_or_else(|e| e.into_inner());
        let count = users.entry(user).or_insert(0);
        if *count >= MAX_USER_CONNECTIONS
            || registry.connections.load(Ordering::Acquire) >= MAX_CONNECTIONS
        {
            return Err(Error(StatusCode::TOO_MANY_REQUESTS, "too_many_connections"));
        }
        *count += 1;
        registry.connections.fetch_add(1, Ordering::AcqRel);
    }
    // A room is shared only while it is registered: a member that raced with the last one leaving
    // (which unregisters it) retries, so two rooms never hold the same document.
    loop {
        let existing = registry
            .rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&node)
            .cloned();
        let room = match existing {
            Some(room) => room,
            None => match Room::open(app, node).await {
                Ok((opened, jobs)) => {
                    let mut rooms = registry.rooms.lock().unwrap_or_else(|e| e.into_inner());
                    // Another member may have opened it meanwhile: keep the first.
                    match rooms.get(&node) {
                        Some(first) => first.clone(),
                        None => {
                            let room = Arc::new(opened);
                            rooms.insert(node, room.clone());
                            tokio::spawn(write(app.clone(), node, Arc::downgrade(&room), jobs));
                            room
                        }
                    }
                }
                Err(error) => {
                    release_user(user);
                    return Err(error);
                }
            },
        };
        if room.members.fetch_add(1, Ordering::AcqRel) >= MAX_MEMBERS {
            room.members.fetch_sub(1, Ordering::AcqRel);
            release_user(user);
            return Err(Error(StatusCode::TOO_MANY_REQUESTS, "room_full"));
        }
        let registered = registry
            .rooms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&node)
            .is_some_and(|open| Arc::ptr_eq(open, &room));
        if registered && !room.poisoned.load(Ordering::Acquire) {
            return Ok(Membership {
                room,
                user,
                app: app.clone(),
            });
        }
        room.members.fetch_sub(1, Ordering::AcqRel);
    }
}

fn release_user(user: Uuid) {
    let registry = registry();
    let mut users = registry.users.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(count) = users.get_mut(&user) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            users.remove(&user);
        }
    }
    registry.connections.fetch_sub(1, Ordering::AcqRel);
}

impl Drop for Membership {
    fn drop(&mut self) {
        release_user(self.user);
        if self.room.members.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let room = self.room.clone();
        let app = self.app.clone();
        tokio::spawn(async move {
            if room.members.load(Ordering::Acquire) != 0 {
                return;
            }
            room.compact(&app).await;
            let mut rooms = registry().rooms.lock().unwrap_or_else(|e| e.into_inner());
            if room.members.load(Ordering::Acquire) == 0
                && rooms
                    .get(&room.node)
                    .is_some_and(|open| Arc::ptr_eq(open, &room))
            {
                rooms.remove(&room.node);
            }
        });
    }
}
