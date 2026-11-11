//! Asking for subtitles for what's playing: an issue about them in Seerr, for
//! the server to find them.

use super::*;
use crate::api::Item;
use crate::session;
use crate::ui::Ctx;
use adw::prelude::*;
use std::rc::Rc;

/// The languages to ask for, and Bazarr's code for each: picked from a list
/// so a worker on the server can act on them. Not always ISO's (pb, zt, ea).
const LANGUAGES: &[(&str, &str)] = &[
    ("Arabic", "ar"),
    ("Bulgarian", "bg"),
    ("Catalan", "ca"),
    ("Chinese (Simplified)", "zh"),
    ("Chinese (Traditional)", "zt"),
    ("Croatian", "hr"),
    ("Czech", "cs"),
    ("Danish", "da"),
    ("Dutch", "nl"),
    ("English", "en"),
    ("Estonian", "et"),
    ("Finnish", "fi"),
    ("French", "fr"),
    ("German", "de"),
    ("Greek", "el"),
    ("Hebrew", "he"),
    ("Hindi", "hi"),
    ("Hungarian", "hu"),
    ("Icelandic", "is"),
    ("Indonesian", "id"),
    ("Italian", "it"),
    ("Japanese", "ja"),
    ("Korean", "ko"),
    ("Latvian", "lv"),
    ("Lithuanian", "lt"),
    ("Malay", "ms"),
    ("Norwegian", "no"),
    ("Persian", "fa"),
    ("Polish", "pl"),
    ("Portuguese", "pt"),
    ("Portuguese (Brazil)", "pb"),
    ("Romanian", "ro"),
    ("Russian", "ru"),
    ("Serbian", "sr"),
    ("Slovak", "sk"),
    ("Slovenian", "sl"),
    ("Spanish", "es"),
    ("Spanish (Latin America)", "ea"),
    ("Swedish", "sv"),
    ("Thai", "th"),
    ("Turkish", "tr"),
    ("Ukrainian", "uk"),
    ("Vietnamese", "vi"),
];

/// Seerr only knows movies and shows
pub fn can_request_subtitles(item: &Item) -> bool {
    matches!(item.kind.as_str(), "Episode" | "Movie")
}

/// Ask which language, and how much of the show, and send the request
/// (signing in to Seerr first, if need be)
pub fn request_subtitles(ctx: &Rc<Ctx>, item: &Item) {
    if client().is_some() {
        ask(ctx, item);
        return;
    }
    let (c, item) = (Rc::downgrade(ctx), item.clone());
    let then = move || {
        if let Some(ctx) = c.upgrade() {
            ask(&ctx, &item);
        }
    };
    sign_in(ctx, None, None, Rc::new(then));
}

/// What of the show `item`'s from they can be for: a label, and Seerr's
/// season and episode (0: all of them). Nothing to choose in a movie.
fn parts(item: &Item) -> Vec<(String, (i32, i32))> {
    if item.series_id.is_none() {
        return Vec::new();
    }
    let mut parts = vec![("The whole show".to_string(), (0, 0))];
    // Season 0, the specials, would mean all of them
    if let Some(season) = item.parent_index_number.filter(|s| *s > 0) {
        parts.push((format!("Season {season}"), (season, 0)));
        if let Some(episode) = item.index_number.filter(|e| *e > 0) {
            parts.push((
                format!("Season {season}, episode {episode}"),
                (season, episode),
            ));
        }
    }
    parts
}

/// Where `locale` ("pt_BR.UTF-8") is in LANGUAGES, when it is
fn language_of(locale: &str) -> Option<usize> {
    let locale = locale.split(['.', '@']).next()?.to_lowercase();
    let (language, country) = locale.split_once('_').unwrap_or((&locale, ""));
    let code = match (language, country) {
        ("pt", "br") => "pb",
        ("zh", "tw" | "hk") => "zt",
        ("es", "" | "es") => "es",
        ("es", _) => "ea",
        ("nb" | "nn", _) => "no",
        _ => language,
    };
    LANGUAGES.iter().position(|(_, known)| *known == code)
}

/// Which language the list starts on: the system's, or English
fn system_language() -> usize {
    let names = gtk::glib::language_names();
    let found = names.iter().find_map(|name| language_of(name));
    found.or_else(|| language_of("en")).unwrap_or(0)
}

fn name(item: &Item) -> String {
    item.series_name
        .clone()
        .unwrap_or_else(|| item.name.clone())
}

