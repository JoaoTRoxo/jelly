//! A movie's or show's page from Seerr: what it is, where its request is at,
//! how its downloads are going, and its cast.

use super::dialogs::ask_to_request;
use super::*;
use crate::seerr::{self, Details, Found};
use crate::ui::Ctx;
use crate::ui::cards;
use crate::{images, session};
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// What changes on the page as its request goes on
struct Live {
    state: gtk::Label,
    bar: gtk::ProgressBar,
    actions: gtk::Box,
    downloads: gtk::Box,
    list: gtk::ListBox,
}

struct DetailsPage {
    ctx: Weak<Ctx>,
    kind: String,
    id: i64,
    stack: gtk::Stack,
    header: adw::HeaderBar,
    live: RefCell<Option<Live>>,
}

/// A movie's or show's page from Seerr (`media_type`: movie or tv): what it
/// is, where its request is at and how its downloads are going, and its cast
pub fn page(ctx: &Rc<Ctx>, media_type: &str, tmdb_id: i64, title: &str) -> adw::NavigationPage {
    let stack = gtk::Stack::new();
    stack.add_named(&crate::ui::loading(), Some("loading"));
    let header = adw::HeaderBar::builder().show_title(false).build();
    let toolbar = adw::ToolbarView::builder()
        .extend_content_to_top_edge(true)
        .build();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    let page = adw::NavigationPage::builder()
        .title(title)
        .child(&toolbar)
        .build();

    let this = Rc::new(DetailsPage {
        ctx: Rc::downgrade(ctx),
        kind: media_type.to_string(),
        id: tmdb_id,
        stack,
        header,
        live: RefCell::default(),
    });
    // Up to date whenever it's shown (back from the library), and every half
    // minute while it is (Seerr looks at the downloads once a minute)
    page.connect_showing(move |_| this.refresh());
    let weak = page.downgrade();
    glib::timeout_add_seconds_local(30, move || {
        let Some(page) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if page.is_mapped() {
            page.emit_by_name::<()>("showing", &[]);
        }
        glib::ControlFlow::Continue
    });
    page
}

impl DetailsPage {
    fn refresh(self: &Rc<Self>) {
        let Some(client) = client() else {
            self.show_problem(seerr::Error::SignIn);
            return;
        };
        let (this, kind, id) = (Rc::downgrade(self), self.kind.clone(), self.id);
        run(
            client,
            move |c| c.details(&kind, id),
            move |result| {
                let Some(this) = this.upgrade() else { return };
                match result {
                    Ok(details) => this.show(&details),
                    // Already showing: keep that until the next try
                    Err(e) if this.live.borrow().is_none() => this.show_problem(e),
                    Err(_) => {}
                }
            },
        );
    }

    fn again(self: &Rc<Self>) -> Rc<dyn Fn()> {
        let weak = Rc::downgrade(self);
        Rc::new(move || {
            if let Some(this) = weak.upgrade() {
                this.refresh();
            }
        })
    }

    fn show_problem(self: &Rc<Self>, error: seerr::Error) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        if let Some(old) = self.stack.child_by_name("problem") {
            self.stack.remove(&old);
        }
        let again = self.again();
        let problem = match error {
            seerr::Error::SignIn => {
                session::clear_seerr();
                let button = gtk::Button::builder()
                    .label("Sign In to Seerr")
                    .css_classes(["pill", "suggested-action"])
                    .halign(gtk::Align::Center)
                    .build();
                let w = Rc::downgrade(&ctx);
                button.connect_clicked(move |_| {
                    if let Some(ctx) = w.upgrade() {
                        sign_in(&ctx, None, None, again.clone());
                    }
                });
                adw::StatusPage::builder()
                    .icon_name("folder-download-symbolic")
                    .title("Sign in to Seerr")
                    .description("This comes from Seerr, which needs you signed in")
                    .child(&button)
                    .vexpand(true)
                    .build()
                    .upcast()
            }
            error => crate::ui::error_page(&error.to_string(), move || again()),
        };
        self.stack.add_named(&problem, Some("problem"));
        self.stack.set_visible_child_name("problem");
    }

    fn show(self: &Rc<Self>, details: &Details) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        if self.live.borrow().is_none() {
            let (scroller, live) = build(details);
            crate::ui::header_over_hero(&self.header, &scroller);
            self.stack.add_named(&scroller, Some("content"));
            self.stack.set_visible_child_name("content");
            *self.live.borrow_mut() = Some(live);
        }
        if let Some(live) = self.live.borrow().as_ref() {
            update(&ctx, live, details, self.again());
        }
    }
}

