//! What's new: the first time Jelly opens after an update, what the versions
//! since the last one to run add and adjust, from the changelog.

use super::Ctx;
use crate::{session, update};
use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

/// The changelog as this version was built with it: its section for the
/// version is the release's notes
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A version's notes
#[derive(Debug, Default, PartialEq)]
struct Notes {
    version: String,
    /// What it adds
    new: Vec<String>,
    /// What it changes and fixes
    adjustments: Vec<String>,
}

/// Each version's notes in `changelog`, newest first, without Unreleased
fn parse(changelog: &str) -> Vec<Notes> {
    let mut all: Vec<Notes> = Vec::new();
    let mut new = true;
    for line in changelog.lines() {
        if let Some(heading) = line.strip_prefix("## [") {
            let version = heading.split(']').next().unwrap_or_default();
            all.push(Notes {
                version: version.to_string(),
                ..Default::default()
            });
            new = true;
            continue;
        }
        // The links under the last version
        if line.starts_with('[') && !all.is_empty() {
            break;
        }
        let Some(notes) = all.last_mut() else {
            continue;
        };
        if let Some(kind) = line.strip_prefix("### ") {
            new = kind.trim() == "Added";
            continue;
        }
        let list = if new {
            &mut notes.new
        } else {
            &mut notes.adjustments
        };
        if let Some(entry) = line.strip_prefix("- ") {
            list.push(entry.trim().to_string());
        } else if line.starts_with("  ")
            && let Some(entry) = list.last_mut()
        {
            entry.push(' ');
            entry.push_str(line.trim());
        }
    }
    all.retain(|notes| notes.version != "Unreleased");
    all
}

/// What to show on starting `current`, having last run `seen` (empty when it
/// wasn't kept: before 0.5.0, or on the first start). `used_before`: whether
/// Jelly kept settings before, which tells an update from a first start.
fn to_show(changelog: &str, seen: &str, used_before: bool, current: &str) -> Vec<Notes> {
    let first_start = seen.is_empty() && !used_before;
    if seen == current || first_start || update::is_newer(seen, current) {
        return Vec::new();
    }
    let since = |version: &str| {
        // From a version that didn't say which: only this one's
        if seen.is_empty() {
            version == current
        } else {
            update::is_newer(version, seen)
        }
    };
    parse(changelog)
        .into_iter()
        .filter(|notes| !update::is_newer(&notes.version, current) && since(&notes.version))
        .collect()
}

/// The first time this version runs, after another: what's new since
pub fn after_update(ctx: &Rc<Ctx>) {
    let mut settings = session::settings();
    if settings.last_version == VERSION {
        return;
    }
    let notes = to_show(
        CHANGELOG,
        &settings.last_version,
        session::has_settings(),
        VERSION,
    );
    settings.last_version = VERSION.to_string();
    session::save_settings(&settings);
    if !notes.is_empty() {
        present(ctx, &notes);
    }
}

fn present(ctx: &Rc<Ctx>, notes: &[Notes]) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_start(24)
        .margin_end(24)
        .margin_bottom(24)
        .build();
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 4);
    heading.append(
        &gtk::Label::builder()
            .label("What's New")
            .css_classes(["title-1"])
            .build(),
    );
    heading.append(
        &gtk::Label::builder()
            .label(format!("Jelly {VERSION}"))
            .css_classes(["dim-label"])
            .build(),
    );
    content.append(&heading);
    for release in notes {
        // Updated past several versions: which is which
        if notes.len() > 1 {
            content.append(
                &gtk::Label::builder()
                    .label(format!("Jelly {}", release.version))
                    .xalign(0.0)
                    .css_classes(["title-3"])
                    .build(),
            );
        }
        section(&content, "New", &release.new);
        section(&content, "Adjustments", &release.adjustments);
    }
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(560)
        .build();
    let done = gtk::Button::builder()
        .label("Continue")
        .css_classes(["pill", "suggested-action"])
        .halign(gtk::Align::Center)
        .margin_top(12)
        .margin_bottom(18)
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    view.set_content(Some(&scroller));
    view.add_bottom_bar(&done);
    let dialog = adw::Dialog::builder()
        .title("What's New")
        .content_width(560)
        .child(&view)
        .build();
    let d = dialog.downgrade();
    done.connect_clicked(move |_| {
        if let Some(dialog) = d.upgrade() {
            dialog.close();
        }
    });
    dialog.present(Some(&ctx.window));
    dialog.set_focus(Some(&done));
}

