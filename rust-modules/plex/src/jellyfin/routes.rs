//! The Plex endpoint table: each PMS request the app makes, answered from Jellyfin.
//!
//! Endpoints the app calls but this backend has no equivalent for answer an EMPTY container (the
//! app already copes with a server that has nothing to say) or a 404 (the app already copes with a
//! server that does not have the endpoint). Every unhandled path is logged once per call so a
//! missing route is visible in the event log rather than as a blank shelf.

use serde_json::{json, Value};

use super::api::{self, Conn, Ctx, View};
use super::ids;
use super::map::{self, int, s, Env};
use super::query::{self, enc, Query};
use crate::http::Method;
use crate::plex::Origin;

/// The fields every LIST request asks for. `MediaSources` is what lets a tile start playback
/// (`Media[0].Part[0].key`) and show codec badges, as it does on Plex.
const LIST_FIELDS: &str = "Overview,PrimaryImageAspectRatio,DateCreated,ChildCount,RecursiveItemCount,\
MediaSources,Genres,ProductionYear,PremiereDate,OfficialRating,CommunityRating,CriticRating,Taglines,\
Studios,ParentId";

const IMAGE_FIELDS: &str = "enableImageTypes=Primary,Backdrop,Logo,Thumb&imageTypeLimit=1";

fn log(s: &str) {
    plx_base::eventlog::log(s);
}

type Answer = Option<(i32, Vec<u8>)>;

fn ok(mc: Value) -> Answer {
    Some((200, map::envelope(mc)))
}

fn empty() -> Answer {
    ok(json!({ "size": 0 }))
}

fn not_found() -> Answer {
    Some((404, b"{}".to_vec()))
}

fn ok_bare() -> Answer {
    Some((200, Vec::new()))
}

/// Answer one PMS request. `path` carries its query (and the `X-Plex-Token` the client appended);
/// `None` is "nothing answered", which the caller reports as a transport failure.
pub fn handle(origin: &Origin, path: &str, method: Method, headers: &[&str]) -> Answer {
    let (p, q) = query::split(path);
    let token = q
        .get("X-Plex-Token")
        .map(str::to_string)
        .or_else(|| {
            headers.iter().find_map(|h| {
                let (n, v) = h.split_once(':')?;
                n.trim().eq_ignore_ascii_case("x-plex-token").then(|| v.trim().to_string())
            })
        })
        .unwrap_or_default();
    let conn = Conn { origin: origin.clone(), token };
    let seg: Vec<&str> = p.trim_start_matches('/').split('/').collect();

    // Endpoints that need no user.
    match seg.as_slice() {
        [""] | ["identity"] => return root(&conn),
        ["photo", ":", "transcode"] => return photo(&conn, &q),
        _ => {}
    }

    let Some(ctx) = api::ctx(&conn) else {
        // No user behind this token (or no server): the shape a PMS 401 has.
        return Some((401, b"{}".to_vec()));
    };
    let env = Env { prefix: conn.prefix(), section: None };
    let r = Route { conn: &conn, ctx: &ctx, env, q: &q };

    let answer = match (method, seg.as_slice()) {
        (_, ["library", "sections"]) => r.sections(),
        (_, ["library", "sections", k, "all"]) => r.section_all(k),
        (_, ["library", "sections", k, "genre"]) => r.genres(k),
        (_, ["library", "sections", _, _]) => empty(),
        (_, ["library", "sections", _, _, ..]) => empty(),
        (_, ["hubs"]) => r.home_hubs(),
        (_, ["hubs", "continueWatching"]) => r.continue_watching(),
        (_, ["hubs", "continueWatching", "items"]) => r.continue_watching_items(),
        (_, ["hubs", "home", "recentlyAdded"]) => r.recently_added_page(),
        (_, ["hubs", "sections", k]) => r.section_hubs(k),
        (_, ["hubs", "sections", k, "continueWatching", "items"]) => r.section_resume_page(k),
        (_, ["hubs", "search"]) => r.search(),
        (_, ["hubs", "promoted"]) | (_, ["hubs", "sections", _, "promoted"]) => ok(json!({ "size": 0, "Hub": [] })),
        (_, ["library", "metadata", csv]) => r.metadata(csv),
        (_, ["library", "metadata", id, "children"]) => r.children(id),
        (_, ["library", "metadata", id, "allLeaves"]) => r.all_leaves(id),
        (_, ["library", "metadata", id, "related"]) | (_, ["library", "metadata", id, "similar"]) => r.related(id),
        (_, ["library", "metadata", id, "clearLogo"]) => image(&conn, id, "Logo", "", 0, 0, true),
        (_, ["library", "metadata", _, _]) => empty(),
        (_, ["library", "people", id, "media"]) => r.person_media(id),
        (_, ["library", "streams", n]) => r.subtitle(n),
        (_, ["library", "all"]) => empty(),
        (_, ["library", "collections", id, "children"]) => r.collection_members(id),
        (_, ["library", "collections", ..]) => empty(),
        (Method::Post, ["playQueues"]) => r.play_queue(),
        (_, ["video", ":", "transcode", "universal", "decision"]) => r.decision(),
        (_, ["video", ":", "transcode", "universal", "stop"]) => r.stop_transcode(),
        (_, ["video", ":", "transcode", "universal", "ping"]) => ok_bare(),
        (_, ["status", "sessions", "close"]) => ok_bare(),
        (Method::Put, ["library", "parts", ..]) => ok_bare(),
        (_, [":", "timeline"]) => r.timeline(),
        (_, [":", "scrobble"]) => r.played(true),
        (_, [":", "unscrobble"]) => r.played(false),
        (_, ["actions", "removeFromContinueWatching"]) => r.remove_from_resume(),
        _ => {
            log(&format!("jellyfin: no route for {} /{}", method_name(method), seg.join("/")));
            not_found()
        }
    };
    answer
}

