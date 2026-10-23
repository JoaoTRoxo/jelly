//! A movie's or series' page: backdrop, poster, info, badges and play buttons;
//! for series the seasons and their episodes; then the cast and similar
//! titles, over a wash of the backdrop's colours.

use super::Ctx;
use super::cards::{self, Shape};
use crate::{api, images};
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

struct Loaded {
    item: api::Item,
    seasons: Vec<api::Item>,
    /// For series: the episode to continue with
    next_up: Option<api::Item>,
    similar: Vec<api::Item>,
}

/// `season`: for series, the season to show first
pub fn page(ctx: &Rc<Ctx>, item: api::Item, season: Option<String>) -> adw::NavigationPage {
    let stack = gtk::Stack::new();
    stack.add_named(&super::loading(), Some("loading"));
    let header = adw::HeaderBar::builder().show_title(false).build();
    let toolbar = adw::ToolbarView::builder()
        .extend_content_to_top_edge(true)
        .build();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    let page = adw::NavigationPage::builder()
        .title(item.name.as_str())
        .child(&toolbar)
        .build();

    // The season shown; kept across reloads (after playing something)
    let selected_season = Rc::new(RefCell::new(season));
    let load: Rc<dyn Fn()> = {
        let ctx = Rc::downgrade(ctx);
        let stack = stack.clone();
        let id = item.id.clone();
        let is_series = item.kind == "Series";
        let (page, header) = (page.downgrade(), header.downgrade());
        Rc::new(move || {
            let Some(ctx) = ctx.upgrade() else { return };
            let id = id.clone();
            let (weak, stack, selected_season, page, header) = (
                Rc::downgrade(&ctx),
                stack.clone(),
                selected_season.clone(),
                page.clone(),
                header.clone(),
            );
            super::run_result(
                &ctx,
                move |client| {
                    let item = client.item(&id)?;
                    let (seasons, next_up) = if is_series {
                        (
                            client.seasons(&id)?,
                            client.next_up(Some(&id))?.into_iter().next(),
                        )
                    } else {
                        (Vec::new(), None)
                    };
                    // An extra: the page shows without it
                    let similar = client.similar(&id).unwrap_or_default();
                    Ok(Loaded {
                        item,
                        seasons,
                        next_up,
                        similar,
                    })
                },
                move |result| {
                    let Some(ctx) = weak.upgrade() else { return };
                    if let Some(old) = stack.child_by_name("content") {
                        stack.remove(&old);
                    }
                    match result {
                        Ok(loaded) => {
                            if let Some(page) = page.upgrade() {
                                page.set_title(&loaded.item.name);
                            }
                            let scroller = build(&ctx, loaded, &selected_season);
                            if let Some(header) = header.upgrade() {
                                super::header_over_hero(&header, &scroller);
                            }
                            stack.add_named(&scroller, Some("content"));
                        }
                        Err(e) => {
                            let (s, w) = (stack.clone(), Rc::downgrade(&ctx));
                            let retry = move || {
                                s.set_visible_child_name("loading");
                                if let Some(ctx) = w.upgrade() {
                                    ctx.refresh_visible();
                                }
                            };
                            stack.add_named(
                                &super::error_page(&e.to_string(), retry),
                                Some("content"),
                            );
                        }
                    }
                    stack.set_visible_child_name("content");
                },
            );
        })
    };
    let l = load.clone();
    super::reloading_page(ctx, &page, move || l());
    page
}

/// The page's scrolling content
fn build(
    ctx: &Rc<Ctx>,
    loaded: Loaded,
    selected_season: &Rc<RefCell<Option<String>>>,
) -> gtk::ScrolledWindow {
    let item = &loaded.item;
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(28)
        .margin_bottom(32)
        .build();
    if item.kind == "Series" && !loaded.seasons.is_empty() {
        body.append(&seasons(ctx, item, &loaded, selected_season));
    }
    // Directors first, then the cast in billing order
    let directors = item.people.iter().filter(|p| p.kind == "Director");
    let people: Vec<_> = directors
        .chain(item.people.iter().filter(|p| p.kind == "Actor"))
        .take(24)
        .map(|p| cards::person(ctx, p))
        .collect();
    if !people.is_empty() {
        body.append(&cards::shelf("Cast & Crew", people, None));
    }
    let similar: Vec<_> = loaded
        .similar
        .iter()
        .map(|similar| {
            let (c, i) = (ctx.clone(), similar.clone());
            cards::card(ctx, similar, Shape::Poster, move || super::open(&c, &i))
        })
        .collect();
    if !similar.is_empty() {
        body.append(&cards::shelf("More Like This", similar, None));
    }

    // With nothing below it, the hero fills the page
    let has_body = body.first_child().is_some();
    let hero = hero(ctx, item, loaded.next_up.as_ref(), !has_body);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&hero);
    if has_body {
        content.append(&wash(ctx, item, &body));
    }
    gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
}

