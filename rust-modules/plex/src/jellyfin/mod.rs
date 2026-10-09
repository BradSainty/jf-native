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
mod map;
mod query;
mod routes;

pub use api::{path_prefix, set_path_prefix};
pub use routes::handle;

use crate::plex::Origin;

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
