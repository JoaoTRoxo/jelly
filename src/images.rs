//! Posters and thumbnails: fetched and decoded off the main thread, cached on
//! disk (image URLs carry the image's tag, so a changed image gets a new URL)
//! and, for the most recent ones, in memory.

use crate::{api, http};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

const MEMORY_ENTRIES: usize = 150;
/// The disk cache is trimmed to this when the app starts, oldest first
const DISK_LIMIT: u64 = 500 * 1024 * 1024;

thread_local! {
    static MEMORY: RefCell<(HashMap<String, gdk::Texture>, VecDeque<String>)> = Default::default();
}

fn cache_dir() -> PathBuf {
    glib::user_cache_dir().join("jelly").join("images")
}

/// The cached copy's name: the URL's SHA-256, which (unlike Rust's own
/// hashing) stays the same from one version to the next
fn cache_name(url: &str) -> String {
    glib::compute_checksum_for_string(glib::ChecksumType::Sha256, url)
        .map(String::from)
        .unwrap_or_default()
}

fn load(fetch: impl FnOnce(&str) -> Option<Vec<u8>>, url: &str) -> Option<gdk::Texture> {
    let path = cache_dir().join(cache_name(url));
    if let Ok(bytes) = std::fs::read(&path) {
        match gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) {
            Ok(texture) => return Some(texture),
            // Not an image after all: fetch it again
            Err(_) => drop(std::fs::remove_file(&path)),
        }
    }
    let bytes = fetch(url)?;
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from(&bytes)).ok()?;
    save(&path, &bytes);
    Some(texture)
}

/// Write the whole file or none of it, so a cut-off one isn't left behind
fn save(path: &Path, bytes: &[u8]) {
    let Some(dir) = path.parent() else { return };
    let partial = path.with_extension(format!("{}.part", glib::random_int()));
    let saved = std::fs::create_dir_all(dir)
        .and_then(|_| std::fs::write(&partial, bytes))
        .and_then(|_| std::fs::rename(&partial, path));
    if saved.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
}

/// Trim the disk cache to DISK_LIMIT, removing the oldest images first.
/// Blocking: call it off the main thread.
pub fn trim_cache() {
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            meta.is_file().then(|| (modified, meta.len(), entry.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    files.sort();
    for (_, size, path) in files {
        if total <= DISK_LIMIT {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

fn remember(url: &str, texture: &gdk::Texture) {
    MEMORY.with(|m| {
        let (map, order) = &mut *m.borrow_mut();
        if map.insert(url.to_string(), texture.clone()).is_none() {
            order.push_back(url.to_string());
            while order.len() > MEMORY_ENTRIES {
                if let Some(old) = order.pop_front() {
                    map.remove(&old);
                }
            }
        }
    });
}

/// Show the server's image at `url` in `picture` once it's loaded: a shimmer
/// while it loads (see style.css), then it fades in.
pub fn set(picture: &gtk::Picture, client: &api::Client, url: String) {
    let client = client.clone();
    show(picture, url, move |url| client.fetch(url).ok());
}

/// Like `set`, for an image from elsewhere (TMDB's posters, for Seerr): fetched
/// without the server's sign-in, which mustn't go anywhere else
pub fn set_public(picture: &gtk::Picture, url: String) {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| http::agent(None));
    show(picture, url, |url| {
        let response = AGENT.get(url).call().ok()?;
        if !response.status().is_success() {
            return None;
        }
        http::read_image(response).ok()
    });
}

fn show(
    picture: &gtk::Picture,
    url: String,
    fetch: impl FnOnce(&str) -> Option<Vec<u8>> + Send + 'static,
) {
    if let Some(texture) = MEMORY.with(|m| m.borrow().0.get(&url).cloned()) {
        picture.set_paintable(Some(&texture));
        return;
    }
    picture.add_css_class("image-loading");
    let weak = picture.downgrade();
    glib::spawn_future_local(async move {
        let fetch_url = url.clone();
        let texture = gio::spawn_blocking(move || load(fetch, &fetch_url))
            .await
            .ok()
            .flatten();
        if let Some(texture) = &texture {
            remember(&url, texture);
        }
        let Some(picture) = weak.upgrade() else {
            return;
        };
        picture.remove_css_class("image-loading");
        if let Some(texture) = texture {
            picture.set_opacity(0.0);
            picture.set_paintable(Some(&texture));
            let target = adw::PropertyAnimationTarget::new(&picture, "opacity");
            adw::TimedAnimation::new(&picture, 0.0, 1.0, 300, target).play();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cached_images_by_their_urls_sha256() {
        assert_eq!(
            cache_name("https://jellyfin.example.com/Items/1/Images/Primary?tag=a"),
            "05220809d1e717b9bb7b86ea6b0c40aee971369d02007f1055af980582a81573"
        );
    }
}
