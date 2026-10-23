//! What's on its way: on Home, and at the top of the library it'll land in,
//! with how its download is going.

use super::*;
use crate::seerr::{self, Found};
use crate::ui::Ctx;
use crate::ui::cards;
use crate::{api, session};
use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Whether something on its way will land in `library`: movies in a movie
/// library; shows in a TV library, and when one is named "anime", anime there
/// and the rest in the others
fn lands_in(found: &Found, library: &api::Item, libraries: &[api::Item]) -> bool {
    let anime = |l: &api::Item| l.name.to_lowercase().contains("anime");
    match (found.is_show(), library.collection_type.as_deref()) {
        (false, Some("movies")) => true,
        (true, Some("tvshows")) => {
            let anime_library = libraries
                .iter()
                .any(|l| l.collection_type.as_deref() == Some("tvshows") && anime(l));
            !anime_library || found.is_anime() == anime(library)
        }
        _ => false,
    }
}

/// What's been requested and isn't in the library yet, and how its download
/// is going: on Home ("On the Way" shelf, `library` None) or at the top of a
/// library's page (just what'll land there). Keeps itself up to date while on
/// screen, and stays hidden with nothing on its way (or no Seerr).
pub fn underway(ctx: &Rc<Ctx>, library: Option<api::Item>) -> gtk::Box {
    let holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .visible(false)
        .build();
    // Each card's request, progress bar and second line
    let shown: Rc<RefCell<Vec<(i64, gtk::ProgressBar, gtk::Label)>>> = Rc::default();
    let library = Rc::new(library);
    let refresh = {
        let (holder, ctx) = (holder.downgrade(), Rc::downgrade(ctx));
        Rc::new(move || {
            let (Some(holder), Some(ctx)) = (holder.upgrade(), ctx.upgrade()) else {
                return;
            };
            let Some(client) = client() else {
                holder.set_visible(false);
                return;
            };
            // In a library: which libraries there are, to tell where things land
            let jellyfin = library.is_some().then(|| ctx.client()).flatten();
            let (shown, library, weak_ctx) = (shown.clone(), library.clone(), Rc::downgrade(&ctx));
            let work = move |c: &seerr::Client| {
                let underway = c.underway()?;
                Ok((
                    underway,
                    jellyfin
                        .map(|j| j.views().unwrap_or_default())
                        .unwrap_or_default(),
                ))
            };
            run(client, work, move |result| {
                let (underway, libraries) = match result {
                    Ok(found) => found,
                    Err(e) => {
                        if let seerr::Error::SignIn = e {
                            session::clear_seerr();
                        }
                        holder.set_visible(false);
                        return;
                    }
                };
                let underway: Vec<_> = underway
                    .into_iter()
                    .filter(|u| {
                        library
                            .as_ref()
                            .as_ref()
                            .is_none_or(|l| lands_in(&u.found, l, &libraries))
                    })
                    .collect();
                holder.set_visible(!underway.is_empty());
                // The same requests: move their progress on, keeping where it's scrolled
                let same = shown
                    .borrow()
                    .iter()
                    .map(|(id, _, _)| *id)
                    .eq(underway.iter().map(|u| u.found.id));
                if same {
                    for ((_, bar, line), u) in shown.borrow().iter().zip(&underway) {
                        show_progress(bar, line, u);
                    }
                    return;
                }
                crate::ui::clear(&holder);
                let mut cards = Vec::new();
                let mut handles = Vec::new();
                for u in &underway {
                    let (card, bar, line) = underway_card(&weak_ctx, u);
                    cards.push(card);
                    handles.push((u.found.id, bar, line));
                }
                *shown.borrow_mut() = handles;
                if library.is_some() {
                    // A library's page: a grid like its own, under a heading, and set apart from the library
                    holder.append(
                        &gtk::Label::builder()
                            .label("On the Way")
                            .xalign(0.0)
                            .margin_start(6)
                            .css_classes(["shelf-title"])
                            .build(),
                    );
                    let grid = crate::ui::library::grid();
                    for card in cards {
                        grid.append(&card);
                    }
                    holder.append(&grid);
                    holder.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                } else {
                    holder.append(&cards::shelf("On the Way", cards, None));
                }
            });
        })
    };
    refresh();
    // Seerr looks at the downloads once a minute
    let weak = holder.downgrade();
    glib::timeout_add_seconds_local(30, move || {
        let Some(holder) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if holder.parent().is_some_and(|page| page.is_mapped()) {
            refresh();
        }
        glib::ControlFlow::Continue
    });
    holder
}

/// A poster with its download's progress, opening its page
fn underway_card(
    ctx: &Weak<Ctx>,
    underway: &seerr::Underway,
) -> (gtk::Button, gtk::ProgressBar, gtk::Label) {
    let (card, art, line) = poster(&underway.found, "");
    let bar = gtk::ProgressBar::builder()
        .css_classes(["progress-strip"])
        .valign(gtk::Align::End)
        .build();
    art.add_overlay(&bar);
    // Two lines for "Downloading · 43% · 12 min left"
    let line = line.unwrap_or_default();
    line.set_wrap(true);
    line.set_lines(2);
    show_progress(&bar, &line, underway);
    let (ctx, found) = (ctx.clone(), underway.found.clone());
    card.connect_clicked(move |_| open_page(&ctx, &found));
    (card, bar, line)
}

fn show_progress(bar: &gtk::ProgressBar, line: &gtk::Label, underway: &seerr::Underway) {
    let downloaded = underway
        .found
        .media_info
        .as_ref()
        .and_then(|m| m.download_progress());
    bar.set_visible(downloaded.is_some());
    bar.set_fraction(downloaded.unwrap_or(0.0));
    line.set_label(&underway.progress());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(name: &str, kind: &str) -> api::Item {
        api::Item {
            name: name.into(),
            collection_type: Some(kind.into()),
            ..Default::default()
        }
    }

    #[test]
    fn knows_which_library_things_land_in() {
        let (anime, series, films) = (
            library("Anime", "tvshows"),
            library("Series", "tvshows"),
            library("Filmes", "movies"),
        );
        let all = vec![anime.clone(), series.clone(), films.clone()];
        let anime_show = Found {
            media_type: "tv".into(),
            keywords: vec![seerr::Keyword {
                name: "anime".into(),
            }],
            ..Default::default()
        };
        let show = Found {
            media_type: "tv".into(),
            ..Default::default()
        };
        let movie = Found {
            media_type: "movie".into(),
            ..Default::default()
        };
        assert!(lands_in(&anime_show, &anime, &all) && !lands_in(&anime_show, &series, &all));
        assert!(lands_in(&show, &series, &all) && !lands_in(&show, &anime, &all));
        assert!(lands_in(&movie, &films, &all) && !lands_in(&movie, &series, &all));
        // With one TV library, every show goes there
        assert!(lands_in(
            &anime_show,
            &series,
            &[series.clone(), films.clone()]
        ));
    }
}
