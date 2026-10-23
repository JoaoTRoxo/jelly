//! Requesting (a movie after a yes, a show's seasons after choosing them),
//! and signing in to Seerr with the Jellyfin password.

use super::*;
use crate::seerr::{self, Details, Found};
use crate::session;
use crate::ui::Ctx;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

/// Request it: a movie after a yes, a show after choosing its seasons. `then`
/// runs once it's done.
pub(super) fn ask_to_request(ctx: &Rc<Ctx>, details: &Details, then: Rc<dyn Fn()>) {
    if client().is_none() {
        sign_in(ctx, None, None, then);
        return;
    }
    if details.is_show() {
        choose_seasons(ctx, details, then);
    } else {
        confirm_movie(ctx, details, then);
    }
}

fn confirm_movie(ctx: &Rc<Ctx>, details: &Details, then: Rc<dyn Fn()>) {
    let found = details.found();
    let overview: String = found
        .overview
        .as_deref()
        .unwrap_or_default()
        .chars()
        .take(400)
        .collect();
    let dialog = adw::AlertDialog::new(
        Some(&format!("Request {}?", name_year(&found))),
        Some(&overview),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("request", "Request")]);
    dialog.set_response_appearance("request", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("request"));
    dialog.set_close_response("cancel");
    let c = Rc::downgrade(ctx);
    dialog.connect_response(Some("request"), move |_, _| {
        if let Some(ctx) = c.upgrade() {
            send(&ctx, &found, Vec::new(), then.clone());
        }
    });
    dialog.present(Some(&ctx.window));
}

/// A show's seasons to tick: the missing ones ticked, the rest greyed out with why
fn choose_seasons(ctx: &Rc<Ctx>, details: &Details, then: Rc<dyn Fn()>) {
    let found = details.found();
    if details.requestable().is_empty() {
        ctx.toast(&format!(
            "Every season of “{}” is requested or in the library",
            found.title()
        ));
        return;
    }
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let mut ticks = Vec::new();
    for season in details.seasons.iter().filter(|s| s.season_number > 0) {
        let number = season.season_number;
        let taken = details.season_taken(number);
        let tick = gtk::CheckButton::builder()
            .active(true)
            .sensitive(taken.is_none())
            .valign(gtk::Align::Center)
            .build();
        let detail = match (taken, season.episode_count) {
            (Some(why), _) => why.to_string(),
            (None, Some(n)) => format!("{n} episode{}", if n == 1 { "" } else { "s" }),
            (None, None) => String::new(),
        };
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&season.title()).as_str())
            .subtitle(detail)
            .activatable_widget(&tick)
            .build();
        row.add_prefix(&tick);
        list.append(&row);
        if taken.is_none() {
            ticks.push((number, tick));
        }
    }
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(360)
        .build();
    let dialog = adw::AlertDialog::new(
        Some(&format!("Request {}", name_year(&found))),
        Some("Which seasons?"),
    );
    dialog.set_extra_child(Some(&scroller));
    dialog.add_responses(&[("cancel", "Cancel"), ("request", "Request")]);
    dialog.set_response_appearance("request", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("request"));
    dialog.set_close_response("cancel");
    // Something to request, or nothing to do
    let ticks = Rc::new(ticks);
    for (_, tick) in ticks.iter() {
        let (d, t) = (dialog.downgrade(), ticks.clone());
        tick.connect_toggled(move |_| {
            if let Some(dialog) = d.upgrade() {
                dialog.set_response_enabled("request", t.iter().any(|(_, tick)| tick.is_active()));
            }
        });
    }
    let c = Rc::downgrade(ctx);
    dialog.connect_response(Some("request"), move |_, _| {
        let seasons = ticks
            .iter()
            .filter(|(_, tick)| tick.is_active())
            .map(|(number, _)| *number)
            .collect();
        if let Some(ctx) = c.upgrade() {
            send(&ctx, &found, seasons, then.clone());
        }
    });
    dialog.present(Some(&ctx.window));
}

