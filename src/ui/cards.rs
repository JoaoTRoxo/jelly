//! Poster and thumbnail cards, horizontal shelves of them, and right-click menus.

use super::Ctx;
use crate::{api, images};
use adw::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    /// 2:3, for movies and series
    Poster,
    /// 16:9, for episodes and Continue Watching
    Thumb,
}

impl Shape {
    pub fn size(self) -> (i32, i32) {
        match self {
            Shape::Poster => (168, 252),
            Shape::Thumb => (300, 169),
        }
    }
}

/// A rounded frame of `width`×`height` for a card's picture, and the picture
pub fn frame(width: i32, height: i32) -> (gtk::Overlay, gtk::Picture) {
    // The overlay takes its size from `sizer`; a picture as the main child would
    // take the image's pixel size instead
    let sizer = gtk::Box::builder()
        .width_request(width)
        .height_request(height)
        .build();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    let overlay = gtk::Overlay::builder()
        .child(&sizer)
        .css_classes(["art"])
        .overflow(gtk::Overflow::Hidden)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .build();
    overlay.add_overlay(&picture);
    (overlay, picture)
}

/// The picture of an item, with watched / unplayed / progress marks on it
pub fn art(ctx: &Rc<Ctx>, item: &api::Item, shape: Shape, width: i32, height: i32) -> gtk::Overlay {
    let (overlay, picture) = frame(width, height);
    let image = match shape {
        Shape::Poster => item.poster_image().or_else(|| item.thumb_image()),
        Shape::Thumb => item.thumb_image().or_else(|| item.poster_image()),
    };
    match (image, ctx.client()) {
        (Some((id, kind, tag)), Some(client)) => {
            // Twice the size, for HiDPI screens
            let url = client.image_url(&id, kind, &tag, (width * 2) as u32);
            images::set(&picture, &client, url);
        }
        _ => {
            let icon = gtk::Image::builder()
                .icon_name("folder-videos-symbolic")
                .pixel_size(48)
                .css_classes(["dim-label"])
                .build();
            overlay.add_overlay(&icon);
        }
    }

    let unplayed = item
        .user_data
        .as_ref()
        .and_then(|u| u.unplayed_item_count)
        .unwrap_or(0);
    if item.played() {
        let check = gtk::Image::builder()
            .icon_name("object-select-symbolic")
            .css_classes(["watched-badge"])
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .build();
        overlay.add_overlay(&check);
    } else if unplayed > 0 && item.kind == "Series" {
        let badge = gtk::Label::builder()
            .label(unplayed.to_string())
            .css_classes(["badge"])
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .build();
        overlay.add_overlay(&badge);
    }
    let progress = item.progress();
    if progress > 0.0 && !item.played() {
        let bar = gtk::ProgressBar::builder()
            .fraction(progress)
            .css_classes(["progress-strip"])
            .valign(gtk::Align::End)
            .build();
        overlay.add_overlay(&bar);
    }
    overlay
}

/// A clickable card: picture, title, and a dim second line
pub fn card(
    ctx: &Rc<Ctx>,
    item: &api::Item,
    shape: Shape,
    on_click: impl Fn() + 'static,
) -> gtk::Button {
    let (width, height) = shape.size();
    let art = art(ctx, item, shape, width, height);
    let (title, subtitle) = card_text(item, shape);
    // Library images already show the library's name
    let is_library = matches!(item.kind.as_str(), "CollectionFolder" | "UserView");
    let text = (!(is_library && item.image_tags.contains_key("Primary")))
        .then_some((title.as_str(), subtitle.as_str()));
    let button = card_button(&art, text, width, &item.name);
    button.connect_clicked(move |_| on_click());
    attach_item_menu(ctx, &button, item);
    button
}

/// `art` with a title and a dim second line under it, as a button
pub fn card_button(
    art: &gtk::Overlay,
    text: Option<(&str, &str)>,
    width: i32,
    tooltip: &str,
) -> gtk::Button {
    card_with_subtitle(art, text, width, tooltip).0
}