/// A heading, and each entry under it as a point
fn section(content: &gtk::Box, title: &str, entries: &[String]) {
    if entries.is_empty() {
        return;
    }
    let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    list.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .build(),
    );
    for entry in entries {
        let point = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        point.append(
            &gtk::Label::builder()
                .label("•")
                .valign(gtk::Align::Start)
                .build(),
        );
        point.append(
            &gtk::Label::builder()
                .label(markup(entry))
                .use_markup(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        list.append(&point);
    }
    content.append(&list);
}

/// An entry as Pango markup, with `code` in a fixed-width font
fn markup(entry: &str) -> String {
    entry
        .split('`')
        .enumerate()
        .map(|(i, part)| {
            let text = glib::markup_escape_text(part);
            if i % 2 == 1 {
                format!("<tt>{text}</tt>")
            } else {
                text.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog

[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and so on.

## [Unreleased]

### Added

- Not out yet.

## [0.5.0] - 2026-11-11

### Added

- What's New, once Jelly is
  updated.
- Subtitle requests.

### Changed

- The controls hide after 1.5 s.

### Fixed

- Memory that no longer grows.

## [0.4.1] - 2026-10-26

### Changed

- A and S switch menus.

## [0.4.0] - 2026-10-25

### Added

- A previous episode button.

[Unreleased]: https://github.com/JoaoTRoxo/jelly/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/JoaoTRoxo/jelly/compare/v0.4.1...v0.5.0
";

    fn versions(notes: &[Notes]) -> Vec<&str> {
        notes.iter().map(|n| n.version.as_str()).collect()
    }

    #[test]
    fn reads_whats_new_and_whats_adjusted() {
        let all = parse(LOG);
        assert_eq!(versions(&all), ["0.5.0", "0.4.1", "0.4.0"]);
        assert_eq!(
            all[0],
            Notes {
                version: "0.5.0".into(),
                new: vec![
                    "What's New, once Jelly is updated.".into(),
                    "Subtitle requests.".into()
                ],
                adjustments: vec![
                    "The controls hide after 1.5 s.".into(),
                    "Memory that no longer grows.".into()
                ],
            }
        );
        assert!(all[1].new.is_empty());
    }

    #[test]
    fn shows_whats_come_since_the_last_version_to_run() {
        let show =
            |seen, used_before| versions(&to_show(LOG, seen, used_before, "0.5.0")).join(" ");
        assert_eq!(show("0.4.1", true), "0.5.0");
        // Updated past a version
        assert_eq!(show("0.4.0", true), "0.5.0 0.4.1");
        // From a version that didn't say: this one's
        assert_eq!(show("", true), "0.5.0");
    }

    #[test]
    fn says_nothing_otherwise() {
        // The same version, the first start, back to an older one
        assert!(to_show(LOG, "0.5.0", true, "0.5.0").is_empty());
        assert!(to_show(LOG, "", false, "0.5.0").is_empty());
        assert!(to_show(LOG, "0.6.0", true, "0.5.0").is_empty());
        // A version without notes
        assert!(to_show(LOG, "0.5.0", true, "0.5.1").is_empty());
    }

    #[test]
    fn this_versions_notes_are_there() {
        // Built from a release's tag, the changelog has its section
        let all = parse(CHANGELOG);
        assert!(all.iter().any(|n| n.version == "0.4.1"));
        assert!(
            all.iter()
                .all(|n| !n.new.is_empty() || !n.adjustments.is_empty())
        );
    }

    #[test]
    fn code_is_in_a_fixed_width_font() {
        assert_eq!(
            markup("Kept in `settings.json` & `mpv.conf`"),
            "Kept in <tt>settings.json</tt> &amp; <tt>mpv.conf</tt>"
        );
    }
}
