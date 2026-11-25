use santolvdrive::{App, core::manager, installer::Config, postgres::Managed, router};
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
            "santolvdrive [serve | services [list | enable ID | disable ID] | users [list [AFTER_UUID] | create | disable USERNAME | enable USERNAME | reset-password | audit [AFTER_ID] | modules USERNAME | deny-module USERNAME ID | allow-module USERNAME ID] | install [--directory PATH] [--origin http://localhost:8443] [--dependencies-only | --stdin] | install --check | configure-local | bootstrap-admin [--stdin] | reset-admin | admin-initialized | database-stop] [--config server.json]"
        );
        return Ok(());
    }
    let command = args.first().map(String::as_str).unwrap_or("serve");
    if !matches!(
        command,
        "serve"
            | "users"
            | "services"
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
        santolvdrive::installer::Config::from_installation_or_env()?
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
    let (s, mut database) = config.connect(command == "serve").await?;
    let result = run(command, &args, s, &config, &mut database).await;
    let shutdown = match database {
        Some(database) if database.pid().is_some() => database.shutdown().await,
        _ => Ok(()),
    };
    result.and(shutdown)
}

async fn run(
    command: &str,
    args: &[String],
    s: App,
    config: &Config,
    database: &mut Option<Managed>,
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(command, "users" | "services") {
        let end = args
            .iter()
            .position(|v| v == "--config")
            .unwrap_or(args.len());
        let result = if command == "services" {
            santolvdrive::core::manager::console(&s, &args[1..end]).await?
        } else {
            santolvdrive::admin::console(&s, &args[1..end]).await?
        };
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
    // Legacy launchers pass --frontend with the old Drive page; all services now share the entry point.
    if let Some(i) = args.iter().position(|v| v == "--frontend") {
        args.get(i + 1).ok_or("--frontend requires an HTML path")?;
        println!(
            "--frontend es una opción antigua; se sirve Inicio, Drive y Fotos desde el servidor central."
        );
    }
    let frontend = if config.app_origin.starts_with("http://") {
        let view = santolvdrive::frontend::Frontend::bundled(
            &config.frontend_origin,
            &config.app_origin,
            s.clone(),
        )?;
        let origin = url::Url::parse(&config.frontend_origin)?;
        let socket = tokio::net::TcpListener::bind((
            std::net::Ipv4Addr::LOCALHOST,
            origin.port().ok_or("frontend requires a port")?,
        ))
        .await?;
        println!("Servicios: {}", config.frontend_origin);
        Some(tokio::spawn(santolvdrive::tls::serve_http(
            socket,
            santolvdrive::frontend::router(view),
            shutdown(),
        )))
    } else {
        None
    };
    println!("Servidor local: {}", config.app_origin);
    if let Some(pid) = database.as_ref().and_then(Managed::pid) {
        println!("Santolv PID {} | PostgreSQL PID {pid}", std::process::id());
    }
    let task = tokio::spawn(manager::run(s.clone()));
    let result = tokio::select! {
        result = async {
            if let Some(tls) = tls {
                santolvdrive::tls::serve(listener, tls, router(s), shutdown()).await
            } else {
                santolvdrive::tls::serve_http(listener, router(s), shutdown()).await
            }
        } => result,
        result = wait_for_database_exit(database.as_mut()) => result,
    };
    task.abort();
    let _ = task.await;
    if let Some(frontend) = frontend {
        frontend.abort();
        let _ = frontend.await;
    }
    result?;
    Ok(())
}

async fn wait_for_database_exit(database: Option<&mut Managed>) -> std::io::Result<()> {
    if let Some(database) = database
        && database.pid().is_some()
    {
        loop {
            if database.exited()? {
                return Err(std::io::Error::other("Managed PostgreSQL stopped"));
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
    std::future::pending().await
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
