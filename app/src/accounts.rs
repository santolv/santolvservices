use crate::{json_error, query_param, safe_path, server, vault_root};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Request, Response};
use url::Url;

const TTL: Duration = Duration::from_secs(8 * 3600);
const PRESENT: Duration = Duration::from_secs(16);
type Token = [u8; 32];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Data {
    vault: PathBuf,
    owner: String,
    users: BTreeMap<String, String>,
    #[serde(default)]
    grants: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone)]
pub struct Actor {
    pub name: String,
    pub owner: bool,
    key: Token,
}

pub struct Accounts {
    file: PathBuf,
    data: Mutex<Data>,
    sessions: Mutex<HashMap<Token, (String, Instant)>>,
    attempts: Mutex<VecDeque<Instant>>,
    active: Mutex<HashMap<(Token, String), (String, Instant)>>,
    origin: Url,
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn key(value: &str) -> Token {
    Sha256::digest(value.as_bytes()).into()
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| "No hay una fuente de azar disponible")?;
    Ok(bytes)
}

fn valid_user(value: &str) -> bool {
    (1..=40).contains(&value.len())
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-' | b'.'))
}

fn private_file() -> Result<PathBuf, String> {
    let path = PathBuf::from(
        server::env("SUPER_CEREBRO_USERS_FILE").ok_or("Configura SUPER_CEREBRO_USERS_FILE")?,
    );
    if !path.is_absolute() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return Err("Usa una ruta absoluta a un archivo .json privado".into());
    }
    let parent = fs::canonicalize(path.parent().ok_or("Ruta de cuentas inválida")?)
        .map_err(|_| "Crea primero el directorio privado para las cuentas")?;
    let path = parent.join(path.file_name().ok_or("Ruta de cuentas inválida")?);
    let vault = fs::canonicalize(vault_root()).map_err(|_| "No se pudo resolver la bóveda")?;
    let default = std::env::current_dir()
        .map_err(|_| "No se pudo resolver la ruta actual")?
        .join("vault");
    let inside = |path: &Path, folder: &Path| {
        path.starts_with(folder)
            || (cfg!(windows)
                && path
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with(&format!("{}\\", folder.to_string_lossy().to_lowercase())))
    };
    if inside(&path, &vault)
        || fs::canonicalize(default).is_ok_and(|default| inside(&path, &default))
        || crate::linked_path(&path)
    {
        return Err("Guarda las cuentas fuera de la bóveda y usa un archivo sin enlaces".into());
    }
    Ok(path)
}

fn persist(path: &Path, data: &Data, replace: bool) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(data).map_err(|_| "No se pudieron serializar las cuentas")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("El registro de cuentas ha alcanzado su límite".into());
    }
    server::write_atomic(path, &bytes, replace)
        .map_err(|_| "No se pudieron guardar las cuentas".into())
}

fn read_data(path: &Path) -> Result<Data, String> {
    let file = fs::File::open(path)
        .map_err(|_| "Crea la primera cuenta con cargo run -- user-add NOMBRE")?;
    let bytes =
        server::read_limited(file, 2 * 1024 * 1024).map_err(|_| "Archivo de cuentas inválido")?;
    let data: Data = serde_json::from_slice(&bytes).map_err(|_| "Archivo de cuentas inválido")?;
    if fs::canonicalize(vault_root()).ok().as_ref() != Some(&data.vault) {
        return Err("El archivo de cuentas pertenece a otra bóveda".into());
    }
    if !data.users.contains_key(&data.owner)
        || data.users.iter().any(|(name, hash)| {
            !valid_user(name) || !hash.starts_with("$argon2id$") || PasswordHash::new(hash).is_err()
        })
    {
        return Err("Archivo de cuentas inválido".into());
    }
    Ok(data)
}

pub fn add_user(name: &str, password: &str) -> Result<(), String> {
    let path = private_file()?;
    let exists = path.exists();
    let mut data = if exists {
        read_data(&path)?
    } else {
        Data {
            vault: fs::canonicalize(vault_root()).map_err(|_| "No se pudo resolver la bóveda")?,
            owner: name.into(),
            users: BTreeMap::new(),
            grants: BTreeMap::new(),
        }
    };
    insert_user(&mut data, name, password)?;
    persist(&path, &data, exists)
}