/// Season tabs and the chosen season's episodes
fn seasons(
    ctx: &Rc<Ctx>,
    item: &api::Item,
    loaded: &Loaded,
    selected_season: &Rc<RefCell<Option<String>>>,
) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_start(30)
        .margin_end(30)
        .build();
    let tabs = gtk::Box::builder().spacing(6).build();
    let tabs_scroller = gtk::ScrolledWindow::builder()
        .child(&tabs)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let episodes = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    section.append(&tabs_scroller);
    section.append(&episodes);

    // Season to show: the one asked for, else the next-up episode's, else the first
    let wanted = selected_season
        .borrow()
        .clone()
        .or_else(|| loaded.next_up.as_ref().and_then(|e| e.season_id.clone()));
    let initial = loaded
        .seasons
        .iter()
        .position(|s| Some(&s.id) == wanted.as_ref())
        .unwrap_or(0);
    let mut group: Option<gtk::ToggleButton> = None;
    for (i, season) in loaded.seasons.iter().enumerate() {
        let tab = gtk::ToggleButton::builder()
            .label(season.name.as_str())
            .css_classes(["pill"])
            .build();
        match &group {
            Some(g) => tab.set_group(Some(g)),
            None => group = Some(tab.clone()),
        }
        let (c, s, list, sel) = (
            Rc::downgrade(ctx),
            season.clone(),
            episodes.clone(),
            selected_season.clone(),
        );
        let series_id = item.id.clone();
        tab.connect_toggled(move |tab| {
            if !tab.is_active() {
                return;
            }
            *sel.borrow_mut() = Some(s.id.clone());
            if let Some(ctx) = c.upgrade() {
                load_episodes(&ctx, &series_id, &s.id, &list);
            }
        });
        cards::attach_item_menu(ctx, &tab, season);
        tabs.append(&tab);
        if i == initial {
            tab.set_active(true);
        }
    }
    tabs_scroller.set_visible(loaded.seasons.len() > 1);
    section
}

/// `body` over a blurred wash of the item's backdrop, which fades in below the hero
fn wash(ctx: &Rc<Ctx>, item: &api::Item, body: &gtk::Box) -> gtk::Widget {
    // The picture is tiny, so `body` sets the size
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    if let (Some((id, kind, tag)), Some(client)) = (item.backdrop_image(), ctx.client()) {
        images::set(&picture, &client, client.blurred_image_url(&id, kind, &tag));
    }
    let overlay = gtk::Overlay::builder().child(&picture).build();
    overlay.add_overlay(&gtk::Box::builder().css_classes(["wash-scrim"]).build());
    overlay.add_overlay(body);
    overlay.set_measure_overlay(body, true);
    overlay.upcast()
}

