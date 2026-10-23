//! Jellyfin API client. Blocking: call it off the main thread (ui::run does).

use crate::http::{self, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::collections::HashMap;

pub const CLIENT_NAME: &str = "Jelly";
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TICKS_PER_SECOND: f64 = 10_000_000.0;

const ITEM_FIELDS: &str = "Overview,Genres,ProductionYear,PremiereDate,RunTimeTicks,ChildCount,OfficialRating,CommunityRating";
/// With the video and audio streams, for the badges on an item's page
const ITEM_FIELDS_STREAMS: &str = "Overview,Genres,ProductionYear,PremiereDate,RunTimeTicks,ChildCount,OfficialRating,CommunityRating,MediaStreams";

#[derive(Debug, Clone)]
pub enum Error {
    /// The session is no longer valid: sign in again
    Unauthorized,
    Http(u16, String),
    Network(String),
    Decode(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "Not signed in"),
            Error::Http(code, msg) if msg.is_empty() => write!(f, "Server error {code}"),
            Error::Http(code, msg) => write!(f, "Server error {code}: {msg}"),
            Error::Network(msg) => write!(f, "Can't reach the server: {msg}"),
            Error::Decode(msg) => write!(f, "Unexpected reply from the server: {msg}"),
        }
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Error {
        Error::Network(http::problem(&e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The reply, if it's a success; else what went wrong
fn check(mut response: Response) -> Result<Response> {
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 => Err(Error::Unauthorized),
        code => {
            let body = response.body_mut().read_to_string().unwrap_or_default();
            Err(Error::Http(code, body.chars().take(200).collect()))
        }
    }
}

fn read_json<T: DeserializeOwned>(response: Response) -> Result<T> {
    http::read_json(response).map_err(|e| Error::Decode(e.to_string()))
}

/// Jellyfin sends `null` for some lists; treat it like a missing field.
fn null_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(
    d: D,
) -> std::result::Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
    pub collection_type: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub index_number: Option<i32>,
    pub parent_index_number: Option<i32>,
    pub production_year: Option<i32>,
    pub premiere_date: Option<String>,
    pub overview: Option<String>,
    pub run_time_ticks: Option<i64>,
    pub official_rating: Option<String>,
    pub community_rating: Option<f32>,
    pub child_count: Option<i32>,
    #[serde(deserialize_with = "null_default")]
    pub genres: Vec<String>,
    #[serde(deserialize_with = "null_default")]
    pub image_tags: HashMap<String, String>,
    #[serde(deserialize_with = "null_default")]
    pub backdrop_image_tags: Vec<String>,
    pub parent_backdrop_item_id: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub parent_backdrop_image_tags: Vec<String>,
    pub parent_thumb_item_id: Option<String>,
    pub parent_thumb_image_tag: Option<String>,
    pub series_primary_image_tag: Option<String>,
    pub user_data: Option<UserData>,
    #[serde(deserialize_with = "null_default")]
    pub taglines: Vec<String>,
    #[serde(deserialize_with = "null_default")]
    pub people: Vec<Person>,
    /// Only in single-item replies, or with ITEM_FIELDS_STREAMS
    #[serde(deserialize_with = "null_default")]
    pub media_streams: Vec<MediaStream>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    /// Actor, Director, Writer, Producer…
    #[serde(rename = "Type")]
    pub kind: String,
    pub primary_image_tag: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserData {
    pub playback_position_ticks: i64,
    pub played: bool,
    pub played_percentage: Option<f64>,
    pub unplayed_item_count: Option<i32>,
    pub last_played_date: Option<String>,
}

impl Item {
    pub fn is_folder_like(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "Series" | "Season" | "BoxSet" | "Folder" | "CollectionFolder"
        )
    }

    pub fn resume_seconds(&self) -> f64 {
        self.user_data
            .as_ref()
            .map_or(0, |u| u.playback_position_ticks) as f64
            / TICKS_PER_SECOND
    }

    pub fn played(&self) -> bool {
        self.user_data.as_ref().is_some_and(|u| u.played)
    }

    /// 0..1, how far into it the user is
    pub fn progress(&self) -> f64 {
        match (self.user_data.as_ref(), self.run_time_ticks) {
            (Some(u), Some(total)) if total > 0 && u.playback_position_ticks > 0 => {
                (u.playback_position_ticks as f64 / total as f64).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    /// When it was last played (ISO 8601 in UTC, so they compare as text)
    pub fn last_played(&self) -> Option<&str> {
        self.user_data.as_ref()?.last_played_date.as_deref()
    }

    pub fn runtime_minutes(&self) -> Option<i64> {
        self.run_time_ticks
            .map(|t| (t as f64 / TICKS_PER_SECOND / 60.0).round() as i64)
    }

    /// "S1:E8" for episodes
    pub fn episode_code(&self) -> Option<String> {
        match (self.parent_index_number, self.index_number) {
            (Some(s), Some(e)) => Some(format!("S{s}:E{e}")),
            (None, Some(e)) => Some(format!("E{e}")),
            _ => None,
        }
    }

    /// The best wide (16:9) image: (item id, image type, tag)
    pub fn thumb_image(&self) -> Option<(String, &'static str, String)> {
        if self.kind == "Episode"
            && let Some(tag) = self.image_tags.get("Primary")
        {
            return Some((self.id.clone(), "Primary", tag.clone()));
        }
        if let Some(tag) = self.image_tags.get("Thumb") {
            return Some((self.id.clone(), "Thumb", tag.clone()));
        }
        if let (Some(id), Some(tag)) = (&self.parent_thumb_item_id, &self.parent_thumb_image_tag) {
            return Some((id.clone(), "Thumb", tag.clone()));
        }
        self.backdrop_image()
    }

    pub fn backdrop_image(&self) -> Option<(String, &'static str, String)> {
        if let Some(tag) = self.backdrop_image_tags.first() {
            return Some((self.id.clone(), "Backdrop", tag.clone()));
        }
        if let (Some(id), Some(tag)) = (
            &self.parent_backdrop_item_id,
            self.parent_backdrop_image_tags.first(),
        ) {
            return Some((id.clone(), "Backdrop", tag.clone()));
        }
        None
    }

    /// The poster (2:3) image; for episodes, the series' poster
    pub fn poster_image(&self) -> Option<(String, &'static str, String)> {
        if (self.kind == "Episode" || self.kind == "Season")
            && let (Some(id), Some(tag)) = (&self.series_id, &self.series_primary_image_tag)
        {
            return Some((id.clone(), "Primary", tag.clone()));
        }
        self.image_tags
            .get("Primary")
            .map(|tag| (self.id.clone(), "Primary", tag.clone()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ItemList {
    #[serde(deserialize_with = "null_default", default)]
    items: Vec<Item>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthResult {
    pub access_token: String,
    pub user: User,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuickConnect {
    pub authenticated: bool,
    pub secret: String,
    pub code: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PlaybackInfo {
    #[serde(deserialize_with = "null_default")]
    pub media_sources: Vec<MediaSource>,
    pub play_session_id: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaSource {
    pub id: String,
    pub container: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub media_streams: Vec<MediaStream>,
    pub default_audio_stream_index: Option<i32>,
    pub default_subtitle_stream_index: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaStream {
    pub index: i32,
    #[serde(rename = "Type")]
    pub kind: String,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub display_title: Option<String>,
    pub title: Option<String>,
    pub is_external: bool,
    pub is_forced: bool,
    pub is_default: bool,
    pub delivery_url: Option<String>,
    pub profile: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// SDR, HDR10, HDR10Plus, HLG, DOVI, DOVIWithHDR10…
    pub video_range_type: Option<String>,
    pub channels: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Segment {
    #[serde(rename = "Type")]
    pub kind: String,
    pub start_ticks: i64,
    pub end_ticks: i64,
}

impl Segment {
    pub fn start(&self) -> f64 {
        self.start_ticks as f64 / TICKS_PER_SECOND
    }
    pub fn end(&self) -> f64 {
        self.end_ticks as f64 / TICKS_PER_SECOND
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Report {
    Start,
    Progress,
    Stopped,
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    pub server: String,
    device_id: String,
    device_name: String,
    token: Option<String>,
    pub user_id: String,
}

/// "stream.example.com/" → "https://stream.example.com". An IP address, a
/// port or a name without dots ("192.168.1.5", "nas:8096") is a server at
/// home, likely without HTTPS: those get http.
pub fn normalize_server(input: &str) -> String {
    let s = input.trim().trim_end_matches('/');
    if s.contains("://") {
        return s.to_string();
    }
    let authority = s.split('/').next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, true)
        }
        _ => (authority, false),
    };
    let ip = host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok();
    let home = ip || port || !host.contains('.') || host.ends_with(".local");
    format!("{}://{s}", if home { "http" } else { "https" })
}

impl Client {
    pub fn new(server: &str, device_id: &str) -> Client {
        let agent = http::agent(None);
        let device_name = gtk::glib::host_name().to_string();
        Client {
            agent,
            server: normalize_server(server),
            device_id: device_id.to_string(),
            device_name,
            token: None,
            user_id: String::new(),
        }
    }

    pub fn signed_in(&self, token: &str, user_id: &str) -> Client {
        Client {
            token: Some(token.to_string()),
            user_id: user_id.to_string(),
            ..self.clone()
        }
    }

    /// The same, giving up on each call after `limit` (completing the command
    /// line mustn't hang)
    pub fn time_limit(self, limit: std::time::Duration) -> Client {
        Client {
            agent: http::agent(Some(limit)),
            ..self
        }
    }

    fn auth_header(&self) -> String {
        let mut h = format!(
            r#"MediaBrowser Client="{CLIENT_NAME}", Device="{}", DeviceId="{}", Version="{CLIENT_VERSION}""#,
            self.device_name.replace('"', ""),
            self.device_id
        );
        if let Some(token) = &self.token {
            h.push_str(&format!(r#", Token="{token}""#));
        }
        h
    }

    /// An HTTP header for mpv's own requests (no commas: mpv splits lists on them)
    pub fn mpv_header(&self) -> String {
        format!(
            r#"Authorization: MediaBrowser Token="{}""#,
            self.token.as_deref().unwrap_or("")
        )
    }

    /// `request` to `path` on the server, with the sign-in and `query`
    fn prepare<B>(
        &self,
        request: ureq::RequestBuilder<B>,
        query: &[(&str, &str)],
    ) -> ureq::RequestBuilder<B> {
        let request = request.header("Authorization", &self.auth_header());
        query
            .iter()
            .fold(request, |request, (k, v)| request.query(*k, *v))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let response = self.prepare(self.agent.get(self.url(path)), query).call()?;
        read_json(check(response)?)
    }

    fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        body: Value,
    ) -> Result<T> {
        let request = self.prepare(self.agent.post(self.url(path)), query);
        read_json(check(request.send_json(body)?)?)
    }

    /// POST `body`, if any, when the reply doesn't matter
    fn send(&self, path: &str, query: &[(&str, &str)], body: Option<Value>) -> Result<()> {
        let request = self.prepare(self.agent.post(self.url(path)), query);
        let response = match body {
            Some(body) => request.send_json(body)?,
            None => request.send_empty()?,
        };
        check(response).map(drop)
    }

    fn delete(&self, path: &str, query: &[(&str, &str)]) -> Result<()> {
        let response = self
            .prepare(self.agent.delete(self.url(path)), query)
            .call()?;
        check(response).map(drop)
    }

    /// Check that the address is a Jellyfin server
    pub fn check_server(&self) -> Result<()> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct PublicInfo {
            #[allow(dead_code)]
            id: String,
        }
        self.get::<PublicInfo>("/System/Info/Public", &[])
            .map(|_| ())
    }

    pub fn authenticate(&self, username: &str, password: &str) -> Result<AuthResult> {
        self.post(
            "/Users/AuthenticateByName",
            &[],
            json!({ "Username": username, "Pw": password }),
        )
    }

    pub fn quick_connect_enabled(&self) -> Result<bool> {
        self.get("/QuickConnect/Enabled", &[])
    }

    pub fn quick_connect_start(&self) -> Result<QuickConnect> {
        self.post("/QuickConnect/Initiate", &[], json!({}))
    }

    pub fn quick_connect_state(&self, secret: &str) -> Result<QuickConnect> {
        self.get("/QuickConnect/Connect", &[("secret", secret)])
    }

    pub fn quick_connect_finish(&self, secret: &str) -> Result<AuthResult> {
        self.post(
            "/Users/AuthenticateWithQuickConnect",
            &[],
            json!({ "Secret": secret }),
        )
    }

    pub fn logout(&self) -> Result<()> {
        self.send("/Sessions/Logout", &[], None)
    }

    pub fn views(&self) -> Result<Vec<Item>> {
        let list: ItemList = self.get("/UserViews", &[("userId", &self.user_id)])?;
        Ok(list.items)
    }

    pub fn resume(&self) -> Result<Vec<Item>> {
        let list: ItemList = self.get(
            "/UserItems/Resume",
            &[
                ("userId", &self.user_id),
                ("limit", "16"),
                ("mediaTypes", "Video"),
                ("fields", ITEM_FIELDS),
                ("enableImageTypes", "Primary,Backdrop,Thumb"),
            ],
        )?;
        Ok(list.items)
    }

    pub fn next_up(&self, series_id: Option<&str>) -> Result<Vec<Item>> {
        let mut query = vec![
            ("userId", self.user_id.as_str()),
            ("limit", "16"),
            ("enableImageTypes", "Primary,Backdrop,Thumb"),
        ];
        match series_id {
            // For a series page: the episode to continue, resumable or not
            Some(id) => query.extend([
                ("seriesId", id),
                ("enableResumable", "true"),
                ("fields", ITEM_FIELDS_STREAMS),
            ]),
            // Home: Continue Watching already lists the resumable ones
            None => query.extend([("enableResumable", "false"), ("fields", ITEM_FIELDS)]),
        }
        let list: ItemList = self.get("/Shows/NextUp", &query)?;
        Ok(list.items)
    }

    /// The movie or episode most recently watched to the end
    fn last_finished(&self) -> Result<Option<Item>> {
        let list: ItemList = self.get(
            "/Items",
            &[
                ("userId", &self.user_id),
                ("recursive", "true"),
                ("includeItemTypes", "Movie,Episode"),
                ("isPlayed", "true"),
                ("sortBy", "DatePlayed"),
                ("sortOrder", "Descending"),
                ("limit", "1"),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items.into_iter().next())
    }

    /// What "continue watching" plays: the last thing watched, from where it
    /// stopped; but if that was an episode watched to the end, its show's next
    /// episode. With neither, the first of Next Up.
    pub fn what_to_continue(&self) -> Result<Option<Item>> {
        let stopped = self.resume()?.into_iter().next();
        if let Some(finished) = self.last_finished()?.filter(|f| f.kind == "Episode") {
            let newer = stopped
                .as_ref()
                .is_none_or(|s| finished.last_played() > s.last_played());
            if let (true, Some(series)) = (newer, &finished.series_id)
                && let Some(next) = self.next_up(Some(series))?.into_iter().next()
            {
                return Ok(Some(next));
            }
        }
        if stopped.is_some() {
            return Ok(stopped);
        }
        Ok(self.next_up(None)?.into_iter().next())
    }

    pub fn latest(&self, parent_id: &str) -> Result<Vec<Item>> {
        self.get(
            "/Items/Latest",
            &[
                ("userId", &self.user_id),
                ("parentId", parent_id),
                ("limit", "16"),
                ("fields", ITEM_FIELDS),
            ],
        )
    }

    /// Everything in a library, A to Z
    pub fn library(&self, view: &Item) -> Result<Vec<Item>> {
        let types = match view.collection_type.as_deref() {
            Some("movies") => "Movie",
            Some("tvshows") => "Series",
            Some("boxsets") => "BoxSet",
            _ => "Movie,Series",
        };
        let list: ItemList = self.get(
            "/Items",
            &[
                ("userId", &self.user_id),
                ("parentId", &view.id),
                ("includeItemTypes", types),
                ("recursive", "true"),
                ("sortBy", "SortName"),
                ("sortOrder", "Ascending"),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items)
    }

    pub fn search(&self, term: &str) -> Result<Vec<Item>> {
        self.search_for(term, "Movie,Series,Episode")
    }

    /// Only movies and shows (episodes otherwise come first)
    pub fn search_titles(&self, term: &str) -> Result<Vec<Item>> {
        self.search_for(term, "Movie,Series")
    }

    fn search_for(&self, term: &str, kinds: &str) -> Result<Vec<Item>> {
        let list: ItemList = self.get(
            "/Items",
            &[
                ("userId", &self.user_id),
                ("searchTerm", term),
                ("includeItemTypes", kinds),
                ("recursive", "true"),
                ("limit", "60"),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items)
    }

    pub fn children(&self, parent_id: &str) -> Result<Vec<Item>> {
        let list: ItemList = self.get(
            "/Items",
            &[
                ("userId", &self.user_id),
                ("parentId", parent_id),
                ("sortBy", "SortName"),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items)
    }

    pub fn item(&self, id: &str) -> Result<Item> {
        self.get(&format!("/Items/{id}"), &[("userId", &self.user_id)])
    }

    /// Movies and series like `id`, from the server's own matching
    pub fn similar(&self, id: &str) -> Result<Vec<Item>> {
        let list: ItemList = self.get(
            &format!("/Items/{id}/Similar"),
            &[
                ("userId", &self.user_id),
                ("limit", "16"),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items)
    }

    pub fn seasons(&self, series_id: &str) -> Result<Vec<Item>> {
        let list: ItemList = self.get(
            &format!("/Shows/{series_id}/Seasons"),
            &[("userId", &self.user_id), ("fields", ITEM_FIELDS)],
        )?;
        Ok(list.items)
    }

    pub fn episodes(&self, series_id: &str, season_id: &str) -> Result<Vec<Item>> {
        let list: ItemList = self.get(
            &format!("/Shows/{series_id}/Episodes"),
            &[
                ("userId", &self.user_id),
                ("seasonId", season_id),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items)
    }

    /// The episode after `episode` (across seasons), if any
    pub fn next_episode(&self, episode: &Item) -> Result<Option<Item>> {
        let Some(series) = &episode.series_id else {
            return Ok(None);
        };
        let list: ItemList = self.get(
            &format!("/Shows/{series}/Episodes"),
            &[
                ("userId", &self.user_id),
                ("startItemId", &episode.id),
                ("limit", "2"),
                ("fields", ITEM_FIELDS),
            ],
        )?;
        Ok(list.items.into_iter().find(|i| i.id != episode.id))
    }

    /// Ask for direct play of everything: mpv plays any container and codec,
    /// so the server never transcodes (many home servers can't keep up).
    pub fn playback_info(&self, item_id: &str) -> Result<PlaybackInfo> {
        let mut subtitles = Vec::new();
        for format in [
            "ass", "ssa", "srt", "subrip", "vtt", "webvtt", "pgssub", "dvdsub", "dvbsub",
            "mov_text", "sub", "smi", "ttml",
        ] {
            subtitles.push(json!({ "Format": format, "Method": "Embed" }));
        }
        for format in [
            "ass", "ssa", "srt", "subrip", "vtt", "webvtt", "sub", "smi", "ttml", "pgssub",
        ] {
            subtitles.push(json!({ "Format": format, "Method": "External" }));
        }
        let body = json!({
            "UserId": self.user_id,
            "MaxStreamingBitrate": 1_000_000_000,
            "EnableDirectPlay": true,
            "EnableDirectStream": true,
            "EnableTranscoding": false,
            "AutoOpenLiveStream": true,
            "DeviceProfile": {
                "Name": "Jelly (mpv)",
                "MaxStreamingBitrate": 1_000_000_000,
                "MaxStaticBitrate": 1_000_000_000,
                "DirectPlayProfiles": [ { "Type": "Video" }, { "Type": "Audio" } ],
                "TranscodingProfiles": [],
                "SubtitleProfiles": subtitles,
            },
        });
        self.post(
            &format!("/Items/{item_id}/PlaybackInfo"),
            &[("userId", &self.user_id)],
            body,
        )
    }

    pub fn segments(&self, item_id: &str) -> Result<Vec<Segment>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct List {
            #[serde(deserialize_with = "null_default", default)]
            items: Vec<Segment>,
        }
        let list: List = self.get(&format!("/MediaSegments/{item_id}"), &[])?;
        Ok(list.items)
    }

    pub fn report(&self, kind: Report, body: Value) -> Result<()> {
        let path = match kind {
            Report::Start => "/Sessions/Playing",
            Report::Progress => "/Sessions/Playing/Progress",
            Report::Stopped => "/Sessions/Playing/Stopped",
        };
        self.send(path, &[], Some(body))
    }

    pub fn set_played(&self, item_id: &str, played: bool) -> Result<()> {
        let path = format!("/UserPlayedItems/{item_id}");
        let query = [("userId", self.user_id.as_str())];
        if played {
            self.send(&path, &query, None)
        } else {
            self.delete(&path, &query)
        }
    }

    pub fn image_url(&self, item_id: &str, kind: &str, tag: &str, width: u32) -> String {
        format!(
            "{}/Items/{item_id}/Images/{kind}?fillWidth={width}&quality=90&tag={tag}",
            self.server
        )
    }

    /// A small, blurred copy of an image, for washes of its colours
    pub fn blurred_image_url(&self, item_id: &str, kind: &str, tag: &str) -> String {
        format!(
            "{}/Items/{item_id}/Images/{kind}?fillWidth=160&blur=8&quality=80&tag={tag}",
            self.server
        )
    }

    pub fn stream_url(
        &self,
        item_id: &str,
        source: &MediaSource,
        play_session_id: Option<&str>,
    ) -> String {
        let mut url = format!(
            "{}/Videos/{item_id}/stream?static=true&mediaSourceId={}&deviceId={}",
            self.server, source.id, self.device_id
        );
        if let Some(id) = play_session_id {
            url.push_str(&format!("&playSessionId={id}"));
        }
        url
    }

    pub fn subtitle_url(
        &self,
        item_id: &str,
        source: &MediaSource,
        stream: &MediaStream,
    ) -> String {
        if let Some(url) = &stream.delivery_url {
            if url.starts_with("http") {
                return url.clone();
            }
            return format!("{}{url}", self.server);
        }
        let ext = match stream.codec.as_deref() {
            Some("subrip") | None => "srt",
            Some("webvtt") => "vtt",
            Some(other) => other,
        };
        format!(
            "{}/Videos/{item_id}/{}/Subtitles/{}/0/Stream.{ext}",
            self.server, source.id, stream.index
        )
    }

    /// Fetch raw bytes (images)
    pub fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        let request = self
            .agent
            .get(url)
            .header("Authorization", &self.auth_header());
        http::read_image(check(request.call()?)?).map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_on_the_internet_get_https() {
        assert_eq!(
            normalize_server("stream.example.com/"),
            "https://stream.example.com"
        );
        assert_eq!(
            normalize_server("http://stream.example.com"),
            "http://stream.example.com"
        );
        assert_eq!(normalize_server("192.0.2.7:8096"), "http://192.0.2.7:8096");
        assert_eq!(normalize_server("192.168.1.5"), "http://192.168.1.5");
        assert_eq!(normalize_server("nas:8096"), "http://nas:8096");
        assert_eq!(normalize_server("jellyfin.local"), "http://jellyfin.local");
        assert_eq!(
            normalize_server("media.example.com:8920"),
            "http://media.example.com:8920"
        );
    }
}