/// `card_button`, and its second line to change later
pub fn card_with_subtitle(
    art: &gtk::Overlay,
    text: Option<(&str, &str)>,
    width: i32,
    tooltip: &str,
) -> (gtk::Button, Option<gtk::Label>) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(art);
    let mut second_line = None;
    if let Some((title, subtitle)) = text {
        let title = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .width_request(width)
            .css_classes(["card-title"])
            .build();
        let subtitle = gtk::Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .width_request(width)
            .css_classes(["dim-label", "caption"])
            .build();
        content.append(&title);
        content.append(&subtitle);
        second_line = Some(subtitle);
    }
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["flat", "card-button", "media-card"])
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .tooltip_text(tooltip)
        .build();
    (button, second_line)
}

fn card_text(item: &api::Item, shape: Shape) -> (String, String) {
    match item.kind.as_str() {
        "Episode" => {
            let code = item.episode_code().unwrap_or_default();
            let series = item.series_name.clone().unwrap_or_default();
            if shape == Shape::Thumb {
                (series, format!("{code} · {}", item.name))
            } else {
                (item.name.clone(), format!("{series} · {code}"))
            }
        }
        "Series" => {
            let years = match (item.production_year, item.child_count) {
                (Some(y), Some(n)) => format!("{y} · {n} season{}", if n == 1 { "" } else { "s" }),
                (Some(y), None) => y.to_string(),
                _ => String::new(),
            };
            (item.name.clone(), years)
        }
        "CollectionFolder" | "UserView" => (item.name.clone(), String::new()),
        _ => {
            let mut parts = Vec::new();
            if let Some(y) = item.production_year {
                parts.push(y.to_string());
            }
            if let Some(m) = item.runtime_minutes() {
                parts.push(super::format_runtime(m));
            }
            (item.name.clone(), parts.join(" · "))
        }
    }
}

/// A titled, horizontally scrolling row of cards, running edge to edge, with
/// arrows that page through it. `see_all` makes the title a link.
pub fn shelf(
    title: &str,
    cards: Vec<impl IsA<gtk::Widget>>,
    see_all: Option<Box<dyn Fn()>>,
) -> gtk::Box {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["shelf-title"])
        .build();
    let heading: gtk::Widget = match see_all {
        Some(open) => {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            content.append(&label);
            content.append(
                &gtk::Image::builder()
                    .icon_name("go-next-symbolic")
                    .css_classes(["dim-label"])
                    .build(),
            );
            let link = gtk::Button::builder()
                .child(&content)
                .css_classes(["flat", "shelf-heading"])
                .halign(gtk::Align::Start)
                .build();
            link.connect_clicked(move |_| open());
            link.upcast()
        }
        None => {
            label.add_css_class("shelf-heading");
            label.upcast()
        }
    };
    let row = gtk::Box::builder()
        .spacing(4)
        .css_classes(["shelf-row"])
        .build();
    for card in cards {
        row.append(&card);
    }
    // No scrollbar: the arrows page through it
    let scroller = gtk::ScrolledWindow::builder()
        .child(&row)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hscrollbar_policy(gtk::PolicyType::External)
        .build();
    let overlay = gtk::Overlay::builder().child(&scroller).build();

    let adjustment = scroller.hadjustment();
    let target = adw::PropertyAnimationTarget::new(&adjustment, "value");
    let animation = adw::TimedAnimation::builder()
        .widget(&scroller)
        .duration(450)
        .target(&target)
        .build();
    let mut arrows = Vec::new();
    for (icon, align, direction) in [
        ("go-previous-symbolic", gtk::Align::Start, -1.0),
        ("go-next-symbolic", gtk::Align::End, 1.0),
    ] {
        let arrow = gtk::Button::builder()
            .icon_name(icon)
            .css_classes(["circular", "osd", "shelf-arrow"])
            .halign(align)
            .valign(gtk::Align::Center)
            .focusable(false)
            .visible(false)
            .build();
        let (adjustment, animation) = (adjustment.clone(), animation.clone());
        arrow.connect_clicked(move |_| {
            // Clicking again while it moves goes on from where it was headed
            let from = if animation.state() == adw::AnimationState::Playing {
                animation.value_to()
            } else {
                adjustment.value()
            };
            let to = (from + direction * adjustment.page_size() * 0.8).clamp(
                adjustment.lower(),
                adjustment.upper() - adjustment.page_size(),
            );
            animation.set_value_from(adjustment.value());
            animation.set_value_to(to);
            animation.play();
        });
        overlay.add_overlay(&arrow);
        arrows.push(arrow);
    }
    // Each arrow only shows while there's more to see that way
    let update = move |adjustment: &gtk::Adjustment| {
        arrows[0].set_visible(adjustment.value() > 1.0);
        arrows[1]
            .set_visible(adjustment.value() < adjustment.upper() - adjustment.page_size() - 1.0);
    };
    let update = Rc::new(update);
    let u = update.clone();
    adjustment.connect_value_changed(move |a| u(a));
    adjustment.connect_changed(move |a| update(a));

    let shelf = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["shelf"])
        .build();
    shelf.append(&heading);
    shelf.append(&overlay);
    shelf
}

