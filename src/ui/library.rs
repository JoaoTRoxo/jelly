//! A library (or folder) as a grid of posters, and search.

use super::Ctx;
use super::cards::{self, Shape};
use crate::api;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn grid() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        // One card fits narrow windows (two episode thumbnails need ~670 px)
        .min_children_per_line(1)
        .max_children_per_line(30)
        .column_spacing(4)
        .row_spacing(12)
        .valign(gtk::Align::Start)
        .build()
}

fn fill(ctx: &Rc<Ctx>, grid: &gtk::FlowBox, items: &[api::Item]) {
    grid.remove_all();
    for item in items {
        let (c, i) = (ctx.clone(), item.clone());
        let shape = if item.kind == "Episode" {
            Shape::Thumb
        } else {
            Shape::Poster
        };
        grid.append(&cards::card(ctx, item, shape, move || super::open(&c, &i)));
    }
}

pub fn page(ctx: &Rc<Ctx>, view: api::Item) -> adw::NavigationPage {
    let grid = grid();
    let filter = gtk::SearchEntry::builder()
        .placeholder_text(format!("Filter {}", view.name))
        .hexpand(true)
        .build();
    let filter_clamp = adw::Clamp::builder()
        .maximum_size(520)
        .child(&filter)
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&super::loading(), Some("loading"));
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(12)
        .margin_bottom(32)
        .margin_start(24)
        .margin_end(24)
        .build();
    content.append(&filter_clamp);
    // What's been requested through Seerr and will land in this library
    content.append(&super::request::underway(ctx, Some(view.clone())));
    content.append(&grid);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    stack.add_named(&scroller, Some("grid"));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&stack));
    let page = adw::NavigationPage::builder()
        .title(view.name.as_str())
        .child(&toolbar)
        .build();

    // Filter by name as you type; Enter opens the first match
    let names: Rc<RefCell<Vec<String>>> = Rc::default();
    let n = names.clone();
    let f = filter.clone();
    grid.set_filter_func(move |child| {
        let query = f.text().to_lowercase();
        query.is_empty()
            || n.borrow()
                .get(child.index() as usize)
                .is_some_and(|name| name.to_lowercase().contains(&query))
    });
    let g = grid.clone();
    filter.connect_search_changed(move |_| g.invalidate_filter());
    let g = grid.clone();
    filter.connect_activate(move |_| {
        let mut i = 0;
        while let Some(child) = g.child_at_index(i) {
            if child.is_child_visible() {
                if let Some(button) = child.child().and_downcast::<gtk::Button>() {
                    button.emit_clicked();
                }
                return;
            }
            i += 1;
        }
    });

    let load = {
        let ctx = Rc::downgrade(ctx);
        let (grid, stack) = (grid.clone(), stack.clone());
        move || {
            let Some(ctx) = ctx.upgrade() else { return };
            let (grid, stack, names, view) =
                (grid.clone(), stack.clone(), names.clone(), view.clone());
            let weak = Rc::downgrade(&ctx);
            let is_folder = !matches!(view.kind.as_str(), "CollectionFolder" | "UserView");
            super::run_result(
                &ctx,
                move |client| {
                    if is_folder {
                        client.children(&view.id)
                    } else {
                        client.library(&view)
                    }
                },
                move |result| {
                    let Some(ctx) = weak.upgrade() else { return };
                    match result {
                        Ok(items) => {
                            *names.borrow_mut() = items.iter().map(|i| i.name.clone()).collect();
                            fill(&ctx, &grid, &items);
                            stack.set_visible_child_name("grid");
                        }
                        Err(e) => {
                            if let Some(old) = stack.child_by_name("error") {
                                stack.remove(&old);
                            }
                            let (s, w) = (stack.clone(), Rc::downgrade(&ctx));
                            let retry = move || {
                                s.set_visible_child_name("loading");
                                if let Some(ctx) = w.upgrade() {
                                    ctx.refresh_visible();
                                }
                            };
                            stack.add_named(
                                &super::error_page(&e.to_string(), retry),
                                Some("error"),
                            );
                            stack.set_visible_child_name("error");
                        }
                    }
                },
            );
        }
    };
    super::reloading_page(ctx, &page, load);
    page
}

pub fn search_page(ctx: &Rc<Ctx>) -> adw::NavigationPage {
    let entry = gtk::SearchEntry::builder()
        .placeholder_text("Search movies, series and episodes")
        .hexpand(true)
        .search_delay(250)
        .build();
    let header = adw::HeaderBar::builder()
        .title_widget(
            &adw::Clamp::builder()
                .maximum_size(520)
                .child(&entry)
                .build(),
        )
        .build();
    let grid = grid();
    let nothing = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .margin_start(6)
        .margin_top(12)
        .css_classes(["dim-label"])
        .visible(false)
        .build();
    // Below the library's results: what can be requested through Seerr
    let requests = super::request::Section::new(ctx);
    let again = requests.clone();
    let status = adw::StatusPage::builder()
        .icon_name("system-search-symbolic")
        .title("Search")
        .vexpand(true)
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&status, Some("status"));
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(32)
        .margin_start(24)
        .margin_end(24)
        .build();
    content.append(&nothing);
    content.append(&grid);
    content.append(&requests.root);
    stack.add_named(
        &gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build(),
        Some("results"),
    );

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    let page = adw::NavigationPage::builder()
        .title("Search")
        .tag("search")
        .child(&toolbar)
        .build();

    let latest_query: Rc<RefCell<String>> = Rc::default();
    let weak = Rc::downgrade(ctx);
    entry.connect_search_changed(move |entry| {
        let Some(ctx) = weak.upgrade() else { return };
        let query = entry.text().trim().to_string();
        *latest_query.borrow_mut() = query.clone();
        requests.search(&query);
        if query.is_empty() {
            stack.set_visible_child_name("status");
            return;
        }
        stack.set_visible_child_name("results");
        let (grid, nothing, latest, w) = (
            grid.clone(),
            nothing.clone(),
            latest_query.clone(),
            Rc::downgrade(&ctx),
        );
        let q = query.clone();
        super::run(
            &ctx,
            move |client| client.search(&q),
            move |items| {
                let Some(ctx) = w.upgrade() else { return };
                if *latest.borrow() != query {
                    return; // typed more since
                }
                fill(&ctx, &grid, &items);
                grid.set_visible(!items.is_empty());
                nothing.set_label(&format!("Nothing in your library for “{query}”"));
                nothing.set_visible(items.is_empty());
            },
        );
    });
    let e = entry.clone();
    page.connect_shown(move |_| {
        e.grab_focus();
    });
    // Back from a movie's or show's page: its request may have changed
    page.connect_showing(move |_| again.refresh());
    page
}
