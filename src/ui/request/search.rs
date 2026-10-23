//! The search page's part for what isn't in the library: Seerr's matches, to
//! request.

use super::*;
use crate::seerr::{self, Found};
use crate::session;
use crate::ui::Ctx;
use adw::prelude::*;
use gtk::gio;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// The search page's part for what isn't in the library
pub struct Section {
    pub root: gtk::Box,
    heading: gtk::Box,
    grid: gtk::FlowBox,
    sign_in: gtk::Box,
    sign_in_text: gtk::Label,
    message: gtk::Label,
    spinner: adw::Spinner,
    ctx: Weak<Ctx>,
    query: RefCell<String>,
    /// Bumped by each search, so answers to older ones are dropped
    generation: Cell<u64>,
}

impl Section {
    pub fn new(ctx: &Rc<Ctx>) -> Rc<Section> {
        let title = gtk::Label::builder()
            .label("Request")
            .css_classes(["shelf-title"])
            .build();
        let source = gtk::Label::builder()
            .label("from Seerr")
            .css_classes(["dim-label"])
            .build();
        let sign_out = gtk::Button::builder()
            .label("Sign Out of Seerr")
            .css_classes(["flat"])
            .halign(gtk::Align::End)
            .hexpand(true)
            .build();
        let heading = gtk::Box::builder()
            .spacing(10)
            .margin_start(6)
            .visible(false)
            .build();
        heading.append(&title);
        heading.append(&source);
        heading.append(&sign_out);

        let sign_in_text = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .build();
        let sign_in_button = gtk::Button::builder()
            .label("Sign In to Seerr")
            .css_classes(["pill", "suggested-action"])
            .valign(gtk::Align::Center)
            .build();
        let callout = gtk::Box::builder()
            .spacing(16)
            .css_classes(["card", "request-callout"])
            .visible(false)
            .build();
        callout.append(
            &gtk::Image::builder()
                .icon_name("folder-download-symbolic")
                .pixel_size(24)
                .css_classes(["dim-label"])
                .build(),
        );
        callout.append(&sign_in_text);
        callout.append(&sign_in_button);

        let message = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .margin_start(6)
            .css_classes(["dim-label"])
            .visible(false)
            .build();
        let spinner = adw::Spinner::builder()
            .width_request(32)
            .height_request(32)
            .halign(gtk::Align::Center)
            .margin_top(12)
            .visible(false)
            .build();
        let grid = crate::ui::library::grid();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(14)
            .margin_top(28)
            .visible(false)
            .build();
        for widget in [
            heading.upcast_ref::<gtk::Widget>(),
            callout.upcast_ref(),
            spinner.upcast_ref(),
            message.upcast_ref(),
            grid.upcast_ref(),
        ] {
            root.append(widget);
        }

        let section = Rc::new(Section {
            root,
            heading,
            grid,
            sign_in: callout,
            sign_in_text,
            message,
            spinner,
            ctx: Rc::downgrade(ctx),
            query: RefCell::default(),
            generation: Cell::new(0),
        });
        let weak = Rc::downgrade(&section);
        sign_in_button.connect_clicked(move |_| {
            let Some(section) = weak.upgrade() else {
                return;
            };
            let Some(ctx) = section.ctx.upgrade() else {
                return;
            };
            let again = Rc::downgrade(&section);
            sign_in(
                &ctx,
                None,
                None,
                Rc::new(move || {
                    if let Some(section) = again.upgrade() {
                        section.refresh();
                    }
                }),
            );
        });
        let weak = Rc::downgrade(&section);
        sign_out.connect_clicked(move |_| {
            if let Some(client) = client() {
                gio::spawn_blocking(move || client.sign_out());
            }
            session::clear_seerr();
            if let Some(section) = weak.upgrade() {
                section.refresh();
            }
        });
        section
    }

    /// Show Seerr's matches for `query` that aren't all in the library
    pub fn search(self: &Rc<Self>, query: &str) {
        *self.query.borrow_mut() = query.to_string();
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.grid.remove_all();
        self.heading.set_visible(false);
        self.message.set_visible(false);
        self.sign_in.set_visible(false);
        self.root.set_visible(!query.is_empty());
        if query.is_empty() {
            return;
        }
        let Some(client) = client() else {
            self.show_sign_in("Not in your library? Sign in to Seerr to request movies and shows.");
            return;
        };
        self.spinner.set_visible(true);
        let (this, query) = (Rc::downgrade(self), query.to_string());
        run(
            client,
            move |c| c.search(&query),
            move |result| {
                let Some(this) = this.upgrade() else { return };
                if this.generation.get() != generation {
                    return; // searched again since
                }
                this.spinner.set_visible(false);
                match result {
                    Ok(found) => {
                        // What's all there is in the library results above
                        let found: Vec<Found> = found
                            .into_iter()
                            .filter(|f| f.status() != seerr::AVAILABLE)
                            .collect();
                        this.heading.set_visible(!found.is_empty());
                        for found in found {
                            this.grid.append(&this.card(found));
                        }
                    }
                    Err(e) => this.failed(e),
                }
            },
        );
    }

    /// Search again (when coming back from a page, after a request)
    pub fn refresh(self: &Rc<Self>) {
        let query = self.query.borrow().clone();
        self.search(&query);
    }

    fn show_sign_in(&self, text: &str) {
        self.spinner.set_visible(false);
        self.sign_in_text.set_label(text);
        self.sign_in.set_visible(true);
    }

    fn failed(self: &Rc<Self>, error: seerr::Error) {
        match error {
            seerr::Error::SignIn => {
                session::clear_seerr();
                self.show_sign_in(
                    "Your Seerr sign-in ended. Sign in again to request movies and shows.",
                );
            }
            error => {
                self.message.set_label(&error.to_string());
                self.message.set_visible(true);
            }
        }
    }

    fn card(self: &Rc<Self>, found: Found) -> gtk::Button {
        let kind = if found.is_show() { "Show" } else { "Movie" };
        let subtitle = found
            .year()
            .map_or(kind.to_string(), |year| format!("{year} · {kind}"));
        let (card, art, _) = poster(&found, &subtitle);
        let downloaded = found
            .media_info
            .as_ref()
            .and_then(|m| m.download_progress());
        let badge = match downloaded {
            Some(fraction) => Some(format!("Downloading {:.0}%", fraction * 100.0)),
            None => seerr::status_label(found.status()).map(String::from),
        };
        if let Some(badge) = badge {
            art.add_overlay(
                &gtk::Label::builder()
                    .label(badge)
                    .css_classes(["badge"])
                    .halign(gtk::Align::End)
                    .valign(gtk::Align::Start)
                    .build(),
            );
        }
        let ctx = self.ctx.clone();
        card.connect_clicked(move |_| open_page(&ctx, &found));
        card
    }
}
