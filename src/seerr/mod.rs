//! Seerr (Jellyseerr / Overseerr): finding movies and shows that aren't in
//! the library, and requesting them. Blocking: call it off the main thread.

use crate::http::{self, Response};
use gtk::glib;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

/// A movie's, show's or season's status in Seerr ("MediaStatus")
pub const PENDING: i32 = 2;
pub const PROCESSING: i32 = 3;
pub const PARTLY_AVAILABLE: i32 = 4;
pub const AVAILABLE: i32 = 5;
pub const BLOCKLISTED: i32 = 6;

/// What a status means to someone requesting, when it means anything
pub fn status_label(status: i32) -> Option<&'static str> {
    match status {
        PENDING => Some("Requested"),
        PROCESSING => Some("Processing"),
        PARTLY_AVAILABLE => Some("Partly available"),
        AVAILABLE => Some("Available"),
        BLOCKLISTED => Some("Blocklisted"),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    /// Not signed in to Seerr, or the sign-in expired
    SignIn,
    Server(String),
    Network(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::SignIn => write!(f, "Sign in to Seerr again"),
            Error::Server(msg) => write!(f, "{msg}"),
            Error::Network(msg) => write!(f, "Can't reach Seerr: {msg}"),
        }
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Error {
        Error::Network(http::problem(&e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What an error status means: Seerr explains in {"message": …} (or
/// {"error": …})
fn status_error(response: Response) -> Error {
    let code = response.status().as_u16();
    if code == 401 {
        return Error::SignIn;
    }
    let body: Value = http::read_json(response).unwrap_or_default();
    let message = body["message"]
        .as_str()
        .or(body["error"].as_str())
        .map(String::from);
    Error::Server(message.unwrap_or_else(|| format!("Seerr error {code}")))
}

/// A movie or show found by Seerr's search (from TMDB)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Found {
    /// TMDB's
    pub id: i64,
    /// movie, tv or person
    pub media_type: String,
    /// Movies have a title, shows a name
    pub title: Option<String>,
    pub name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
    pub media_info: Option<MediaInfo>,
    /// TMDB's: only in details (not search results)
    pub keywords: Vec<Keyword>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Keyword {
    pub name: String,
}

impl Found {
    /// Anime, as Seerr tells it (which also makes Sonarr use its anime settings)
    pub fn is_anime(&self) -> bool {
        self.keywords
            .iter()
            .any(|k| k.name.eq_ignore_ascii_case("anime"))
    }

    pub fn title(&self) -> &str {
        self.title
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or_default()
    }

    pub fn year(&self) -> Option<&str> {
        self.release_date
            .as_deref()
            .or(self.first_air_date.as_deref())
            .and_then(|d| d.get(..4))
    }

    pub fn is_show(&self) -> bool {
        self.media_type == "tv"
    }

    /// 0 when Seerr knows nothing of it yet
    pub fn status(&self) -> i32 {
        self.media_info.as_ref().map_or(0, |m| m.status)
    }

    pub fn poster_url(&self) -> Option<String> {
        self.poster_path
            .as_ref()
            .map(|path| format!("https://image.tmdb.org/t/p/w342{path}"))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaInfo {
    pub tmdb_id: i64,
    pub media_type: String,
    pub status: i32,
    /// The Jellyfin item it is, once it's in the library (all or partly)
    pub jellyfin_media_id: Option<String>,
    /// For shows: each season's status
    pub seasons: Vec<SeasonStatus>,
    pub requests: Vec<Request>,
    /// What Radarr/Sonarr are downloading for it, as of Seerr's last check
    /// (once a minute)
    pub download_status: Vec<Download>,
}

impl MediaInfo {
    /// How far along its downloads are, all together (0 to 1), if any
    pub fn download_progress(&self) -> Option<f64> {
        let size: f64 = self.download_status.iter().map(|d| d.size).sum();
        let done: f64 = self
            .download_status
            .iter()
            .map(|d| d.size - d.size_left)
            .sum();
        (size > 0.0).then(|| (done / size).clamp(0.0, 1.0))
    }

    /// When the last of its downloads should be done (ISO 8601)
    pub fn download_done_at(&self) -> Option<&str> {
        self.download_status
            .iter()
            .filter_map(|d| d.estimated_completion_time.as_deref())
            .max()
    }

    /// The downloads' state, the one that matters most first: "downloading"
    /// while any is, else "failed", "warning", "queued", "paused", "completed"…
    pub fn download_state(&self) -> Option<&str> {
        let states: Vec<&str> = self
            .download_status
            .iter()
            .map(|d| d.status.as_str())
            .collect();
        [
            "downloading",
            "failed",
            "warning",
            "queued",
            "delay",
            "paused",
            "completed",
        ]
        .into_iter()
        .find(|s| states.contains(s))
        .or(states.first().copied())
    }
}

/// One of Radarr's or Sonarr's downloads (a movie, an episode, a season)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Download {
    pub size: f64,
    pub size_left: f64,
    /// downloading, queued, paused, delay, completed, warning, failed…
    pub status: String,
    pub estimated_completion_time: Option<String>,
    /// The release's name ("… S03E01 1080p CR WEB-DL…")
    pub title: String,
    /// For a show's: which episode
    pub episode: Option<DownloadEpisode>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadEpisode {
    pub season_number: i32,
    pub episode_number: i32,
    pub title: Option<String>,
}

impl Download {
    pub fn fraction(&self) -> f64 {
        if self.size > 0.0 {
            ((self.size - self.size_left) / self.size).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// "Downloading · 43% · 2 min left"
    pub fn describe(&self) -> String {
        describe_download(
            &self.status,
            self.fraction(),
            self.estimated_completion_time.as_deref(),
        )
    }

    /// "S3 E1 · The title", for a show's
    pub fn episode_name(&self) -> Option<String> {
        let e = self.episode.as_ref()?;
        let number = format!("S{} E{}", e.season_number, e.episode_number);
        Some(match e.title.as_deref().filter(|t| !t.is_empty()) {
            Some(title) => format!("{number} · {title}"),
            None => number,
        })
    }
}

/// "Downloading · 43% · 12 min left", "Queued · 0%"…, for Radarr/Sonarr's
/// state of a download
fn describe_download(state: &str, fraction: f64, done_at: Option<&str>) -> String {
    let word = match state {
        "queued" | "delay" => "Queued",
        "paused" => "Paused",
        "completed" => "Downloaded, importing",
        "warning" => "Stalled",
        "failed" => "Download failed",
        _ => "Downloading",
    };
    let mut text = format!("{word} · {:.0}%", fraction * 100.0);
    let left = done_at.and_then(minutes_until).filter(|m| *m > 0);
    if let (Some(minutes), "Downloading") = (left, word) {
        text.push_str(&format!(" · {} left", format_minutes(minutes)));
    }
    text
}

/// A requested movie or show that isn't all in the library yet
#[derive(Debug, Clone)]
pub struct Underway {
    /// Its title, poster and so on, and its status and downloads
    pub found: Found,
    pub awaiting_approval: bool,
}

impl Underway {
    /// "Downloading · 43% · 12 min left", "Waiting for approval"…
    pub fn progress(&self) -> String {
        let media = self.found.media_info.clone().unwrap_or_default();
        if self.awaiting_approval {
            return "Waiting for approval".into();
        }
        let Some(fraction) = media.download_progress() else {
            // Approved and not downloading: being searched for, or imported
            // and waiting for the library to see it
            return if media.status == PARTLY_AVAILABLE {
                "Partly available"
            } else {
                "Processing"
            }
            .into();
        };
        describe_download(
            media.download_state().unwrap_or_default(),
            fraction,
            media.download_done_at(),
        )
    }

    /// Its page on Seerr's website
    pub fn url(&self, seerr: &str) -> String {
        format!("{seerr}/{}/{}", self.found.media_type, self.found.id)
    }
}

fn minutes_until(iso: &str) -> Option<i64> {
    let then = glib::DateTime::from_iso8601(iso, None).ok()?;
    Some(
        then.difference(&glib::DateTime::now_utc().ok()?)
            .as_minutes(),
    )
}

/// "12 min", "1 h 5 min", "2 d 3 h"
fn format_minutes(minutes: i64) -> String {
    match minutes {
        m if m < 60 => format!("{m} min"),
        m if m < 24 * 60 => format!("{} h {} min", m / 60, m % 60),
        m => format!("{} d {} h", m / (24 * 60), m / 60 % 24),
    }
}

/// Whether a request (its status: 1 waiting for approval, 2 approved, 3
/// declined…) for `media` is still on its way: not all there yet, or
/// downloading more (an airing show)
fn on_its_way(request_status: i32, media: &MediaInfo) -> bool {
    matches!(request_status, 1 | 2)
        && (matches!(media.status, PENDING | PROCESSING) || !media.download_status.is_empty())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SeasonStatus {
    pub season_number: i32,
    pub status: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Request {
    /// 1 waiting for approval, 2 approved, 3 declined, 4 failed, 5 done
    pub status: i32,
    pub seasons: Vec<SeasonStatus>,
}

/// A movie's or show's details: what it is, its seasons and cast, and where
/// it's at in Seerr
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Details {
    pub id: i64,
    /// movie or tv (from `Client::details`)
    #[serde(skip)]
    pub media_type: String,
    /// Movies have a title, shows a name
    pub title: Option<String>,
    pub name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub genres: Vec<Genre>,
    pub vote_average: Option<f64>,
    /// A movie's minutes
    pub runtime: Option<i32>,
    /// A show's episodes' minutes
    pub episode_run_time: Vec<i32>,
    pub next_episode_to_air: Option<NextEpisode>,
    pub seasons: Vec<Season>,
    pub credits: Credits,
    pub media_info: Option<MediaInfo>,
    pub keywords: Vec<Keyword>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Genre {
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Credits {
    pub cast: Vec<CastMember>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CastMember {
    pub name: String,
    pub character: Option<String>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NextEpisode {
    pub air_date: Option<String>,
    pub season_number: i32,
    pub episode_number: i32,
}

/// One of TMDB's images, `size` wide ("w342", "w1280"…)
pub fn tmdb_image(path: &str, size: &str) -> String {
    format!("https://image.tmdb.org/t/p/{size}{path}")
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Season {
    pub season_number: i32,
    pub name: Option<String>,
    pub episode_count: Option<i32>,
}

impl Season {
    /// Its name ("Season 2", "The Final Season"): TheTVDB's are just numbers
    pub fn title(&self) -> String {
        match self.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() && name.parse::<i32>().is_err() => name.to_string(),
            _ => format!("Season {}", self.season_number),
        }
    }
}

impl Details {
    /// As a search result, to request it
    pub fn found(&self) -> Found {
        Found {
            id: self.id,
            media_type: self.media_type.clone(),
            title: self.title.clone(),
            name: self.name.clone(),
            release_date: self.release_date.clone(),
            first_air_date: self.first_air_date.clone(),
            poster_path: self.poster_path.clone(),
            overview: self.overview.clone(),
            media_info: self.media_info.clone(),
            keywords: self.keywords.clone(),
        }
    }

    pub fn is_show(&self) -> bool {
        self.media_type == "tv"
    }

    /// Whether there's anything to request: a movie Seerr hasn't got
    /// requested, or a show's missing seasons
    pub fn can_request(&self) -> bool {
        if self.is_show() {
            !self.requestable().is_empty()
        } else {
            let media = self.media_info.as_ref();
            media.is_none_or(|m| {
                m.status <= 1 && !m.requests.iter().any(|r| matches!(r.status, 1 | 2))
            })
        }
    }

    /// "In your library", "Waiting for approval", "Downloading · 43%…"
    pub fn state(&self) -> String {
        let media = self.media_info.clone().unwrap_or_default();
        let wanted = media.requests.iter().any(|r| matches!(r.status, 1 | 2));
        match media.status {
            AVAILABLE => "In your library".into(),
            BLOCKLISTED => "Blocklisted".into(),
            _ if !wanted && media.download_status.is_empty() => if media.status == PARTLY_AVAILABLE
            {
                "Partly in your library"
            } else {
                "Not requested yet"
            }
            .into(),
            _ => {
                let awaiting = !media.requests.iter().any(|r| r.status == 2);
                Underway {
                    found: self.found(),
                    awaiting_approval: awaiting,
                }
                .progress()
            }
        }
    }

    /// "S3 E4", for the next episode, when its numbers fit the seasons shown
    /// (TMDB numbers it even when the seasons come from TheTVDB)
    pub fn next_episode_number(&self) -> Option<String> {
        let next = self.next_episode_to_air.as_ref()?;
        let season = self
            .seasons
            .iter()
            .find(|s| s.season_number == next.season_number)?;
        let fits = season
            .episode_count
            .is_none_or(|count| next.episode_number <= count);
        fits.then(|| format!("S{} E{}", next.season_number, next.episode_number))
    }

    /// Why a season can't be requested ("Available", "Requested"…), if it can't
    pub fn season_taken(&self, number: i32) -> Option<&'static str> {
        let info = self.media_info.as_ref()?;
        let status = info
            .seasons
            .iter()
            .find(|s| s.season_number == number)
            .map_or(0, |s| s.status);
        if let Some(label) = status_label(status) {
            return Some(label);
        }
        // Waiting for approval, or approved and not started: only on the requests
        let requested = info
            .requests
            .iter()
            .filter(|r| matches!(r.status, 1 | 2))
            .any(|r| r.seasons.iter().any(|s| s.season_number == number));
        requested.then_some("Requested")
    }

    /// The seasons there are to request (without specials)
    pub fn requestable(&self) -> Vec<i32> {
        self.seasons
            .iter()
            .map(|s| s.season_number)
            .filter(|&n| n > 0 && self.season_taken(n).is_none())
            .collect()
    }
}

/// Where Seerr might be for a Jellyfin server, likeliest first: for one at a
/// domain, requests.<domain> and the like; then the same machine, port 5055
pub fn candidates(jellyfin_server: &str) -> Vec<String> {
    let host = default_url(jellyfin_server)
        .trim_start_matches("http://")
        .trim_end_matches(":5055")
        .to_string();
    let mut found = Vec::new();
    let is_ip = host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok();
    if !is_ip && host.contains('.') {
        // stream.example.com → example.com; example.com stays
        let labels = host.split('.').count();
        let domain = if labels > 2 {
            host.split_once('.').map_or(host.as_str(), |(_, d)| d)
        } else {
            host.as_str()
        };
        for name in ["requests", "request", "seerr", "jellyseerr", "overseerr"] {
            found.push(format!("https://{name}.{domain}"));
        }
    }
    found.push(default_url(jellyfin_server));
    found
}

/// The first of `candidates` where Seerr answers
pub fn find(jellyfin_server: &str) -> Option<String> {
    let agent = http::agent(Some(Duration::from_secs(3)));
    candidates(jellyfin_server).into_iter().find(|url| {
        let status: Option<Value> = agent
            .get(&format!("{url}/api/v1/status"))
            .call()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| http::read_json(r).ok());
        status.is_some_and(|s| s["version"].is_string())
    })
}

/// Where Seerr usually is at home: on the Jellyfin server's machine, port 5055
pub fn default_url(jellyfin_server: &str) -> String {
    let authority = jellyfin_server
        .split_once("://")
        .map_or(jellyfin_server, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();
    // Without the port ("[::1]:8096" keeps its brackets)
    let host = match authority.rfind(':') {
        Some(i) if !authority[i..].contains(']') => &authority[..i],
        _ => authority,
    };
    format!("http://{host}:5055")
}

/// URL-encoded the way Seerr insists on (%20 for spaces, not +)
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Seerr, signed in
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    pub url: String,
    /// Seerr's session cookie, "connect.sid=…"
    pub cookie: String,
}

/// Sign in with a Jellyfin account (Seerr checks it with Jellyfin)
pub fn sign_in(url: &str, username: &str, password: &str) -> Result<Client> {
    let agent = http::agent(None);
    let url = url.trim().trim_end_matches('/').to_string();
    let resp = agent
        .post(&format!("{url}/api/v1/auth/jellyfin"))
        .send_json(json!({ "username": username, "password": password }))?;
    if !resp.status().is_success() {
        return Err(match status_error(resp) {
            Error::SignIn => Error::Server("Seerr didn't accept that password".into()),
            e => e,
        });
    }
    let cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|c| c.to_str().ok()?.split(';').next())
        .find(|c| c.starts_with("connect.sid="));
    let cookie = cookie
        .ok_or_else(|| Error::Server("Seerr didn't start a session".into()))?
        .to_string();
    Ok(Client { agent, url, cookie })
}

impl Client {
    pub fn new(url: &str, cookie: &str) -> Client {
        Client {
            agent: http::agent(None),
            url: url.to_string(),
            cookie: cookie.to_string(),
        }
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .agent
            .get(&format!("{}{path}", self.url))
            .header("Cookie", &self.cookie)
            .call()?;
        http::read_json(self.check(resp)?)
            .map_err(|e| Error::Server(format!("Unexpected reply from Seerr: {e}")))
    }

    /// The reply, if it's a success; else what went wrong. Seerr answers 403
    /// both when signed out and when something isn't allowed; only asking
    /// who's signed in tells them apart.
    fn check(&self, resp: Response) -> Result<Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let forbidden = resp.status().as_u16() == 403;
        let error = status_error(resp);
        let signed_in = || {
            self.agent
                .get(&format!("{}/api/v1/auth/me", self.url))
                .header("Cookie", &self.cookie)
                .call()
                .is_ok_and(|r| r.status().is_success())
        };
        if forbidden && !signed_in() {
            return Err(Error::SignIn);
        }
        Err(error)
    }

    /// End the session on Seerr's side too
    pub fn sign_out(&self) {
        let _ = self
            .agent
            .post(&format!("{}/api/v1/auth/logout", self.url))
            .header("Cookie", &self.cookie)
            .send_empty();
    }

    /// Movies and shows matching `query` (people left out)
    pub fn search(&self, query: &str) -> Result<Vec<Found>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            results: Vec<Found>,
        }
        let page: Page = self.get(&format!("/api/v1/search?query={}&page=1", encode(query)))?;
        Ok(page
            .results
            .into_iter()
            .filter(|f| matches!(f.media_type.as_str(), "movie" | "tv"))
            .collect())
    }

    /// The requests on their way, latest first (only one's own, unless allowed
    /// to see everyone's)
    pub fn underway(&self) -> Result<Vec<Underway>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            results: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            status: i32,
            #[serde(default)]
            media: MediaInfo,
        }
        let page: Page = self.get("/api/v1/request?take=50&skip=0&filter=all&sort=modified")?;
        let mut underway: Vec<Underway> = Vec::new();
        for entry in page
            .results
            .into_iter()
            .filter(|e| on_its_way(e.status, &e.media))
        {
            if underway.iter().any(|u| u.found.id == entry.media.tmdb_id) {
                continue;
            }
            // The request has the status and downloads; the title and poster are TMDB's
            let kind = if entry.media.media_type == "tv" {
                "tv"
            } else {
                "movie"
            };
            let mut found: Found = self.get(&format!("/api/v1/{kind}/{}", entry.media.tmdb_id))?;
            found.media_type = kind.to_string();
            found.media_info = Some(entry.media);
            underway.push(Underway {
                found,
                awaiting_approval: entry.status == 1,
            });
        }
        Ok(underway)
    }

    /// A movie's or show's details (`media_type`: movie or tv)
    pub fn details(&self, media_type: &str, tmdb_id: i64) -> Result<Details> {
        let kind = if media_type == "tv" { "tv" } else { "movie" };
        let mut details: Details = self.get(&format!("/api/v1/{kind}/{tmdb_id}"))?;
        details.media_type = kind.to_string();
        Ok(details)
    }

    /// Request a movie, or a show's `seasons`. True when it's approved
    /// straight away, false when it waits for approval.
    pub fn request(&self, found: &Found, seasons: &[i32]) -> Result<bool> {
        let mut body = json!({ "mediaType": found.media_type, "mediaId": found.id });
        if found.is_show() {
            body["seasons"] = json!(seasons);
        }
        let resp = self
            .agent
            .post(&format!("{}/api/v1/request", self.url))
            .header("Cookie", &self.cookie)
            .send_json(body)?;
        let request: Value = http::read_json(self.check(resp)?).unwrap_or_default();
        Ok(request["status"].as_i64() == Some(2))
    }
}

#[cfg(test)]
mod tests;
