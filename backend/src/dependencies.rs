use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct Artifact {
    pub url: String,
    pub sha256: &'static str,
    pub max_bytes: u64,
}

fn allowed(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str() == Some("get.enterprisedb.com")
}

pub fn verify(path: &Path, artifact: &Artifact) -> Result<()> {
    if std::fs::metadata(path)?.len() > artifact.max_bytes {
        return Err("dependency exceeds size limit".into());
    }
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    if hex::encode(digest.finalize()) != artifact.sha256 {
        return Err(
            "dependency SHA-256 mismatch; existing file preserved and never executed".into(),
        );
    }
    Ok(())
}

pub async fn download(path: &Path, artifact: &Artifact) -> Result<()> {
    if path.exists() {
        return verify(path, artifact);
    }
    let url = url::Url::parse(&artifact.url)?;
    if !allowed(&url) {
        return Err("untrusted dependency download URL".into());
    }
    let root = path
        .parent()
        .ok_or("dependency requires parent directory")?;
    crate::permissions::private_directory(root)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .user_agent("SantolvDrive-installer/0.1")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed(attempt.url()) {
                attempt.error("untrusted dependency redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|n| n > artifact.max_bytes)
    {
        return Err("dependency exceeds size limit".into());
    }
    let temporary = tempfile::NamedTempFile::new_in(root)?;
    let mut file = tokio::fs::File::from_std(temporary.reopen()?);
    let mut stream = response.bytes_stream();
    let mut total = 0u64;
    let mut digest = Sha256::new();
    while let Some(block) = stream.next().await {
        let block = block?;
        total = total
            .checked_add(block.len() as u64)
            .ok_or("dependency size overflow")?;
        if total > artifact.max_bytes {
            return Err("dependency exceeds size limit".into());
        }
        digest.update(&block);
        file.write_all(&block).await?;
    }
    file.sync_all().await?;
    drop(file);
    if hex::encode(digest.finalize()) != artifact.sha256 {
        return Err("downloaded dependency SHA-256 mismatch; discarded without execution".into());
    }
    temporary.persist_noclobber(path)?;
    #[cfg(unix)]
    std::fs::File::open(root)?.sync_all()?;
    Ok(())
}

pub const POSTGRES_VERSION: &str = "17.12";
pub fn postgres_artifact() -> Artifact {
    Artifact {
        url: "https://get.enterprisedb.com/postgresql/postgresql-17.12-1-windows-x64-binaries.zip"
            .into(),
        sha256: "a2cd8a774389347b70be7748ac4a37ef43cb6ee539c5fc3e85ae264b15e4a04c",
        max_bytes: 512 * 1024 * 1024,
    }
}

pub fn extract_postgres(archive: &Path, destination: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    if zip.len() > 30000 {
        return Err("too many PostgreSQL archive entries".into());
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let relative = entry
            .enclosed_name()
            .ok_or("unsafe PostgreSQL archive path")?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("archive symlink rejected".into());
        }
        if !["pgsql/bin", "pgsql/lib", "pgsql/share", "pgsql/licenses"]
            .iter()
            .any(|p| relative.starts_with(p))
            && relative != Path::new("pgsql/server_license.txt")
            && relative != Path::new("pgsql/commandlinetools_3rd_party_licenses.txt")
        {
            continue;
        }
        total = total
            .checked_add(entry.size())
            .ok_or("archive size overflow")?;
        if total > 2 * 1024 * 1024 * 1024 {
            return Err("PostgreSQL extraction exceeds size limit".into());
        }
        let path = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(path)?;
            continue;
        }
        std::fs::create_dir_all(path.parent().ok_or("archive parent missing")?)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let expected = entry.size();
        if std::io::copy(&mut entry.by_ref().take(expected + 1), &mut output)? != expected {
            return Err("invalid PostgreSQL archive size".into());
        }
        output.flush()?;
        output.sync_all()?;
    }
    if !crate::postgres::tools(&destination.join("pgsql/bin")) {
        return Err("PostgreSQL archive is missing required tools".into());
    }
    Ok(())
}

pub async fn postgres(root: &Path) -> Result<PathBuf> {
    if let Ok(bin) = crate::postgres::locate() {
        return Ok(bin);
    }
    if std::env::var_os("POSTGRES_BIN").is_some() {
        return Err("explicit POSTGRES_BIN is invalid; it was not replaced".into());
    }
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return portable_postgres(root).await;
    }
    install_system_postgres().await?;
    crate::postgres::locate().map_err(|e| e.to_string().into())
}

async fn portable_postgres(root: &Path) -> Result<PathBuf> {
    let deps = root.join("dependencies");
    crate::permissions::private_directory(&deps)?;
    let target = deps.join(format!("postgresql-{POSTGRES_VERSION}"));
    if !target.exists() {
        let archive = deps.join(format!("postgresql-{POSTGRES_VERSION}.zip"));
        println!(
            "Descargando PostgreSQL portable {POSTGRES_VERSION} (sin pgAdmin ni StackBuilder)..."
        );
        download(&archive, &postgres_artifact()).await?;
        let stage = tempfile::Builder::new()
            .prefix("postgresql-staging-")
            .tempdir_in(&deps)?;
        extract_postgres(&archive, stage.path())?;
        std::fs::rename(stage.path(), &target)?;
        #[cfg(unix)]
        std::fs::File::open(&deps)?.sync_all()?;
        std::fs::remove_file(archive)?;
    }
    if !crate::postgres::tools(&target.join("pgsql/bin")) {
        return Err("existing portable PostgreSQL is incomplete; preserved for inspection".into());
    }
    crate::postgres::canonical_tool_path(&target.join("pgsql/bin"))
        .map_err(|e| e.to_string().into())
}

