//! Home: a carousel of new titles, Continue Watching, the libraries,
//! and what's new in each.

use super::Ctx;
use super::cards::{self, Shape};
use crate::{api, images};
use adw::prelude::*;
use gtk::glib;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const FEATURED_HEIGHT: i32 = 520;

struct Home {
    views: Vec<api::Item>,
    continue_watching: Vec<api::Item>,
    latest: Vec<(api::Item, Vec<api::Item>)>,
}

/// What loading fills in
#[derive(Clone)]
struct View {
    content: gtk::Box,
    toolbar: adw::ToolbarView,
}

pub fn page(ctx: &Rc<Ctx>) -> adw::NavigationPage {
    // No side margins: the shelves run edge to edge
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(26)
        .margin_bottom(32)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let header = adw::HeaderBar::builder().show_title(false).build();
    let search = gtk::Button::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text("Search (Ctrl+F)")
        .build();
    header.pack_start(&search);
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Menu")
        .popover(&gtk::Popover::builder().child(&menu).build())
        .build();
    header.pack_end(&menu_button);
    let refresh = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh (F5)")
        .build();
    header.pack_end(&refresh);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));
    let page = adw::NavigationPage::builder()
        .title("Home")
        .tag("home")
        .child(&toolbar)
        .build();
    // Over the carousel, when there is one
    super::header_over_hero(&header, &scroller);

    let weak = Rc::downgrade(ctx);
    search.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            ctx.nav.push(&super::library::search_page(&ctx));
        }
    });
    let sign_out = gtk::Button::builder()
        .label("Sign Out")
        .css_classes(["flat"])
        .build();
    let weak = Rc::downgrade(ctx);
    let popover_menu = menu_button.clone();
    sign_out.connect_clicked(move |_| {
        popover_menu.popdown();
        if let Some(ctx) = weak.upgrade() {
            ctx.sign_out(true);
        }
    });
    let user = gtk::Label::builder()
        .css_classes(["dim-label"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .xalign(0.0)
        .build();
    if let Some(s) = crate::session::load() {
        user.set_label(&format!("{} on {}", s.user_name, s.server));
    }
    menu.append(&user);
    menu.append(&sign_out);

    let load = {
        let ctx = Rc::downgrade(ctx);
        let view = View { content, toolbar };
        move || {
            if let Some(ctx) = ctx.upgrade() {
                load(&ctx, &view);
            }
        }
    };
    let load = Rc::new(load);
    let reload = load.clone();
    refresh.connect_clicked(move |_| reload());
    let reload = load.clone();
    super::reloading_page(ctx, &page, move || reload());
    page
}

fn load(ctx: &Rc<Ctx>, view: &View) {
    let content = view.content.clone();
    if content.first_child().is_none() {
        show_under_header(view, false);
        content.append(&super::loading());
    }
    let weak = Rc::downgrade(ctx);
    let view = view.clone();
    super::run_result(
        ctx,
        |client| {
            let views = client.views()?;
            let resume = client.resume()?;
            let next_up = client.next_up(None)?;
            let continue_watching = if resume.is_empty() {
                next_up
            } else if next_up.is_empty() {
                resume
            } else {
                let played = client.recently_played_episodes().unwrap_or_default();
                merge_continue_watching(resume, next_up, &played)
            };
            let mut latest = Vec::new();
            for view in &views {
                if matches!(
                    view.collection_type.as_deref(),
                    Some("movies" | "tvshows" | "homevideos" | "boxsets") | None
                ) {
                    latest.push((view.clone(), client.latest(&view.id).unwrap_or_default()));
                }
            }
            Ok(Home {
                views,
                continue_watching,
                latest,
            })
        },
        move |result| {
            let Some(ctx) = weak.upgrade() else { return };
            super::clear(&content);
            let home = match result {
                Ok(home) => home,
                Err(e) => {
                    show_under_header(&view, false);
                    let (v, w) = (view.clone(), Rc::downgrade(&ctx));
                    content.append(&super::error_page(&e.to_string(), move || {
                        if let Some(ctx) = w.upgrade() {
                            super::clear(&v.content);
                            load(&ctx, &v);
                        }
                    }));
                    return;
                }
            };
            let featured = featured(&home.latest);
            show_under_header(&view, !featured.is_empty());
            if !featured.is_empty() {
                content.append(&carousel(&ctx, &featured));
            }
            let play_cards = |items: &[api::Item]| -> Vec<gtk::Button> {
                items
                    .iter()
                    .map(|item| {
                        let (c, i) = (ctx.clone(), item.clone());
                        cards::card(&ctx, item, Shape::Thumb, move || super::play(&c, &i, false))
                    })
                    .collect()
            };
            if !home.continue_watching.is_empty() {
                content.append(&cards::shelf(
                    "Continue Watching",
                    play_cards(&home.continue_watching),
                    None,
                ));
            }
            // Requests on their way, from Seerr (hidden without any)
            content.append(&super::request::underway(&ctx, None));
            if !home.views.is_empty() {
                let views = home
                    .views
                    .iter()
                    .map(|view| {
                        let (c, v) = (ctx.clone(), view.clone());
                        cards::card(&ctx, view, Shape::Thumb, move || {
                            c.nav.push(&super::library::page(&c, v.clone()))
                        })
                    })
                    .collect();
                content.append(&cards::shelf("Libraries", views, None));
            }
            for (view, items) in home.latest.iter().filter(|(_, items)| !items.is_empty()) {
                let posters = items
                    .iter()
                    .map(|item| {
                        let (c, i) = (ctx.clone(), item.clone());
                        cards::card(&ctx, item, Shape::Poster, move || super::open(&c, &i))
                    })
                    .collect();
                let (c, v) = (ctx.clone(), view.clone());
                let see_all = Box::new(move || c.nav.push(&super::library::page(&c, v.clone())));
                content.append(&cards::shelf(
                    &format!("New in {}", view.name),
                    posters,
                    Some(see_all),
                ));
            }
            if content.first_child().is_none() {
                let empty = adw::StatusPage::builder()
                    .icon_name("folder-videos-symbolic")
                    .title("Nothing here yet")
                    .description("The server has no libraries for this account")
                    .vexpand(true)
                    .build();
                content.append(&empty);
            }
        },
    );
}

/// Whether the content starts under the header bar (with the carousel), or below it
fn show_under_header(view: &View, under: bool) {
    view.toolbar.set_extend_content_to_top_edge(under);
    view.content.set_margin_top(if under { 0 } else { 12 });
}

/// Up to six new movies and series with a backdrop, taking turns between the
/// libraries, unwatched ones first
fn featured(latest: &[(api::Item, Vec<api::Item>)]) -> Vec<api::Item> {
    let mut picks: Vec<api::Item> = Vec::new();
    let longest = latest
        .iter()
        .map(|(_, items)| items.len())
        .max()
        .unwrap_or(0);
    for i in 0..longest {
        for (_, items) in latest {
            let Some(item) = items.get(i) else { continue };
            let fits =
                matches!(item.kind.as_str(), "Movie" | "Series") && item.backdrop_image().is_some();
            if fits && !picks.iter().any(|p| p.id == item.id) {
                picks.push(item.clone());
            }
        }
    }
    picks.sort_by_key(|item| item.played());
    picks.truncate(6);
    picks
}

/// Big slides of `items` that move on by themselves, unless the pointer is on them
fn carousel(ctx: &Rc<Ctx>, items: &[api::Item]) -> gtk::Widget {
    // Not the scroll wheel: that scrolls the page
    let carousel = adw::Carousel::builder()
        .allow_scroll_wheel(false)
        .allow_long_swipes(false)
        .build();
    for item in items {
        carousel.append(&slide(ctx, item));
    }
    let overlay = gtk::Overlay::builder()
        .child(&carousel)
        .css_classes(["featured"])
        .build();
    if items.len() > 1 {
        let lines = adw::CarouselIndicatorLines::builder()
            .carousel(&carousel)
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(30)
            .margin_bottom(28)
            .build();
        overlay.add_overlay(&lines);
    }
    let weak = carousel.downgrade();
    glib::timeout_add_seconds_local(8, move || {
        let Some(carousel) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let hovered = carousel.state_flags().contains(gtk::StateFlags::PRELIGHT);
        if carousel.is_mapped() && !hovered && carousel.n_pages() > 1 {
            let next = (carousel.position().round() as u32 + 1) % carousel.n_pages();
            carousel.scroll_to(&carousel.nth_page(next), true);
        }
        glib::ControlFlow::Continue
    });
    overlay.upcast()
}

/// A backdrop with the title, a little about it, and Play / More Info
fn slide(ctx: &Rc<Ctx>, item: &api::Item) -> gtk::Widget {
    let sizer = gtk::Box::builder()
        .hexpand(true)
        .height_request(FEATURED_HEIGHT)
        .build();
    let backdrop = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    if let (Some((id, kind, tag)), Some(client)) = (item.backdrop_image(), ctx.client()) {
        images::set(&backdrop, &client, client.image_url(&id, kind, &tag, 1920));
    }
    // The carousel only gives the whole width to pages that ask for it themselves
    let overlay = gtk::Overlay::builder()
        .child(&sizer)
        .overflow(gtk::Overflow::Hidden)
        .hexpand(true)
        .build();
    overlay.add_overlay(&backdrop);
    overlay.add_overlay(&gtk::Box::builder().css_classes(["featured-shade"]).build());

    let info = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    info.append(&cards::title(ctx, item, "featured-title", 440.0, 140.0));
    let mut meta = Vec::new();
    if let Some(y) = item.production_year {
        meta.push(y.to_string());
    }
    match (item.kind.as_str(), item.child_count, item.runtime_minutes()) {
        ("Series", Some(n), _) => meta.push(format!("{n} season{}", if n == 1 { "" } else { "s" })),
        ("Movie", _, Some(m)) => meta.push(super::format_runtime(m)),
        _ => {}
    }
    if !item.genres.is_empty() {
        meta.push(
            item.genres
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if let Some(score) = item.community_rating {
        meta.push(format!("★ {score:.1}"));
    }
    if !meta.is_empty() {
        info.append(
            &gtk::Label::builder()
                .label(meta.join("  ·  "))
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build(),
        );
    }
    if let Some(overview) = item.overview.as_deref().filter(|o| !o.is_empty()) {
        info.append(
            &gtk::Label::builder()
                .label(overview)
                .xalign(0.0)
                .wrap(true)
                .lines(3)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build(),
        );
    }

    let buttons = gtk::Box::builder().spacing(10).margin_top(6).build();
    let play = gtk::Button::builder()
        .css_classes(["pill", "suggested-action"])
        .build();
    let label = if item.resume_seconds() > 0.0 {
        "Resume"
    } else {
        "Play"
    };
    play.set_child(Some(
        &adw::ButtonContent::builder()
            .icon_name("media-playback-start-symbolic")
            .label(label)
            .build(),
    ));
    let (c, i) = (ctx.clone(), item.clone());
    play.connect_clicked(move |_| {
        if i.kind != "Series" {
            super::play(&c, &i, false);
            return;
        }
        // A series plays its next episode; with none, its page shows
        let (id, c2, i2) = (i.id.clone(), c.clone(), i.clone());
        super::run(
            &c,
            move |client| client.next_up(Some(&id)),
            move |next| match next.first() {
                Some(episode) => super::play(&c2, episode, false),
                None => super::open(&c2, &i2),
            },
        );
    });
    buttons.append(&play);
    let more = gtk::Button::builder()
        .label("More Info")
        .css_classes(["pill"])
        .build();
    let (c, i) = (ctx.clone(), item.clone());
    more.connect_clicked(move |_| super::open(&c, &i));
    buttons.append(&more);
    info.append(&buttons);
    cards::attach_item_menu(ctx, &overlay, item);

    // Text no wider than this, lined up with the shelves below
    let clamp = adw::Clamp::builder()
        .child(&info)
        .maximum_size(600)
        .tightening_threshold(600)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::End)
        .margin_start(30)
        .margin_end(30)
        .margin_bottom(56)
        .build();
    overlay.add_overlay(&clamp);
    overlay.upcast()
}

/// Merge `resume` (in progress) and `next_up` (next unwatched episodes) into
/// one shelf ordered by when each item or show was last watched. Each show
/// appears at most once, keeping an in-progress episode over Next Up.
fn merge_continue_watching(
    resume: Vec<api::Item>,
    next_up: Vec<api::Item>,
    played: &[api::Item],
) -> Vec<api::Item> {
    let mut series_played: HashMap<&str, &str> = HashMap::new();
    for episode in played {
        if let (Some(series), Some(date)) = (episode.series_id.as_deref(), episode.last_played()) {
            series_played
                .entry(series)
                .and_modify(|latest| {
                    if date > *latest {
                        *latest = date;
                    }
                })
                .or_insert(date);
        }
    }
    let mut seen_ids = HashSet::new();
    let mut seen_series = HashSet::new();
    let mut entries: Vec<(Option<String>, bool, usize, api::Item)> = Vec::new();
    for (i, item) in resume.into_iter().enumerate() {
        if !seen_ids.insert(item.id.clone()) {
            continue;
        }
        if item.kind == "Episode"
            && let Some(series) = &item.series_id
        {
            seen_series.insert(series.clone());
        }
        let date = item.last_played().map(str::to_string);
        entries.push((date, false, i, item));
    }
    for (i, item) in next_up.into_iter().enumerate() {
        if !seen_ids.insert(item.id.clone()) {
            continue;
        }
        if let Some(series) = &item.series_id
            && !seen_series.insert(series.clone())
        {
            continue;
        }
        let date = item
            .last_played()
            .or_else(|| {
                item.series_id
                    .as_deref()
                    .and_then(|s| series_played.get(s).copied())
            })
            .map(str::to_string);
        entries.push((date, true, i, item));
    }
    entries.sort_by(|a, b| {
        (a.0.is_none(), Reverse(a.0.as_deref()), a.1, a.2).cmp(&(
            b.0.is_none(),
            Reverse(b.0.as_deref()),
            b.1,
            b.2,
        ))
    });
    entries.into_iter().map(|(_, _, _, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(id: &str, played: Option<&str>) -> api::Item {
        api::Item {
            id: id.into(),
            kind: "Movie".into(),
            user_data: played.map(|d| api::UserData {
                last_played_date: Some(d.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn episode(id: &str, series: &str, played: Option<&str>) -> api::Item {
        api::Item {
            id: id.into(),
            kind: "Episode".into(),
            series_id: Some(series.into()),
            user_data: played.map(|d| api::UserData {
                last_played_date: Some(d.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn merges_resume_and_next_up_by_when_they_were_watched() {
        let resume = vec![
            movie("movie-a", Some("2026-11-24T10:00:00Z")),
            episode("show-b-e2", "show-b", Some("2026-11-24T08:00:00Z")),
        ];
        let next_up = vec![
            episode("show-c-e4", "show-c", None),
            episode("show-d-e2", "show-d", None),
            episode("show-e-e5", "show-e", None),
        ];
        let played = vec![
            episode("show-c-e3", "show-c", Some("2026-11-24T11:00:00Z")),
            episode("show-d-e1", "show-d", Some("2026-11-24T09:00:00Z")),
        ];
        let ids: Vec<String> = merge_continue_watching(resume, next_up, &played)
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(
            ids,
            [
                "show-c-e4",
                "movie-a",
                "show-d-e2",
                "show-b-e2",
                "show-e-e5"
            ]
        );
    }

    #[test]
    fn deduplicates_shows_keeping_in_progress_over_next_up() {
        let resume = vec![episode("show-a-e2", "show-a", Some("2026-11-24T09:00:00Z"))];
        let next_up = vec![
            episode("show-a-e3", "show-a", None),
            episode("show-b-e2", "show-b", None),
        ];
        let played = vec![episode("show-a-e1", "show-a", Some("2026-11-24T12:00:00Z"))];
        let ids: Vec<String> = merge_continue_watching(resume, next_up, &played)
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(ids, ["show-a-e2", "show-b-e2"]);
    }
}
