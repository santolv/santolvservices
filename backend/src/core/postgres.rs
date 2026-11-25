use crate::{auth, installer::write_new, permissions};
use serde::{Deserialize, Serialize};
use sqlx::{
    ConnectOptions, Connection,
    postgres::{PgConnectOptions, PgConnection},
};
use std::{
    env, fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize)]
struct State {
    bin: PathBuf,
    data: PathBuf,
    port: u16,
    owner_password: String,
    app_password: String,
}
impl State {
    fn load(path: &Path) -> Result<Self> {
        let mut state: Self = serde_json::from_slice(&fs::read(path)?)?;
        let parent = path
            .parent()
            .ok_or("managed PostgreSQL state needs a directory")?;
        state.bin = parent.join(state.bin);
        state.data = parent.join(state.data);
        Ok(state)
    }
}
fn executable(bin: &Path, name: &str) -> PathBuf {
    bin.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    })
}
fn command(bin: &Path, name: &str) -> Command {
    let c = Command::new(executable(bin, name));
    #[cfg(windows)]
    let c = {
        use std::os::windows::process::CommandExt;
        let mut c = c;
        c.creation_flags(0x08000000);
        c
    };
    c
}
pub(crate) fn tools(bin: &Path) -> bool {
    ["postgres", "pg_ctl", "initdb"]
        .iter()
        .all(|n| executable(bin, n).is_file())
}
pub(crate) fn canonical_tool_path(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    #[cfg(windows)]
    {
        let value = path
            .to_str()
            .ok_or("PostgreSQL path must be valid Unicode")?;
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(local) = value.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(local));
        }
    }
    Ok(path)
}
pub fn locate() -> Result<PathBuf> {
    if let Some(bin) = env::var_os("POSTGRES_BIN") {
        let bin = PathBuf::from(bin);
        if tools(&bin) {
            return canonical_tool_path(&bin);
        }
        return Err("POSTGRES_BIN must contain postgres, initdb and pg_ctl".into());
    }
    let mut candidates = env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    for parent in [
        PathBuf::from("/usr/lib/postgresql"),
        PathBuf::from(env::var_os("ProgramFiles").unwrap_or_else(|| "C:\\Program Files".into()))
            .join("PostgreSQL"),
    ] {
        if let Ok(entries) = fs::read_dir(parent) {
            let mut bins = entries
                .flatten()
                .map(|e| e.path().join("bin"))
                .collect::<Vec<_>>();
            bins.sort();
            bins.reverse();
            candidates.extend(bins);
        }
    }
    candidates.extend(
        [
            "/usr/local/pgsql/bin",
            "/opt/homebrew/opt/postgresql@17/bin",
            "/usr/local/opt/postgresql@17/bin",
        ]
        .map(PathBuf::from),
    );
    for bin in candidates {
        if tools(&bin) {
            return canonical_tool_path(&bin);
        }
    }
    Err("PostgreSQL tools not found: install the authorized PostgreSQL dependency or set POSTGRES_BIN. No database URL is needed.".into())
}
pub fn prepare(root: &Path, bin: PathBuf) -> Result<PathBuf> {
    permissions::private_directory(root)?;
    let path = root.join("postgres-state.json");
    if path.exists() {
        return Ok(path);
    }
    if !tools(&bin) {
        return Err("PostgreSQL binary directory is incomplete".into());
    }
    let socket = TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let bin = canonical_tool_path(&bin)?;
    let state = State {
        bin: bin.strip_prefix(root).unwrap_or(&bin).to_path_buf(),
        data: PathBuf::from("postgres"),
        port,
        owner_password: auth::token(),
        app_password: auth::token(),
    };
    write_new(&path, &serde_json::to_vec_pretty(&state)?)?;
    Ok(path)
}
fn url(state: &State, owner: bool) -> String {
    format!(
        "postgres://{}:{}@127.0.0.1:{}/{}?sslmode=disable",
        if owner { "drive_owner" } else { "drive" },
        if owner {
            &state.owner_password
        } else {
            &state.app_password
        },
        state.port,
        if owner { "postgres" } else { "drive" }
    )
}
async fn output(mut c: Command) -> Result<Output> {
    tokio::task::spawn_blocking(move || c.output())
        .await?
        .map_err(Into::into)
}
fn checked(result: Output, action: &str) -> Result<()> {
    if !result.status.success() {
        return Err(format!(
            "PostgreSQL {action} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}
pub async fn ensure(path: &Path) -> Result<String> {
    let state = State::load(path)?;
    let parent = path
        .parent()
        .ok_or("managed PostgreSQL state needs a directory")?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(".postgres.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    if !tools(&state.bin) {
        return Err("managed PostgreSQL tools are unavailable; preserve state and configure the binary directory".into());
    }
    if !state.data.join("PG_VERSION").is_file() {
        if state.data.exists() {
            return Err(
                "existing unrecognized PostgreSQL directory preserved; manual recovery required"
                    .into(),
            );
        }
        let staging = parent.join(format!("postgres-initializing-{}", uuid::Uuid::new_v4()));
        let password_file = parent.join(format!(".pg-password-{}", uuid::Uuid::new_v4()));
        write_new(&password_file, state.owner_password.as_bytes())?;
        let mut init = command(&state.bin, "initdb");
        init.arg("-D")
            .arg(&staging)
            .args([
                "--username=drive_owner",
                "--auth=scram-sha-256",
                "--encoding=UTF8",
                "--no-locale",
                "--pwfile",
            ])
            .arg(&password_file);
        let result = output(init).await;
        let removed = fs::remove_file(&password_file);
        removed?;
        checked(
            result?,
            "initialization (run as an unprivileged OS account)",
        )?;
        fs::rename(staging, &state.data)?;
        crate::storage::sync_dir(parent).await?;
    }
    let mut status = command(&state.bin, "pg_ctl");
    status.arg("-D").arg(&state.data).arg("status");
    let running = output(status).await?;
    if !running.status.success() {
        let mut start = command(&state.bin, "pg_ctl");
        start
            .arg("-D")
            .arg(&state.data)
            .args(["-w", "-t", "30", "-l"])
            .arg(parent.join("postgresql.log"))
            .args([
                "-o",
                &format!("-h 127.0.0.1 -p {} -c unix_socket_directories=", state.port),
                "start",
            ]);
        // A detached Windows postmaster can inherit pipe handles; never capture startup via pipes.
        let status = tokio::task::spawn_blocking(move || {
            start.stdout(Stdio::null()).stderr(Stdio::null()).status()
        })
        .await??;
        if !status.success() {
            return Err(format!(
                "PostgreSQL startup failed; see {} (the local port may be occupied)",
                parent.join("postgresql.log").display()
            )
            .into());
        }
    }
    let options: PgConnectOptions = url(&state, true).parse()?;
    let mut owner = PgConnection::connect_with(&options.disable_statement_logging()).await?;
    let data: String = sqlx::query_scalar("SELECT current_setting('data_directory')")
        .fetch_one(&mut owner)
        .await?;
    if Path::new(&data).canonicalize()? != state.data.canonicalize()? {
        return Err("PostgreSQL instance identity mismatch".into());
    }
    let addresses: String = sqlx::query_scalar("SELECT current_setting('listen_addresses')")
        .fetch_one(&mut owner)
        .await?;
    if addresses != "127.0.0.1" {
        return Err("managed PostgreSQL must listen exclusively on loopback".into());
    }
    sqlx::query("SET password_encryption='scram-sha-256'")
        .execute(&mut owner)
        .await?;
    let role: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='drive')")
            .fetch_one(&mut owner)
            .await?;
    if !role {
        let ddl:String=sqlx::query_scalar("SELECT format('CREATE ROLE drive LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD %L',$1::text)").bind(&state.app_password).fetch_one(&mut owner).await?;
        // PostgreSQL %L quotes the bound password; identifiers and SQL are fixed here.
        sqlx::query(sqlx::AssertSqlSafe(ddl.as_str()))
            .execute(&mut owner)
            .await?;
    }
    let database: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname='drive')")
            .fetch_one(&mut owner)
            .await?;
    if !database {
        sqlx::query("CREATE DATABASE drive OWNER drive")
            .execute(&mut owner)
            .await?;
    }
    let options: PgConnectOptions = url(&state, false).parse()?;
    let mut app = PgConnection::connect_with(&options.disable_statement_logging()).await?;
    let limited:bool=sqlx::query_scalar("SELECT NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole FROM pg_roles WHERE rolname=current_user").fetch_one(&mut app).await?;
    if !limited {
        return Err("managed application role has excessive PostgreSQL privileges".into());
    }
    Ok(url(&state, false))
}
pub async fn stop(path: &Path) -> Result<()> {
    let state = State::load(path)?;
    let mut stop = command(&state.bin, "pg_ctl");
    stop.arg("-D")
        .arg(state.data)
        .args(["-w", "-m", "fast", "stop"]);
    checked(output(stop).await?, "shutdown")
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires PostgreSQL binaries; creates an isolated local cluster"]
    async fn provisioned_database_is_private_limited_and_resumable() {
        use sqlx::{Connection, Row};
        let t = tempfile::tempdir().unwrap();
        let state = super::prepare(t.path(), super::locate().unwrap()).unwrap();
        let url = super::ensure(&state).await.unwrap_or_else(|error| {
            panic!(
                "{error}; server log: {}",
                std::fs::read_to_string(t.path().join("postgresql.log")).unwrap_or_default()
            )
        });
        let saved = std::fs::read(&state).unwrap();
        assert_eq!(super::ensure(&state).await.unwrap(), url);
        assert_eq!(std::fs::read(&state).unwrap(), saved);
        let mut wrong = url::Url::parse(&url).unwrap();
        wrong.set_password(Some("invalid-password")).unwrap();
        assert!(sqlx::PgConnection::connect(wrong.as_str()).await.is_err());
        let mut c = sqlx::PgConnection::connect(&url).await.unwrap();
        let row=sqlx::query("SELECT current_user,current_setting('listen_addresses') AS addresses,current_setting('password_encryption') AS encryption").fetch_one(&mut c).await.unwrap();
        assert_eq!(row.get::<String, _>("current_user"), "drive");
        assert_eq!(row.get::<String, _>("addresses"), "127.0.0.1");
        assert_eq!(row.get::<String, _>("encryption"), "scram-sha-256");
        c.close().await.unwrap();
        super::stop(&state).await.unwrap();
        assert_eq!(super::ensure(&state).await.unwrap(), url);
        super::stop(&state).await.unwrap();
    }
}