/// The page's content: what doesn't change, and the parts that do
fn build(details: &Details) -> (gtk::ScrolledWindow, Live) {
    let found = details.found();
    // Hero: backdrop, poster, and what it is (as on a library item's page)
    let sizer = gtk::Box::builder()
        .hexpand(true)
        .css_classes(["hero"])
        .build();
    let backdrop = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    if let Some(path) = &details.backdrop_path {
        images::set_public(&backdrop, seerr::tmdb_image(path, "w1280"));
    }
    let overlay = gtk::Overlay::builder()
        .child(&sizer)
        .overflow(gtk::Overflow::Hidden)
        .build();
    overlay.add_overlay(&backdrop);
    overlay.add_overlay(&gtk::Box::builder().css_classes(["hero-shade"]).build());

    let info = gtk::Box::builder()
        .spacing(28)
        .valign(gtk::Align::End)
        .margin_start(30)
        .margin_end(30)
        .margin_bottom(28)
        .margin_top(72)
        .build();
    let (art, picture) = cards::frame(200, 300);
    match found.poster_url() {
        Some(url) => images::set_public(&picture, url),
        None => art.add_overlay(
            &gtk::Image::builder()
                .icon_name("folder-videos-symbolic")
                .pixel_size(48)
                .css_classes(["dim-label"])
                .build(),
        ),
    }
    info.append(&art);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .valign(gtk::Align::End)
        .hexpand(true)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(found.title())
            .xalign(0.0)
            .wrap(true)
            .css_classes(["hero-title"])
            .build(),
    );
    let mut meta: Vec<String> = Vec::new();
    meta.extend(found.year().map(String::from));
    if let Some(score) = details.vote_average.filter(|s| *s > 0.0) {
        meta.push(format!("★ {score:.1}"));
    }
    let minutes = details
        .runtime
        .or(details.episode_run_time.first().copied())
        .filter(|m| *m > 0);
    if let Some(m) = minutes {
        let each = if details.is_show() { " episodes" } else { "" };
        meta.push(format!("{}{each}", crate::ui::format_runtime(m.into())));
    }
    if !details.genres.is_empty() {
        meta.push(
            details
                .genres
                .iter()
                .take(3)
                .map(|g| g.name.as_str())
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
    // Where its request is at
    let state = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let bar = gtk::ProgressBar::builder()
        .css_classes(["request-progress"])
        .visible(false)
        .build();
    let status = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .width_request(320)
        .halign(gtk::Align::Start)
        .build();
    status.append(&state);
    status.append(&bar);
    text.append(&status);
    if let Some(next) = &details.next_episode_to_air {
        let when = next
            .air_date
            .as_deref()
            .and_then(|d| glib::DateTime::from_iso8601(&format!("{d}T12:00:00Z"), None).ok())
            .and_then(|d| d.format("%-d %B").ok());
        let parts: Vec<String> = details
            .next_episode_number()
            .into_iter()
            .chain(when.map(String::from))
            .collect();
        if !parts.is_empty() {
            text.append(
                &gtk::Label::builder()
                    .label(format!("Next episode: {}", parts.join(", ")))
                    .xalign(0.0)
                    .css_classes(["dim-label"])
                    .build(),
            );
        }
    }
    if let Some(tagline) = details.tagline.as_deref().filter(|t| !t.is_empty()) {
        text.append(
            &gtk::Label::builder()
                .label(tagline)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["tagline"])
                .build(),
        );
    }
    if let Some(overview) = details.overview.as_deref().filter(|o| !o.is_empty()) {
        text.append(
            &gtk::Label::builder()
                .label(overview)
                .xalign(0.0)
                .wrap(true)
                .lines(5)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(110)
                .build(),
        );
    }
    let actions = gtk::Box::builder().spacing(10).margin_top(8).build();
    text.append(&actions);
    info.append(&text);
    overlay.add_overlay(&info);
    overlay.set_measure_overlay(&info, true);

    // Below: the downloads, one per episode for shows, and the cast
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(28)
        .margin_bottom(32)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let downloads = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(30)
        .margin_end(30)
        .visible(false)
        .build();
    downloads.append(
        &gtk::Label::builder()
            .label("Downloads")
            .xalign(0.0)
            .css_classes(["shelf-title"])
            .build(),
    );
    downloads.append(
        &adw::Clamp::builder()
            .child(&list)
            .maximum_size(820)
            .halign(gtk::Align::Start)
            .build(),
    );
    body.append(&downloads);
    let people: Vec<_> = details
        .credits
        .cast
        .iter()
        .take(20)
        .map(|member| {
            let photo = member.profile_path.as_deref().map(|path| {
                let url = seerr::tmdb_image(path, "w185");
                Box::new(move |picture: &gtk::Picture| images::set_public(picture, url))
                    as cards::Photo
            });
            cards::person_card(
                &member.name,
                member.character.as_deref().unwrap_or_default(),
                photo,
            )
        })
        .collect();
    if !people.is_empty() {
        body.append(&cards::shelf("Cast", people, None));
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&overlay);
    content.append(&body);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    (
        scroller,
        Live {
            state,
            bar,
            actions,
            downloads,
            list,
        },
    )
}

