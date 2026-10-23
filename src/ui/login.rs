//! Sign in with a username and password, or with Quick Connect (approve a
//! code from another device that's signed in).

use super::Ctx;
use crate::{api, session};
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

pub fn page(ctx: &Rc<Ctx>) -> adw::NavigationPage {
    let last = session::settings().last_server;
    let server = adw::EntryRow::builder()
        .title("Server")
        .text(last.as_str())
        .build();
    let username = adw::EntryRow::builder().title("Username").build();
    let password = adw::PasswordEntryRow::builder().title("Password").build();
    let fields = adw::PreferencesGroup::new();
    fields.add(&server);
    fields.add(&username);
    fields.add(&password);

    let sign_in = gtk::Button::builder()
        .label("Sign In")
        .css_classes(["pill", "suggested-action"])
        .halign(gtk::Align::Center)
        .build();
    let quick = gtk::Button::builder()
        .label("Use Quick Connect")
        .css_classes(["pill", "flat"])
        .halign(gtk::Align::Center)
        .build();
    let status = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .visible(false)
        .build();
    let code = gtk::Label::builder()
        .css_classes(["title-1", "numeric"])
        .selectable(true)
        .visible(false)
        .build();
    let spinner = adw::Spinner::builder()
        .visible(false)
        .halign(gtk::Align::Center)
        .build();

    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .valign(gtk::Align::Center)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    let icon = gtk::Image::builder()
        .icon_name("video-display-symbolic")
        .pixel_size(96)
        .css_classes(["dim-label"])
        .build();
    column.append(&icon);
    column.append(
        &gtk::Label::builder()
            .label("Sign in to Jellyfin")
            .css_classes(["title-1"])
            .build(),
    );
    column.append(&fields);
    column.append(&status);
    column.append(&code);
    column.append(&spinner);
    column.append(&sign_in);
    column.append(&quick);
    let clamp = adw::Clamp::builder()
        .maximum_size(440)
        .child(&column)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    toolbar.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    ));
    let page = adw::NavigationPage::builder()
        .title("Sign In")
        .tag("login")
        .child(&toolbar)
        .build();

    // Bumped to stop a Quick Connect wait that's no longer wanted
    let attempt = Rc::new(Cell::new(0u64));

    let busy = {
        let (sign_in, quick, spinner, fields) = (
            sign_in.clone(),
            quick.clone(),
            spinner.clone(),
            fields.clone(),
        );
        move |on: bool| {
            sign_in.set_sensitive(!on);
            quick.set_sensitive(!on);
            fields.set_sensitive(!on);
            spinner.set_visible(on);
        }
    };
    let show = {
        let status = status.clone();
        move |message: &str, error: bool| {
            status.set_label(message);
            status.set_visible(!message.is_empty());
            if error {
                status.add_css_class("error");
            } else {
                status.remove_css_class("error");
            }
        }
    };

    let submit = {
        let (ctx, server, username, password, busy, show, attempt, code) = (
            Rc::downgrade(ctx),
            server.clone(),
            username.clone(),
            password.clone(),
            busy.clone(),
            show.clone(),
            attempt.clone(),
            code.clone(),
        );
        move || {
            let Some(ctx) = ctx.upgrade() else { return };
            attempt.set(attempt.get() + 1);
            code.set_visible(false);
            let client = api::Client::new(&server.text(), &ctx.device_id);
            let (user, pass) = (
                username.text().trim().to_string(),
                password.text().to_string(),
            );
            if server.text().trim().is_empty() {
                show("Enter your server's address", true);
                server.grab_focus();
                return;
            }
            if user.is_empty() {
                show("Enter your username", true);
                username.grab_focus();
                return;
            }
            busy(true);
            show("", false);
            let (weak, busy, show) = (Rc::downgrade(&ctx), busy.clone(), show.clone());
            let server_url = client.server.clone();
            let secret = pass.clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    client
                        .check_server()
                        .and_then(|_| client.authenticate(&user, &pass))
                })
                .await;
                busy(false);
                match result {
                    Ok(Ok(auth)) => {
                        if let Some(ctx) = weak.upgrade() {
                            let name = auth.user.name.clone();
                            ctx.sign_in(session::Session {
                                server: server_url.clone(),
                                user_id: auth.user.id,
                                user_name: auth.user.name,
                                token: auth.access_token,
                            });
                            // Seerr signs in with the same account
                            super::request::sign_in_too(&ctx, server_url, name, secret);
                        }
                    }
                    Ok(Err(api::Error::Unauthorized)) => show("Wrong username or password", true),
                    Ok(Err(e)) => show(&e.to_string(), true),
                    Err(_) => show("Something went wrong", true),
                }
            });
        }
    };
    let submit = Rc::new(submit);
    let s = submit.clone();
    sign_in.connect_clicked(move |_| s());
    let s = submit.clone();
    password.connect_entry_activated(move |_| s());
    let p = password.clone();
    username.connect_entry_activated(move |_| {
        p.grab_focus();
    });

    let (weak, server_row) = (Rc::downgrade(ctx), server.clone());
    quick.connect_clicked(move |_| {
        let Some(ctx) = weak.upgrade() else { return };
        if server_row.text().trim().is_empty() {
            show("Enter your server's address", true);
            server_row.grab_focus();
            return;
        }
        let client = api::Client::new(&server_row.text(), &ctx.device_id);
        let this_attempt = attempt.get() + 1;
        attempt.set(this_attempt);
        busy(true);
        show("", false);
        let (weak, busy, show, attempt, code) = (Rc::downgrade(&ctx), busy.clone(), show.clone(), attempt.clone(), code.clone());
        glib::spawn_future_local(async move {
            let c = client.clone();
            let started = gio::spawn_blocking(move || {
                if !c.quick_connect_enabled()? {
                    return Err(api::Error::Http(0, "Quick Connect is turned off on this server".into()));
                }
                c.quick_connect_start()
            })
            .await;
            let qc = match started {
                Ok(Ok(qc)) => qc,
                Ok(Err(e)) => {
                    busy(false);
                    return show(&e.to_string(), true);
                }
                Err(_) => {
                    busy(false);
                    return show("Something went wrong", true);
                }
            };
            code.set_label(&qc.code);
            code.set_visible(true);
            show("In Jellyfin on another device, open your profile → Quick Connect and enter this code", false);
            // Wait for approval, for up to 5 minutes
            for _ in 0..100 {
                glib::timeout_future(Duration::from_secs(3)).await;
                if attempt.get() != this_attempt || weak.upgrade().is_none() {
                    return;
                }
                let (c, secret) = (client.clone(), qc.secret.clone());
                let state = gio::spawn_blocking(move || c.quick_connect_state(&secret)).await;
                if let Ok(Ok(state)) = state
                    && state.authenticated {
                        let (c, secret) = (client.clone(), qc.secret.clone());
                        let auth = gio::spawn_blocking(move || c.quick_connect_finish(&secret)).await;
                        busy(false);
                        code.set_visible(false);
                        match auth {
                            Ok(Ok(auth)) => {
                                if let Some(ctx) = weak.upgrade() {
                                    ctx.sign_in(session::Session { server: client.server.clone(), user_id: auth.user.id, user_name: auth.user.name, token: auth.access_token });
                                }
                            }
                            Ok(Err(e)) => show(&e.to_string(), true),
                            Err(_) => show("Something went wrong", true),
                        }
                        return;
                    }
            }
            busy(false);
            code.set_visible(false);
            show("The Quick Connect code expired, try again", true);
        });
    });

    let u = username.clone();
    page.connect_shown(move |_| {
        if server.text().is_empty() {
            server.grab_focus();
        } else {
            u.grab_focus();
        }
    });
    page
}
