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
            "santolvdrive [serve | users [list [AFTER_UUID] | create | disable USERNAME | enable USERNAME | reset-password | audit [AFTER_ID]] | install [--directory PATH] [--origin http://localhost:8443] [--dependencies-only] | install --check | configure-local | bootstrap-admin [--stdin] | reset-admin | admin-initialized | database-stop] [--config server.json]"
        );
        return Ok(());
    }
    let command = args.first().map(String::as_str).unwrap_or("serve");
    if !matches!(
        command,
        "serve"
            | "users"
            | "configure-local"
            | "bootstrap-admin"
            | "reset-admin"
            | "admin-initialized"
            | "database-stop"
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
    if command == "configure-local" {
        return santolvdrive::installer::configure_local(
            config_path
                .as_deref()
                .ok_or("configure-local requires --config PATH")?,
        );
    }
    let config = if let Some(path) = config_path {
        santolvdrive::installer::Config::load(&path)?
    } else {
        santolvdrive::installer::Config::from_env()?
    };
    if command == "database-stop" {
        santolvdrive::postgres::stop(
            config
                .managed_postgres
                .as_ref()
                .ok_or("this configuration uses an external PostgreSQL instance")?,
        )
        .await?;
        return Ok(());
    }
    let s = config.connect().await?;
    if command == "users" {
        let end = args
            .iter()
            .position(|v| v == "--config")
            .unwrap_or(args.len());
        let result = santolvdrive::admin::console(&s, &args[1..end]).await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
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
                santolvdrive::installer::stdin_credentials()?
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
    let tls = match (&config.tls_cert_file, &config.tls_key_file) {
        (Some(cert), Some(key)) if !config.allow_insecure_localhost => {
            Some(santolvdrive::tls::config(cert, key)?)
        }
        _ => None,
    };
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    println!("Servidor local: {}", config.app_origin);
    let task = tokio::spawn(worker::run(s.clone()));
    let result = if let Some(tls) = tls {
        santolvdrive::tls::serve(listener, tls, router(s), shutdown()).await
    } else {
        santolvdrive::tls::serve_http(listener, router(s), shutdown()).await
    };
    task.abort();
    result?;
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
