//! Jellyfin `BaseItemDto` → Plex `Metadata` JSON, in exactly the field names `plex::models` reads.
//!
//! Artwork is addressed the Plex way — `thumb`/`art` are server PATHS
//! (`/library/metadata/{id}/{kind}/{tag}`) that the poster pipeline later wraps in
//! `/photo/:/transcode?url=…` — so the image cache, whose key grammar is Plex's, keeps working.
//! [`super::routes`] turns that path back into a Jellyfin image request.

use serde_json::{json, Map, Value};

use super::ids;

pub fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

pub fn int(v: &Value, k: &str) -> i64 {
    v.get(k)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
        .unwrap_or(0)
}

pub fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

pub fn flag(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

pub fn items(v: &Value) -> &[Value] {
    v.get("Items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// 100 ns ticks → ms.
pub fn ticks_ms(t: i64) -> i64 {
    t / 10_000
}

/// ISO-8601 (`2026-10-09T20:59:46.7897853Z`) → unix seconds; 0 when absent or unparseable.
pub fn unix_secs(iso: &str) -> i64 {
    let b = iso.as_bytes();
    if b.len() < 10 {
        return 0;
    }
    let n = |r: std::ops::Range<usize>| iso.get(r).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d)) = (n(0..4), n(5..7), n(8..10)) else { return 0 };
    let (hh, mm, ss) = if b.len() >= 19 {
        (n(11..13).unwrap_or(0), n(14..16).unwrap_or(0), n(17..19).unwrap_or(0))
    } else {
        (0, 0, 0)
    };
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hh * 3_600 + mm * 60 + ss
}

/// Jellyfin item type → Plex `type`.
pub fn plex_kind(jf: &str) -> Option<&'static str> {
    Some(match jf {
        "Movie" | "Video" | "MusicVideo" => "movie",
        "Series" => "show",
        "Season" => "season",
        "Episode" => "episode",
        "BoxSet" => "collection",
        _ => return None,
    })
}

/// The Jellyfin `includeItemTypes` for a Plex numeric type.
pub fn jf_types(plex_type: i64) -> &'static str {
    match plex_type {
        2 => "Series",
        3 => "Season",
        4 => "Episode",
        18 => "BoxSet",
        _ => "Movie",
    }
}

/// An artwork path in the Plex shape. `kind`: `thumb` (Primary), `art` (Backdrop), `logo`,
/// `landscape` (Thumb).
pub fn img(id: &str, kind: &str, tag: &str) -> String {
    format!("/library/metadata/{id}/{kind}/{tag}")
}

/// Jellyfin codec names → the spellings the app's Plex-era tables use.
pub fn plex_codec(c: &str) -> String {
    let c = c.to_ascii_lowercase();
    match c.as_str() {
        "dts" => "dca".into(),
        "subrip" => "srt".into(),
        "pgssub" | "hdmv_pgs_subtitle" => "pgs".into(),
        "dvdsub" | "dvd_subtitle" => "vobsub".into(),
        "webvtt" => "vtt".into(),
        _ => c,
    }
}

/// A Jellyfin container list (`"mov,mp4,m4a,3gp,3g2,mj2"`) → the one container name the app
/// expects and the file extension its streamability check reads.
pub fn container(c: &str) -> String {
    let c = c.to_ascii_lowercase();
    let parts: Vec<&str> = c.split(',').map(str::trim).collect();
    if parts.iter().any(|p| matches!(*p, "mkv" | "matroska")) {
        "mkv".into()
    } else if parts.iter().any(|p| matches!(*p, "mp4" | "mov" | "m4v")) {
        "mp4".into()
    } else if parts.contains(&"webm") {
        "webm".into()
    } else {
        parts.first().copied().unwrap_or("").to_string()
    }
}

fn resolution_class(w: i64, h: i64) -> &'static str {
    if w >= 3200 || h >= 1600 {
        "4k"
    } else if w >= 1700 || h >= 900 {
        "1080"
    } else if w >= 1100 || h >= 650 {
        "720"
    } else if h >= 540 {
        "576"
    } else {
        "sd"
    }
}

