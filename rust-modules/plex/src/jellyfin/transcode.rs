//! Server transcoding: what the app asks Plex's universal transcoder for, asked of Jellyfin.
//!
//! The player streams a transcode URL itself (it never passes through [`super::handle`]), so the
//! URL has to be Jellyfin's own: progressive `/Videos/{id}/stream.mkv` — Plex's `start.mkv`, a
//! stream restarted at an offset on every seek — or `/Videos/{id}/main.m3u8` for the fixed-segment
//! HLS that Auto quality falls back to on a slow link. The output codecs are the ones the app's
//! direct-play profile lists (H.264/HEVC video, AAC/AC-3/E-AC-3 audio), so the Load payload
//! [`decision_codecs`] promises is the one Jellyfin sends.

use serde_json::{json, Value};

use super::api;
use super::query::enc;
use crate::plex::{Origin, TranscodeDelivery, TranscodeOffset, TranscodeSpec};

/// Audio the television decodes as-is; anything else is re-encoded to AAC.
const AUDIO_OUT: &[&str] = &["aac", "ac3", "eac3"];
/// Video a remux may carry through untouched.
const VIDEO_COPY: &[&str] = &["h264", "hevc"];

/// The path (with query and token) of the Jellyfin stream for `spec`.
pub fn path(origin: &Origin, spec: &TranscodeSpec, token: &str) -> String {
    let id = spec.rating_key;
    let hls = matches!(spec.contract.delivery, TranscodeDelivery::FixedHls { .. });
    let copy_video = spec.contract.remux && !spec.contract.no_video_copy;
    let mut q: Vec<(&str, String)> = vec![
        ("MediaSourceId", id.to_string()),
        ("PlaySessionId", spec.session.to_string()),
        ("DeviceId", api::device_id()),
        ("VideoCodec", if copy_video { "h264,hevc".into() } else { "h264".into() }),
        ("AllowVideoStreamCopy", copy_video.to_string()),
        ("AudioCodec", AUDIO_OUT.join(",")),
        // An audio enhancement (dialogue boost, loudness) is Plex's; Jellyfin has none to apply,
        // so copying a decodable track is always right.
        ("AllowAudioStreamCopy", "true".into()),
        ("TranscodingMaxAudioChannels", "6".into()),
    ];
    if let Some(c) = spec.contract.ceiling {
        q.push(("MaxStreamingBitrate", (c.max_kbps * 1000).to_string()));
        q.push(("VideoBitrate", (c.max_kbps * 1000).to_string()));
        q.push(("MaxWidth", c.max_w.to_string()));
        q.push(("MaxHeight", c.max_h.to_string()));
    }
    // Stream ids on this backend are the Jellyfin stream index + 1 (`map::stream`).
    if spec.audio_stream_id > 0 {
        q.push(("AudioStreamIndex", (spec.audio_stream_id - 1).to_string()));
    }
    if spec.subtitle_stream_id > 0 && !spec.client_subtitles {
        q.push(("SubtitleStreamIndex", (spec.subtitle_stream_id - 1).to_string()));
        q.push(("SubtitleMethod", "Encode".into()));
    }
    if let TranscodeOffset::AtMicros(us) = spec.offset {
        q.push(("StartTimeTicks", (us.max(0) * 10).to_string()));
    }
    if let TranscodeDelivery::FixedHls { seconds_per_segment } = spec.contract.delivery {
        q.push(("SegmentContainer", "ts".into()));
        q.push(("SegmentLength", seconds_per_segment.max(1).to_string()));
        q.push(("MinSegments", "1".into()));
        q.push(("BreakOnNonKeyFrames", "true".into()));
    }
    //  is what Jellyfin 12 reads; 10.x also reads . Unknown ones are ignored.
    q.push(("ApiKey", token.to_string()));
    q.push(("api_key", token.to_string()));
    let query: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect();
    let endpoint = if hls { "main.m3u8" } else { "stream.mkv" };
    format!("{}/Videos/{id}/{endpoint}?{}", api::path_prefix(origin), query.join("&"))
}

/// Rewrite a mapped item into the `/decision` body for a TRANSCODE request: the part marked
/// `transcode`, and the selected audio stream first, carrying the codecs Jellyfin will output —
/// which is what the player's Load payload is built from.
pub fn decision_body(md: &mut Value, copy_video: bool, audio_stream_id: i64) {
    let Some(part) = md["Media"].get_mut(0).and_then(|m| m["Part"].get_mut(0)) else { return };
    part["decision"] = json!("transcode");
    let Some(streams) = part["Stream"].as_array_mut() else { return };
    let chosen = streams
        .iter()
        .position(|s| s["streamType"] == 2 && s["id"].as_i64() == Some(audio_stream_id))
        .or_else(|| streams.iter().position(|s| s["streamType"] == 2 && s["selected"] == 1))
        .or_else(|| streams.iter().position(|s| s["streamType"] == 2));
    if let Some(i) = chosen {
        let s = streams.remove(i);
        streams.insert(0, s);
    }
    for s in streams.iter_mut() {
        let codec = s["codec"].as_str().unwrap_or("").to_string();
        match s["streamType"].as_i64() {
            Some(1) => {
                let keep = copy_video && VIDEO_COPY.contains(&codec.as_str());
                s["decision"] = json!(if keep { "copy" } else { "transcode" });
                if !keep {
                    s["codec"] = json!("h264");
                }
            }
            Some(2) => {
                let keep = AUDIO_OUT.contains(&codec.as_str());
                s["decision"] = json!(if keep { "copy" } else { "transcode" });
                if !keep {
                    s["codec"] = json!("aac");
                }
            }
            _ => {}
        }
    }
}
