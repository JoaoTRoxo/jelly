//! The window, navigation between pages, and helpers shared by the pages.

pub mod cards;
pub mod home;
pub mod item;
pub mod library;
pub mod login;
pub mod player;
pub mod request;

use crate::{api, session};
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct Ctx {
    pub app: adw::Application,
    pub window: adw::ApplicationWindow,
    pub nav: adw::NavigationView,
    pub toasts: adw::ToastOverlay,
    client: RefCell<Option<api::Client>>,
    pub device_id: String,
    /// Bumped when watch state changes, so pages reload when shown again
    pub generation: Cell<u64>,
}

impl Ctx {
    pub fn new(app: &adw::Application) -> Rc<Ctx> {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(api::CLIENT_NAME)
            .default_width(1400)
            .default_height(860)
            .build();
        let nav = adw::NavigationView::new();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&nav));
        window.set_content(Some(&toasts));
        let ctx = Rc::new(Ctx {
            app: app.clone(),
            window,
            nav,
            toasts,
            client: RefCell::new(None),
            device_id: session::device_id(),
            generation: Cell::new(0),
        });
        ctx.add_shortcut("search", &["<Control>f"], |ctx| {
            ctx.nav.push(&library::search_page(ctx))
        });
        ctx.add_shortcut("refresh", &["F5", "<Control>r"], |ctx| {
            ctx.refresh_visible()
        });
        ctx
    }

    /// A window action with keyboard shortcuts, active on the browsing pages
    fn add_shortcut(self: &Rc<Self>, name: &str, accels: &[&str], f: impl Fn(&Rc<Ctx>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            let Some(ctx) = weak.upgrade() else { return };
            let tag = ctx.nav.visible_page().and_then(|p| p.tag());
            if ctx.client().is_some()
                && !matches!(tag.as_deref(), Some("player" | "search" | "login"))
            {
                f(&ctx);
            }
        });
        self.window.add_action(&action);
        self.app
            .set_accels_for_action(&format!("win.{name}"), accels);
    }

    pub fn client(&self) -> Option<api::Client> {
        self.client.borrow().clone()
    }

    pub fn toast(&self, message: &str) {
        // Toasts take markup: "Fast & Furious" would show nothing
        self.toasts
            .add_toast(adw::Toast::new(&glib::markup_escape_text(message)));
    }

    /// Watch state changed (played something, marked watched)
    pub fn changed(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    /// Reload the page on screen (pages reload when shown after a change)
    pub fn refresh_visible(&self) {
        self.changed();
        if let Some(page) = self.nav.visible_page() {
            page.emit_by_name::<()>("showing", &[]);
        }
    }

    /// Start (or restore) a signed-in session and show the home page
    pub fn sign_in(self: &Rc<Self>, s: session::Session) {
        let client = api::Client::new(&s.server, &self.device_id).signed_in(&s.token, &s.user_id);
        *self.client.borrow_mut() = Some(client);
        let mut settings = session::settings();
        settings.last_server = s.server.clone();
        session::save_settings(&settings);
        session::save(&s);
        self.nav.replace(&[home::page(self)]);
    }

    pub fn sign_out(self: &Rc<Self>, tell_server: bool) {
        if let Some(client) = self.client.borrow_mut().take()
            && tell_server
        {
            gio::spawn_blocking(move || {
                let _ = client.logout();
            });
        }
        session::clear();
        self.nav.replace(&[login::page(self)]);
    }
}

/// Run `work` with the API client on a worker thread, then `done` with its
/// result on the main thread. Signs out if the session was revoked.
pub fn run_result<T: Send + 'static>(
    ctx: &Rc<Ctx>,
    work: impl FnOnce(api::Client) -> api::Result<T> + Send + 'static,
    done: impl FnOnce(api::Result<T>) + 'static,
) {
    let Some(client) = ctx.client() else { return };
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || work(client))
            .await
            .unwrap_or_else(|_| Err(api::Error::Decode("internal error".into())));
        if let Err(api::Error::Unauthorized) = result {
            ctx.toast("Your session ended, sign in again");
            ctx.sign_out(false);
            return;
        }
        done(result);
    });
}