/// Context a mapping needs beyond the item itself.
#[derive(Clone, Default)]
pub struct Env {
    /// The server's path prefix — baked into direct-play part keys, which the player dials itself.
    pub prefix: String,
    /// The library the item was listed from, when the request said (`librarySectionID` gates Home
    /// pins; 0 = unknown, which Home treats as "cannot be filtered out").
    pub section: Option<(i64, String)>,
}

fn stream(item_id: &str, ms_id: &str, st: &Value, default_audio: i64, default_sub: i64) -> Option<Value> {
    let t = match s(st, "Type") {
        "Video" => 1,
        "Audio" => 2,
        "Subtitle" => 3,
        _ => return None,
    };
    let idx = int(st, "Index");
    let codec = plex_codec(s(st, "Codec"));
    let mut o = Map::new();
    o.insert("id".into(), json!(idx + 1));
    o.insert("streamType".into(), json!(t));
    o.insert("index".into(), json!(idx));
    o.insert("codec".into(), json!(codec));
    o.insert("displayTitle".into(), json!(s(st, "DisplayTitle")));
    o.insert("title".into(), json!(s(st, "Title")));
    let lang = s(st, "Language");
    if !lang.is_empty() && lang != "und" {
        o.insert("language".into(), json!(lang));
        o.insert("languageCode".into(), json!(lang));
    }
    o.insert("channels".into(), json!(int(st, "Channels")));
    o.insert("audioChannelLayout".into(), json!(s(st, "ChannelLayout")));
    o.insert("bitrate".into(), json!(int(st, "BitRate") / 1000));
    let fps = if num(st, "RealFrameRate") > 0.0 { num(st, "RealFrameRate") } else { num(st, "AverageFrameRate") };
    if fps > 0.0 {
        o.insert("frameRate".into(), json!(fps));
    }
    o.insert("bitDepth".into(), json!(int(st, "BitDepth")));
    o.insert("profile".into(), json!(s(st, "Profile").to_ascii_lowercase()));
    o.insert("colorTrc".into(), json!(s(st, "ColorTransfer")));
    o.insert("default".into(), json!(i64::from(flag(st, "IsDefault"))));
    o.insert("forced".into(), json!(i64::from(flag(st, "IsForced"))));
    o.insert("hearingImpaired".into(), json!(i64::from(flag(st, "IsHearingImpaired"))));
    if t == 1 && s(st, "VideoRangeType").to_ascii_uppercase().starts_with("DOVI") {
        o.insert("DOVIPresent".into(), json!(1));
        o.insert("DOVIProfile".into(), json!(int(st, "DvProfile")));
        o.insert("DOVILevel".into(), json!(int(st, "DvLevel")));
        o.insert("DOVIBLCompatID".into(), json!(int(st, "DvBlSignalCompatibilityId")));
        o.insert("DOVIELPresent".into(), json!(int(st, "ElPresentFlag")));
        o.insert("DOVIBLPresent".into(), json!(int(st, "BlPresentFlag")));
        o.insert("DOVIRPUPresent".into(), json!(int(st, "RpuPresentFlag")));
        o.insert(
            "DOVIVersion".into(),
            json!(format!("{}.{}", int(st, "DvVersionMajor"), int(st, "DvVersionMinor"))),
        );
    }
    if t == 3 && flag(st, "IsExternal") {
        let n = ids::sub_key(ids::SubRef {
            item: item_id.to_string(),
            media_source: ms_id.to_string(),
            index: idx,
            codec: codec.clone(),
        });
        o.insert("key".into(), json!(format!("/library/streams/{n}")));
        o.insert("format".into(), json!(codec));
    }
    let selected = (t == 2 && idx == default_audio) || (t == 3 && idx == default_sub);
    o.insert("selected".into(), json!(i64::from(selected)));
    Some(Value::Object(o))
}