fn ask(ctx: &Rc<Ctx>, item: &Item) {
    let names: Vec<&str> = LANGUAGES.iter().map(|(name, _)| *name).collect();
    let language = adw::ComboRow::builder()
        .title("Language")
        .model(&gtk::StringList::new(&names))
        .expression(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        ))
        .enable_search(true)
        .build();
    language.set_selected(system_language() as u32);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&language);
    let parts = parts(item);
    let labels: Vec<&str> = parts.iter().map(|(label, _)| label.as_str()).collect();
    let part = adw::ComboRow::builder()
        .title("For")
        .model(&gtk::StringList::new(&labels))
        .build();
    if !parts.is_empty() {
        list.append(&part);
    }
    let body = format!(
        "Your request for “{}” goes to the server through Seerr. Once they're found, \
         they'll be in the subtitle menu the next time you play it.",
        name(item)
    );
    let dialog = adw::AlertDialog::new(Some("Request Subtitles"), Some(&body));
    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[("cancel", "Cancel"), ("request", "Request")]);
    dialog.set_response_appearance("request", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("request"));
    dialog.set_close_response("cancel");
    let (c, item, row) = (Rc::downgrade(ctx), item.clone(), language.clone());
    dialog.connect_response(Some("request"), move |_, _| {
        // A movie has no parts, and no row selected
        let chosen = parts.get(part.selected() as usize);
        let chosen = chosen.map_or((0, 0), |(_, part)| *part);
        if let (Some(ctx), Some(language)) = (c.upgrade(), LANGUAGES.get(row.selected() as usize)) {
            send(&ctx, &item, chosen, *language);
        }
    });
    dialog.present(Some(&ctx.window));
    dialog.set_focus(Some(&language));
}

fn send(ctx: &Rc<Ctx>, item: &Item, part: (i32, i32), language: (&'static str, &'static str)) {
    let (Some(client), Some(jellyfin)) = (client(), ctx.client()) else {
        return;
    };
    let (c, item) = (Rc::downgrade(ctx), item.clone());
    // An episode's own ids aren't the show's, which is what Seerr knows
    let id = item.series_id.clone().unwrap_or_else(|| item.id.clone());
    let media_type = if item.series_id.is_some() {
        "tv"
    } else {
        "movie"
    };
    run(
        client,
        move |client| {
            let found = jellyfin
                .item(&id)
                .map_err(|e| seerr::Error::Server(e.to_string()))?;
            let tmdb_id = found.tmdb_id().ok_or_else(|| {
                seerr::Error::Server("The server has no TMDB number for this".into())
            })?;
            client.request_subtitles(media_type, tmdb_id, part, language)
        },
        move |result| {
            let Some(ctx) = c.upgrade() else { return };
            match result {
                Ok(()) => ctx.toast(&format!("Requested subtitles for “{}”", name(&item))),
                Err(seerr::Error::SignIn) => {
                    session::clear_seerr();
                    request_subtitles(&ctx, &item);
                }
                Err(e) => ctx.toast(&e.to_string()),
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(season: Option<i32>, number: Option<i32>) -> Item {
        Item {
            kind: "Episode".into(),
            series_id: Some("show".into()),
            parent_index_number: season,
            index_number: number,
            ..Default::default()
        }
    }

    fn numbers(item: &Item) -> Vec<(i32, i32)> {
        parts(item).into_iter().map(|(_, part)| part).collect()
    }

    #[test]
    fn languages_are_listed_by_name_with_their_own_codes() {
        let names: Vec<_> = LANGUAGES.iter().map(|(name, _)| *name).collect();
        assert!(names.is_sorted());
        let mut codes: Vec<_> = LANGUAGES.iter().map(|(_, code)| *code).collect();
        // What a worker on the server reads back from the request
        assert!(codes.iter().all(|c| c.len() == 2));
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len());
    }

    #[test]
    fn starts_on_the_systems_language() {
        let code = |locale| language_of(locale).map(|i| LANGUAGES[i].1);
        assert_eq!(code("pt_PT.UTF-8"), Some("pt"));
        assert_eq!(code("pt_BR.UTF-8"), Some("pb"));
        assert_eq!(code("pt"), Some("pt"));
        assert_eq!(code("en_US"), Some("en"));
        assert_eq!(code("zh_TW.UTF-8"), Some("zt"));
        assert_eq!(code("zh_CN.UTF-8"), Some("zh"));
        assert_eq!(code("es_ES.UTF-8"), Some("es"));
        assert_eq!(code("es_MX.UTF-8"), Some("ea"));
        assert_eq!(code("nb_NO.UTF-8"), Some("no"));
        assert_eq!(code("de_DE@euro"), Some("de"));
        // Nothing to go by: the caller's English
        assert_eq!(code("C"), None);
    }

    #[test]
    fn offers_the_show_its_season_and_the_episode() {
        assert_eq!(
            numbers(&episode(Some(3), Some(2))),
            [(0, 0), (3, 0), (3, 2)]
        );
        // Without its number, what's known
        assert_eq!(numbers(&episode(Some(3), None)), [(0, 0), (3, 0)]);
        assert_eq!(numbers(&episode(None, Some(2))), [(0, 0)]);
    }

    #[test]
    fn specials_only_go_by_the_whole_show() {
        // To Seerr, season 0 is every season
        assert_eq!(numbers(&episode(Some(0), Some(4))), [(0, 0)]);
    }

    #[test]
    fn a_movie_has_no_parts() {
        let movie = Item {
            kind: "Movie".into(),
            ..Default::default()
        };
        assert!(parts(&movie).is_empty());
        assert!(can_request_subtitles(&movie));
    }

    #[test]
    fn only_what_seerr_knows() {
        assert!(can_request_subtitles(&episode(Some(1), Some(1))));
        let video = Item {
            kind: "Video".into(),
            ..Default::default()
        };
        assert!(!can_request_subtitles(&video));
    }
}