fn method_name(m: Method) -> &'static str {
    match m {
        Method::Get => "GET",
        Method::Put => "PUT",
        Method::Post => "POST",
    }
}

/// `GET /` and `/identity` — the server describing itself.
fn root(conn: &Conn) -> Answer {
    let info = api::server_info(conn)?;
    ok(json!({
        "friendlyName": s(&info, "ServerName"),
        "machineIdentifier": s(&info, "Id"),
        "version": s(&info, "Version"),
        "size": 0,
    }))
}

/// `/photo/:/transcode?url=/library/metadata/{id}/{kind}/{tag}&width=&height=[&format=png]`.
fn photo(conn: &Conn, q: &Query) -> Answer {
    let url = q.str("url");
    let png = q.str("format") == "png";
    let (w, h) = (q.int("width").unwrap_or(0), q.int("height").unwrap_or(0));
    let seg: Vec<&str> = url.trim_start_matches('/').split('/').collect();
    match seg.as_slice() {
        ["library", "metadata", id, kind, tag] => {
            let jf = match *kind {
                "art" => "Backdrop",
                "logo" | "clearLogo" => "Logo",
                "landscape" => "Thumb",
                _ => "Primary",
            };
            image(conn, id, jf, tag, w, h, png)
        }
        ["library", "metadata", id, "clearLogo"] => image(conn, id, "Logo", "", w, h, true),
        _ => not_found(),
    }
}

fn image(conn: &Conn, id: &str, kind: &str, tag: &str, w: i64, h: i64, png: bool) -> Answer {
    // Plex's transcoder (`minSize=1`) scales WITHOUT cropping, and the poster pipeline crops to
    // its own frame. Jellyfin's `fillWidth`+`fillHeight` crops to the box, which would cut the ends
    // off a wide logo, so ask by width alone (height follows the image's own aspect) — and fit
    // logos inside the whole box, since they are drawn whole.
    let mut p = format!("/Items/{id}/Images/{kind}?quality=90");
    if kind == "Logo" {
        if w > 0 {
            p.push_str(&format!("&maxWidth={w}"));
        }
        if h > 0 {
            p.push_str(&format!("&maxHeight={h}"));
        }
    } else if w > 0 {
        p.push_str(&format!("&maxWidth={w}"));
    }
    if !tag.is_empty() && tag != "0" {
        p.push_str(&format!("&tag={}", enc(tag)));
    }
    if png {
        p.push_str("&format=Png");
    }
    let (status, body) = conn.get_bytes(&p)?;
    Some((i32::from(status), body))
}

struct Route<'a> {
    conn: &'a Conn,
    ctx: &'a Ctx,
    env: Env,
    q: &'a Query,
}

