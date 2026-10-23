//! What isn't in the library yet, through Seerr: its matches on the search
//! page, what's on its way (on Home and in the library it'll land in), a page
//! for each movie or show (what it is, how its request and downloads are
//! going), the request dialogs, and signing in to Seerr with the Jellyfin
//! password.

mod dialogs;
mod page;
mod search;
mod underway;

pub use dialogs::sign_in;
pub use page::page;
pub use search::Section;
pub use underway::underway;

use super::Ctx;
use super::cards::{self, Shape};
use crate::seerr::{self, Found};
use crate::{images, session};
use gtk::{gio, glib};
use std::rc::Weak;

/// Seerr, if signed in
fn client() -> Option<seerr::Client> {
    session::load_seerr().map(|s| seerr::Client::new(&s.url, &s.cookie))
}

/// Run `work` with `client` on a worker thread, then `done` on the main thread
fn run<T: Send + 'static>(
    client: seerr::Client,
    work: impl FnOnce(&seerr::Client) -> seerr::Result<T> + Send + 'static,
    done: impl FnOnce(seerr::Result<T>) + 'static,
) {
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || work(&client))
            .await
            .unwrap_or_else(|_| Err(seerr::Error::Server("Internal error".into())));
        done(result);
    });
}

/// "Dune: Part Two (2024)"
fn name_year(found: &Found) -> String {
    match found.year() {
        Some(year) => format!("{} ({year})", found.title()),
        None => found.title().to_string(),
    }
}

/// A poster card for something from Seerr (TMDB's poster), and its picture frame
fn poster(found: &Found, subtitle: &str) -> (gtk::Button, gtk::Overlay, Option<gtk::Label>) {
    let (width, height) = Shape::Poster.size();
    let (art, picture) = cards::frame(width, height);
    match found.poster_url() {
        Some(url) => images::set_public(&picture, url),
        None => art.add_overlay(
            &gtk::Image::builder()
                .icon_name("folder-videos-symbolic")
                .pixel_size(48)
                .css_classes(["dim-label"])
                .build(),
        ),
    }
    let (card, line) =
        cards::card_with_subtitle(&art, Some((found.title(), subtitle)), width, found.title());
    (card, art, line)
}

fn open_page(ctx: &Weak<Ctx>, found: &Found) {
    if let Some(ctx) = ctx.upgrade() {
        ctx.nav
            .push(&page(&ctx, &found.media_type, found.id, found.title()));
    }
}
