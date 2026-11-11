//! What's new: the first time Jelly opens after an update, a line for each
//! change in the versions since the last one to run. They're the releases'
//! notes in the AppStream metadata, which software centres show too: short,
//! under "New" and "Adjustments". CHANGELOG.md has the details.

use super::Ctx;
use crate::{session, update};
use adw::prelude::*;
use std::rc::Rc;

/// The metadata as this version was built with it: its release for the
/// version has the notes
const METAINFO: &str = include_str!("../../data/io.github.joaotroxo.Jelly.metainfo.xml");
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A version's notes: each heading (`<p>New</p>`), if there is one, and the
/// lines under it
#[derive(Debug, Default, PartialEq)]
struct Notes {
    version: String,
    sections: Vec<(Option<String>, Vec<String>)>,
}

/// Each release's notes in `metainfo`, newest first
fn parse(metainfo: &str) -> Vec<Notes> {
    metainfo
        .split("<release ")
        .skip(1)
        .filter_map(|release| {
            let version = release.split("version=\"").nth(1)?.split('"').next()?;
            let body = release.split("</release>").next()?;
            Some(Notes {
                version: version.to_string(),
                sections: sections(body),
            })
        })
        .collect()
}

/// The headings and list items in a release's description, in order. A list
/// without a heading (as older releases have) comes under none.
fn sections(mut rest: &str) -> Vec<(Option<String>, Vec<String>)> {
    let mut sections: Vec<(Option<String>, Vec<String>)> = Vec::new();
    loop {
        let heading = rest.find("<p>");
        let item = rest.find("<li>");
        if let Some(at) = heading.filter(|p| item.is_none_or(|li| *p < li)) {
            let Some((text, after)) = rest[at + 3..].split_once("</p>") else {
                break;
            };
            sections.push((Some(plain(text)), Vec::new()));
            rest = after;
        } else if let Some(at) = item {
            let Some((text, after)) = rest[at + 4..].split_once("</li>") else {
                break;
            };
            if sections.is_empty() {
                sections.push((None, Vec::new()));
            }
            if let Some((_, lines)) = sections.last_mut() {
                lines.push(plain(text));
            }
            rest = after;
        } else {
            break;
        }
    }
    sections
}

/// XML's text as it reads: without tags (`<em>`, `<code>`), and with its
/// entities
fn plain(xml: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in xml.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// What to show on starting `current`, having last run `seen` (empty when it
/// wasn't kept: before 0.5.0, or on the first start). `used_before`: whether
/// Jelly kept settings before, which tells an update from a first start.
fn to_show(metainfo: &str, seen: &str, used_before: bool, current: &str) -> Vec<Notes> {
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
    parse(metainfo)
        .into_iter()
        .filter(|notes| !update::is_newer(&notes.version, current) && since(&notes.version))
        .filter(|notes| notes.sections.iter().any(|(_, lines)| !lines.is_empty()))
        .collect()
}

/// The first time this version runs, after another: what's new since
pub fn after_update(ctx: &Rc<Ctx>) {
    let mut settings = session::settings();
    if settings.last_version == VERSION {
        return;
    }
    let notes = to_show(
        METAINFO,
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
        for (title, lines) in &release.sections {
            section(&content, title.as_deref(), lines);
        }
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
        .content_width(480)
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

/// A heading, if there is one, and each line under it as a point
fn section(content: &gtk::Box, title: Option<&str>, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if let Some(title) = title {
        list.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
    }
    for line in lines {
        let point = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        point.append(
            &gtk::Label::builder()
                .label("•")
                .valign(gtk::Align::Start)
                .build(),
        );
        point.append(
            &gtk::Label::builder()
                .label(line)
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

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<component>
  <releases>
    <release version="0.6.0" date="2026-12-01">
      <description>
        <p>New</p>
        <ul>
          <li>Second subtitles</li>
          <li>Request subtitles
            from the player</li>
        </ul>
        <p>Adjustments</p>
        <ul>
          <li>Memory usage improved</li>
          <li>Faster &amp; <em>smoother</em> seeking</li>
        </ul>
      </description>
    </release>
    <release version="0.5.0" date="2026-11-11">
      <description>
        <ul>
          <li>A and S switch menus</li>
        </ul>
      </description>
    </release>
    <release version="0.4.0" date="2026-10-25">
      <description>
        <p>The first release.</p>
      </description>
    </release>
  </releases>
</component>"#;

    fn versions(notes: &[Notes]) -> Vec<&str> {
        notes.iter().map(|n| n.version.as_str()).collect()
    }

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reads_each_releases_lines_under_their_headings() {
        let all = parse(XML);
        assert_eq!(versions(&all), ["0.6.0", "0.5.0", "0.4.0"]);
        assert_eq!(
            all[0].sections,
            [
                (
                    Some("New".into()),
                    lines(&["Second subtitles", "Request subtitles from the player"])
                ),
                (
                    Some("Adjustments".into()),
                    lines(&["Memory usage improved", "Faster & smoother seeking"])
                ),
            ]
        );
        // As older releases have them: a list on its own
        assert_eq!(all[1].sections, [(None, lines(&["A and S switch menus"]))]);
    }

    #[test]
    fn shows_whats_come_since_the_last_version_to_run() {
        let show =
            |seen, used_before| versions(&to_show(XML, seen, used_before, "0.6.0")).join(" ");
        assert_eq!(show("0.5.0", true), "0.6.0");
        // Updated past a version
        assert_eq!(show("0.4.0", true), "0.6.0 0.5.0");
        // From a version that didn't say: this one's
        assert_eq!(show("", true), "0.6.0");
    }

    #[test]
    fn says_nothing_otherwise() {
        // The same version, the first start, back to an older one
        assert!(to_show(XML, "0.6.0", true, "0.6.0").is_empty());
        assert!(to_show(XML, "", false, "0.6.0").is_empty());
        assert!(to_show(XML, "0.7.0", true, "0.6.0").is_empty());
        // A version without notes, or without lines in them
        assert!(to_show(XML, "0.6.0", true, "0.6.1").is_empty());
        assert!(to_show(XML, "0.3.0", true, "0.4.0").is_empty());
    }

    #[test]
    fn this_versions_notes_are_short_lines() {
        // Built from a release's tag, the metadata has its notes
        let all = parse(METAINFO);
        let ours = all.iter().find(|n| n.version == "0.5.0").unwrap();
        let headings: Vec<_> = ours.sections.iter().map(|(t, _)| t.as_deref()).collect();
        assert_eq!(headings, [Some("New"), Some("Adjustments")]);
        assert!(
            ours.sections
                .iter()
                .flat_map(|(_, l)| l)
                .all(|l| l.len() <= 60)
        );
    }

    #[test]
    fn reads_xml_text_as_it_reads() {
        assert_eq!(
            plain("Kept in <code>settings.json</code> &amp;\n  more"),
            "Kept in settings.json & more"
        );
    }
}