/// An item's name as its logo art once that loads, else as text in a label
/// with `css_class`. Text only in light mode: logos are mostly white.
pub fn title(
    ctx: &Rc<Ctx>,
    item: &api::Item,
    css_class: &str,
    max_width: f64,
    max_height: f64,
) -> gtk::Widget {
    let label = gtk::Label::builder()
        .label(item.name.as_str())
        .xalign(0.0)
        .wrap(true)
        .css_classes([css_class])
        .build();
    let (Some(tag), Some(client), true) = (
        item.image_tags.get("Logo"),
        ctx.client(),
        adw::StyleManager::default().is_dark(),
    ) else {
        return label.upcast();
    };
    let logo = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .alternative_text(item.name.as_str())
        .build();
    let clamp = adw::Clamp::builder()
        .child(&logo)
        .halign(gtk::Align::Start)
        .visible(false)
        .build();
    let title = gtk::Box::new(gtk::Orientation::Vertical, 0);
    title.append(&label);
    title.append(&clamp);
    let (c, l) = (clamp.clone(), label.clone());
    logo.connect_paintable_notify(move |logo| {
        let Some(paintable) = logo.paintable() else {
            return;
        };
        // As wide as it can be without getting too tall
        let width = (max_height * paintable.intrinsic_aspect_ratio()).min(max_width) as i32;
        c.set_maximum_size(width);
        c.set_tightening_threshold(width);
        c.set_visible(true);
        l.set_visible(false);
    });
    images::set(
        &logo,
        &client,
        client.image_url(&item.id, "Logo", tag, (max_width * 2.0) as u32),
    );
    title.upcast()
}

/// A cast or crew member: round photo, name, and role
pub fn person(ctx: &Rc<Ctx>, person: &api::Person) -> gtk::Widget {
    let role = match (person.kind.as_str(), person.role.as_deref()) {
        (_, Some(role)) if !role.is_empty() => role.to_string(),
        (kind, _) => kind.to_string(),
    };
    let photo: Option<Photo> = match (&person.primary_image_tag, ctx.client()) {
        (Some(tag), Some(client)) => {
            let url = client.image_url(&person.id, "Primary", tag, (PERSON_SIZE * 2) as u32);
            Some(Box::new(move |picture| images::set(picture, &client, url)))
        }
        _ => None,
    };
    person_card(&person.name, &role, photo)
}

const PERSON_SIZE: i32 = 112;

/// Puts a photo in a person card's picture, from wherever it comes
pub type Photo = Box<dyn FnOnce(&gtk::Picture)>;

