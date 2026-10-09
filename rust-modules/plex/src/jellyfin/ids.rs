//! Integers for the places the app insists on one where Jellyfin has a GUID.
//!
//! Library section keys, person ids and stream ids are `i64` all the way up (section pins are even
//! persisted by key), so each GUID gets a STABLE integer — a hash, the same on every boot — and a
//! reverse table filled whenever one is handed out.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::api::fnv64;

fn table() -> &'static Mutex<HashMap<i64, String>> {
    static T: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The stable integer for `s`: positive, below 2^52 (exact in a JSON double), never 0 (0 means
/// "absent" throughout the Plex DTOs).
pub fn int_of(s: &str) -> i64 {
    let n = (fnv64(s) & ((1u64 << 52) - 1)) as i64;
    let n = if n == 0 { 1 } else { n };
    if let Ok(mut t) = table().lock() {
        t.entry(n).or_insert_with(|| s.to_string());
    }
    n
}

/// The string [`int_of`] was given for `n`, if this process has handed `n` out.
pub fn string_of(n: i64) -> Option<String> {
    table().lock().ok().and_then(|t| t.get(&n).cloned())
}

/// A sidecar (external) subtitle: where Jellyfin serves it.
#[derive(Clone)]
pub struct SubRef {
    pub item: String,
    pub media_source: String,
    pub index: i64,
    pub codec: String,
}

fn subs() -> &'static Mutex<HashMap<i64, SubRef>> {
    static S: OnceLock<Mutex<HashMap<i64, SubRef>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The `/library/streams/{n}` number for a sidecar subtitle.
pub fn sub_key(r: SubRef) -> i64 {
    let n = int_of(&format!("sub:{}:{}:{}", r.item, r.media_source, r.index));
    if let Ok(mut t) = subs().lock() {
        t.insert(n, r);
    }
    n
}

pub fn sub_of(n: i64) -> Option<SubRef> {
    subs().lock().ok().and_then(|t| t.get(&n).cloned())
}

/// What kind of item an id is, and (for a season) its series — remembered as items are mapped,
/// so `/children` of a season can ask Jellyfin's episode endpoint without fetching the season.
#[derive(Clone)]
pub struct Known {
    pub kind: String,
    pub series_id: String,
}

fn known() -> &'static Mutex<HashMap<String, Known>> {
    static K: OnceLock<Mutex<HashMap<String, Known>>> = OnceLock::new();
    K.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn remember(id: &str, kind: &str, series_id: &str) {
    if id.is_empty() {
        return;
    }
    if let Ok(mut k) = known().lock() {
        k.insert(id.to_string(), Known { kind: kind.to_string(), series_id: series_id.to_string() });
    }
}

pub fn recall(id: &str) -> Option<Known> {
    known().lock().ok().and_then(|k| k.get(id).cloned())
}
