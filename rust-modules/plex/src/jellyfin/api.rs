//! Outbound Jellyfin requests, and what the backend remembers about each server.
//!
//! Every call goes through libcurl (`plx_net::net::request`) rather than the plaintext socket the
//! PMS path uses for http origins: Jellyfin's write endpoints (`/Sessions/Playing*`, user data)
//! take a JSON BODY, which the raw socket cannot send. libcurl handles http and https alike.
//!
//! No call follows a redirect. Each carries the access token in its `Authorization` header, and
//! libcurl would re-send that header to whatever host a `Location:` names, so `net::request`
//! refuses the pairing outright. A server that answers 3xx is logged as such: the address the app
//! was given (discovery, Quick Connect, `jellyfin-server.txt`) has to be the one that answers.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::plex::Origin;

pub const CLIENT: &str = "JFNative";
pub const VERSION: &str = "0.1.0";
const DEVICE: &str = "LG webOS TV";

/// A Jellyfin server behind a reverse proxy can live under a path (`https://host/jellyfin`). An
/// [`Origin`] is scheme + host + port only, so the path is kept here, keyed by the origin's base.
///
/// The session file persists only the origin, so the table is kept on disk beside it
/// ([`prefix_file`]) and read back on first use — a server signed in under a path keeps working
/// after a restart.
fn prefixes() -> &'static Mutex<HashMap<String, String>> {
    static P: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    P.get_or_init(|| {
        let saved = std::fs::read(prefix_file())
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, String>>(&b).ok())
            .unwrap_or_default();
        Mutex::new(saved)
    })
}

fn prefix_file() -> std::path::PathBuf {
    plx_base::paths::persistent_state_root().join("jellyfin-paths.json")
}

/// Record the path a server lives under (`"/jellyfin"`; empty for the root).
pub fn set_path_prefix(origin: &Origin, prefix: &str) {
    let p = prefix.trim().trim_end_matches('/');
    let p = if p.is_empty() || p.starts_with('/') { p.to_string() } else { format!("/{p}") };
    if let Ok(mut m) = prefixes().lock() {
        if m.get(&origin.base()) == Some(&p) {
            return;
        }
        m.insert(origin.base(), p);
        #[cfg(not(any(test, feature = "test-support")))]
        if plx_base::paths::ensure_persistent_state_root().is_ok() {
            if let Ok(bytes) = serde_json::to_vec(&*m) {
                let _ = std::fs::write(prefix_file(), bytes);
            }
        }
    }
}

/// The path `origin` lives under, `""` for the root. `PLXNATIVE_JF_PATH_PREFIX` covers the
/// developer boot, which names its server through a trigger file that only carries an origin.
pub fn path_prefix(origin: &Origin) -> String {
    if let Some(p) = prefixes().lock().ok().and_then(|m| m.get(&origin.base()).cloned()) {
        return p;
    }
    std::env::var("PLXNATIVE_JF_PATH_PREFIX")
        .map(|p| p.trim().trim_end_matches('/').to_string())
        .unwrap_or_default()
}

/// FNV-1a, for ids that only need to be stable, not secret.
pub fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn device_slot() -> &'static Mutex<String> {
    static D: OnceLock<Mutex<String>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(String::new()))
}

/// Adopt this install's identity (the session's client id, a per-install uuid) as the Jellyfin
/// `DeviceId`. Jellyfin ties every access token to the device it was issued to and REVOKES a
/// device's older tokens when it signs in again — so the id must be stable for one television and
/// different between two, or each would sign the other out.
pub fn set_device_id(client_id: &str) {
    if client_id.is_empty() {
        return;
    }
    if let Ok(mut d) = device_slot().lock() {
        if d.is_empty() {
            *d = format!("jfnative-{client_id}");
        }
    }
}

/// The `DeviceId` this install reports (see [`set_device_id`]).
pub fn device_id() -> String {
    device_slot()
        .lock()
        .ok()
        .map(|d| d.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "jfnative-unidentified".to_string())
}

