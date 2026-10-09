//! Signing in to a Jellyfin server without typing on a television remote.
//!
//! 1. [`discover`] finds the server: Jellyfin answers a UDP broadcast of `Who is JellyfinServer?`
//!    on port 7359 with its own address, so nobody has to enter a URL.
//! 2. [`QuickConnect`] puts a six-digit code on screen; the user approves it from any device that
//!    is already signed in (the Jellyfin app or web: Settings → Quick Connect), and
//!    [`QuickConnect::token`] redeems the approval for an access token.
//!
//! Quick Connect must be enabled on the server (Dashboard → General); [`quick_connect_enabled`]
//! says whether it is, so the screen can say what to do rather than wait forever.

use std::net::UdpSocket;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::api::{self, Conn};
use crate::plex::Origin;

/// One server that answered discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub id: String,
    /// Its base URL, path included (`http://192.168.1.10:8096`, `https://host/jellyfin`).
    pub url: String,
}

/// Broadcast for servers on this network and collect every answer that arrives within `wait`.
pub fn discover(wait: Duration) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { return out };
    if sock.set_broadcast(true).is_err() || sock.set_read_timeout(Some(Duration::from_millis(200))).is_err() {
        return out;
    }
    let deadline = Instant::now() + wait;
    let mut sent_at = None::<Instant>;
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        // Re-ask every second: a broadcast is one datagram and one can be lost.
        if sent_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            let _ = sock.send_to(b"Who is JellyfinServer?", "255.255.255.255:7359");
            sent_at = Some(Instant::now());
        }
        let Ok((n, _)) = sock.recv_from(&mut buf) else { continue };
        let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) else { continue };
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let found = Found { name: s("Name"), id: s("Id"), url: s("Address").trim_end_matches('/').to_string() };
        if !found.url.is_empty() && !out.iter().any(|f| f.id == found.id) {
            out.push(found);
        }
    }
    out
}

/// A server URL → its origin, with its path recorded as the origin's prefix.
pub fn origin_of(url: &str) -> Option<Origin> {
    let url = url.trim().trim_end_matches('/');
    let rest = url.split_once("://").map(|(_, r)| r)?;
    let path = rest.find('/').map(|i| &rest[i..]).unwrap_or("");
    let base = &url[..url.len() - path.len()];
    // `Origin::parse` fills a missing port with Plex's 32400; a URL without one means the scheme's.
    let authority = &rest[..rest.len() - path.len()];
    let host_end = authority.rfind(']').map_or(0, |i| i + 1); // past a bracketed v6 literal
    let has_port = authority[host_end..].contains(':');
    let base = if has_port {
        base.to_string()
    } else if url.starts_with("https://") {
        format!("{base}:443")
    } else {
        format!("{base}:80")
    };
    let origin = Origin::parse(&base)?;
    api::set_path_prefix(&origin, path);
    Some(origin)
}

/// The developer override for discovery (`PLXNATIVE_JF_SERVER`), for a simulator on a network
/// where the server cannot hear a broadcast.
pub fn server_override() -> Option<String> {
    std::env::var("PLXNATIVE_JF_SERVER").ok().filter(|s| !s.trim().is_empty())
}

/// What the sign-in screen shows beside the code: the address the user should approve it on.
fn hint() -> &'static Mutex<String> {
    static H: OnceLock<Mutex<String>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(String::new()))
}

pub fn set_login_hint(url: &str) {
    if let Ok(mut h) = hint().lock() {
        *h = url.to_string();
    }
}

/// The server address the sign-in screen names (empty before discovery has found one).
pub fn login_hint() -> String {
    hint().lock().map(|h| h.clone()).unwrap_or_default()
}

/// The page on the server where a signed-in user approves a code.
pub fn approve_url(server_url: &str) -> String {
    format!("{}/web/#/quickconnect", server_url.trim_end_matches('/'))
}

fn anon(origin: &Origin) -> Conn {
    Conn { origin: origin.clone(), token: String::new() }
}

/// The server's name and id, or `None` when nothing at `origin` answers as Jellyfin.
pub fn server_identity(origin: &Origin) -> Option<(String, String)> {
    let info = api::server_info(&anon(origin))?;
    let id = info.get("Id").and_then(Value::as_str)?.to_string();
    let name = info.get("ServerName").and_then(Value::as_str).unwrap_or("Jellyfin").to_string();
    Some((name, id))
}

pub fn quick_connect_enabled(origin: &Origin) -> Option<bool> {
    let (status, body) = anon(origin).send("GET", "/QuickConnect/Enabled", None, true)?;
    if !(200..300).contains(&status) {
        return Some(false);
    }
    Some(serde_json::from_slice::<Value>(&body).ok()?.as_bool().unwrap_or(false))
}

/// One Quick Connect request: the code on screen and the secret that redeems it.
pub struct QuickConnect {
    pub code: String,
    secret: String,
}

impl QuickConnect {
    /// Ask the server for a code. POST on 10.9+; GET before.
    pub fn initiate(origin: &Origin) -> Option<QuickConnect> {
        let conn = anon(origin);
        let body = match conn.send("POST", "/QuickConnect/Initiate", None, true) {
            Some((s, b)) if (200..300).contains(&s) => b,
            _ => conn.send("GET", "/QuickConnect/Initiate", None, true).filter(|(s, _)| (200..300).contains(s))?.1,
        };
        let v: Value = serde_json::from_slice(&body).ok()?;
        let code = v.get("Code").and_then(Value::as_str)?.to_string();
        let secret = v.get("Secret").and_then(Value::as_str)?.to_string();
        Some(QuickConnect { code, secret })
    }

    /// `Some(true)` once the code has been approved, `Some(false)` while it waits, `None` when the
    /// server no longer knows it (expired) or did not answer.
    pub fn approved(&self, origin: &Origin) -> Option<bool> {
        let v = anon(origin).get_json(&format!("/QuickConnect/Connect?secret={}", super::query::enc(&self.secret)))?;
        Some(v.get("Authenticated").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Redeem an approved code: the access token and the user it belongs to.
    pub fn token(&self, origin: &Origin) -> Option<(String, String)> {
        let (status, body) = anon(origin).send(
            "POST",
            "/Users/AuthenticateWithQuickConnect",
            Some(&json!({ "Secret": self.secret })),
            true,
        )?;
        if !(200..300).contains(&status) {
            return None;
        }
        let v: Value = serde_json::from_slice(&body).ok()?;
        let token = v.get("AccessToken").and_then(Value::as_str)?.to_string();
        let user = v.pointer("/User/Name").and_then(Value::as_str).unwrap_or("").to_string();
        Some((token, user))
    }
}
