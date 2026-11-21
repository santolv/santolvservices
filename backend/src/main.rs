use santolvdrive::{router, worker};
use std::{env, path::PathBuf};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|v| v == "install") {
        return santolvdrive::installer::install(&args[1..]).await;
    }
    if args
        .first()
        .is_some_and(|v| matches!(v.as_str(), "--help" | "help"))
    {
        println!(
            "santolvdrive [serve | install [--directory PATH] [--origin https://domain] | install --check | bootstrap-admin [--stdin] | reset-admin | admin-initialized] [--config server.json]"
        );
        return Ok(());
    }
    let command = args.first().map(String::as_str).unwrap_or("serve");
    if !matches!(
        command,
        "serve" | "bootstrap-admin" | "reset-admin" | "admin-initialized"
    ) {
        return Err("unknown command; use --help".into());
    }
    let config_path = args
        .iter()
        .position(|v| v == "--config")
        .map(|i| {
            args.get(i + 1)
                .map(PathBuf::from)
                .ok_or("--config requires a path")
        })
        .transpose()?;
    let config = if let Some(path) = config_path {
        santolvdrive::installer::Config::load(&path)?
    } else {
        santolvdrive::installer::Config::from_env()?
    };
    let s = config.connect().await?;
    match command {
        "admin-initialized" => {
            let initialized: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE account_type='admin')")
                    .fetch_one(&s.db)
                    .await?;
            if !initialized {
                return Err("server administrator not initialized".into());
            }
            return Ok(());
        }
        "bootstrap-admin" | "reset-admin" => {
            let credentials = if command == "bootstrap-admin" && args.iter().any(|v| v == "--stdin")
            {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::io::stdin().take(4097).read_to_end(&mut bytes)?;
                if bytes.len() > 4096 {
                    return Err("bootstrap input too large".into());
                }
                serde_json::from_slice(&bytes)?
            } else {
                santolvdrive::installer::credentials()?
            };
            if command == "bootstrap-admin" {
                santolvdrive::admin::bootstrap(&s, credentials).await?;
            } else {
                santolvdrive::admin::local_reset(&s, credentials).await?;
            }
            println!("Administrador actualizado. No tiene acceso a operaciones de archivos.");
            return Ok(());
        }
        _ => {}
    }
    let tls = if config.allow_insecure_localhost {
        None
    } else {
        Some(santolvdrive::tls::config(
            config
                .tls_cert_file
                .as_ref()
                .ok_or("TLS certificate required")?,
            config.tls_key_file.as_ref().ok_or("TLS key required")?,
        )?)
    };
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let task = tokio::spawn(worker::run(s.clone()));
    if config.allow_insecure_localhost {
        axum::serve(
            listener,
            router(s).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown())
        .await?;
    } else {
        santolvdrive::tls::serve(
            listener,
            tls.ok_or("TLS configuration required")?,
            router(s),
            shutdown(),
        )
        .await?;
    }
    task.abort();
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