impl Route<'_> {
    fn uid(&self) -> &str {
        &self.ctx.user_id
    }

    fn env_for(&self, v: &View) -> Env {
        Env { prefix: self.env.prefix.clone(), section: Some((v.key, v.name.clone())) }
    }

    fn start(&self) -> i64 {
        self.q.int("X-Plex-Container-Start").unwrap_or(0).max(0)
    }

    fn size(&self, default: i64) -> i64 {
        self.q.int("X-Plex-Container-Size").or_else(|| self.q.int("count")).unwrap_or(default)
    }

    fn view(&self, k: &str) -> Option<View> {
        let key: i64 = k.parse().ok()?;
        api::view_by_key(self.conn, self.ctx, key)
    }

    /// `/Items?…` for this user, with the list fields.
    fn items(&self, extra: &str) -> Option<Value> {
        self.conn.get_json(&format!(
            "/Items?userId={}&fields={LIST_FIELDS}&{IMAGE_FIELDS}&{extra}",
            self.uid()
        ))
    }

    fn one(&self, id: &str) -> Option<Value> {
        self.conn
            .get_json(&format!("/Items/{id}?userId={}", self.uid()))
            .or_else(|| self.conn.get_json(&format!("/Users/{}/Items/{id}", self.uid())))
    }

    // ---- libraries ----

    fn sections(&self) -> Answer {
        let views = api::views(self.conn, self.ctx, true);
        let dirs: Vec<Value> = views
            .iter()
            .map(|v| json!({ "key": v.key.to_string(), "type": v.kind, "title": v.name }))
            .collect();
        ok(json!({ "size": dirs.len(), "Directory": dirs }))
    }

    /// The sort menu the grid's Sort button lists, and its Jellyfin spelling.
    fn sort_options(kind: &str) -> Value {
        if kind == "collection" {
            // A collection has no release date, rating or runtime of its own to sort by.
            return json!({ "Type": [{ "active": 1, "Sort": [
                { "key": "titleSort", "descKey": "titleSort:desc", "defaultDirection": "asc", "title": "Title" },
                { "key": "addedAt", "descKey": "addedAt:desc", "defaultDirection": "desc", "title": "Date Added" },
            ] }] });
        }
        let mut opts = vec![
            json!({ "key": "titleSort", "descKey": "titleSort:desc", "defaultDirection": "asc", "title": "Title" }),
            json!({ "key": "addedAt", "descKey": "addedAt:desc", "defaultDirection": "desc", "title": "Date Added" }),
            json!({ "key": "originallyAvailableAt", "descKey": "originallyAvailableAt:desc", "defaultDirection": "desc", "title": "Release Date" }),
            json!({ "key": "audienceRating", "descKey": "audienceRating:desc", "defaultDirection": "desc", "title": "Rating" }),
        ];
        if kind == "movie" {
            opts.push(json!({ "key": "lastViewedAt", "descKey": "lastViewedAt:desc", "defaultDirection": "desc", "title": "Last Played" }));
            opts.push(json!({ "key": "duration", "descKey": "duration:desc", "defaultDirection": "asc", "title": "Duration" }));
        }
        json!({ "Type": [{ "active": 1, "Sort": opts }] })
    }

    fn jf_sort(plex: &str) -> (&'static str, &'static str) {
        let (k, dir) = plex.split_once(':').unwrap_or((plex, ""));
        let by = match k {
            "addedAt" => "DateCreated,SortName",
            "originallyAvailableAt" | "year" => "PremiereDate,ProductionYear,SortName",
            "audienceRating" => "CommunityRating,SortName",
            "rating" => "CriticRating,SortName",
            "lastViewedAt" => "DatePlayed,SortName",
            "duration" => "Runtime,SortName",
            "random" => "Random",
            _ => "SortName",
        };
        (by, if dir == "desc" { "Descending" } else { "Ascending" })
    }

    fn section_all(&self, k: &str) -> Answer {
        let Some(v) = self.view(k) else { return empty() };
        let plex_type = self.q.int("type").unwrap_or(match v.kind {
            "show" => 2,
            "collection" => 18,
            _ => 1,
        });
        let (by, order) = Self::jf_sort(self.q.str("sort"));
        let start = self.start();
        let size = self.size(60);
        let mut extra = format!(
            "parentId={}&recursive=true&includeItemTypes={}&sortBy={by}&sortOrder={order}&startIndex={start}&limit={}",
            v.id,
            map::jf_types(plex_type),
            size.max(1)
        );
        if self.q.flag("unwatched") || self.q.flag("unwatchedLeaves") {
            extra.push_str("&isPlayed=false");
        }
        if let Some(g) = self.q.int("genre").and_then(ids::string_of) {
            extra.push_str(&format!("&genreIds={g}"));
        }
        let body = self.items(&extra)?;
        let total = int(&body, "TotalRecordCount");
        let md = if size == 0 { Vec::new() } else { map::list(&body, &self.env_for(&v)) };
        let mut mc = map::container_of(md, total, start);
        mc["librarySectionID"] = json!(v.key);
        mc["librarySectionTitle"] = json!(v.name);
        if self.q.flag("includeMeta") {
            mc["Meta"] = Self::sort_options(v.kind);
        }
        ok(mc)
    }

    /// `/library/collections/{rk}/children` — a collection's films and shows, paged as the
    /// Collection page asks, in release order across both (a show by its first air date). Each
    /// member keeps its own library: a collection gathers items from any of them.
    fn collection_members(&self, id: &str) -> Answer {
        let start = self.start();
        let size = self.size(60);
        let body = self.items(&format!(
            "parentId={id}&sortBy=PremiereDate,ProductionYear,SortName&sortOrder=Ascending&startIndex={start}&limit={}",
            size.max(1)
        ))?;
        let md = if size == 0 { Vec::new() } else { map::list(&body, &self.env) };
        ok(map::container_of(md, int(&body, "TotalRecordCount"), start))
    }

    fn genres(&self, k: &str) -> Answer {
        let Some(v) = self.view(k) else { return empty() };
        let Some(body) = self
            .conn
            .get_json(&format!("/Genres?userId={}&parentId={}&sortBy=SortName", self.uid(), v.id))
        else {
            return empty();
        };
        let dirs: Vec<Value> = map::items(&body)
            .iter()
            .map(|g| json!({ "key": ids::int_of(s(g, "Id")).to_string(), "title": s(g, "Name") }))
            .collect();
        ok(json!({ "size": dirs.len(), "Directory": dirs }))
    }

    // ---- hubs ----

    fn recently_added_query(v: &View, start: i64, limit: i64) -> String {
        let (types, by) = if v.kind == "show" {
            ("Series", "DateLastContentAdded,SortName")
        } else {
            ("Movie", "DateCreated,SortName")
        };
        format!(
            "parentId={}&recursive=true&includeItemTypes={types}&sortBy={by}&sortOrder=Descending&startIndex={start}&limit={limit}",
            v.id
        )
    }

    fn recently_added_hub(&self, v: &View, count: i64) -> Option<Value> {
        let body = self.items(&Self::recently_added_query(v, 0, count))?;
        let md = map::list(&body, &self.env_for(v));
        if md.is_empty() {
            return None;
        }
        let t = if v.kind == "show" { 2 } else { 1 };
        Some(map::hub(
            &format!("{}.recentlyadded.{}", v.kind, v.key),
            &format!("/hubs/home/recentlyAdded?type={t}&sectionID={}", v.key),
            &format!("Recently Added in {}", v.name),
            v.kind,
            md,
            int(&body, "TotalRecordCount"),
        ))
    }

    fn home_hubs(&self) -> Answer {
        let count = self.q.int("count").unwrap_or(12);
        let hubs: Vec<Value> = api::views(self.conn, self.ctx, false)
            .iter()
            .filter(|v| v.holds_media())
            .filter_map(|v| self.recently_added_hub(v, count))
            .collect();
        ok(json!({ "size": hubs.len(), "Hub": hubs }))
    }

    /// Resume points first, then the next episode of each show in progress — what Plex's
    /// Continue Watching row holds.
    ///
    /// Resume points are asked for PER LIBRARY and merged by recency, never with one server-wide
    /// call: a server-wide `/UserItems/Resume` answered empty on Jellyfin 12.2 while the same call
    /// scoped to each library answered its items, and the per-library form also tags every row
    /// with the library it came from (which Home's per-library pins need).
    fn resume_and_next(&self, parent: Option<&View>, limit: i64) -> Vec<Value> {
        let parent_q = parent.map(|v| format!("&parentId={}", v.id)).unwrap_or_default();
        let env = parent.map(|v| self.env_for(v)).unwrap_or_else(|| self.env.clone());
        let u = self.uid();
        let views = match parent {
            Some(v) => vec![v.clone()],
            None => api::views(self.conn, self.ctx, false).into_iter().filter(View::holds_media).collect(),
        };
        let mut out: Vec<Value> = Vec::new();
        for v in &views {
            let resume = self
                .conn
                .get_json(&format!(
                    "/UserItems/Resume?userId={u}&parentId={}&limit={limit}&mediaTypes=Video&fields={LIST_FIELDS}&{IMAGE_FIELDS}",
                    v.id
                ))
                .or_else(|| {
                    self.conn.get_json(&format!(
                        "/Users/{u}/Items/Resume?parentId={}&limit={limit}&mediaTypes=Video&fields={LIST_FIELDS}&{IMAGE_FIELDS}",
                        v.id
                    ))
                });
            if let Some(b) = resume {
                out.extend(map::list(&b, &self.env_for(v)));
            }
        }
        out.sort_by_key(|m| std::cmp::Reverse(m["lastViewedAt"].as_i64().unwrap_or(0)));
        if parent.is_none_or(|v| v.kind == "show") {
            if let Some(next) = self.conn.get_json(&format!(
                "/Shows/NextUp?userId={u}&limit={limit}&enableResumable=false&fields={LIST_FIELDS}&{IMAGE_FIELDS}{parent_q}"
            )) {
                out.extend(map::list(&next, &env));
            }
        }
        out.truncate(limit.max(0) as usize);
        out
    }

    fn continue_watching(&self) -> Answer {
        let count = self.q.int("count").unwrap_or(12);
        let md = self.resume_and_next(None, count.max(24));
        let total = md.len() as i64;
        let md: Vec<Value> = md.into_iter().take(count.max(0) as usize).collect();
        let hub = map::hub("home.continue", "/hubs/continueWatching/items", "Continue Watching", "mixed", md, total);
        ok(json!({ "size": 1, "Hub": [hub] }))
    }

    fn continue_watching_items(&self) -> Answer {
        let start = self.start();
        let size = self.size(24);
        let md = self.resume_and_next(None, start + size);
        let total = md.len() as i64;
        ok(map::container_of(md.into_iter().skip(start as usize).collect(), total, start))
    }

    fn recently_added_page(&self) -> Answer {
        let Some(v) = self.view(self.q.str("sectionID")) else { return empty() };
        let (start, size) = (self.start(), self.size(24));
        let body = self.items(&Self::recently_added_query(&v, start, size.max(1)))?;
        ok(map::container_of(map::list(&body, &self.env_for(&v)), int(&body, "TotalRecordCount"), start))
    }

    fn section_hubs(&self, k: &str) -> Answer {
        let Some(v) = self.view(k).filter(View::holds_media) else { return ok(json!({ "size": 0, "Hub": [] })) };
        let count = self.q.int("count").unwrap_or(12);
        let mut hubs = Vec::new();
        let resume = self.resume_and_next(Some(&v), count);
        if !resume.is_empty() {
            let n = resume.len() as i64;
            hubs.push(map::hub(
                &format!("{}.inprogress.{}", v.kind, v.key),
                &format!("/hubs/sections/{}/continueWatching/items", v.key),
                "Continue Watching",
                v.kind,
                resume,
                n,
            ));
        }
        if let Some(h) = self.recently_added_hub(&v, count) {
            hubs.push(h);
        }
        ok(json!({ "size": hubs.len(), "Hub": hubs }))
    }

    fn section_resume_page(&self, k: &str) -> Answer {
        let Some(v) = self.view(k) else { return empty() };
        let (start, size) = (self.start(), self.size(24));
        let md = self.resume_and_next(Some(&v), start + size);
        let total = md.len() as i64;
        ok(map::container_of(md.into_iter().skip(start as usize).collect(), total, start))
    }

    fn search(&self) -> Answer {
        let term = self.q.str("query");
        if term.trim().is_empty() {
            return ok(json!({ "size": 0, "Hub": [] }));
        }
        let limit = self.q.int("limit").unwrap_or(30);
        let mut extra = format!(
            "searchTerm={}&recursive=true&includeItemTypes=Movie,Series,Episode&limit={}",
            enc(term),
            limit * 3
        );
        if let Some(v) = self.q.int("sectionId").filter(|n| *n > 0).and_then(|n| api::view_by_key(self.conn, self.ctx, n)) {
            extra.push_str(&format!("&parentId={}", v.id));
        }
        let body = self.items(&extra)?;
        let all = map::list(&body, &self.env);
        let mut hubs = Vec::new();
        for (kind, title) in [("movie", "Movies"), ("show", "Shows"), ("episode", "Episodes")] {
            let md: Vec<Value> = all
                .iter()
                .filter(|m| m["type"] == kind)
                .take(limit as usize)
                .cloned()
                .collect();
            let n = md.len() as i64;
            hubs.push(map::hub(kind, &format!("/hubs/search/{kind}"), title, kind, md, n));
        }
        // People, as Plex sends them: a `Directory[]` of tags.
        if let Some(p) = self
            .conn
            .get_json(&format!("/Persons?userId={}&searchTerm={}&limit={limit}", self.uid(), enc(term)))
        {
            let dirs: Vec<Value> = map::items(&p)
                .iter()
                .map(|x| {
                    let id = s(x, "Id");
                    let tag = x.get("ImageTags").map(|t| s(t, "Primary")).unwrap_or("");
                    json!({
                        "tag": s(x, "Name"),
                        "id": ids::int_of(id),
                        "tagKey": id,
                        "thumb": if tag.is_empty() { String::new() } else { map::img(id, "thumb", tag) },
                    })
                })
                .collect();
            let n = dirs.len();
            hubs.push(json!({
                "hubIdentifier": "actor", "type": "actor", "title": "People",
                "key": "/hubs/search/actor", "size": n, "Directory": dirs,
            }));
        }
        ok(json!({ "size": hubs.len(), "Hub": hubs }))
    }

    // ---- items ----

    /// Intro/credits markers from Jellyfin's media segments (10.10+); empty on older servers.
    fn markers(&self, id: &str, runtime_ms: i64) -> Vec<Value> {
        let Some(body) = self.conn.get_json(&format!("/MediaSegments/{id}")) else { return Vec::new() };
        map::items(&body)
            .iter()
            .filter_map(|seg| {
                let kind = match s(seg, "Type") {
                    "Intro" => "intro",
                    "Outro" => "credits",
                    _ => return None,
                };
                let start = map::ticks_ms(int(seg, "StartTicks"));
                let end = map::ticks_ms(int(seg, "EndTicks"));
                let fin = kind == "credits" && runtime_ms > 0 && end >= runtime_ms - 2_000;
                Some(json!({ "type": kind, "startTimeOffset": start, "endTimeOffset": end, "final": fin }))
            })
            .collect()
    }

    fn detail(&self, id: &str) -> Option<Value> {
        let it = self.one(id)?;
        let mut md = map::item(&it, &self.env)?;
        let kind = md["type"].as_str().unwrap_or("").to_string();
        if matches!(kind.as_str(), "movie" | "episode") {
            let run = md["duration"].as_i64().unwrap_or(0);
            md["Marker"] = Value::Array(self.markers(id, run));
        }
        if kind == "show" {
            if let Some(next) = self.conn.get_json(&format!(
                "/Shows/NextUp?userId={}&seriesId={id}&limit=1&enableResumable=true&fields={LIST_FIELDS}",
                self.uid()
            )) {
                if let Some(ep) = map::list(&next, &self.env).into_iter().next() {
                    md["OnDeck"] = json!({ "Metadata": ep });
                }
            }
        }
        Some(md)
    }

    fn metadata(&self, ids_csv: &str) -> Answer {
        let ids: Vec<&str> = ids_csv.split(',').filter(|x| !x.is_empty()).collect();
        if ids.len() == 1 {
            return match self.detail(ids[0]) {
                Some(md) => ok(json!({ "size": 1, "Metadata": [md] })),
                None => not_found(),
            };
        }
        let body = self.items(&format!("ids={}", ids.join(",")))?;
        let md = map::list(&body, &self.env);
        ok(json!({ "size": md.len(), "Metadata": md }))
    }

    fn kind_of(&self, id: &str) -> Option<ids::Known> {
        if let Some(k) = ids::recall(id) {
            return Some(k);
        }
        let it = self.one(id)?;
        let kind = map::plex_kind(s(&it, "Type")).unwrap_or("folder");
        ids::remember(id, kind, s(&it, "SeriesId"));
        ids::recall(id)
    }

    fn children(&self, id: &str) -> Answer {
        let Some(k) = self.kind_of(id) else { return not_found() };
        let u = self.uid();
        let body = match k.kind.as_str() {
            "show" => self.conn.get_json(&format!(
                "/Shows/{id}/Seasons?userId={u}&fields={LIST_FIELDS}&{IMAGE_FIELDS}"
            ))?,
            "season" => {
                let series = if k.series_id.is_empty() {
                    self.one(id).map(|it| s(&it, "SeriesId").to_string()).unwrap_or_default()
                } else {
                    k.series_id.clone()
                };
                self.conn.get_json(&format!(
                    "/Shows/{series}/Episodes?userId={u}&seasonId={id}&fields={LIST_FIELDS}&{IMAGE_FIELDS}"
                ))?
            }
            _ => self.items(&format!("parentId={id}&sortBy=SortName"))?,
        };
        let md = map::list(&body, &self.env);
        ok(json!({ "size": md.len(), "totalSize": md.len(), "Metadata": md }))
    }

    fn all_leaves(&self, id: &str) -> Answer {
        let body = self.conn.get_json(&format!(
            "/Shows/{id}/Episodes?userId={}&fields={LIST_FIELDS}&{IMAGE_FIELDS}",
            self.uid()
        ))?;
        let md = map::list(&body, &self.env);
        ok(json!({ "size": md.len(), "Metadata": md }))
    }

    fn related(&self, id: &str) -> Answer {
        let Some(body) = self.conn.get_json(&format!(
            "/Items/{id}/Similar?userId={}&limit=12&fields={LIST_FIELDS}&{IMAGE_FIELDS}",
            self.uid()
        )) else {
            return ok(json!({ "size": 0, "Hub": [] }));
        };
        let md = map::list(&body, &self.env);
        if md.is_empty() {
            return ok(json!({ "size": 0, "Hub": [] }));
        }
        let kind = md[0]["type"].as_str().unwrap_or("movie").to_string();
        let n = md.len() as i64;
        let hub = map::hub(&format!("{kind}.similar"), &format!("/library/metadata/{id}/similar"), "More Like This", &kind, md, n);
        ok(json!({ "size": 1, "Hub": [hub] }))
    }

    fn person_media(&self, id: &str) -> Answer {
        let guid = id.parse::<i64>().ok().and_then(ids::string_of).unwrap_or_else(|| id.to_string());
        let body = self.items(&format!(
            "personIds={guid}&recursive=true&includeItemTypes=Movie,Series&sortBy=PremiereDate,SortName&sortOrder=Descending"
        ))?;
        let md = map::list(&body, &self.env);
        ok(json!({ "size": md.len(), "Metadata": md }))
    }

    /// `/library/streams/{n}[.ext]` — a sidecar subtitle file.
    fn subtitle(&self, n: &str) -> Answer {
        let num = n.split('.').next().unwrap_or(n);
        let Some(r) = num.parse::<i64>().ok().and_then(ids::sub_of) else { return not_found() };
        let fmt = match r.codec.as_str() {
            "ass" | "ssa" => "ass",
            "vtt" => "vtt",
            _ => "srt",
        };
        let (status, body) = self.conn.get_bytes(&format!(
            "/Videos/{}/{}/Subtitles/{}/0/Stream.{fmt}",
            r.item, r.media_source, r.index
        ))?;
        Some((i32::from(status), body))
    }

    // ---- playback ----

    /// The rating key a playback request names, from `uri=server://…/library/metadata/{rk}`,
    /// `key=/library/metadata/{rk}` or `path=/library/metadata/{rk}`.
    fn rating_key_param(&self) -> String {
        for k in ["uri", "key", "path"] {
            let v = self.q.str(k);
            if let Some(i) = v.find("/library/metadata/") {
                let rest = &v[i + "/library/metadata/".len()..];
                return rest.split(['/', '?', '&']).next().unwrap_or("").to_string();
            }
        }
        self.q.str("ratingKey").to_string()
    }

    /// `POST /playQueues` — the item, followed (for an episode) by the rest of its show, so the
    /// player's Up Next has somewhere to go.
    fn play_queue(&self) -> Answer {
        let rk = self.rating_key_param();
        let first = self.detail(&rk)?;
        let mut queue = vec![first];
        if queue[0]["type"] == "episode" {
            let series = queue[0]["grandparentRatingKey"].as_str().unwrap_or("").to_string();
            if !series.is_empty() {
                if let Some(body) = self.conn.get_json(&format!(
                    "/Shows/{series}/Episodes?userId={}&startItemId={rk}&limit=25&fields={LIST_FIELDS}&{IMAGE_FIELDS}",
                    self.uid()
                )) {
                    queue.extend(map::list(&body, &self.env).into_iter().filter(|m| m["ratingKey"] != rk.as_str()));
                }
            }
        }
        for (i, m) in queue.iter_mut().enumerate() {
            m["playQueueItemID"] = json!(i as i64 + 1);
        }
        let n = queue.len();
        ok(json!({
            "playQueueID": 1,
            "playQueueSelectedItemID": 1,
            "playQueueSelectedItemOffset": 0,
            "playQueueTotalCount": n,
            "size": n,
            "Metadata": queue,
        }))
    }

    /// The universal-transcoder DECISION, answered as Direct Play. This backend only direct-plays:
    /// the app's own gates (codec, container, device limits) have already run before it asks, so
    /// "directplay" here means "the server will serve the file as it is", which Jellyfin always will.
    fn decision(&self) -> Answer {
        let rk = self.rating_key_param();
        let mut md = self.detail(&rk)?;
        let media_index = self.q.int("mediaIndex").unwrap_or(0).max(0) as usize;
        if let Some(media) = md["Media"].as_array_mut() {
            if media_index < media.len() && media_index > 0 {
                media.swap(0, media_index);
            }
            media.truncate(1);
            if let Some(part) = media.get_mut(0).and_then(|m| m["Part"].get_mut(0)) {
                part["decision"] = json!("directplay");
                if let Some(streams) = part["Stream"].as_array_mut() {
                    for st in streams {
                        st["decision"] = json!("directplay");
                    }
                }
            }
        }
        // `directPlay=0` is the universal transcoder's own decision (the app has already ruled
        // direct play out): answer with what Jellyfin's transcode will send.
        if self.q.str("directPlay") == "0" {
            super::transcode::decision_body(
                &mut md,
                self.q.flag("directStream"),
                self.q.int("audioStreamID").unwrap_or(0),
            );
            return ok(json!({
                "generalDecisionCode": 1001,
                "generalDecisionText": "Transcode OK.",
                "size": 1,
                "Metadata": [md],
            }));
        }
        ok(json!({
            "generalDecisionCode": 1000,
            "generalDecisionText": "Direct play OK.",
            "mdeDecisionCode": 1000,
            "size": 1,
            "Metadata": [md],
        }))
    }

    /// `/:/timeline` → Jellyfin's playback sessions. The first report of a session opens it
    /// (`/Sessions/Playing`); `state=stopped` closes it, which is also what marks an item watched
    /// past the server's threshold.
    fn timeline(&self) -> Answer {
        let rk = self.q.str("ratingKey").to_string();
        if rk.is_empty() {
            return ok_bare();
        }
        let session = match self.q.str("X-Plex-Session-Identifier") {
            "" => format!("jfnative-{rk}"),
            s => s.to_string(),
        };
        let ms = self.q.int("time").unwrap_or(0).max(0);
        let ticks = ms * 10_000;
        let state = self.q.str("state");
        // Past 90% Jellyfin counts the item played and clears its position; remembering one there
        // would bring back a Resume the server has rightly dropped.
        let near_end = super::runtimes()
            .lock()
            .ok()
            .and_then(|r| r.get(&rk).copied())
            .is_some_and(|run| run > 0 && ms * 10 > run * 9);
        if near_end {
            super::forget_position(&rk);
        } else if ms > 0 {
            super::note_position(&rk, ms);
        }
        let body = |paused: bool| {
            json!({
                "ItemId": rk,
                "PlaySessionId": session,
                "PositionTicks": ticks,
                "IsPaused": paused,
                "CanSeek": true,
                "PlayMethod": "DirectPlay",
            })
        };
        if state == "stopped" {
            log(&format!("jellyfin: report stopped playSessionId={session} at {}s", ms / 1000));
            started().lock().map(|mut s| s.remove(&session)).ok();
            self.conn.post("/Sessions/Playing/Stopped", Some(&body(false)))?;
            return ok_bare();
        }
        let fresh = started().lock().map(|mut s| s.insert(session.clone())).unwrap_or(false);
        if fresh {
            self.conn.post("/Sessions/Playing", Some(&body(state == "paused")))?;
        }
        let mut b = body(state == "paused");
        b["EventName"] = json!("TimeUpdate");
        self.conn.post("/Sessions/Playing/Progress", Some(&b))?;
        ok_bare()
    }

    /// Stop a transcode the player has finished with, so the server does not keep encoding.
    fn stop_transcode(&self) -> Answer {
        let session = match self.q.str("session") {
            "" => self.q.str("X-Plex-Session-Identifier"),
            s => s,
        };
        if !session.is_empty() {
            log(&format!("jellyfin: stop encoder playSessionId={session}"));
            self.conn.delete(&format!(
                "/Videos/ActiveEncodings?deviceId={}&playSessionId={}",
                enc(&super::api::device_id()),
                enc(session)
            ));
        }
        ok_bare()
    }

    fn played(&self, played: bool) -> Answer {
        let id = self.q.str("key");
        if id.is_empty() {
            return not_found();
        }
        super::forget_position(id);
        let u = self.uid();
        let new = format!("/UserPlayedItems/{id}?userId={u}");
        let old = format!("/Users/{u}/PlayedItems/{id}");
        let status = if played {
            self.conn.post(&new, None).filter(|s| *s != 404).or_else(|| self.conn.post(&old, None))?
        } else {
            self.conn.delete(&new).filter(|s| *s != 404).or_else(|| self.conn.delete(&old))?
        };
        Some((i32::from(status), Vec::new()))
    }

    fn remove_from_resume(&self) -> Answer {
        let id = self.q.str("ratingKey");
        if id.is_empty() {
            return not_found();
        }
        super::forget_position(id);
        let status = self.conn.post(
            &format!("/UserItems/{id}/UserData?userId={}", self.uid()),
            Some(&json!({ "PlaybackPositionTicks": 0 })),
        )?;
        Some((i32::from(status), Vec::new()))
    }
}

/// Playback sessions already announced to the server.
fn started() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}