/// `fill`: take all the height there is (when nothing comes below)
fn hero(ctx: &Rc<Ctx>, item: &api::Item, next_up: Option<&api::Item>, fill: bool) -> gtk::Widget {
    // Sized by `sizer` (at least as tall as .hero in style.css) and the info on
    // it, not by the backdrop image
    let sizer = gtk::Box::builder()
        .hexpand(true)
        .vexpand(fill)
        .css_classes(["hero"])
        .build();
    let backdrop = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    if let (Some((id, kind, tag)), Some(client)) = (item.backdrop_image(), ctx.client()) {
        images::set(&backdrop, &client, client.image_url(&id, kind, &tag, 1920));
    }
    let overlay = gtk::Overlay::builder()
        .child(&sizer)
        .overflow(gtk::Overflow::Hidden)
        .build();
    overlay.add_overlay(&backdrop);
    let shade = gtk::Box::builder().css_classes(["hero-shade"]).build();
    overlay.add_overlay(&shade);

    let info = gtk::Box::builder()
        .spacing(28)
        .valign(gtk::Align::End)
        .margin_start(30)
        .margin_end(30)
        .margin_bottom(28)
        .margin_top(72)
        .build();
    info.append(&cards::art(ctx, item, Shape::Poster, 200, 300));

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .valign(gtk::Align::End)
        .hexpand(true)
        .build();
    text.append(&cards::title(ctx, item, "hero-title", 480.0, 150.0));

    let mut meta = Vec::new();
    if let Some(y) = item.production_year {
        meta.push(y.to_string());
    }
    if let Some(r) = &item.official_rating {
        meta.push(r.clone());
    }
    if let Some(score) = item.community_rating {
        meta.push(format!("★ {score:.1}"));
    }
    if item.kind != "Series"
        && let Some(m) = item.runtime_minutes()
    {
        meta.push(super::format_runtime(m));
    }
    if !item.genres.is_empty() {
        meta.push(
            item.genres
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !meta.is_empty() {
        text.append(
            &gtk::Label::builder()
                .label(meta.join("  ·  "))
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build(),
        );
    }
    // What the main button plays: the movie, or the series' next episode
    let target = if item.kind == "Series" {
        next_up.cloned()
    } else {
        Some(item.clone())
    };
    let badges = target.as_ref().map(badges).unwrap_or_default();
    if !badges.is_empty() {
        let row = gtk::Box::builder().spacing(6).build();
        for badge in badges {
            row.append(
                &gtk::Label::builder()
                    .label(badge)
                    .css_classes(["tech-badge"])
                    .build(),
            );
        }
        text.append(&row);
    }
    if let Some(tagline) = item.taglines.first().filter(|t| !t.is_empty()) {
        text.append(
            &gtk::Label::builder()
                .label(tagline)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["tagline"])
                .build(),
        );
    }
    if let Some(overview) = item.overview.as_deref().filter(|o| !o.is_empty()) {
        let label = gtk::Label::builder()
            .label(overview)
            .xalign(0.0)
            .wrap(true)
            .lines(4)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(110)
            .build();
        text.append(&label);
    }

    let buttons = gtk::Box::builder().spacing(10).margin_top(8).build();
    if let Some(target) = target {
        let resume = target.resume_seconds();
        let label = match (target.kind.as_str(), resume > 0.0) {
            ("Episode", true) => format!("Resume {}", target.episode_code().unwrap_or_default()),
            ("Episode", false) => format!("Play {}", target.episode_code().unwrap_or_default()),
            (_, true) => format!("Resume from {}", super::format_time(resume)),
            (_, false) => "Play".to_string(),
        };
        let play = gtk::Button::builder()
            .css_classes(["pill", "suggested-action"])
            .build();
        play.set_child(Some(
            &adw::ButtonContent::builder()
                .icon_name("media-playback-start-symbolic")
                .label(label)
                .build(),
        ));
        let (c, t) = (ctx.clone(), target.clone());
        play.connect_clicked(move |_| super::play(&c, &t, false));
        buttons.append(&play);
        if resume > 0.0 {
            let again = gtk::Button::builder()
                .label("From the Beginning")
                .css_classes(["pill"])
                .build();
            let (c, t) = (ctx.clone(), target.clone());
            again.connect_clicked(move |_| super::play(&c, &t, true));
            buttons.append(&again);
        }
    }
    let played = item.played();
    let watched = gtk::Button::builder()
        .icon_name(if played {
            "edit-undo-symbolic"
        } else {
            "object-select-symbolic"
        })
        .tooltip_text(if played {
            "Mark as unwatched"
        } else {
            "Mark as watched"
        })
        .css_classes(["circular"])
        .valign(gtk::Align::Center)
        .build();
    let (c, id) = (ctx.clone(), item.id.clone());
    watched.connect_clicked(move |_| cards::set_played(&c, &id, !played));
    buttons.append(&watched);
    text.append(&buttons);

    info.append(&text);
    overlay.add_overlay(&info);
    overlay.set_measure_overlay(&info, true);
    overlay.upcast()
}

/// "4K", "Dolby Vision", "Dolby Atmos", "7.1"… for a movie or episode
fn badges(item: &api::Item) -> Vec<String> {
    let mut badges = Vec::new();
    let streams = &item.media_streams;
    if let Some(video) = streams.iter().find(|s| s.kind == "Video") {
        // Letterboxed films are short for their resolution, 4:3 ones narrow
        let width = video
            .width
            .unwrap_or(0)
            .max(video.height.unwrap_or(0) * 16 / 9);
        let resolution = match width {
            0 => None,
            w if w >= 3200 => Some("4K"),
            w if w >= 1800 => Some("1080p"),
            w if w >= 1200 => Some("720p"),
            _ => Some("SD"),
        };
        badges.extend(resolution.map(String::from));
        let range = match video.video_range_type.as_deref() {
            Some(t) if t.starts_with("DOVI") => Some("Dolby Vision"),
            Some("HDR10Plus") => Some("HDR10+"),
            Some("HDR10") => Some("HDR10"),
            Some("HLG") => Some("HLG"),
            _ => None,
        };
        badges.extend(range.map(String::from));
    }
    let audio = streams.iter().filter(|s| s.kind == "Audio");
    if let Some(audio) = audio
        .clone()
        .find(|s| s.is_default)
        .or_else(|| audio.clone().next())
    {
        let profile = audio.profile.as_deref().unwrap_or("");
        if profile.contains("Atmos") {
            badges.push("Dolby Atmos".into());
        } else if profile.contains("DTS:X") {
            badges.push("DTS:X".into());
        }
        let channels = match audio.channels {
            Some(1) => Some("Mono".to_string()),
            Some(2) => Some("Stereo".to_string()),
            Some(6) => Some("5.1".to_string()),
            Some(7) => Some("6.1".to_string()),
            Some(8) => Some("7.1".to_string()),
            Some(n) if n > 2 => Some(format!("{n} ch")),
            _ => None,
        };
        badges.extend(channels);
    }
    badges
}

fn load_episodes(ctx: &Rc<Ctx>, series_id: &str, season_id: &str, list: &gtk::Box) {
    super::clear(list);
    list.append(&super::loading());
    let (series, season, list, weak) = (
        series_id.to_string(),
        season_id.to_string(),
        list.clone(),
        Rc::downgrade(ctx),
    );
    super::run(
        ctx,
        move |client| client.episodes(&series, &season),
        move |episodes| {
            let Some(ctx) = weak.upgrade() else { return };
            super::clear(&list);
            if episodes.is_empty() {
                list.append(
                    &gtk::Label::builder()
                        .label("No episodes")
                        .css_classes(["dim-label"])
                        .margin_top(24)
                        .build(),
                );
            }
            for episode in &episodes {
                list.append(&episode_row(&ctx, episode));
            }
        },
    );
}

fn episode_row(ctx: &Rc<Ctx>, episode: &api::Item) -> gtk::Widget {
    let row = gtk::Box::builder()
        .spacing(18)
        .css_classes(["episode-row"])
        .build();
    row.append(&cards::art(ctx, episode, Shape::Thumb, 256, 144));

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let number = episode
        .index_number
        .map(|n| format!("{n}. "))
        .unwrap_or_default();
    text.append(
        &gtk::Label::builder()
            .label(format!("{number}{}", episode.name))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["heading"])
            .build(),
    );
    let mut meta = Vec::new();
    if let Some(m) = episode.runtime_minutes() {
        meta.push(super::format_runtime(m));
    }
    if episode.resume_seconds() > 0.0 && !episode.played() {
        meta.push(format!(
            "stopped at {}",
            super::format_time(episode.resume_seconds())
        ));
    }
    if let Some(date) = episode.premiere_date.as_deref().and_then(|d| d.get(..10)) {
        meta.push(date.to_string());
    }
    if !meta.is_empty() {
        text.append(
            &gtk::Label::builder()
                .label(meta.join("  ·  "))
                .xalign(0.0)
                .css_classes(["dim-label", "caption"])
                .build(),
        );
    }
    if let Some(overview) = episode.overview.as_deref().filter(|o| !o.is_empty()) {
        text.append(
            &gtk::Label::builder()
                .label(overview)
                .xalign(0.0)
                .wrap(true)
                .lines(3)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["dim-label"])
                .build(),
        );
    }
    row.append(&text);

    let button = gtk::Button::builder()
        .child(&row)
        .css_classes(["flat", "card-button"])
        .tooltip_text("Play")
        .build();
    let (c, e) = (ctx.clone(), episode.clone());
    button.connect_clicked(move |_| super::play(&c, &e, false));
    cards::attach_item_menu(ctx, &button, episode);
    button.upcast()
}