/// `MediaSources[]` → Plex `Media[]` (one part each; Jellyfin's multi-part stacking is rare).
fn media(it: &Value, env: &Env) -> Vec<Value> {
    let item_id = s(it, "Id");
    let Some(sources) = it.get("MediaSources").and_then(Value::as_array) else { return Vec::new() };
    sources
        .iter()
        .map(|ms| {
            let ms_id = s(ms, "Id");
            let streams_src = ms.get("MediaStreams").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            let default_audio = ms
                .get("DefaultAudioStreamIndex")
                .and_then(Value::as_i64)
                .or_else(|| streams_src.iter().find(|x| s(x, "Type") == "Audio").map(|x| int(x, "Index")))
                .unwrap_or(-1);
            let default_sub = ms.get("DefaultSubtitleStreamIndex").and_then(Value::as_i64).unwrap_or(-1);
            let streams: Vec<Value> = streams_src
                .iter()
                .filter_map(|x| stream(item_id, ms_id, x, default_audio, default_sub))
                .collect();
            let video = streams_src.iter().find(|x| s(x, "Type") == "Video");
            let audio = streams_src
                .iter()
                .find(|x| s(x, "Type") == "Audio" && int(x, "Index") == default_audio)
                .or_else(|| streams_src.iter().find(|x| s(x, "Type") == "Audio"));
            let (w, h) = video.map(|v| (int(v, "Width"), int(v, "Height"))).unwrap_or((0, 0));
            let aspect = video
                .map(|v| s(v, "AspectRatio"))
                .and_then(|a| a.split_once(':'))
                .and_then(|(a, b)| Some(a.parse::<f64>().ok()? / b.parse::<f64>().ok()?))
                .unwrap_or(if h > 0 { w as f64 / h as f64 } else { 0.0 });
            let cont = container(s(ms, "Container"));
            let ext = if cont.is_empty() { "mkv".to_string() } else { cont.clone() };
            let part_key = format!(
                "{}/Videos/{item_id}/stream.{ext}?static=true&mediaSourceId={ms_id}",
                env.prefix
            );
            json!({
                "id": ids::int_of(ms_id),
                "videoCodec": video.map(|v| plex_codec(s(v, "Codec"))).unwrap_or_default(),
                "audioCodec": audio.map(|a| plex_codec(s(a, "Codec"))).unwrap_or_default(),
                "bitrate": int(ms, "Bitrate") / 1000,
                "width": w,
                "height": h,
                "videoResolution": if video.is_some() { resolution_class(w, h) } else { "" },
                "container": cont,
                "aspectRatio": (aspect * 100.0).round() / 100.0,
                "videoProfile": video.map(|v| s(v, "Profile").to_ascii_lowercase()).unwrap_or_default(),
                "Part": [{
                    "id": ids::int_of(&format!("part:{ms_id}")),
                    "key": part_key,
                    "file": s(ms, "Path"),
                    "size": int(ms, "Size"),
                    "container": cont,
                    "Stream": streams,
                }],
            })
        })
        .collect()
}

fn people(it: &Value, kinds: &[&str], with_role: bool) -> Vec<Value> {
    let Some(ps) = it.get("People").and_then(Value::as_array) else { return Vec::new() };
    ps.iter()
        .filter(|p| kinds.contains(&s(p, "Type")))
        .map(|p| {
            let id = s(p, "Id");
            let tag = s(p, "PrimaryImageTag");
            let mut o = json!({
                "tag": s(p, "Name"),
                "id": ids::int_of(id),
                "tagKey": id,
                "thumb": if tag.is_empty() { String::new() } else { img(id, "thumb", tag) },
            });
            if with_role {
                o["role"] = json!(s(p, "Role"));
            }
            o
        })
        .collect()
}

fn tags(it: &Value, k: &str) -> Vec<Value> {
    it.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|g| g.as_str().or_else(|| g.get("Name").and_then(Value::as_str)))
                .map(|g| json!({ "tag": g }))
                .collect()
        })
        .unwrap_or_default()
}

fn chapters(it: &Value) -> Vec<Value> {
    let Some(cs) = it.get("Chapters").and_then(Value::as_array) else { return Vec::new() };
    let run = ticks_ms(int(it, "RunTimeTicks"));
    cs.iter()
        .enumerate()
        .map(|(i, c)| {
            let start = ticks_ms(int(c, "StartPositionTicks"));
            let end = cs.get(i + 1).map(|n| ticks_ms(int(n, "StartPositionTicks"))).unwrap_or(run);
            json!({
                "index": i as i64 + 1,
                "startTimeOffset": start,
                "endTimeOffset": end,
                "tag": s(c, "Name"),
            })
        })
        .collect()
}

