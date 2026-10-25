//! The saved sign-in and settings, in ~/.config/jelly (on Windows,
//! %LOCALAPPDATA%\jelly), readable only by the user.

use gtk::glib;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub server: String,
    pub user_id: String,
    pub user_name: String,
    pub token: String,
}

/// The Seerr sign-in, for requesting what isn't in the library
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeerrSession {
    pub url: String,
    /// Seerr's session cookie (not the password)
    pub cookie: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The last server signed in to, offered on the sign-in page
    pub last_server: String,
    pub volume: f64,
    /// Seerr's address, as last signed in to (else seerr::default_url)
    pub seerr_url: String,
    /// The tracks last picked in each show (by its Jellyfin id), to pick
    /// again in its other episodes
    pub show_tracks: HashMap<String, ShowTracks>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowTracks {
    pub audio: Option<Track>,
    pub subtitles: Option<Subtitles>,
}

/// A track, as told apart from the others in an episode
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    /// As Jellyfin has it ("jpn")
    pub language: Option<String>,
    /// The track's own title ("Dialogue", "Signs/Songs")
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subtitles {
    Off,
    On(Track),
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            last_server: String::new(),
            volume: 100.0,
            seerr_url: String::new(),
            show_tracks: HashMap::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    glib::user_config_dir().join("jelly")
}

fn write_private(name: &str, contents: &[u8]) -> std::io::Result<()> {
    let dir = config_dir();
    let mut folder = fs::DirBuilder::new();
    folder.recursive(true);
    let mut file = fs::OpenOptions::new();
    file.write(true).create(true).truncate(true);
    // On Windows, the user's profile folders are already theirs alone
    #[cfg(unix)]
    {
        folder.mode(0o700);
        file.mode(0o600);
    }
    folder.create(&dir)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = file.open(&tmp)?;
    f.write_all(contents)?;
    f.sync_all()?;
    fs::rename(tmp, dir.join(name))
}

fn read_json<T: for<'de> Deserialize<'de>>(name: &str) -> Option<T> {
    serde_json::from_slice(&fs::read(config_dir().join(name)).ok()?).ok()
}

pub fn load() -> Option<Session> {
    read_json::<Session>("session.json").filter(|s| !s.token.is_empty())
}

pub fn save(session: &Session) {
    if let Err(e) = write_private("session.json", &serde_json::to_vec_pretty(session).unwrap()) {
        eprintln!("couldn't save the session: {e}");
    }
}

pub fn clear() {
    let _ = fs::remove_file(config_dir().join("session.json"));
    clear_seerr();
}

pub fn load_seerr() -> Option<SeerrSession> {
    read_json::<SeerrSession>("seerr.json").filter(|s| !s.cookie.is_empty())
}

pub fn save_seerr(session: &SeerrSession) {
    if let Err(e) = write_private("seerr.json", &serde_json::to_vec_pretty(session).unwrap()) {
        eprintln!("couldn't save the Seerr session: {e}");
    }
}

pub fn clear_seerr() {
    let _ = fs::remove_file(config_dir().join("seerr.json"));
}

pub fn settings() -> Settings {
    read_json("settings.json").unwrap_or_default()
}

pub fn save_settings(settings: &Settings) {
    if let Err(e) = write_private(
        "settings.json",
        &serde_json::to_vec_pretty(settings).unwrap(),
    ) {
        eprintln!("couldn't save settings: {e}");
    }
}

/// A random id for this install, which Jellyfin uses to tell devices apart
pub fn device_id() -> String {
    let path = config_dir().join("device-id");
    if let Ok(id) = fs::read_to_string(&path) {
        let id = id.trim();
        if !id.is_empty() {
            return id.to_string();
        }
    }
    let id = glib::uuid_string_random().replace('-', "");
    if let Err(e) = write_private("device-id", id.as_bytes()) {
        eprintln!("couldn't save the device id: {e}");
    }
    id
}