/// Like `run_result`, showing errors as a toast
pub fn run<T: Send + 'static>(
    ctx: &Rc<Ctx>,
    work: impl FnOnce(api::Client) -> api::Result<T> + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let toast_ctx = ctx.clone();
    run_result(ctx, work, move |result| match result {
        Ok(value) => done(value),
        Err(e) => toast_ctx.toast(&e.to_string()),
    });
}

/// Open an item's page (or play it, for things that have no page)
pub fn open(ctx: &Rc<Ctx>, item: &api::Item) {
    match item.kind.as_str() {
        "Series" | "Movie" | "Video" => ctx.nav.push(&item::page(ctx, item.clone(), None)),
        "Season" => {
            if let Some(series) = &item.series_id {
                let series = api::Item {
                    id: series.clone(),
                    kind: "Series".into(),
                    name: item.series_name.clone().unwrap_or_default(),
                    ..Default::default()
                };
                ctx.nav
                    .push(&item::page(ctx, series, Some(item.id.clone())));
            }
        }
        "Episode" => {
            if let Some(series) = &item.series_id {
                let series = api::Item {
                    id: series.clone(),
                    kind: "Series".into(),
                    name: item.series_name.clone().unwrap_or_default(),
                    ..Default::default()
                };
                ctx.nav
                    .push(&item::page(ctx, series, item.season_id.clone()));
            }
        }
        _ if item.is_folder_like() => ctx.nav.push(&library::page(ctx, item.clone())),
        _ => play(ctx, item, false),
    }
}

/// Play an item, resuming where it was left unless `from_start`
pub fn play(ctx: &Rc<Ctx>, item: &api::Item, from_start: bool) {
    let start = if from_start {
        0.0
    } else {
        item.resume_seconds()
    };
    player::open(
        ctx,
        player::Media::Jellyfin {
            item: Box::new(item.clone()),
            start,
        },
    );
}

/// "1:02:03" or "12:03"
pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as i64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "1 h 42 min" or "24 min"
pub fn format_runtime(minutes: i64) -> String {
    if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// Remove every child of a box
pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// A page whose content reloads when it's shown again after watch state changed
pub fn reloading_page(ctx: &Rc<Ctx>, page: &adw::NavigationPage, load: impl Fn() + 'static) {
    let seen = Cell::new(u64::MAX);
    let ctx = Rc::downgrade(ctx);
    page.connect_showing(move |_| {
        let Some(ctx) = ctx.upgrade() else { return };
        if seen.get() != ctx.generation.get() {
            seen.set(ctx.generation.get());
            load();
        }
    });
}

/// A header bar over a hero picture at the top of `scroller`: clear at the
/// top, solid once anything scrolls under it
pub fn header_over_hero(header: &adw::HeaderBar, scroller: &gtk::ScrolledWindow) {
    header.add_css_class("over-hero");
    let header = header.downgrade();
    let update = move |adjustment: &gtk::Adjustment| {
        let Some(header) = header.upgrade() else {
            return;
        };
        if adjustment.value() > 0.0 {
            header.add_css_class("scrolled");
        } else {
            header.remove_css_class("scrolled");
        }
    };
    let adjustment = scroller.vadjustment();
    update(&adjustment);
    adjustment.connect_value_changed(update);
}

pub fn loading() -> gtk::Widget {
    let spinner = adw::Spinner::builder()
        .width_request(48)
        .height_request(48)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    spinner.upcast()
}

pub fn error_page(message: &str, retry: impl Fn() + 'static) -> gtk::Widget {
    let button = gtk::Button::builder()
        .label("Try Again")
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    button.connect_clicked(move |_| retry());
    adw::StatusPage::builder()
        .icon_name("network-error-symbolic")
        .title("Couldn't load this")
        .description(glib::markup_escape_text(message).as_str())
        .child(&button)
        .vexpand(true)
        .build()
        .upcast()
}