/// The `Authorization: MediaBrowser …` header Jellyfin authenticates by.
pub fn auth_header(token: &str) -> String {
    let mut h = format!(
        "Authorization: MediaBrowser Client=\"{CLIENT}\", Device=\"{DEVICE}\", DeviceId=\"{}\", Version=\"{VERSION}\"",
        device_id()
    );
    if !token.is_empty() {
        h.push_str(&format!(", Token=\"{token}\""));
    }
    h
}

fn log(s: &str) {
    plx_base::eventlog::log(s);
}

/// The endpoint of a path, for logs: never the query, which can carry a token.
pub fn endpoint(path_q: &str) -> &str {
    path_q.split('?').next().unwrap_or(path_q)
}

/// One Jellyfin server as one user.
pub struct Conn {
    pub origin: Origin,
    pub token: String,
}

impl Conn {
    pub fn prefix(&self) -> String {
        path_prefix(&self.origin)
    }

    fn url(&self, path_q: &str) -> String {
        format!("{}{}{}", self.origin.base(), self.prefix(), path_q)
    }

    /// One request; `None` only when nothing answered.
    pub fn send(&self, verb: &str, path_q: &str, body: Option<&Value>, json: bool) -> Option<(u16, Vec<u8>)> {
        let mut headers = vec![auth_header(&self.token)];
        if json {
            headers.push("Accept: application/json".to_string());
        }
        let payload: Option<Vec<u8>> = match body {
            Some(v) => {
                headers.push("Content-Type: application/json".to_string());
                Some(serde_json::to_vec(v).unwrap_or_default())
            }
            None if verb == "POST" => Some(Vec::new()),
            None => None,
        };
        let t0 = std::time::Instant::now();
        let r = plx_net::net::request(
            &self.url(path_q),
            &headers,
            verb,
            payload.as_deref(),
            plx_net::net::BULK,
            false,
            None,
            None,
        );
        match &r {
            Some(r) if (300..400).contains(&r.status) => log(&format!(
                "jellyfin: {verb} {} -> {} — the server redirected; give the app the address it redirects to",
                endpoint(path_q),
                r.status
            )),
            Some(r) => log(&format!(
                "jellyfin: {verb} {} -> {} ({} bytes, {} ms)",
                endpoint(path_q),
                r.status,
                r.body.len(),
                t0.elapsed().as_millis()
            )),
            None => log(&format!("jellyfin: {verb} {} — no response", endpoint(path_q))),
        }
        r.map(|r| (r.status, r.body))
    }

    /// GET → JSON, or `None` for a failure of any kind (logged).
    pub fn get_json(&self, path_q: &str) -> Option<Value> {
        let (status, body) = self.send("GET", path_q, None, true)?;
        if !(200..300).contains(&status) {
            log(&format!("jellyfin: GET {} -> {status}", endpoint(path_q)));
            return None;
        }
        match serde_json::from_slice(&body) {
            Ok(v) => Some(v),
            Err(e) => {
                log(&format!("jellyfin: GET {} answered {} bytes that will not parse — {e}", endpoint(path_q), body.len()));
                None
            }
        }
    }

    pub fn post(&self, path_q: &str, body: Option<&Value>) -> Option<u16> {
        let (status, _) = self.send("POST", path_q, body, true)?;
        if !(200..300).contains(&status) {
            log(&format!("jellyfin: POST {} -> {status}", endpoint(path_q)));
        }
        Some(status)
    }

    pub fn delete(&self, path_q: &str) -> Option<u16> {
        let (status, _) = self.send("DELETE", path_q, None, true)?;
        Some(status)
    }

    pub fn get_bytes(&self, path_q: &str) -> Option<(u16, Vec<u8>)> {
        self.send("GET", path_q, None, false)
    }
}