/// A round photo (put in its picture by `photo`), a name, and a role
pub fn person_card(name: &str, role: &str, photo: Option<Photo>) -> gtk::Widget {
    let sizer = gtk::Box::builder()
        .width_request(PERSON_SIZE)
        .height_request(PERSON_SIZE)
        .build();
    let frame = gtk::Overlay::builder()
        .child(&sizer)
        .css_classes(["avatar-art"])
        .overflow(gtk::Overflow::Hidden)
        .halign(gtk::Align::Center)
        .build();
    match photo {
        Some(photo) => {
            let picture = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Cover)
                .can_shrink(true)
                .build();
            photo(&picture);
            frame.add_overlay(&picture);
        }
        None => frame.add_overlay(
            &gtk::Image::builder()
                .icon_name("avatar-default-symbolic")
                .pixel_size(48)
                .css_classes(["dim-label"])
                .build(),
        ),
    }
    let text = |text: &str, classes: &[&str]| {
        gtk::Label::builder()
            .label(text)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .width_request(PERSON_SIZE + 16)
            .css_classes(classes.to_vec())
            .build()
    };
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(6)
        .margin_end(6)
        .tooltip_text(format!("{name} · {role}"))
        .build();
    card.append(&frame);
    card.append(&text(name, &["card-title"]));
    card.append(&text(role, &["dim-label", "caption"]));
    card.upcast()
}

/// Show a menu of `entries` at the pointer when `widget` is right-clicked
pub fn attach_menu(
    widget: &impl IsA<gtk::Widget>,
    entries: impl Fn() -> Vec<(String, Box<dyn Fn()>)> + 'static,
) {
    let click = gtk::GestureClick::builder()
        .button(gtk::gdk::BUTTON_SECONDARY)
        .build();
    let anchor = widget.as_ref().downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(anchor) = anchor.upgrade() else {
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::builder()
            .child(&list)
            .has_arrow(false)
            .halign(gtk::Align::Start)
            .build();
        popover.set_parent(&anchor);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        for (label, action) in entries() {
            let button = gtk::Button::builder()
                .label(label)
                .css_classes(["flat"])
                .build();
            if let Some(l) = button.child().and_downcast::<gtk::Label>() {
                l.set_xalign(0.0);
            }
            let weak = popover.downgrade();
            button.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.popdown();
                }
                action();
            });
            list.append(&button);
        }
        popover.connect_closed(|p| {
            let p = p.clone();
            gtk::glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    });
    widget.add_controller(click);
}

/// Right-click menu with what can be done with an item
pub fn attach_item_menu(ctx: &Rc<Ctx>, widget: &impl IsA<gtk::Widget>, item: &api::Item) {
    let ctx = Rc::downgrade(ctx);
    let item = item.clone();
    attach_menu(widget, move || {
        let Some(ctx) = ctx.upgrade() else {
            return Vec::new();
        };
        let mut entries: Vec<(String, Box<dyn Fn()>)> = Vec::new();
        let playable = matches!(item.kind.as_str(), "Episode" | "Movie" | "Video");
        if playable {
            let (c, i) = (ctx.clone(), item.clone());
            let label = if item.resume_seconds() > 0.0 {
                format!("Resume from {}", super::format_time(item.resume_seconds()))
            } else {
                "Play".into()
            };
            entries.push((label, Box::new(move || super::play(&c, &i, false))));
            if item.resume_seconds() > 0.0 {
                let (c, i) = (ctx.clone(), item.clone());
                entries.push((
                    "Play from the Beginning".into(),
                    Box::new(move || super::play(&c, &i, true)),
                ));
            }
        }
        if item.kind == "Episode" && item.series_id.is_some() {
            let (c, i) = (ctx.clone(), item.clone());
            entries.push(("Go to Series".into(), Box::new(move || super::open(&c, &i))));
        }
        if playable || item.kind == "Series" || item.kind == "Season" {
            let (c, id, played) = (ctx.clone(), item.id.clone(), item.played());
            let label = if played {
                "Mark as Unwatched"
            } else {
                "Mark as Watched"
            };
            entries.push((label.into(), Box::new(move || set_played(&c, &id, !played))));
        }
        entries
    });
}

pub fn set_played(ctx: &Rc<Ctx>, item_id: &str, played: bool) {
    let id = item_id.to_string();
    let c = ctx.clone();
    super::run(
        ctx,
        move |client| client.set_played(&id, played),
        move |_| {
            c.toast(if played {
                "Marked as watched"
            } else {
                "Marked as unwatched"
            });
            c.refresh_visible();
        },
    );
}
