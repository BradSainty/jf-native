//! The Jellyfin backend.
//!
//! This fork talks to a Jellyfin server, but every layer above [`crate::http`] still speaks Plex:
//! the screens, the data stores and the player ask for `/hubs`, `/library/metadata/{rk}`,
//! `/:/timeline` and parse `{ "MediaContainer": … }`. Rewriting those layers would touch most of
//! the application, so instead `http::request_with` (the one door every PMS request goes through)
//! hands each request for a Jellyfin origin to [`handle`], which answers it from the Jellyfin API
//! and returns a body in the shape the Plex DTOs (`plex::models`) already parse.
//!
//! The pieces:
//! - [`api`]: the outbound Jellyfin transport, the per-server context (user id, server name) and
//!   the library-view table.
//! - [`ids`]: the few places the app needs an INTEGER where Jellyfin has a GUID (library section
//!   keys, person ids, sidecar subtitle streams), as a stable hash plus a reverse table.
//! - [`map`]: `BaseItemDto` → Plex `Metadata` JSON.
//! - [`query`]: query-string parsing.
//! - [`routes`]: the Plex endpoint table.
//!
//! Item identity is the Jellyfin item id itself: `ratingKey` is a string everywhere in the app, so
//! the GUID rides through unchanged and needs no table.
#![allow(dead_code)]

mod api;
mod ids;
pub mod login;
mod map;
mod query;
mod routes;
mod transcode;

pub use api::{device_id, path_prefix, set_device_id, set_path_prefix};
pub use routes::handle;
pub use transcode::path as transcode_path;

use crate::plex::Origin;

fn runtimes() -> &'static std::sync::Mutex<std::collections::HashMap<String, i64>> {
    static R: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, i64>>> =
        std::sync::OnceLock::new();
    R.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn remember_runtime(id: &str, ms: i64) {
    if let Ok(mut r) = runtimes().lock() {
        r.insert(id.to_string(), ms);
    }
}

/// The runtime, in ms, of the item a Jellyfin stream path names (`…/Videos/{id}/…`), as the item's
/// metadata gave it. The player's fallback for a stream that declares no duration — Jellyfin's
/// live remux/transcode does not — without which there is no seek bar and no progress report.
pub fn runtime_ms_for_stream(path: &str) -> Option<i64> {
    let rest = &path[path.find("/Videos/")? + "/Videos/".len()..];
    let id = rest.split(['/', '?']).next()?;
    runtimes().lock().ok()?.get(id).copied().filter(|ms| *ms > 0)
}

/// The last position this app reported for an item, kept until the server agrees with it.
///
/// Leaving the player refreshes the item page at once, but the `/Sessions/Playing/Stopped` that
/// carries the final position is still in flight (it takes seconds on a busy server), so the
/// refresh used to read the position from the play BEFORE and Resume restarted there. The app is
/// the one place that knows the newest position, so a report it made wins over what the server
/// answers until the server answers the same, or until the entry is too old to trust over a play
/// on another device.
struct Reported {
    ms: i64,
    at: std::time::Instant,
}

const REPORTED_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn reported() -> &'static std::sync::Mutex<std::collections::HashMap<String, Reported>> {
    static R: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Reported>>> =
        std::sync::OnceLock::new();
    R.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn note_position(id: &str, ms: i64) {
    if let Ok(mut r) = reported().lock() {
        r.insert(id.to_string(), Reported { ms, at: std::time::Instant::now() });
    }
}

fn forget_position(id: &str) {
    if let Ok(mut r) = reported().lock() {
        r.remove(id);
    }
}

/// The position to show for `id`, given the one the server answered: the app's own newer report
/// while the server has not caught up with it, otherwise the server's.
fn position_ms(id: &str, server_ms: i64) -> i64 {
    let Ok(mut r) = reported().lock() else { return server_ms };
    let Some(mine) = r.get(id) else { return server_ms };
    if mine.at.elapsed() > REPORTED_TTL || (mine.ms - server_ms).abs() < 2_000 {
        r.remove(id);
        return server_ms;
    }
    mine.ms
}

/// Is `origin` a Jellyfin server? Everything except the plex.tv family is: this fork has no PMS.
/// plex.tv itself is left alone so the remaining account code fails the way it always did when
/// plex.tv is unreachable, rather than being answered by a server that has no idea what it means.
///
/// Test builds answer `false` unless a test arms [`intercept_in_tests`]: the inherited suites grade
/// the PMS transport against loopback fixtures that speak Plex, and must keep reaching them.
pub fn intercepts(origin: &Origin) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if !TEST_INTERCEPT.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let host = origin.host().to_ascii_lowercase();
    !(host == "plex.tv" || host.ends_with(".plex.tv"))
}

#[cfg(any(test, feature = "test-support"))]
static TEST_INTERCEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Route requests through the Jellyfin backend in a test build, as a shipped build always does.
#[cfg(any(test, feature = "test-support"))]
pub fn intercept_in_tests(on: bool) {
    TEST_INTERCEPT.store(on, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reported_position_wins_until_the_server_catches_up() {
        let id = "position-test-item";
        assert_eq!(position_ms(id, 1_000), 1_000, "nothing reported: the server's");
        note_position(id, 4_135_000);
        assert_eq!(position_ms(id, 2_568_000), 4_135_000, "a stale server answer loses");
        assert_eq!(position_ms(id, 4_134_500), 4_134_500, "the server caught up");
        assert_eq!(position_ms(id, 2_568_000), 2_568_000, "and the report is spent");
        note_position(id, 4_135_000);
        forget_position(id);
        assert_eq!(position_ms(id, 0), 0, "marking played forgets the report");
    }

    /// A Jellyfin collection (BoxSet) is a Plex `collection` whose members are its children, and
    /// it names release order: the order `/library/collections/{rk}/children` answers in.
    #[test]
    fn a_boxset_is_a_collection_in_release_order() {
        let it = serde_json::json!({ "Type": "BoxSet", "Id": "boxset-test", "Name": "Pokémon", "ChildCount": 24 });
        let env = map::Env { prefix: String::new(), section: None };
        let o = map::item(&it, &env).expect("a BoxSet maps");
        assert_eq!(o["type"], "collection");
        assert_eq!(o["title"], "Pokémon");
        assert_eq!(o["childCount"], 24);
        assert_eq!(o["collectionSort"], 0, "release order");
        assert_eq!(o["key"], "/library/metadata/boxset-test/children");
        let film = serde_json::json!({ "Type": "Movie", "Id": "film-test", "Name": "Mewtwo Strikes Back" });
        assert!(map::item(&film, &env).unwrap().get("collectionSort").is_none(), "only a collection names an order");
    }
}