/// What the backend knows about one (server, user) pair.
#[derive(Clone, Default)]
pub struct Ctx {
    pub user_id: String,
    pub server_id: String,
    pub server_name: String,
    pub version: String,
}

/// One library ("view") the app can browse, with the integer key the Plex layers address it by.
#[derive(Clone)]
pub struct View {
    pub id: String,
    pub key: i64,
    pub name: String,
    /// `movie` | `show` | `collection` — the Plex section type. `collection` is Jellyfin's
    /// Collections folder (the `boxsets` view): its listing is the user's collections.
    pub kind: &'static str,
}

impl View {
    /// A library of films or shows, as opposed to the Collections folder: the views a Home
    /// "Recently Added" row or a resume query reads, which ask for films and series by type.
    pub fn holds_media(&self) -> bool {
        self.kind != "collection"
    }
}

fn ctxs() -> &'static Mutex<HashMap<String, Ctx>> {
    static C: OnceLock<Mutex<HashMap<String, Ctx>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn views_cache() -> &'static Mutex<HashMap<String, Vec<View>>> {
    static V: OnceLock<Mutex<HashMap<String, Vec<View>>>> = OnceLock::new();
    V.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(conn: &Conn) -> String {
    format!("{}{}#{:016x}", conn.origin.base(), conn.prefix(), fnv64(&conn.token))
}

/// The public server facts (no token needed).
pub fn server_info(conn: &Conn) -> Option<Value> {
    conn.get_json("/System/Info/Public")
}

/// The context for `conn`, fetched once per (server, token).
pub fn ctx(conn: &Conn) -> Option<Ctx> {
    let key = cache_key(conn);
    if let Some(c) = ctxs().lock().ok().and_then(|m| m.get(&key).cloned()) {
        return Some(c);
    }
    let me = conn.get_json("/Users/Me")?;
    let user_id = me.get("Id").and_then(Value::as_str)?.to_string();
    let info = server_info(conn).unwrap_or(Value::Null);
    let s = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let c = Ctx {
        user_id,
        server_id: s("Id"),
        server_name: s("ServerName"),
        version: s("Version"),
    };
    if let Ok(mut m) = ctxs().lock() {
        m.insert(key, c.clone());
    }
    Some(c)
}

/// The user's movie and TV libraries, and the Collections folder when there is one. Cached; `refresh` re-reads them (a lookup by key that
/// misses asks for that, so a library added mid-session is found).
pub fn views(conn: &Conn, ctx: &Ctx, refresh: bool) -> Vec<View> {
    let key = cache_key(conn);
    if !refresh {
        if let Some(v) = views_cache().lock().ok().and_then(|m| m.get(&key).cloned()) {
            return v;
        }
    }
    let u = &ctx.user_id;
    let body = conn
        .get_json(&format!("/UserViews?userId={u}"))
        .or_else(|| conn.get_json(&format!("/Users/{u}/Views")));
    let Some(body) = body else { return Vec::new() };
    let out: Vec<View> = body
        .get("Items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    let kind = match it.get("CollectionType").and_then(Value::as_str) {
                        Some("movies") => "movie",
                        Some("tvshows") => "show",
                        Some("boxsets") => "collection",
                        _ => return None,
                    };
                    let id = it.get("Id").and_then(Value::as_str)?.to_string();
                    Some(View {
                        key: super::ids::int_of(&id),
                        name: it.get("Name").and_then(Value::as_str).unwrap_or("").to_string(),
                        id,
                        kind,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if let Ok(mut m) = views_cache().lock() {
        m.insert(key, out.clone());
    }
    out
}

/// The view a section key names, re-reading the views once if it is not known yet.
pub fn view_by_key(conn: &Conn, ctx: &Ctx, key: i64) -> Option<View> {
    views(conn, ctx, false)
        .into_iter()
        .find(|v| v.key == key)
        .or_else(|| views(conn, ctx, true).into_iter().find(|v| v.key == key))
}
