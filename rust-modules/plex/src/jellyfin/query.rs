//! Query strings: the app's Plex requests in, Jellyfin requests out.

/// A parsed query, in order, with values percent-decoded.
pub struct Query(Vec<(String, String)>);

impl Query {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn str(&self, k: &str) -> &str {
        self.get(k).unwrap_or("")
    }
    pub fn int(&self, k: &str) -> Option<i64> {
        self.get(k).and_then(|v| v.trim().parse().ok())
    }
    pub fn flag(&self, k: &str) -> bool {
        matches!(self.get(k), Some("1") | Some("true"))
    }
}

/// Split `path?query` into the path and its parsed query.
pub fn split(path_q: &str) -> (String, Query) {
    let (path, q) = path_q.split_once('?').unwrap_or((path_q, ""));
    let pairs = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    (decode(path), Query(pairs))
}

/// `%XX` and `+` decoding. Invalid escapes pass through as written.
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode one query value (RFC 3986 unreserved passthrough).
pub fn enc(s: &str) -> String {
    crate::plex::urlenc_str(s)
}