fn system_package() -> Result<(&'static str, Vec<&'static str>)> {
    if cfg!(target_os = "linux") {
        if Path::new("/usr/bin/apt-get").is_file() {
            return Ok((
                "/usr/bin/apt-get",
                vec!["install", "-y", "--no-install-recommends", "postgresql"],
            ));
        }
        if Path::new("/usr/bin/dnf").is_file() {
            return Ok(("/usr/bin/dnf", vec!["install", "-y", "postgresql-server"]));
        }
    }
    if cfg!(target_os = "macos") {
        for brew in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
            if Path::new(brew).is_file() {
                return Ok((brew, vec!["install", "postgresql@17"]));
            }
        }
    }
    Err("automatic PostgreSQL installation supports Windows x64, apt/dnf Linux, and macOS with existing Homebrew; install PostgreSQL on other systems and rerun (no unrelated package manager is installed)".into())
}

async fn install_system_postgres() -> Result<()> {
    let (program, args) = system_package()?;
    let root = std::fs::read_to_string("/proc/self/status")
        .ok()
        .is_some_and(|s| {
            s.lines()
                .any(|line| line.starts_with("Uid:") && line.split_whitespace().nth(2) == Some("0"))
        });
    let mut command = if cfg!(target_os = "linux") && !root {
        let mut c = tokio::process::Command::new("/usr/bin/sudo");
        c.arg("--").arg(program);
        c
    } else {
        tokio::process::Command::new(program)
    };
    println!(
        "Instalando PostgreSQL con el gestor del sistema; puede solicitar la contraseña del sistema operativo."
    );
    if !command.args(args).status().await?.success() {
        return Err(
            "PostgreSQL package installation failed; no application data was changed".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_downloads_reject_tampering_and_untrusted_urls() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("binary");
        std::fs::write(&path, b"abc").unwrap();
        let a = Artifact {
            url: "https://get.enterprisedb.com/a".into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            max_bytes: 3,
        };
        verify(&path, &a).unwrap();
        std::fs::write(&path, b"abd").unwrap();
        assert!(verify(&path, &a).is_err());
        std::fs::write(&path, b"abcd").unwrap();
        assert!(verify(&path, &a).is_err());
        for url in [
            "http://github.com/a",
            "https://github.com.evil.test/a",
            "https://user:pw@github.com/a",
            "https://github.com:444/a",
        ] {
            assert!(!allowed(&url::Url::parse(url).unwrap()));
        }
        assert!(allowed(
            &url::Url::parse("https://get.enterprisedb.com/file").unwrap()
        ));
    }
    #[test]
    fn rejects_zip_traversal() {
        let t = tempfile::tempdir().unwrap();
        let archive = t.path().join("unsafe.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("../escaped", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        zip.finish().unwrap();
        assert!(extract_postgres(&archive, &t.path().join("output")).is_err());
        assert!(!t.path().join("escaped").exists());
    }
    #[test]
    fn extracts_only_postgres_runtime_and_licenses() {
        let t = tempfile::tempdir().unwrap();
        let archive = t.path().join("runtime.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for name in ["postgres", "initdb", "pg_ctl"] {
            let extension = if cfg!(windows) { ".exe" } else { "" };
            zip.start_file(
                format!("pgsql/bin/{name}{extension}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        for name in [
            "pgsql/server_license.txt",
            "pgsql/commandlinetools_3rd_party_licenses.txt",
            "pgsql/pgAdmin 4/unneeded",
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        zip.finish().unwrap();
        let output = t.path().join("output");
        extract_postgres(&archive, &output).unwrap();
        assert!(output.join("pgsql/server_license.txt").is_file());
        assert!(
            output
                .join("pgsql/commandlinetools_3rd_party_licenses.txt")
                .is_file()
        );
        assert!(!output.join("pgsql/pgAdmin 4").exists());
    }
    #[tokio::test]
    async fn downloads_never_replace_unverified_files() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("binary");
        std::fs::write(&path, b"untrusted").unwrap();
        assert!(download(&path, &postgres_artifact()).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"untrusted");
        let a = Artifact {
            url: "http://github.com/a".into(),
            sha256: "unused",
            max_bytes: 100,
        };
        assert!(download(&t.path().join("other"), &a).await.is_err());
        assert!(!t.path().join("other").exists());
    }
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "uses official PostgreSQL ZIP, creates and stops an isolated cluster"]
    async fn portable_postgres_installs_and_provisions_without_system_installation() {
        let t = tempfile::tempdir().unwrap();
        let deps = t.path().join("dependencies");
        std::fs::create_dir(&deps).unwrap();
        if let Some(archive) = std::env::var_os("POSTGRES_TEST_ARCHIVE") {
            std::fs::copy(
                archive,
                deps.join(format!("postgresql-{POSTGRES_VERSION}.zip")),
            )
            .unwrap();
        }
        let bin = portable_postgres(t.path()).await.unwrap();
        assert!(crate::postgres::tools(&bin));
        assert_eq!(portable_postgres(t.path()).await.unwrap(), bin);
        let state = crate::postgres::prepare(t.path(), bin).unwrap();
        let database = crate::postgres::ensure(&state, true).await.unwrap();
        let url = crate::postgres::configured_url(&state).unwrap();
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        let version: String = sqlx::query_scalar("SHOW server_version")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        database.shutdown().await.unwrap();
        assert!(version.starts_with(POSTGRES_VERSION));
        assert!(
            !deps
                .join(format!("postgresql-{POSTGRES_VERSION}.zip"))
                .exists()
        );
    }
}
