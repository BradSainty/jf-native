//! The account preferences Settings → Audio & subtitles edits, kept in the Jellyfin user's own
//! configuration (the settings Jellyfin's web app calls Playback → Audio and Subtitles).
//!
//! `plex::account` reads and writes them as plex.tv calls, which this fork never makes (net refuses
//! plex.tv outright). Those three calls are answered here instead, in the Plex shape their DTOs
//! already parse: `GET /api/v2/user` (who the preferences belong to, with them inline, as playback's
//! audio rung reads them), `GET /api/v2/user/profile` and `PUT /api/v2/user/profile?…`.
//!
//! The mapping:
//! - audio language: "Original" is Jellyfin's *play default audio track*; a language sets
//!   `AudioLanguagePreference` and clears that flag.
//! - subtitles: Off, "when audio isn't in the subtitle language" and Always are Jellyfin's
//!   `None`, `Smart` and `Always` subtitle modes. Jellyfin's `Default` (the file's own flags) has no
//!   Plex twin and reads as the middle one.
//! - forced subtitles: only "forced only" exists in Jellyfin (the `OnlyForced` mode); the Settings
//!   picker offers just that and "prefer non-forced".
//! - languages: the app speaks Plex's two-letter codes, Jellyfin stores three-letter ones. The
//!   server's own culture table (`/Localization/Cultures`) converts both ways.
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use super::api::Conn;
use super::query;
use crate::plex::Origin;

fn log(s: &str) {
    plx_base::eventlog::log(s);
}

/// One account call, answered from the Jellyfin user `token` signs in as on `origin`. `user_id` and
/// `user_uuid` are the identity the caller expects back. `None` when nothing answered or the call
/// is not one of the three.
pub fn answer(origin: &Origin, token: &str, user_id: i64, user_uuid: &str, method: &str, path: &str, query_s: &str)
    -> Option<(u16, Vec<u8>)>
{
    let conn = Conn { origin: origin.clone(), token: token.to_string() };
    match (method, path) {
        ("GET", "/api/v2/user") => {
            let config = config(&conn)?.1;
            let body = json!({ "id": user_id, "uuid": user_uuid, "profile": profile(&conn, &config) });
            Some((200, body.to_string().into_bytes()))
        }
        ("GET", "/api/v2/user/profile") => {
            let config = config(&conn)?.1;
            Some((200, profile(&conn, &config).to_string().into_bytes()))
        }
        ("PUT", "/api/v2/user/profile") => save(&conn, query_s),
        _ => None,
    }
}

/// The user's id and `Configuration`.
fn config(conn: &Conn) -> Option<(String, Value)> {
    let me = conn.get_json("/Users/Me")?;
    let id = me.get("Id").and_then(Value::as_str)?.to_string();
    let config = me.get("Configuration").cloned().filter(Value::is_object)?;
    Some((id, config))
}

/// `config` in the shape of plex.tv's `/api/v2/user/profile`.
fn profile(conn: &Conn, config: &Value) -> Value {
    let s = |k: &str| config.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let default_track = config.get("PlayDefaultAudioTrack").and_then(Value::as_bool).unwrap_or(true);
    let audio = if default_track { String::new() } else { two_letter(conn, &s("AudioLanguagePreference")) };
    let (mode, forced) = match s("SubtitleMode").as_str() {
        "None" => (0, 0),
        "Always" => (2, 0),
        "OnlyForced" => (2, 2),
        _ => (1, 0),
    };
    json!({
        "autoSelectAudio": true,
        "defaultAudioLanguage": audio,
        "defaultSubtitleLanguage": two_letter(conn, &s("SubtitleLanguagePreference")),
        "autoSelectSubtitle": mode,
        "defaultSubtitleForced": forced,
    })
}