/// Bring the page's changing parts up to date with `details`
fn update(ctx: &Rc<Ctx>, live: &Live, details: &Details, again: Rc<dyn Fn()>) {
    let media = details.media_info.clone().unwrap_or_default();
    live.state.set_label(&details.state());
    let progress = media.download_progress();
    live.bar.set_visible(progress.is_some());
    live.bar.set_fraction(progress.unwrap_or(0.0));

    crate::ui::clear(&live.actions);
    let mut buttons = Vec::new();
    if let Some(id) = media.jellyfin_media_id.clone() {
        let button = gtk::Button::builder().label("Open in Library").build();
        let c = Rc::downgrade(ctx);
        button.connect_clicked(move |_| {
            let Some(ctx) = c.upgrade() else { return };
            let (id, c) = (id.clone(), ctx.clone());
            crate::ui::run(
                &ctx,
                move |client| client.item(&id),
                move |item| crate::ui::open(&c, &item),
            );
        });
        buttons.push(button);
    }
    if details.can_request() {
        let label = if details.is_show() && media.status > 1 {
            "Request More Seasons"
        } else {
            "Request"
        };
        let button = gtk::Button::builder().label(label).build();
        let (c, d) = (Rc::downgrade(ctx), details.clone());
        button.connect_clicked(move |_| {
            if let Some(ctx) = c.upgrade() {
                ask_to_request(&ctx, &d, again.clone());
            }
        });
        buttons.push(button);
    }
    if let Some(seerr) = session::load_seerr() {
        let button = gtk::Button::builder()
            .label("Open in Seerr")
            .css_classes(["flat"])
            .build();
        let (url, window) = (
            format!("{}/{}/{}", seerr.url, details.media_type, details.id),
            ctx.window.downgrade(),
        );
        button.connect_clicked(move |_| {
            gtk::UriLauncher::new(&url).launch(
                window.upgrade().as_ref(),
                gio::Cancellable::NONE,
                |_| {},
            );
        });
        buttons.push(button);
    }
    for (i, button) in buttons.iter().enumerate() {
        button.add_css_class("pill");
        if i == 0 && !button.has_css_class("flat") {
            button.add_css_class("suggested-action");
        }
        live.actions.append(button);
    }

    let mut downloads = media.download_status.clone();
    downloads.sort_by_key(|d| {
        d.episode
            .as_ref()
            .map(|e| (e.season_number, e.episode_number))
    });
    live.list.remove_all();
    for download in &downloads {
        live.list.append(&download_row(download, &details.found()));
    }
    live.downloads.set_visible(!downloads.is_empty());
}

/// "S3 E1 · Title    Downloading · 62% · 1 min left", its bar, and the release
fn download_row(download: &seerr::Download, found: &Found) -> gtk::Widget {
    let name = download
        .episode_name()
        .unwrap_or_else(|| found.title().to_string());
    let top = gtk::Box::builder().spacing(12).build();
    top.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build(),
    );
    top.append(
        &gtk::Label::builder()
            .label(download.describe())
            .css_classes(["dim-label", "caption", "numeric"])
            .build(),
    );
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["download-row"])
        .build();
    row.append(&top);
    row.append(
        &gtk::ProgressBar::builder()
            .fraction(download.fraction())
            .css_classes(["request-progress"])
            .build(),
    );
    if !download.title.is_empty() {
        row.append(
            &gtk::Label::builder()
                .label(&download.title)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .css_classes(["dim-label", "caption"])
                .build(),
        );
    }
    row.upcast()
}
