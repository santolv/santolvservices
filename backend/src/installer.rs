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
    pub app_origin: String,
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
        let bytes = fs::read(path)?;
        if bytes.len() > 64 * 1024 {
            return Err("configuration exceeds 64 KiB".into());
        }
        let mut c: Self = serde_json::from_slice(&bytes)?;
        let parent = path.parent().unwrap_or(Path::new("."));
        if !c.storage_root.is_absolute() {
            c.storage_root = parent.join(&c.storage_root);
        }
        for file in [&mut c.tls_cert_file, &mut c.tls_key_file]
            .into_iter()
            .flatten()
        {
            if !file.is_absolute() {
                *file = parent.join(&*file);
            }
        }
        c.validate()?;
        Ok(c)
    }
    pub fn from_env() -> Result<Self> {
        let c = Self {
            database_url: env::var("DATABASE_URL")?,
            app_origin: env::var("APP_ORIGIN")?,
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
            allow_insecure_localhost: env::var("ALLOW_INSECURE_LOCALHOST").as_deref() == Ok("true"),
        };
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<()> {
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
        if self.allow_insecure_localhost {
            if !self.bind.ip().is_loopback()
                || u.scheme() != "http"
                || !matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            {
                return Err(
                    "insecure development requires HTTP loopback binding and origin".into(),
                );
            }
        } else if u.scheme() != "https"
            || self.tls_cert_file.is_none()
            || self.tls_key_file.is_none()
        {
            return Err("native HTTPS requires TLS_CERT_FILE and TLS_KEY_FILE".into());
        }
        if self.default_quota_bytes <= 0 || !(1..=36500).contains(&self.trash_retention_days) {
            return Err("invalid quota or trash retention".into());
        }
        Ok(())
    }
    pub async fn connect(&self) -> Result<App> {
        App::connect(
            &self.database_url,
            self.storage_root.clone(),
            self.app_origin.clone(),
            !self.allow_insecure_localhost,
            self.default_quota_bytes,
            self.trash_retention_days,
        )
        .await
    }
}
fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut v = String::new();
    io::stdin().read_line(&mut v)?;
    Ok(v.trim().into())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
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
    let username = auth::username(&prompt("Usuario administrador: ")?)?;
    let password = rpassword::prompt_password("Contraseña (mínimo 12 bytes): ")?;
    auth::validate_password(&password)?;
    if password != rpassword::prompt_password("Repetir contraseña: ")? {
        return Err("passwords do not match".into());
    }
    Ok(Credentials { username, password })
}
pub async fn install(args: &[String]) -> Result<()> {
    if args == ["--check"] {
        println!(
            "{}",
            serde_json::json!({"os":env::consts::OS,"architecture":env::consts::ARCH,"runtime":"native Rust","https":"Rustls","external_exception":"PostgreSQL","external_programs_installed":false,"changes_made":false})
        );
        return Ok(());
    }
    let mut root = env::current_dir()?.join("santolvdrive-server");
    let mut origin = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--directory" => {
                i += 1;
                root = args.get(i).ok_or("--directory requires a path")?.into();
            }
            "--origin" => {
                i += 1;
                origin = Some(args.get(i).ok_or("--origin requires a URL")?.clone());
            }
            _ => {
                return Err(
                    "usage: install [--directory PATH] [--origin https://host:port] | --check"
                        .into(),
                );
            }
        }
        i += 1;
    }
    if !root.is_absolute() {
        root = env::current_dir()?.join(root);
    }
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
    let config_path = root.join("server.json");
    let config = if config_path.exists() {
        let config = Config::load(&config_path)?;
        if origin
            .as_ref()
            .is_some_and(|value| value != &config.app_origin)
        {
            return Err("existing origin preserved; change server.json explicitly after updating certificates".into());
        }
        config
    } else {
        let app_origin = match origin {
            Some(v) => v,
            None => prompt("Origen HTTPS (por ejemplo https://drive.example.com:8443): ")?,
        };
        let u = url::Url::parse(&app_origin)?;
        let port = u
            .port_or_known_default()
            .ok_or("origin requires HTTPS port")?;
        let database_url =
            rpassword::prompt_password("URL PostgreSQL existente (entrada oculta): ")?;
        let cert = PathBuf::from(prompt("Ruta del certificado/cadena PEM: ")?);
        let key = PathBuf::from(prompt("Ruta de la clave privada PEM: ")?);
        tls::config(&cert, &key)?;
        let c = Config {
            database_url,
            app_origin,
            bind: format!("0.0.0.0:{port}").parse()?,
            storage_root: root.join("data"),
            tls_cert_file: Some(root.join("certificate.pem")),
            tls_key_file: Some(root.join("private-key.pem")),
            default_quota_bytes: 10737418240,
            trash_retention_days: 30,
            allow_insecure_localhost: false,
        };
        c.validate()?;
        // Validate the allowed database dependency before committing a deployment configuration.
        let _ = c.connect().await?;
        for (source, target) in [
            (&cert, c.tls_cert_file.as_ref().unwrap()),
            (&key, c.tls_key_file.as_ref().unwrap()),
        ] {
            write_new(target, &fs::read(source)?)?;
        }
        write_new(&config_path, &serde_json::to_vec_pretty(&c)?)?;
        c
    };
    tls::config(
        config
            .tls_cert_file
            .as_ref()
            .ok_or("TLS certificate required")?,
        config.tls_key_file.as_ref().ok_or("TLS key required")?,
    )?;
    let app = config.connect().await?;
    let initialized: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE account_type='admin')")
            .fetch_one(&app.db)
            .await?;
    if !initialized {
        crate::admin::bootstrap(&app, credentials()?).await?;
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
        "Instalación nativa preparada. Inicia {} serve --config {}. No se ha instalado ningún programa externo.",
        binary.display(),
        config_path.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn deployment_files_are_resumable_without_overwriting() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("secret");
        super::write_new(&p, b"value").unwrap();
        super::write_new(&p, b"value").unwrap();
        assert!(super::write_new(&p, b"other").is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"value");
    }
    fn config() -> super::Config {
        super::Config {
            database_url: "postgres://localhost/test".into(),
            app_origin: "https://drive.example.com".into(),
            bind: "0.0.0.0:443".parse().unwrap(),
            storage_root: "data".into(),
            tls_cert_file: Some("certificate.pem".into()),
            tls_key_file: Some("key.pem".into()),
            default_quota_bytes: 1024,
            trash_retention_days: 30,
            allow_insecure_localhost: false,
        }
    }
    #[test]
    fn fail_closed_configuration() {
        let mut c = config();
        assert!(c.validate().is_ok());
        c.tls_key_file = None;
        assert!(c.validate().is_err());
        c.allow_insecure_localhost = true;
        c.app_origin = "http://localhost:8080".into();
        assert!(c.validate().is_err());
        c.bind = "127.0.0.1:8080".parse().unwrap();
        assert!(c.validate().is_ok());
        c.app_origin = "http://attacker.example.com".into();
        assert!(c.validate().is_err());
    }
    #[test]
    fn resolves_paths_relative_to_configuration() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("server.json");
        std::fs::write(&path, serde_json::to_vec(&config()).unwrap()).unwrap();
        let c = super::Config::load(&path).unwrap();
        assert_eq!(c.storage_root, t.path().join("data"));
        assert_eq!(c.tls_key_file.unwrap(), t.path().join("key.pem"));
    }
}