fn send(ctx: &Rc<Ctx>, found: &Found, seasons: Vec<i32>, then: Rc<dyn Fn()>) {
    let Some(client) = client() else {
        sign_in(ctx, None, None, then);
        return;
    };
    let (c, found) = (Rc::downgrade(ctx), found.clone());
    let title = found.title().to_string();
    run(
        client,
        move |client| client.request(&found, &seasons),
        move |result| {
            let Some(ctx) = c.upgrade() else { return };
            match result {
                Ok(true) => ctx.toast(&format!("Requested “{title}”")),
                Ok(false) => ctx.toast(&format!("Requested “{title}”: it needs approval")),
                Err(seerr::Error::SignIn) => {
                    session::clear_seerr();
                    sign_in(&ctx, None, None, then);
                    return;
                }
                Err(e) => ctx.toast(&e.to_string()),
            }
            then();
        },
    );
}

/// Sign in to Seerr with the Jellyfin account's password, then run `then`.
/// `problem`: why the last try didn't work; `url`: the address tried.
pub fn sign_in(ctx: &Rc<Ctx>, problem: Option<&str>, url: Option<String>, then: Rc<dyn Fn()>) {
    let Some(account) = session::load() else {
        return;
    };
    let saved = Some(session::settings().seerr_url).filter(|u| !u.is_empty());
    if let Some(url) = url.or(saved) {
        sign_in_dialog(ctx, problem, url, then);
        return;
    }
    // Not signed in before: look for Seerr next to the Jellyfin server first
    let (c, server, problem) = (
        Rc::downgrade(ctx),
        account.server,
        problem.map(String::from),
    );
    glib::spawn_future_local(async move {
        let fallback = seerr::default_url(&server);
        let found = gio::spawn_blocking(move || seerr::find(&server))
            .await
            .ok()
            .flatten();
        if let Some(ctx) = c.upgrade() {
            sign_in_dialog(&ctx, problem.as_deref(), found.unwrap_or(fallback), then);
        }
    });
}

fn sign_in_dialog(ctx: &Rc<Ctx>, problem: Option<&str>, url: String, then: Rc<dyn Fn()>) {
    let Some(account) = session::load() else {
        return;
    };
    let address = adw::EntryRow::builder()
        .title("Seerr address")
        .text(&url)
        .build();
    let password = adw::PasswordEntryRow::builder()
        .title(format!("Jellyfin password for {}", account.user_name))
        .activates_default(true)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&address);
    list.append(&password);
    let body = problem.unwrap_or(
        "Seerr checks your Jellyfin account. The app keeps Seerr's session, not your password.",
    );
    let dialog = adw::AlertDialog::new(Some("Sign In to Seerr"), Some(body));
    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[("cancel", "Cancel"), ("sign-in", "Sign In")]);
    dialog.set_response_appearance("sign-in", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("sign-in"));
    dialog.set_close_response("cancel");
    let (c, entry) = (Rc::downgrade(ctx), password.clone());
    dialog.connect_response(Some("sign-in"), move |_, _| {
        let (url, secret, user) = (
            address.text().trim().to_string(),
            entry.text().to_string(),
            account.user_name.clone(),
        );
        let (c, tried, then) = (c.clone(), url.clone(), then.clone());
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || seerr::sign_in(&url, &user, &secret)).await;
            let Some(ctx) = c.upgrade() else { return };
            match result.unwrap_or_else(|_| Err(seerr::Error::Server("Internal error".into()))) {
                Ok(client) => {
                    session::save_seerr(&session::SeerrSession {
                        url: client.url.clone(),
                        cookie: client.cookie.clone(),
                    });
                    let mut settings = session::settings();
                    settings.seerr_url = client.url;
                    session::save_settings(&settings);
                    ctx.toast("Signed in to Seerr");
                    then();
                }
                Err(e) => sign_in(&ctx, Some(&e.to_string()), Some(tried), then),
            }
        });
    });
    dialog.present(Some(&ctx.window));
    dialog.set_focus(Some(&password));
}
