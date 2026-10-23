//! Development only (the `screenshot` feature; see tools/screenshot.sh):
//! JELLY_SHOT=out.png renders the window to a PNG after JELLY_SHOT_DELAY ms,
//! then quits. At that point JELLY_SHOT_ACTION activates a window action (e.g.
//! "search") and JELLY_SHOT_TYPE types into the search entry; a moment later
//! (JELLY_SHOT_SETTLE ms) JELLY_SHOT_CLICK clicks the button with that label
//! or tooltip ("text#n" for the nth),
//! JELLY_SHOT_SCROLL scrolls the page that many pixels, JELLY_SHOT_HOVER
//! (comma-separated CSS classes, "class#n" for the nth) puts widgets in
//! :hover, and JELLY_SHOT_NO_ANIM turns animations off. The picture is taken
//! JELLY_SHOT_WAIT ms after that.

use adw::prelude::*;
use gtk::{glib, gsk};
use std::time::Duration;

fn mapped_with_class(widget: &gtk::Widget, class: &str, found: &mut Vec<gtk::Widget>) {
    if widget.has_css_class(class) && widget.is_mapped() {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        mapped_with_class(&c, class, found);
        child = c.next_sibling();
    }
}

/// The mapped widgets of type `T`
fn mapped<T: IsA<gtk::Widget>>(widget: &gtk::Widget, found: &mut Vec<T>) {
    if let Some(w) = widget.downcast_ref::<T>().filter(|w| w.is_mapped()) {
        found.push(w.clone());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        mapped(&c, found);
        child = c.next_sibling();
    }
}

fn in_window<T: IsA<gtk::Widget>>(window: &adw::ApplicationWindow) -> Vec<T> {
    let mut found = Vec::new();
    mapped(window.upcast_ref(), &mut found);
    found
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn schedule(window: &adw::ApplicationWindow) {
    let Some(out) = var("JELLY_SHOT") else { return };
    let delay = var("JELLY_SHOT_DELAY")
        .and_then(|d| d.parse().ok())
        .unwrap_or(7000);
    let window = window.clone();
    glib::timeout_add_local_once(Duration::from_millis(delay), move || {
        // Before anything opens: on Broadway, animations don't run to the end
        if var("JELLY_SHOT_NO_ANIM").is_some()
            && let Some(settings) = gtk::Settings::default()
        {
            settings.set_gtk_enable_animations(false);
        }
        if let Some(action) = var("JELLY_SHOT_ACTION") {
            let _ = WidgetExt::activate_action(&window, &format!("win.{action}"), None);
        }
        let typed = var("JELLY_SHOT_TYPE");
        if let Some(text) = &typed {
            match in_window::<gtk::SearchEntry>(&window).first() {
                Some(entry) => entry.set_text(text),
                None => eprintln!("screenshot: no search entry on screen"),
            }
        }
        let settle = var("JELLY_SHOT_SETTLE")
            .and_then(|d| d.parse().ok())
            .unwrap_or(if typed.is_some() { 4000 } else { 0 });
        let (window, out) = (window.clone(), out.clone());
        glib::timeout_add_local_once(Duration::from_millis(settle), move || act(&window, out));
    });
}

/// Click, scroll and hover as asked, then take the picture
fn act(window: &adw::ApplicationWindow, out: String) {
    let window = window.clone();
    if let Some(spec) = var("JELLY_SHOT_CLICK") {
        // A button's label or tooltip; "text#n" for the nth with it
        let (text, n) = spec
            .rsplit_once('#')
            .and_then(|(t, n)| Some((t, n.parse().ok()?)))
            .unwrap_or((spec.as_str(), 0));
        let matches = |b: &gtk::Button| {
            b.label().as_deref() == Some(text) || b.tooltip_text().as_deref() == Some(text)
        };
        match in_window::<gtk::Button>(&window)
            .into_iter()
            .filter(matches)
            .nth(n)
        {
            Some(button) => button.emit_clicked(),
            None => eprintln!("screenshot: no “{spec}” button on screen"),
        }
    }
    if let Some(y) = var("JELLY_SHOT_SCROLL").and_then(|y| y.parse::<f64>().ok()) {
        let found = in_window::<gtk::ScrolledWindow>(&window);
        if let Some(adjustment) = found
            .iter()
            .map(|s| s.vadjustment())
            .find(|a| a.upper() > a.page_size())
        {
            adjustment.set_value(y);
        }
    }
    for spec in var("JELLY_SHOT_HOVER")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let (class, n) = spec
            .split_once('#')
            .map_or((spec, 0), |(c, n)| (c, n.parse().unwrap_or(0)));
        let mut found = Vec::new();
        mapped_with_class(window.upcast_ref(), class, &mut found);
        match found.get(n) {
            Some(widget) => widget.set_state_flags(gtk::StateFlags::PRELIGHT, false),
            None => eprintln!("screenshot: no .{spec} on screen"),
        }
    }
    // Give styles and layout a moment to follow
    let wait = var("JELLY_SHOT_WAIT")
        .and_then(|d| d.parse().ok())
        .unwrap_or(800);
    glib::timeout_add_local_once(Duration::from_millis(wait), move || {
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&window)).snapshot(
            &snapshot,
            window.width().into(),
            window.height().into(),
        );
        let renderer = gsk::CairoRenderer::new();
        match snapshot
            .to_node()
            .map(|node| (renderer.realize(None), node))
        {
            Some((Ok(()), node)) => {
                match renderer.render_texture(&node, None).save_to_png(&out) {
                    Ok(()) => eprintln!("screenshot: saved {out}"),
                    Err(e) => eprintln!("screenshot: couldn't save {out}: {e}"),
                }
                renderer.unrealize();
            }
            _ => eprintln!("screenshot: couldn't render the window"),
        }
        if let Some(app) = window.application() {
            app.quit();
        }
    });
}