fn insert_user(data: &mut Data, name: &str, password: &str) -> Result<(), String> {
    if !valid_user(name) || !(12..=256).contains(&password.len()) {
        return Err(
            "Usa un identificador de 1 a 40 caracteres y una contraseña de 12 a 256 bytes".into(),
        );
    }
    if data.users.contains_key(name) {
        return Err("La cuenta ya existe".into());
    }
    let salt = SaltString::encode_b64(&random::<16>()?).map_err(|_| "No se pudo crear la sal")?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| "No se pudo proteger la contraseña")?
        .to_string();
    data.users.insert(name.into(), hash);
    Ok(())
}

fn doc_path(path: &str) -> Option<String> {
    let file = safe_path(path)?;
    let root = fs::canonicalize(vault_root()).ok()?;
    let full = fs::canonicalize(file).ok()?;
    let path = full.strip_prefix(&root).ok()?.to_str()?.replace('\\', "/");
    let extension = Path::new(&path).extension()?.to_str()?.to_ascii_lowercase();
    (matches!(extension.as_str(), "docx" | "pptx" | "xlsx") && craft_file(&full)).then_some(path)
}

fn craft_file(path: &Path) -> bool {
    crate::craft::document_path(path).is_file()
}

impl Accounts {
    pub fn create_user(&self, mut req: Request, actor: &Actor) {
        if !actor.owner {
            return json_error(req, 403, "Solo la cuenta propietaria puede crear cuentas");
        }
        #[derive(Deserialize)]
        struct NewUser {
            user: String,
            password: String,
        }
        let Ok(new) = server::read_json::<NewUser>(&mut req, 4096) else {
            return json_error(req, 400, "Cuenta inválida");
        };
        let mut data = self.data.lock().unwrap();
        if let Err(error) = insert_user(&mut data, &new.user, &new.password) {
            return json_error(req, 400, &error);
        }
        if let Err(error) = persist(&self.file, &data, true) {
            data.users.remove(&new.user);
            return json_error(req, 500, &error);
        }
        server::json(req, json!({"ok":true}));
    }

    pub fn open(port: u16) -> Result<Self, String> {
        let file = private_file()?;
        let data = read_data(&file)?;
        let origin = server::env("SUPER_CEREBRO_ORIGIN")
            .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
        let url = server::http_url(&origin)
            .filter(|url| {
                url.path() == "/"
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && (url.scheme() == "https"
                        || matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            })
            .ok_or("Configura el origen HTTPS exacto del servicio")?;
        Ok(Self {
            file,
            data: Mutex::new(data),
            sessions: Mutex::default(),
            attempts: Mutex::default(),
            active: Mutex::default(),
            origin: url,
        })
    }

    pub fn host(&self, req: &Request) -> bool {
        let mut hosts = req
            .headers()
            .iter()
            .filter(|value| value.field.equiv("Host"));
        let Some(host) = hosts.next().map(|header| header.value.as_str()) else {
            return false;
        };
        hosts.next().is_none()
            && !host.contains(['/', '\\', '@', '?', '#', ',', '%'])
            && !host.chars().any(char::is_whitespace)
            && server::http_url(&format!("{}://{host}", self.origin.scheme()))
                .is_some_and(|url| url.origin() == self.origin.origin())
    }

    pub fn origin(&self, req: &Request) -> bool {
        let mut origins = req
            .headers()
            .iter()
            .filter(|value| value.field.equiv("Origin"));
        origins.next().is_some_and(|header| {
            header.value.as_str() == self.origin.origin().ascii_serialization()
        }) && origins.next().is_none()
            && server::header(req, "Sec-Fetch-Site") != Some("cross-site")
    }

    fn cookie_name(&self) -> &'static str {
        if self.origin.scheme() == "https" {
            "__Host-sc_user"
        } else {
            "sc_user"
        }
    }

    fn cookie(&self, value: &str, age: u64) -> Header {
        header(
            "Set-Cookie",
            &format!(
                "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{}",
                self.cookie_name(),
                if self.origin.scheme() == "https" {
                    "; Secure"
                } else {
                    ""
                }
            ),
        )
    }

