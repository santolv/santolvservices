use crate::{App, admin::Credentials, auth, permissions, tls};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub database_url: String,
    #[serde(default)]
    pub managed_postgres: Option<PathBuf>,
    pub app_origin: String,
    #[serde(default = "crate::web::default_frontend_origin")]
    pub frontend_origin: String,
    pub bind: SocketAddr,
    pub storage_root: PathBuf,
    pub tls_cert_file: Option<PathBuf>,
    pub tls_key_file: Option<PathBuf>,
    pub default_quota_bytes: i64,
    pub trash_retention_days: i64,
    #[serde(default)]
    pub allow_insecure_localhost: bool,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let c = Self::read(path)?;
        c.validate()?;
        Ok(c)
    }
    pub fn from_installation_or_env() -> Result<Self> {
        if env::var_os("DATABASE_URL").is_none() {
            let path = env::current_exe()?.with_file_name("server.json");
            if path.try_exists()? {
                return Self::load(&path);
            }
        }
        Self::from_env()
    }
    fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        if bytes.len() > 64 * 1024 {
            return Err("configuration exceeds 64 KiB".into());
        }
        let mut c: Self = serde_json::from_slice(&bytes)?;
        let parent = path.parent().unwrap_or(Path::new("."));
        if !c.storage_root.is_absolute() {
            c.storage_root = parent.join(&c.storage_root);
        }
        if let Some(path) = &mut c.managed_postgres
            && !path.is_absolute()
        {
            *path = parent.join(&*path);
        }
        for file in [&mut c.tls_cert_file, &mut c.tls_key_file]
            .into_iter()
            .flatten()
        {
            if !file.is_absolute() {
                *file = parent.join(&*file);
            }
        }
        Ok(c)
    }
    pub fn from_env() -> Result<Self> {
        let c = Self {
            database_url: env::var("DATABASE_URL")?,
            managed_postgres: None,
            app_origin: env::var("APP_ORIGIN").unwrap_or("http://localhost:8443".into()),
            frontend_origin: env::var("FRONTEND_ORIGIN")
                .unwrap_or_else(|_| crate::web::default_frontend_origin()),
            bind: env::var("BIND")
                .unwrap_or("127.0.0.1:8443".into())
                .parse()?,
            storage_root: env::var("STORAGE_ROOT").unwrap_or("data".into()).into(),
            tls_cert_file: env::var_os("TLS_CERT_FILE").map(PathBuf::from),
            tls_key_file: env::var_os("TLS_KEY_FILE").map(PathBuf::from),
            default_quota_bytes: env::var("DEFAULT_QUOTA_BYTES")
                .unwrap_or("10737418240".into())
                .parse()?,
            trash_retention_days: env::var("TRASH_RETENTION_DAYS")
                .unwrap_or("30".into())
                .parse()?,
            allow_insecure_localhost: env::var("ALLOW_INSECURE_LOCALHOST").as_deref()
                != Ok("false"),
        };
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<()> {
        let frontend = url::Url::parse(&self.frontend_origin)?;
        if frontend.scheme() != "http"
            || frontend.host_str() != Some("localhost")
            || self.frontend_origin != frontend.origin().ascii_serialization()
            || frontend.path() != "/"
            || frontend.query().is_some()
            || frontend.fragment().is_some()
            || !frontend.username().is_empty()
            || frontend.password().is_some()
        {
            return Err("FRONTEND_ORIGIN must be an exact HTTP localhost origin, for example http://localhost:5500".into());
        }
        let u = url::Url::parse(&self.app_origin)?;
        if !u.username().is_empty()
            || u.password().is_some()
            || u.host_str().is_none()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.path() != "/"
            || self.app_origin != u.origin().ascii_serialization()
        {
            return Err("APP_ORIGIN must be a canonical origin without credentials or path".into());
        }
        if !self.bind.ip().is_loopback() {
            return Err("local server must bind to loopback (127.0.0.1 or ::1)".into());
        }
        if self.allow_insecure_localhost {
            if u.scheme() != "http"
                || !matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            {
                return Err("HTTP is restricted to local loopback origins".into());
            }
            if self.tls_cert_file.is_some() || self.tls_key_file.is_some() {
                return Err("local HTTP must not configure TLS files".into());
            }
        } else if u.scheme() != "https"
            || self.tls_cert_file.is_none()
            || self.tls_key_file.is_none()
        {
            return Err("local HTTPS requires certificate and private key".into());
        }
        if self.tls_cert_file.is_some() != self.tls_key_file.is_some() {
            return Err("TLS certificate and key must be configured together".into());
        }
        if self.default_quota_bytes <= 0 || !(1..=36500).contains(&self.trash_retention_days) {
            return Err("invalid quota or trash retention".into());
        }
        Ok(())
    }
    pub async fn connect(
        &self,
        own_database: bool,
    ) -> Result<(App, Option<crate::postgres::Managed>)> {
        self.validate()?;
        let database = if let Some(state) = &self.managed_postgres {
            let expected = crate::postgres::configured_url(state)?;
            if expected != self.database_url {
                return Err("managed PostgreSQL URL does not match its private state".into());
            }
            Some(crate::postgres::ensure(state, own_database).await?)
        } else {
            None
        };
        let mut app = App::connect(
            &self.database_url,
            self.storage_root.clone(),
            self.app_origin.clone(),
            !self.allow_insecure_localhost,
            self.default_quota_bytes,
            self.trash_retention_days,
        )
        .await?;
        app.frontend_origin = self.frontend_origin.clone();

        Ok((app, database))
    }
}
pub(crate) fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut v = String::new();
    io::stdin().read_line(&mut v)?;
    Ok(v.trim().into())
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        if fs::read(path)? == bytes {
            return Ok(());
        }
        return Err(
            "existing deployment file preserved; choose an explicit update procedure".into(),
        );
    }
    let temporary = path.with_extension(format!("{}.pending", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        {
            fs::File::open(path.parent().unwrap_or(Path::new(".")))?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
pub fn credentials() -> Result<Credentials> {
    credentials_for("Usuario administrador: ")
}
pub fn credentials_for(label: &str) -> Result<Credentials> {
    let username = auth::username(&prompt(label)?)?;
    let password = rpassword::prompt_password("Contraseña (mínimo 12 bytes): ")?;
    auth::validate_password(&password)?;
    if password != rpassword::prompt_password("Repetir contraseña: ")? {
        return Err("passwords do not match".into());
    }
    Ok(Credentials { username, password })
}
pub fn stdin_credentials() -> Result<Credentials> {
    use std::io::Read;
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err("credentials input too large".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}
async fn automatic_config(root: &Path, app_origin: String, bin: PathBuf) -> Result<Config> {
    let port = url::Url::parse(&app_origin)?
        .port_or_known_default()
        .ok_or("local origin needs a port")?;

    let mut c = Config {
        database_url: String::new(),
        managed_postgres: None,

        app_origin,
        frontend_origin: crate::web::default_frontend_origin(),
        bind: format!("127.0.0.1:{port}").parse()?,
        storage_root: PathBuf::from("data"),
        tls_cert_file: None,
        tls_key_file: None,
        default_quota_bytes: 10737418240,
        trash_retention_days: 30,
        allow_insecure_localhost: true,
    };
    c.validate()?;
    let state = crate::postgres::prepare(root, bin)?;
    c.database_url = crate::postgres::configured_url(&state)?;
    c.managed_postgres = Some(PathBuf::from("postgres-state.json"));
    let bytes = serde_json::to_vec_pretty(&c)?;
    c.storage_root = root.join(&c.storage_root);
    c.managed_postgres = Some(state);
    let (app, database) = c.connect(false).await?;
    app.db.close().await;
    if let Some(database) = database {
        database.shutdown().await?;
    }
    write_new(&root.join("server.json"), &bytes)?;
    Ok(c)
}

pub fn configure_local(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let root = path
        .parent()
        .ok_or("configuration needs a parent directory")?;
    let original = fs::read(&path)?;
    if original.len() > 64 * 1024 {
        return Err("configuration exceeds 64 KiB".into());
    }
    let mut value: serde_json::Value = serde_json::from_slice(&original)?;
    let object = value
        .as_object_mut()
        .ok_or("configuration must be an object")?;
    object.retain(|key, _| {
        matches!(
            key.as_str(),
            "database_url"
                | "managed_postgres"
                | "storage_root"
                | "default_quota_bytes"
                | "trash_retention_days"
        )
    });
    object.insert("app_origin".into(), "http://localhost:8443".into());
    object.insert(
        "frontend_origin".into(),
        crate::web::default_frontend_origin().into(),
    );
    object.insert("bind".into(), "127.0.0.1:8443".into());
    object.insert("allow_insecure_localhost".into(), true.into());
    object.insert("tls_cert_file".into(), serde_json::Value::Null);
    object.insert("tls_key_file".into(), serde_json::Value::Null);
    let config: Config = serde_json::from_value(value)?;
    config.validate()?;
    permissions::private_directory(root)?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    let bytes = serde_json::to_vec_pretty(&config)?;
    if bytes != original {
        let backup = path.with_extension("before-local.json");
        if !backup.exists() {
            write_new(&backup, &original)?;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(root)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path)?;
        #[cfg(unix)]
        fs::File::open(root)?.sync_all()?;
    }
    println!(
        "Configuración local preparada: {}. Datos y cuentas conservados. Detén el proceso anterior e inicia serve --config {}. Acceso: {}",
        path.display(),
        path.display(),
        config.app_origin
    );
    Ok(())
}

pub async fn install(args: &[String]) -> Result<()> {
    if args == ["--check"] {
        println!(
            "{}",
            serde_json::json!({"os":env::consts::OS,"architecture":env::consts::ARCH,"runtime":"native Rust","ingress":"local loopback only","default_origin":"http://localhost:8443","frontend_origin":"http://localhost:5500","local_target":"HTTP loopback (optional Rustls HTTPS)","automatic_dependencies":["PostgreSQL when missing"],"external_programs_installed":false,"changes_made":false})
        );
        return Ok(());
    }
    let mut root = None;
    let mut origin = None;
    let mut dependencies_only = false;
    let mut read_stdin = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dependencies-only" => {
                dependencies_only = true;
            }
            "--stdin" => {
                read_stdin = true;
            }
            "--directory" => {
                i += 1;
                root = Some(PathBuf::from(
                    args.get(i).ok_or("--directory requires a path")?,
                ));
            }
            "--origin" => {
                i += 1;
                origin = Some(args.get(i).ok_or("--origin requires a URL")?.clone());
            }
            _ => {
                return Err(
                    "usage: install [--directory PATH] [--origin http://127.0.0.1:8443] [--dependencies-only | --stdin] | install --check"
                        .into(),
                );
            }
        }
        i += 1;
    }
    if dependencies_only && read_stdin {
        return Err("--dependencies-only and --stdin cannot be combined".into());
    }
    let cwd = env::current_dir()?;
    let legacy = cwd.join("santolvdrive-server");
    let root = cwd.join(root.unwrap_or_else(|| {
        if legacy.join(".santolvdrive-native").is_file() {
            legacy
        } else {
            PathBuf::from("servidor")
        }
    }));
    if root.parent().is_none() {
        return Err("installation directory cannot be a filesystem root".into());
    }
    let marker = root.join(".santolvdrive-native");
    if root.exists() && fs::read_dir(&root)?.next().is_some() && !marker.is_file() {
        return Err(
            "choose an empty dedicated directory; old deployments must be migrated explicitly"
                .into(),
        );
    }
    permissions::private_directory(&root)?;
    if !marker.exists() {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)?;
        f.write_all(b"SantolvDrive native Rust\n")?;
        f.sync_all()?;
    }
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    if dependencies_only {
        let bin = crate::dependencies::postgres(&root)
            .await
            .map_err(|e| e.to_string())?;
        println!(
            "Dependencias preparadas. PostgreSQL: {}. No se ha creado una base ni arrancado el servidor.",
            bin.display()
        );
        return Ok(());
    }
    let config_path = root.join("server.json");
    let config = if config_path.exists() {
        let config = Config::load(&config_path)?;
        if origin
            .as_ref()
            .is_some_and(|value| value != &config.app_origin)
        {
            return Err("existing origin preserved; change the configuration explicitly".into());
        }
        config
    } else {
        let app_origin = origin.unwrap_or("http://localhost:8443".into());
        let bin = crate::dependencies::postgres(&root)
            .await
            .map_err(|e| e.to_string())?;
        automatic_config(&root, app_origin, bin).await?
    };
    if let (Some(cert), Some(key)) = (&config.tls_cert_file, &config.tls_key_file) {
        tls::config(cert, key)?;
    }
    let (app, database) = config.connect(false).await?;
    let initialized: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE account_type='admin')")
            .fetch_one(&app.db)
            .await?;
    if !initialized {
        let credentials = if read_stdin {
            stdin_credentials()?
        } else {
            credentials()?
        };
        crate::admin::bootstrap(&app, credentials).await?;
    }
    let binary = root.join(if cfg!(windows) {
        "santolvdrive.exe"
    } else {
        "santolvdrive"
    });
    let current = env::current_exe()?;
    if current.canonicalize()? != binary.canonicalize().unwrap_or_else(|_| binary.clone()) {
        write_new(&binary, &fs::read(current)?)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))?;
    }
    println!(
        "Santolv Services instalado. Ejecuta \"{}\" serve. Web: {}.",
        binary.display(),
        config.frontend_origin
    );

    app.db.close().await;
    if let Some(database) = database {
        database.shutdown().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn config() -> super::Config {
        super::Config {
            database_url: "postgres://localhost/test".into(),
            managed_postgres: None,
            app_origin: "http://127.0.0.1:8443".into(),
            frontend_origin: crate::web::default_frontend_origin(),
            bind: "127.0.0.1:8443".parse().unwrap(),
            storage_root: "data".into(),
            tls_cert_file: None,
            tls_key_file: None,
            default_quota_bytes: 1024,
            trash_retention_days: 30,
            allow_insecure_localhost: true,
        }
    }
    #[test]
    fn local_configuration_cannot_expose_http_to_network() {
        let mut c = config();
        c.validate().unwrap();
        c.bind = "0.0.0.0:8443".parse().unwrap();
        assert!(c.validate().is_err());
        c.bind = "[::]:8443".parse().unwrap();
        assert!(c.validate().is_err());
        c.bind = "127.0.0.1:8443".parse().unwrap();
        c.app_origin = "http://example.com".into();
        assert!(c.validate().is_err());
        c.app_origin = "http://127.0.0.1:8443/path".into();
        assert!(c.validate().is_err());
        c.app_origin = "http://127.0.0.1:8443".into();
        c.allow_insecure_localhost = false;
        assert!(c.validate().is_err());
    }
    #[test]
    fn frontend_origin_cannot_be_wildcard_file_or_external() {
        let mut c = config();
        for origin in [
            "*",
            "null",
            "file://",
            "http://evil.example:5500",
            "http://localhost:5500/",
            "http://user@localhost:5500",
            "http://localhost:5500/path",
        ] {
            c.frontend_origin = origin.into();
            assert!(c.validate().is_err(), "{origin}");
        }
        c.frontend_origin = "http://localhost:5500".into();
        c.validate().unwrap();
    }
    #[test]
    fn configuration_migration_preserves_data_credentials_and_original_backup() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("server.json");
        let mut old = serde_json::to_value(config()).unwrap();
        old["obsolete_connector"] = serde_json::json!({"secret":"test-private-value"});
        old["app_origin"] = "https://old.example.com".into();
        old["bind"] = "0.0.0.0:443".into();
        let original = serde_json::to_vec(&old).unwrap();
        std::fs::write(&path, &original).unwrap();
        std::fs::create_dir(t.path().join("data")).unwrap();
        std::fs::write(t.path().join("data/content"), b"preserved").unwrap();
        super::configure_local(&path).unwrap();
        super::configure_local(&path).unwrap();
        let c = super::Config::load(&path).unwrap();
        assert_eq!(c.database_url, "postgres://localhost/test");
        assert_eq!(c.app_origin, "http://localhost:8443");
        assert_eq!(c.storage_root, t.path().join("data"));
        assert_eq!(
            std::fs::read(t.path().join("data/content")).unwrap(),
            b"preserved"
        );
        assert_eq!(
            std::fs::read(path.with_extension("before-local.json")).unwrap(),
            original
        );
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains("obsolete_connector")
        );
    }
    #[test]
    fn deployment_files_are_resumable_without_overwriting() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("secret");
        super::write_new(&p, b"value").unwrap();
        super::write_new(&p, b"value").unwrap();
        assert!(super::write_new(&p, b"other").is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"value");
    }
    #[tokio::test]
    #[ignore = "requires PostgreSQL binaries; exercises automatic local installation"]
    async fn automatic_install_preserves_local_configuration_and_database() {
        let t = tempfile::tempdir().unwrap();
        let c = super::automatic_config(
            t.path(),
            "http://127.0.0.1:8443".into(),
            crate::postgres::locate().unwrap(),
        )
        .await
        .unwrap();
        assert!(c.allow_insecure_localhost);
        assert!(c.bind.ip().is_loopback());
        let (app, database) = c.connect(false).await.unwrap();
        assert!(!app.secure);
        app.db.close().await;
        let original = std::fs::read(t.path().join("server.json")).unwrap();
        database.unwrap().shutdown().await.unwrap();
        let reloaded = super::Config::load(&t.path().join("server.json")).unwrap();
        let (restarted, database) = reloaded.connect(true).await.unwrap();
        restarted.db.close().await;
        database.unwrap().shutdown().await.unwrap();
        assert_eq!(
            std::fs::read(t.path().join("server.json")).unwrap(),
            original
        );
    }
}