/// Map one item. `None` for a type the app does not list (music, photos, folders).
pub fn item(it: &Value, env: &Env) -> Option<Value> {
    let jf_type = s(it, "Type");
    let kind = plex_kind(jf_type)?;
    let id = s(it, "Id");
    if id.is_empty() {
        return None;
    }
    let ud = it.get("UserData").cloned().unwrap_or(Value::Null);
    let series_id = s(it, "SeriesId");
    ids::remember(id, kind, series_id);
    if int(it, "RunTimeTicks") > 0 {
        super::remember_runtime(id, ticks_ms(int(it, "RunTimeTicks")));
    }

    let mut o = Map::new();
    o.insert("type".into(), json!(kind));
    o.insert("ratingKey".into(), json!(id));
    let leaf = matches!(kind, "movie" | "episode");
    o.insert(
        "key".into(),
        json!(if leaf { format!("/library/metadata/{id}") } else { format!("/library/metadata/{id}/children") }),
    );
    o.insert("guid".into(), json!(format!("jellyfin://{kind}/{id}")));
    let name = s(it, "Name");
    let title = if name.is_empty() && kind == "season" {
        format!("Season {}", int(it, "IndexNumber"))
    } else {
        name.to_string()
    };
    o.insert("title".into(), json!(title));
    o.insert("year".into(), json!(int(it, "ProductionYear")));
    o.insert("contentRating".into(), json!(s(it, "OfficialRating")));
    o.insert("summary".into(), json!(s(it, "Overview")));
    if let Some(t) = it.get("Taglines").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str) {
        o.insert("tagline".into(), json!(t));
    }
    if let Some(st) = it.get("Studios").and_then(Value::as_array).and_then(|a| a.first()) {
        o.insert("studio".into(), json!(s(st, "Name")));
    }
    let premiere = s(it, "PremiereDate");
    if premiere.len() >= 10 {
        o.insert("originallyAvailableAt".into(), json!(&premiere[..10]));
    }
    o.insert("duration".into(), json!(ticks_ms(int(it, "RunTimeTicks"))));
    let offset = ticks_ms(int(&ud, "PlaybackPositionTicks"));
    let offset = if leaf { super::position_ms(id, offset) } else { offset };
    o.insert("viewOffset".into(), json!(offset));
    o.insert("lastViewedAt".into(), json!(unix_secs(s(&ud, "LastPlayedDate"))));
    o.insert("updatedAt".into(), json!(unix_secs(s(it, "DateCreated"))));
    o.insert("addedAt".into(), json!(unix_secs(s(it, "DateCreated"))));
    if flag(&ud, "Played") {
        o.insert("viewCount".into(), json!(int(&ud, "PlayCount").max(1)));
    }
    o.insert("index".into(), json!(int(it, "IndexNumber")));
    o.insert("childCount".into(), json!(int(it, "ChildCount")));
    if kind == "collection" {
        // PMS's `collectionSort` 0, release order: what `/library/collections/{rk}/children`
        // answers in, so the Collection page names the order it actually shows.
        o.insert("collectionSort".into(), json!(0));
    }

    // Hierarchy.
    match kind {
        "season" => {
            o.insert("parentRatingKey".into(), json!(series_id));
            o.insert("parentTitle".into(), json!(s(it, "SeriesName")));
            o.insert("parentIndex".into(), json!(int(it, "IndexNumber")));
        }
        "episode" => {
            o.insert("parentRatingKey".into(), json!(s(it, "SeasonId")));
            o.insert("parentTitle".into(), json!(s(it, "SeasonName")));
            o.insert("parentIndex".into(), json!(int(it, "ParentIndexNumber")));
            o.insert("grandparentRatingKey".into(), json!(series_id));
            o.insert("grandparentTitle".into(), json!(s(it, "SeriesName")));
        }
        _ => {}
    }

    // Watched counts on containers: Jellyfin reports the unplayed count; Plex the viewed one.
    if matches!(kind, "show" | "season") {
        let total = match int(it, "RecursiveItemCount") {
            0 => int(it, "ChildCount"),
            n => n,
        };
        let unplayed = ud.get("UnplayedItemCount").and_then(Value::as_i64);
        let viewed = match unplayed {
            Some(u) => (total - u).max(0),
            None if flag(&ud, "Played") => total,
            None => 0,
        };
        o.insert("leafCount".into(), json!(total));
        o.insert("viewedLeafCount".into(), json!(viewed));
    }

    // Artwork.
    let tags_obj = it.get("ImageTags").cloned().unwrap_or(Value::Null);
    let primary = s(&tags_obj, "Primary");
    let series_primary = s(it, "SeriesPrimaryImageTag");
    let thumb = if !primary.is_empty() {
        img(id, "thumb", primary)
    } else if !series_primary.is_empty() && !series_id.is_empty() {
        img(series_id, "thumb", series_primary)
    } else {
        // Home drops a row with no thumb; a path Jellyfin answers 404 to keeps the row and lets
        // the poster pipeline draw its placeholder.
        img(id, "thumb", "0")
    };
    o.insert("thumb".into(), json!(thumb));
    if !series_id.is_empty() && !series_primary.is_empty() {
        let st = img(series_id, "thumb", series_primary);
        if kind == "episode" {
            o.insert("grandparentThumb".into(), json!(st.clone()));
        }
        o.insert("parentThumb".into(), json!(st));
    }
    let backdrop = it
        .get("BackdropImageTags")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(|t| img(id, "art", t))
        .or_else(|| {
            let pid = s(it, "ParentBackdropItemId");
            it.get("ParentBackdropImageTags")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .filter(|_| !pid.is_empty())
                .map(|t| img(pid, "art", t))
        });
    if let Some(art) = backdrop {
        o.insert("art".into(), json!(art));
    }

    // Library.
    if let Some((key, name)) = &env.section {
        o.insert("librarySectionID".into(), json!(key));
        o.insert("librarySectionTitle".into(), json!(name));
    }

    // Ratings: CommunityRating is 0–10, CriticRating a 0–100 Tomatometer.
    let community = num(it, "CommunityRating");
    if community > 0.0 {
        o.insert("audienceRating".into(), json!((community * 10.0).round() / 10.0));
        o.insert("audienceRatingImage".into(), json!("themoviedb://image.rating"));
    }
    let critic = num(it, "CriticRating");
    if critic > 0.0 {
        o.insert("rating".into(), json!(critic / 10.0));
        o.insert(
            "ratingImage".into(),
            json!(if critic >= 60.0 { "rottentomatoes://image.rating.ripe" } else { "rottentomatoes://image.rating.rotten" }),
        );
    }

    if leaf {
        o.insert("Media".into(), Value::Array(media(it, env)));
    }
    o.insert("Genre".into(), Value::Array(tags(it, "Genres")));
    o.insert("Country".into(), Value::Array(tags(it, "ProductionLocations")));
    o.insert("Role".into(), Value::Array(people(it, &["Actor", "GuestStar"], true)));
    o.insert("Director".into(), Value::Array(people(it, &["Director"], false)));
    o.insert("Writer".into(), Value::Array(people(it, &["Writer"], false)));
    o.insert("Chapter".into(), Value::Array(chapters(it)));
    Some(Value::Object(o))
}

/// Map every item of a Jellyfin list body.
pub fn list(body: &Value, env: &Env) -> Vec<Value> {
    items(body).iter().filter_map(|it| item(it, env)).collect()
}

/// `{ "MediaContainer": mc }` as bytes.
pub fn envelope(mc: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({ "MediaContainer": mc })).unwrap_or_default()
}

/// A container of items with Plex paging fields.
pub fn container_of(metadata: Vec<Value>, total: i64, offset: i64) -> Value {
    json!({
        "size": metadata.len(),
        "totalSize": total.max(metadata.len() as i64),
        "offset": offset,
        "Metadata": metadata,
    })
}

/// A hub row.
pub fn hub(ident: &str, key: &str, title: &str, kind: &str, metadata: Vec<Value>, total: i64) -> Value {
    let size = metadata.len() as i64;
    json!({
        "hubIdentifier": ident,
        "key": key,
        "title": title,
        "type": kind,
        "size": size,
        "totalSize": total.max(size),
        "more": total > size,
        "Metadata": metadata,
    })
}