    fn session_key(&self, req: &Request) -> Option<Token> {
        let mut values = req
            .headers()
            .iter()
            .filter(|value| value.field.equiv("Cookie"))
            .flat_map(|header| header.value.as_str().split(';'))
            .filter_map(|cookie| cookie.trim().split_once('='))
            .filter(|(name, _)| *name == self.cookie_name())
            .map(|(_, value)| value);
        let value = values.next()?;
        (values.next().is_none() && value.len() == 43).then(|| key(value))
    }

    pub fn actor(&self, req: &Request) -> Option<Actor> {
        let key = self.session_key(req)?;
        let name = {
            let sessions = self.sessions.lock().unwrap();
            let (name, expires) = sessions.get(&key)?;
            if *expires <= Instant::now() {
                return None;
            }
            name.clone()
        };
        let data = self.data.lock().unwrap();
        data.users.contains_key(&name).then(|| Actor {
            owner: name == data.owner,
            name,
            key,
        })
    }

    pub fn login(&self, mut req: Request) {
        #[derive(Deserialize)]
        struct Login {
            user: String,
            password: String,
        }
        let Ok(login) = server::read_json::<Login>(&mut req, 4096) else {
            return json_error(req, 400, "Introduce una cuenta y contraseña válidas");
        };
        if !valid_user(&login.user) || login.password.len() > 256 {
            return json_error(req, 401, "Credenciales incorrectas");
        }
        let now = Instant::now();
        {
            let mut attempts = self.attempts.lock().unwrap();
            while attempts
                .front()
                .is_some_and(|start| now.duration_since(*start) >= Duration::from_secs(60))
            {
                attempts.pop_front();
            }
            if attempts.len() >= 24 {
                return json_error(req, 429, "Espera un minuto antes de intentarlo otra vez");
            }
            attempts.push_back(now);
        }
        let (hash, known) = {
            let data = self.data.lock().unwrap();
            match data.users.get(&login.user) {
                Some(hash) => (hash.clone(), true),
                None => (data.users[&data.owner].clone(), false),
            }
        };
        let checked = PasswordHash::new(&hash).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(login.password.as_bytes(), &hash)
                .is_ok()
        });
        if !(checked && known) {
            return json_error(req, 401, "Credenciales incorrectas");
        }
        let Ok(secret) = random::<32>() else {
            return json_error(req, 500, "No se pudo abrir la sesión");
        };
        let secret = URL_SAFE_NO_PAD.encode(secret);
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, (_, expires)| *expires > now);
        if let Some(old) = self.session_key(&req) {
            sessions.remove(&old);
        }
        if sessions.len() >= 256 {
            return json_error(req, 429, "Demasiadas sesiones activas");
        }
        sessions.insert(key(&secret), (login.user, now + TTL));
        let _ = req.respond(
            Response::empty(204)
                .with_header(self.cookie(&secret, TTL.as_secs()))
                .with_header(header("Cache-Control", "no-store")),
        );
    }

    pub fn logout(&self, req: Request) {
        if let Some(key) = self.session_key(&req) {
            self.sessions.lock().unwrap().remove(&key);
            self.active
                .lock()
                .unwrap()
                .retain(|(token, _), _| *token != key);
        }
        let _ = req.respond(
            Response::empty(204)
                .with_header(self.cookie("", 0))
                .with_header(header("Cache-Control", "no-store")),
        );
    }

    pub fn allows(&self, actor: &Actor, path: &str) -> bool {
        doc_path(path).is_some_and(|path| {
            actor.owner
                || self
                    .data
                    .lock()
                    .unwrap()
                    .grants
                    .get(&path)
                    .is_some_and(|users| users.contains(&actor.name))
        })
    }

    pub fn files(&self, actor: &Actor) -> Vec<serde_json::Value> {
        let data = self.data.lock().unwrap();
        data.grants
            .iter()
            .filter(|(path, users)| users.contains(&actor.name) && doc_path(path).is_some())
            .map(|(path, _)| json!({"path":path,"name":path.rsplit('/').next(),"is_dir":false}))
            .collect()
    }

    pub fn protects(&self, path: &str) -> bool {
        let Some(full) = safe_path(path).and_then(|path| fs::canonicalize(path).ok()) else {
            return false;
        };
        let Ok(root) = fs::canonicalize(vault_root()) else {
            return false;
        };
        let Ok(relative) = full.strip_prefix(root) else {
            return false;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        self.data.lock().unwrap().grants.keys().any(|grant| {
            if cfg!(windows) {
                grant.eq_ignore_ascii_case(&relative)
                    || grant
                        .to_lowercase()
                        .starts_with(&format!("{}/", relative.to_lowercase()))
            } else {
                grant == &relative || grant.starts_with(&format!("{relative}/"))
            }
        })
    }

    pub fn share(&self, mut req: Request, actor: &Actor, query: &str) {
        if !actor.owner {
            return json_error(req, 403, "Solo la cuenta propietaria puede compartir");
        }
        let Some(path) = doc_path(&query_param(query, "path")) else {
            return json_error(req, 404, "Documento no disponible");
        };
        let mut data = self.data.lock().unwrap();
        if req.method() == &Method::Get {
            return server::json(
                req,
                json!({"users":data.users.keys().filter(|user|*user!=&data.owner).collect::<Vec<_>>(),
                "editors":data.grants.get(&path)}),
            );
        }
        #[derive(Deserialize)]
        struct Grant {
            user: String,
            allowed: bool,
        }
        let Ok(change) = server::read_json::<Grant>(&mut req, 4096) else {
            return json_error(req, 400, "Permiso inválido");
        };
        if !data.users.contains_key(&change.user) || change.user == data.owner {
            return json_error(req, 400, "Cuenta no disponible");
        }
        let old = data.grants.get(&path).cloned();
        let users = data.grants.entry(path.clone()).or_default();
        if change.allowed {
            users.insert(change.user.clone());
        } else {
            users.remove(&change.user);
        }
        if users.is_empty() {
            data.grants.remove(&path);
        }
        if persist(&self.file, &data, true).is_err() {
            if let Some(old) = old {
                data.grants.insert(path, old);
            } else {
                data.grants.remove(&path);
            }
            return json_error(req, 500, "No se pudo guardar el permiso");
        }
        if !change.allowed {
            self.active
                .lock()
                .unwrap()
                .retain(|(_, doc), (user, _)| doc != &path || user != &change.user);
            if let Some(full) = safe_path(&path) {
                crate::craft::notify_document(&full);
            }
        }
        server::json(req, json!({"ok":true}));
    }

    pub fn participants(&self, req: Request, actor: &Actor, query: &str) {
        let Some(path) = doc_path(&query_param(query, "path")) else {
            return json_error(req, 404, "Documento no disponible");
        };
        let now = Instant::now();
        let mut active = self.active.lock().unwrap();
        active.retain(|_, (_, deadline)| *deadline > now);
        if query_param(query, "leave") == "true" {
            active.remove(&(actor.key, path.clone()));
        } else {
            if active.len() >= 2048 && !active.contains_key(&(actor.key, path.clone())) {
                return json_error(req, 429, "Demasiados documentos abiertos");
            }
            active.insert(
                (actor.key, path.clone()),
                (actor.name.clone(), now + PRESENT),
            );
        }
        let users = active
            .iter()
            .filter(|((_, doc), _)| doc == &path)
            .map(|(_, (name, _))| name)
            .collect::<BTreeSet<_>>();
        server::json(req, json!({"users":users}));
    }
}

pub fn redirect(req: Request, location: &str) {
    let _ = req.respond(
        Response::empty(303)
            .with_header(header("Location", location))
            .with_header(header("Cache-Control", "no-store")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_creation_stores_only_a_salted_password_hash() {
        let mut data = Data {
            vault: PathBuf::from("test"),
            owner: "owner".into(),
            users: BTreeMap::new(),
            grants: BTreeMap::new(),
        };
        let password = "fixture-password-01";
        insert_user(&mut data, "owner", password).unwrap();
        insert_user(&mut data, "colleague", password).unwrap();
        assert_ne!(data.users["owner"], data.users["colleague"]);
        assert!(!serde_json::to_string(&data).unwrap().contains(password));
        let hash = PasswordHash::new(&data.users["colleague"]).unwrap();
        assert!(Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok());
        assert!(insert_user(&mut data, "colleague", "other-fixture-password").is_err());
    }
}
