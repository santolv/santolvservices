use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use santolvdrive::{App, auth, router};
use sqlx::PgPool;
use std::{sync::Arc, time::Instant};
use tokio::sync::Semaphore;
use tower::ServiceExt;
use uuid::Uuid;

// Run only against an isolated test server: cargo test --test load -- --ignored --nocapture
#[sqlx::test]
#[ignore = "explicit load measurement; creates 10000 metadata rows"]
async fn metadata_baseline(pool: PgPool) {
    let root = tempfile::tempdir().unwrap();
    let user = Uuid::new_v4();
    let space = Uuid::new_v4();
    let raw = auth::token();
    let app = App {
        db: pool.clone(),
        root: root.path().into(),
        origin: "http://localhost".into(),
        secure: false,
        hashes: Arc::new(Semaphore::new(2)),
        transfers: Arc::new(Semaphore::new(8)),
        requests: Arc::new(Semaphore::new(128)),
        retention_days: 30,
        quota: 1024,
    };
    sqlx::query("INSERT INTO users(id,username,password_hash) VALUES($1,'load-user','unused')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO spaces(id,name,kind,owner_id,quota) VALUES($1,'Load','personal',$2,1024)",
    )
    .bind(space)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO members VALUES($1,$2,3)")
        .bind(space)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions VALUES($1,$2,'unused',now()+interval '1 hour')")
        .bind(auth::hash(&raw))
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO nodes(id,space_id,name,kind) SELECT gen_random_uuid(),$1,'folder-'||n,'folder' FROM generate_series(1,10000)n").bind(space).execute(&pool).await.unwrap();
    let api = router(app);
    let all = Instant::now();
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let api = api.clone();
        let raw = raw.clone();
        jobs.spawn(async move {
            let mut times = Vec::new();
            for _ in 0..25 {
                let start = Instant::now();
                let response = api
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(format!("/api/v1/spaces/{space}/nodes?limit=100"))
                            .header("authorization", format!("Bearer {raw}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let _ = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                times.push(start.elapsed().as_micros());
            }
            times
        });
    }
    let mut times = Vec::new();
    while let Some(result) = jobs.join_next().await {
        times.extend(result.unwrap());
    }
    times.sort_unstable();
    println!(
        "rows=10000 concurrency=16 requests={} duration_s={:.3} p50_us={} p95_us={} p99_us={}",
        times.len(),
        all.elapsed().as_secs_f64(),
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() * 99 / 100]
    );
}