/// Apply a `PUT /api/v2/user/profile` query to the user's configuration and post it back whole.
fn save(conn: &Conn, query_s: &str) -> Option<(u16, Vec<u8>)> {
    let (id, mut config) = config(conn)?;
    let (_, q) = query::split(&format!("?{query_s}"));
    let before = config.clone();
    if q.get("autoSelectAudio") == Some("false") {
        config["PlayDefaultAudioTrack"] = json!(true);
    }
    if let Some(language) = q.get("defaultAudioLanguage") {
        if language.is_empty() {
            config["PlayDefaultAudioTrack"] = json!(true);
            config["AudioLanguagePreference"] = Value::Null;
        } else {
            config["PlayDefaultAudioTrack"] = json!(false);
            config["AudioLanguagePreference"] = json!(three_letter(conn, language));
        }
    }
    if let Some(language) = q.get("defaultSubtitleLanguage") {
        config["SubtitleLanguagePreference"] =
            if language.is_empty() { Value::Null } else { json!(three_letter(conn, language)) };
    }
    let mode = q.int("autoSelectSubtitle");
    let forced = q.int("defaultSubtitleForced");
    if mode.is_some() || forced.is_some() {
        let current = profile(conn, &before);
        let mode = mode.or_else(|| current["autoSelectSubtitle"].as_i64()).unwrap_or(1);
        let forced = forced.or_else(|| current["defaultSubtitleForced"].as_i64()).unwrap_or(0);
        config["SubtitleMode"] = json!(match (mode, forced) {
            (0, _) => "None",
            (_, 2) => "OnlyForced",
            (1, _) => "Smart",
            _ => "Always",
        });
    }
    // 10.9 and later take the configuration at /Users/Configuration; older servers only at the
    // per-user path, which newer ones may drop.
    let status = conn
        .post(&format!("/Users/Configuration?userId={id}"), Some(&config))
        .filter(|s| (200..300).contains(s))
        .or_else(|| conn.post(&format!("/Users/{id}/Configuration"), Some(&config)))?;
    log(&format!("jellyfin: account preferences saved -> {status}"));
    Some((if (200..300).contains(&status) { 204 } else { status }, Vec::new()))
}

/// The server's culture table as (two-letter, three-letter codes) pairs, read once per process.
fn cultures(conn: &Conn) -> Vec<(String, Vec<String>)> {
    static TABLE: OnceLock<Mutex<Option<Vec<(String, Vec<String>)>>>> = OnceLock::new();
    let cell = TABLE.get_or_init(|| Mutex::new(None));
    if let Some(table) = cell.lock().ok().and_then(|t| t.clone()) {
        return table;
    }
    let Some(list) = conn.get_json("/Localization/Cultures") else { return Vec::new() };
    let table: Vec<(String, Vec<String>)> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let two = c.get("TwoLetterISOLanguageName").and_then(Value::as_str)?.to_ascii_lowercase();
            let mut three: Vec<String> = c
                .get("ThreeLetterISOLanguageNames")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_ascii_lowercase)
                .collect();
            if let Some(main) = c.get("ThreeLetterISOLanguageName").and_then(Value::as_str) {
                let main = main.to_ascii_lowercase();
                three.retain(|t| *t != main);
                three.insert(0, main);
            }
            (!two.is_empty() && !three.is_empty()).then_some((two, three))
        })
        .collect();
    if let Ok(mut t) = cell.lock() {
        *t = Some(table.clone());
    }
    table
}

/// A Plex code (`en`, `pt-BR`) as Jellyfin's three-letter code; unchanged when the table has none.
fn three_letter(conn: &Conn, code: &str) -> String {
    let base = code.split(['-', '_']).next().unwrap_or(code).to_ascii_lowercase();
    lookup_three(&cultures(conn), &base).unwrap_or_else(|| code.to_string())
}

/// A Jellyfin three-letter code as Plex's two-letter one; unchanged when the table has none.
fn two_letter(conn: &Conn, code: &str) -> String {
    if code.is_empty() {
        return String::new();
    }
    lookup_two(&cultures(conn), &code.to_ascii_lowercase()).unwrap_or_else(|| code.to_string())
}

fn lookup_three(table: &[(String, Vec<String>)], two: &str) -> Option<String> {
    table.iter().find(|(t, _)| t == two).and_then(|(_, three)| three.first().cloned())
}

fn lookup_two(table: &[(String, Vec<String>)], three: &str) -> Option<String> {
    table.iter().find(|(_, t)| t.iter().any(|c| c == three)).map(|(two, _)| two.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<(String, Vec<String>)> {
        vec![
            ("en".into(), vec!["eng".into()]),
            ("fr".into(), vec!["fre".into(), "fra".into()]),
        ]
    }

    #[test]
    fn a_two_letter_code_reads_as_the_cultures_main_three_letter_one() {
        assert_eq!(lookup_three(&table(), "fr").as_deref(), Some("fre"));
        assert_eq!(lookup_three(&table(), "xx"), None);
    }

    #[test]
    fn either_three_letter_form_reads_back_as_the_two_letter_code() {
        assert_eq!(lookup_two(&table(), "fra").as_deref(), Some("fr"));
        assert_eq!(lookup_two(&table(), "fre").as_deref(), Some("fr"));
        assert_eq!(lookup_two(&table(), "eng").as_deref(), Some("en"));
    }
}
